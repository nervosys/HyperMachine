//! The append-only record every other layer points at.
//!
//! # Why append-only is the whole design
//!
//! Everything above this file -- eviction, headlines, the working view -- is
//! allowed to *forget*, because none of it is the record. A headline that says
//! "read the file, found three call sites" is only trustworthy if the events it
//! points at still say exactly what they said. So this module offers no way to
//! edit or remove an event. Not "discourages"; does not offer. The public
//! surface is append, read, and scan.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::payload::{Payload, PayloadStore};
use crate::{ContextError, Result};

/// An event's permanent address.
///
/// Monotonic and never reused, so a reference taken at any point stays valid
/// for the life of the log. Displayed as `#12` because these appear in text an
/// agent reads and writes back.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct Seq(pub u64);

impl std::fmt::Display for Seq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl Seq {
    /// The address after this one.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Who produced an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A person, or whatever is standing in for one.
    User,
    /// The model.
    Assistant,
    /// A tool's result.
    Tool,
    /// The harness itself: instructions, notices, errors.
    System,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::System => "system",
        };
        f.write_str(name)
    }
}

/// An event, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// This event's permanent address.
    pub seq: Seq,
    /// Which session it belongs to. Sessions share one log so that a later
    /// one can search an earlier one, which is the point of keeping them.
    pub session: String,
    /// Who produced it.
    pub role: Role,
    /// A free-form type, used for filtering: `tool_result`, `plan`, `commit`.
    pub kind: String,
    /// Milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// The content, inline or externalized.
    pub payload: Payload,
}

/// An event about to be appended, before it has an address.
#[derive(Debug, Clone)]
pub struct EventDraft {
    /// Which session it belongs to.
    pub session: String,
    /// Who produced it.
    pub role: Role,
    /// A free-form type used for filtering.
    pub kind: String,
    /// The content, before any decision about where it lives.
    pub text: String,
    /// When it happened. `None` stamps it at append time.
    ///
    /// Present so that a caller replaying a transcript can keep the times it
    /// already has, rather than restamping history as the moment it was
    /// imported.
    pub timestamp_ms: Option<u64>,
}

impl EventDraft {
    /// A draft with the required fields, stamped at append time.
    pub fn new(
        session: impl Into<String>,
        role: Role,
        kind: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            session: session.into(),
            role,
            kind: kind.into(),
            text: text.into(),
            timestamp_ms: None,
        }
    }

    /// The same draft with an explicit time.
    #[must_use]
    pub fn at(mut self, timestamp_ms: u64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }
}

/// The log: one file of newline-delimited records, plus the events in memory.
///
/// # Durability
///
/// Each append is written and flushed to the OS. It is **not** fsynced, so a
/// power loss can lose the tail. Said plainly because "append-only durable
/// record" invites the assumption that it is, and a log that quietly loses its
/// last few entries is worse than one that says it might.
///
/// A partly written final line is not corruption in practice -- it is the
/// normal shape of an interrupted write -- so [`EventLog::open`] reports it
/// with its line number rather than discarding the file.
#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    file: File,
    events: Vec<Event>,
    store: PayloadStore,
}

impl EventLog {
    /// Open the log at `root`, replaying whatever is already there.
    ///
    /// Creates `root`, `root/events.jsonl` and `root/payloads/` as needed.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::Io`] if the files cannot be opened, and
    /// [`ContextError::Corrupt`] naming the line if a record will not parse.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        std::fs::create_dir_all(root)
            .map_err(|e| ContextError::Io(format!("creating {}: {e}", root.display())))?;

        let store = PayloadStore::open(root.join("payloads"))?;
        let path = root.join("events.jsonl");
        let events = Self::replay(&path)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| ContextError::Io(format!("opening {}: {e}", path.display())))?;

        Ok(Self {
            path,
            file,
            events,
            store,
        })
    }

    /// Read every record back, in order.
    fn replay(path: &Path) -> Result<Vec<Event>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(path)
            .map_err(|e| ContextError::Io(format!("reading {}: {e}", path.display())))?;

        let mut events = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line_no = index as u64 + 1;
            let line =
                line.map_err(|e| ContextError::Io(format!("reading {}: {e}", path.display())))?;
            if line.trim().is_empty() {
                continue;
            }
            let event: Event = serde_json::from_str(&line)
                .map_err(|e| ContextError::Corrupt(line_no, e.to_string()))?;
            events.push(event);
        }
        Ok(events)
    }

    /// The payload store this log externalizes into.
    pub fn store(&self) -> &PayloadStore {
        &self.store
    }

    /// Replace the payload store, usually to lower its threshold in a test.
    pub fn set_store(&mut self, store: PayloadStore) {
        self.store = store;
    }

    /// How many events the log holds.
    pub fn len(&self) -> u64 {
        self.events.len() as u64
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Every event, oldest first.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The address the next append will get.
    pub fn next_seq(&self) -> Seq {
        Seq(self.len() + 1)
    }

    /// Append an event and return its address.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::Io`] if the record or its payload cannot be
    /// written. Nothing is added to the in-memory log unless the write
    /// succeeded, so a failed append leaves the log exactly as it was.
    pub fn append(&mut self, draft: EventDraft) -> Result<Seq> {
        let seq = self.next_seq();
        let payload = self.store.store(seq.0, &draft.text)?;

        let event = Event {
            seq,
            session: draft.session,
            role: draft.role,
            kind: draft.kind,
            timestamp_ms: draft.timestamp_ms.unwrap_or_else(now_ms),
            payload,
        };

        let mut line = serde_json::to_string(&event)
            .map_err(|e| ContextError::Io(format!("encoding event {seq}: {e}")))?;
        line.push('\n');

        // Write before the in-memory push, so a failure cannot leave the two
        // disagreeing about what the log contains.
        self.file
            .write_all(line.as_bytes())
            .map_err(|e| ContextError::Io(format!("appending to {}: {e}", self.path.display())))?;
        self.file
            .flush()
            .map_err(|e| ContextError::Io(format!("flushing {}: {e}", self.path.display())))?;

        self.events.push(event);
        Ok(seq)
    }

    /// One event by address.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::NoSuchSeq`] for an address the log does not
    /// hold. An out-of-range address is a caller bug, and answering it with
    /// `None` would let that bug read as "nothing was recorded".
    pub fn get(&self, seq: Seq) -> Result<&Event> {
        if seq.0 == 0 || seq.0 > self.len() {
            return Err(ContextError::NoSuchSeq(seq, self.len()));
        }
        Ok(&self.events[(seq.0 - 1) as usize])
    }

    /// Every event in `lo..=hi`, inclusive at both ends.
    ///
    /// A range that runs off the end is clamped rather than refused: the ends
    /// of a span are usually computed, and "the last four events" should not
    /// become an error the moment it reaches the start of the log. A range
    /// that runs *backwards* is refused, because that is never a clamp.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::BackwardRange`] if `hi` precedes `lo`.
    pub fn range(&self, lo: Seq, hi: Seq) -> Result<&[Event]> {
        if hi < lo {
            return Err(ContextError::BackwardRange(lo, hi));
        }
        if self.is_empty() {
            return Ok(&[]);
        }
        let start = lo.0.max(1).min(self.len()) - 1;
        let end = hi.0.max(1).min(self.len());
        Ok(&self.events[start as usize..end as usize])
    }

    /// Read one event's payload in full, opening the store if it was
    /// externalized.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::NoSuchSeq`] for an unknown address and
    /// [`ContextError::LostPayload`] if the bytes behind a handle are gone.
    pub fn materialize(&self, seq: Seq) -> Result<String> {
        let event = self.get(seq)?;
        self.store.materialize(&event.payload)
    }
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(kind: &str, text: &str) -> EventDraft {
        EventDraft::new("s1", Role::Tool, kind, text).at(1_000)
    }

    #[test]
    fn addresses_start_at_one_and_never_repeat() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::open(dir.path()).unwrap();

        assert_eq!(log.append(draft("a", "first")).unwrap(), Seq(1));
        assert_eq!(log.append(draft("b", "second")).unwrap(), Seq(2));
        assert_eq!(log.append(draft("c", "third")).unwrap(), Seq(3));
        assert_eq!(log.next_seq(), Seq(4));
    }

    #[test]
    fn a_reopened_log_is_the_same_log() {
        // The whole design rests on an address staying valid, which is not
        // worth much if it only holds within one process.
        let dir = tempfile::tempdir().unwrap();
        {
            let mut log = EventLog::open(dir.path()).unwrap();
            log.append(draft("plan", "read the file")).unwrap();
            log.append(draft("tool_result", "three call sites"))
                .unwrap();
        }

        let log = EventLog::open(dir.path()).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log.get(Seq(2)).unwrap().kind, "tool_result");
        assert_eq!(log.materialize(Seq(2)).unwrap(), "three call sites");
        assert_eq!(log.next_seq(), Seq(3), "and the next address continues");
    }

    #[test]
    fn an_unknown_address_is_an_error_rather_than_an_empty_answer() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::open(dir.path()).unwrap();
        log.append(draft("a", "only one")).unwrap();

        let err = log.get(Seq(9)).unwrap_err();
        assert!(matches!(err, ContextError::NoSuchSeq(Seq(9), 1)), "{err}");
        assert!(matches!(log.get(Seq(0)), Err(ContextError::NoSuchSeq(..))));
    }

    #[test]
    fn a_range_off_the_end_is_clamped_but_a_backwards_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::open(dir.path()).unwrap();
        for i in 0..5 {
            log.append(draft("k", &format!("event {i}"))).unwrap();
        }

        assert_eq!(log.range(Seq(3), Seq(99)).unwrap().len(), 3);
        assert_eq!(log.range(Seq(1), Seq(5)).unwrap().len(), 5);
        assert!(matches!(
            log.range(Seq(4), Seq(2)),
            Err(ContextError::BackwardRange(..))
        ));
    }

    #[test]
    fn a_large_payload_is_externalized_and_still_reads_back_whole() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::open(dir.path()).unwrap();
        log.set_store(log.store().clone().with_threshold(16));

        let big = "a tool result far too long to keep in the log".repeat(20);
        let seq = log.append(draft("tool_result", &big)).unwrap();

        assert!(
            !log.get(seq).unwrap().payload.is_inline(),
            "this is the case the store exists for"
        );
        assert_eq!(log.materialize(seq).unwrap(), big);
    }

    #[test]
    fn a_corrupt_line_is_reported_with_its_line_number() {
        // An interrupted append leaves a partial final line. Naming it beats
        // failing to open a log that is almost entirely intact.
        let dir = tempfile::tempdir().unwrap();
        {
            let mut log = EventLog::open(dir.path()).unwrap();
            log.append(draft("a", "good")).unwrap();
        }
        let path = dir.path().join("events.jsonl");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"seq\":2,\"partial\"\n").unwrap();

        let err = EventLog::open(dir.path()).unwrap_err();
        assert!(matches!(err, ContextError::Corrupt(2, _)), "got: {err}");
    }
}
