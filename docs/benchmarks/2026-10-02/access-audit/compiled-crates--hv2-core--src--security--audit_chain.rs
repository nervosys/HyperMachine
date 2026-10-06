//! Tamper-evident audit records.
//!
//! The audit logs elsewhere in HyperMachine -- the HTTP audit middleware and
//! the MCP tool-call log -- are in-process: a ring buffer, or a `tracing`
//! line. Anything that compromises the process, or can write the file a log
//! collector tails, can rewrite that history and leave nothing behind.
//!
//! An [`AuditChain`] makes rewriting detectable. Every record carries an
//! HMAC-SHA256, under an operator-held key, over its sequence number, its
//! timestamp, its source, its event and the previous record's MAC. [`verify`]
//! walks a log and reports the first record that does not follow from the one
//! before it, so an edited, deleted, inserted or reordered record is caught at
//! the point it happened.
//!
//! # What it cannot catch, and why forwarding matters
//!
//! Truncation at the tail. A chain that simply stops is indistinguishable from
//! one nothing more was written to, and anyone with the key can re-extend
//! one. The defence is a copy held somewhere the attacker cannot reach: point
//! a SIEM collector (Fluent Bit, Vector, the Splunk forwarder) at the file
//! [`JsonLinesFile`] writes, and the collector's copy is the tamper-evident
//! store. The chain then lets that store, or an auditor, prove that nothing
//! between two records it holds was altered.
//!
//! The key must live outside the audited process's reach to be worth much. A
//! key the attacker can read lets them write a fresh, valid chain.
//!
//! # Format
//!
//! One JSON object per line:
//! `{"seq":…,"timestamp_ms":…,"source":…,"event":{…},"prev":"<hex>","mac":"<hex>"}`.
//! The MAC is over [`MAC_DOMAIN`], `seq` and `timestamp_ms` as big-endian
//! `u64`s, the length-prefixed `source`, `prev`, and the event in canonical
//! JSON (object keys sorted at every depth). Canonical, so a verifier built
//! with different `serde_json` features computes the same bytes.
//!
//! A verifier in another language can reproduce the MAC from this description
//! alone. One was written in Python to check that, and it interoperates. It
//! must format numbers as `serde_json` does: integers plainly, non-integers
//! in their shortest round-trip form. Strings are JSON-escaped with non-ASCII
//! left as UTF-8.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroize;

/// Domain separator for every record's MAC.
pub const MAC_DOMAIN: &[u8] = b"HyperMachine audit chain v1\0";

/// The `prev` of the first record in a chain.
const GENESIS: [u8; 32] = [0; 32];

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Position in the chain, from 0.
    pub seq: u64,
    /// Milliseconds since the Unix epoch, as the writer's clock read it.
    pub timestamp_ms: u64,
    /// Which log produced it, e.g. `"http"` or `"mcp"`.
    pub source: String,
    /// The event itself.
    pub event: Value,
    /// The previous record's MAC, hex. Zeros for the first.
    pub prev: String,
    /// This record's MAC, hex.
    pub mac: String,
}

/// Where records go. A line at a time, already serialised.
pub trait AuditSink: Send {
    /// Write one record's line, without its trailing newline.
    fn write_line(&mut self, line: &str) -> io::Result<()>;
}

/// Appends records to a file, one JSON object per line.
///
/// Opened in append mode, so concurrent writers from other processes cannot
/// interleave within a line on a POSIX filesystem, and a collector tailing
/// the file sees whole records.
pub struct JsonLinesFile {
    file: File,
    sync: bool,
}

impl JsonLinesFile {
    /// Open `path` for appending, creating it if needed. With `sync`, every
    /// record is flushed to stable storage before `append` returns.
    pub fn open(path: &Path, sync: bool) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file, sync })
    }
}

impl AuditSink for JsonLinesFile {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        let mut buf = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        self.file.write_all(&buf)?;
        if self.sync {
            self.file.sync_data()?;
        }
        Ok(())
    }
}

struct ChainState {
    next_seq: u64,
    prev: [u8; 32],
    sink: Box<dyn AuditSink>,
}

/// An append-only, MAC-chained audit log.
pub struct AuditChain {
    key: [u8; 32],
    state: Mutex<ChainState>,
}

impl fmt::Debug for AuditChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the key.
        let state = self.state.lock();
        f.debug_struct("AuditChain")
            .field("next_seq", &state.next_seq)
            .finish_non_exhaustive()
    }
}

impl Drop for AuditChain {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// Why a log failed to verify.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    /// Line `line` (1-based) is not a record.
    #[error("line {line}: not an audit record: {reason}")]
    Malformed { line: usize, reason: String },
    /// The record at `line` has the wrong sequence number: one was deleted,
    /// inserted or moved before it.
    #[error("line {line}: expected seq {expected}, found {found}")]
    Sequence {
        line: usize,
        expected: u64,
        found: u64,
    },
    /// The record at `line` does not name the previous record's MAC.
    #[error("line {line}: does not follow the record before it")]
    Broken { line: usize },
    /// The record at `line` has been altered, or was written under another key.
    #[error("line {line}: MAC does not match its contents")]
    BadMac { line: usize },
}

impl AuditChain {
    /// Verify and resume a synced file while holding its exclusive OS lock.
    /// Each process must use a distinct file. A second writer is rejected.
    /// Corrupt, incomplete or oversized records are rejected before appending.
    pub fn open_exclusive_file(path: &Path, key: [u8; 32]) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;
        file.try_lock().map_err(io::Error::from)?;
        let mut reader = BufReader::new(&file);
        let mut prev = GENESIS;
        let mut next_seq = 0;
        loop {
            let mut bytes = Vec::new();
            reader
                .by_ref()
                .take(1_048_577)
                .read_until(b'\n', &mut bytes)?;
            if bytes.is_empty() {
                break;
            }
            if bytes.len() > 1_048_576 || bytes.last() != Some(&b'\n') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "incomplete or oversized audit record",
                ));
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "audit record is not UTF-8")
            })?;
            let record = verify_record(text, next_seq, &prev, &key)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            prev = decode_hex32(&record.mac).expect("verified MAC");
            next_seq = next_seq
                .checked_add(1)
                .ok_or_else(|| io::Error::other("audit sequence exhausted"))?;
        }
        drop(reader);
        // Sync metadata, including an empty newly created log, before use.
        file.sync_all()?;
        #[cfg(unix)]
        File::open(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .sync_all()?;
        Ok(Self {
            key,
            state: Mutex::new(ChainState {
                next_seq,
                prev,
                sink: Box::new(JsonLinesFile { file, sync: true }),
            }),
        })
    }

    /// A new chain, starting at sequence 0, writing to `sink`.
    pub fn new(key: [u8; 32], sink: Box<dyn AuditSink>) -> Self {
        Self {
            key,
            state: Mutex::new(ChainState {
                next_seq: 0,
                prev: GENESIS,
                sink,
            }),
        }
    }

    /// Continue the chain in the file at `path`, or start one if it is empty
    /// or absent.
    ///
    /// The existing file is verified first. A process restarting against a
    /// log someone edited refuses rather than extending it, which would bury
    /// the edit under records that verify.
    pub fn open_file(path: &Path, key: [u8; 32], sync: bool) -> io::Result<Self> {
        let mut next_seq = 0;
        let mut prev = GENESIS;
        if path.exists() {
            let file = File::open(path)?;
            let lines = BufReader::new(file)
                .lines()
                .collect::<io::Result<Vec<String>>>()?;
            let last = verify_lines(lines.iter().map(String::as_str), &key)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if let Some(last) = last {
                next_seq = last.seq + 1;
                prev = decode_hex32(&last.mac).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "last record's MAC is not hex")
                })?;
            }
        }
        let sink = JsonLinesFile::open(path, sync)?;
        Ok(Self {
            key,
            state: Mutex::new(ChainState {
                next_seq,
                prev,
                sink: Box::new(sink),
            }),
        })
    }

    /// The chain an operator asked for through the environment, if any.
    ///
    /// `HV2_AUDIT_CHAIN` names the log file, and `HV2_AUDIT_KEY_FILE` a file
    /// holding the 32-byte key as 64 hex characters (surrounding whitespace
    /// ignored). Records are synced to disk as they are written.
    ///
    /// `Ok(None)` when neither is set. An error when only one is, or when
    /// either cannot be used: an operator who asked for a tamper-evident log
    /// and is silently given none has been misled, so an embedder should
    /// refuse to start on `Err`.
    ///
    /// Nothing calls this for you. `McpConfig::default()` and
    /// `AuditLogConfig::default()` never read the environment; wire the result
    /// into their `audit_chain` / `chain` fields.
    pub fn from_env() -> io::Result<Option<std::sync::Arc<Self>>> {
        let path = std::env::var_os("HV2_AUDIT_CHAIN");
        let key_file = std::env::var_os("HV2_AUDIT_KEY_FILE");
        let (path, key_file) = match (path, key_file) {
            (None, None) => return Ok(None),
            (Some(p), Some(k)) => (p, k),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "HV2_AUDIT_CHAIN and HV2_AUDIT_KEY_FILE must be set together",
                ))
            }
        };
        let mut text = std::fs::read_to_string(&key_file)?;
        let key = decode_hex32(text.trim()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "HV2_AUDIT_KEY_FILE must hold exactly 64 hex characters",
            )
        });
        text.zeroize();
        Ok(Some(std::sync::Arc::new(Self::open_file(
            Path::new(&path),
            key?,
            true,
        )?)))
    }

    /// Append `event` from `source`, returning the record written.
    ///
    /// The sequence number is only consumed once the sink has the record, so
    /// a write that fails leaves no gap for a verifier to report.
    pub fn append(&self, source: &str, event: Value) -> io::Result<AuditRecord> {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let mut state = self.state.lock();
        let seq = state.next_seq;
        let mac = record_mac(&self.key, seq, timestamp_ms, source, &state.prev, &event);
        let record = AuditRecord {
            seq,
            timestamp_ms,
            source: source.to_string(),
            event,
            prev: hex(&state.prev),
            mac: hex(&mac),
        };
        let line = serde_json::to_string(&record).map_err(io::Error::other)?;
        state.sink.write_line(&line)?;
        state.next_seq = seq + 1;
        state.prev = mac;
        Ok(record)
    }
}

/// Verify a whole log, one record per line, under `key`.
///
/// Returns the number of records on success. Blank lines are not allowed: a
/// line that is not a record is either corruption or something written
/// between records, and neither should pass.
pub fn verify<'a>(
    lines: impl IntoIterator<Item = &'a str>,
    key: &[u8; 32],
) -> Result<u64, VerifyError> {
    Ok(verify_lines(lines, key)?.map_or(0, |last| last.seq + 1))
}

fn verify_lines<'a>(
    lines: impl IntoIterator<Item = &'a str>,
    key: &[u8; 32],
) -> Result<Option<AuditRecord>, VerifyError> {
    let mut prev = GENESIS;
    let mut last = None;
    for (index, line) in lines.into_iter().enumerate() {
        let expected_seq = u64::try_from(index).unwrap_or(u64::MAX);
        let record = verify_record(line, expected_seq, &prev, key)?;
        prev = decode_hex32(&record.mac).expect("verified MAC");
        last = Some(record);
    }
    Ok(last)
}

fn verify_record(
    line: &str,
    expected_seq: u64,
    prev: &[u8; 32],
    key: &[u8; 32],
) -> Result<AuditRecord, VerifyError> {
    let line_no = usize::try_from(expected_seq)
        .unwrap_or(usize::MAX)
        .saturating_add(1);
    let record: AuditRecord = serde_json::from_str(line).map_err(|e| VerifyError::Malformed {
        line: line_no,
        reason: e.to_string(),
    })?;
    if record.seq != expected_seq {
        return Err(VerifyError::Sequence {
            line: line_no,
            expected: expected_seq,
            found: record.seq,
        });
    }
    let claimed_prev = decode_hex32(&record.prev).ok_or(VerifyError::Malformed {
        line: line_no,
        reason: "prev is not 32 hex bytes".into(),
    })?;
    if !ic_core::ct::verify(&claimed_prev, prev) {
        return Err(VerifyError::Broken { line: line_no });
    }
    let claimed_mac = decode_hex32(&record.mac).ok_or(VerifyError::Malformed {
        line: line_no,
        reason: "mac is not 32 hex bytes".into(),
    })?;
    let mac = record_mac(
        key,
        record.seq,
        record.timestamp_ms,
        &record.source,
        &claimed_prev,
        &record.event,
    );
    if !ic_core::ct::verify(&mac, &claimed_mac) {
        return Err(VerifyError::BadMac { line: line_no });
    }
    Ok(record)
}

fn record_mac(
    key: &[u8; 32],
    seq: u64,
    timestamp_ms: u64,
    source: &str,
    prev: &[u8; 32],
    event: &Value,
) -> [u8; 32] {
    use ic_core::traits::Mac;
    let mut message = Vec::with_capacity(128);
    message.extend_from_slice(MAC_DOMAIN);
    message.extend_from_slice(&seq.to_be_bytes());
    message.extend_from_slice(&timestamp_ms.to_be_bytes());
    // Length-prefixed, so no choice of source can be mistaken for the bytes
    // that follow it.
    message.extend_from_slice(
        &u32::try_from(source.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    message.extend_from_slice(source.as_bytes());
    message.extend_from_slice(prev);
    canonical_json(event, &mut message);
    ic_mac::Hmac::<ic_hash::Sha256>::mac(key, &message)
        // A 32-byte key is always a valid HMAC key; nothing to recover from.
        .expect("HMAC-SHA256 accepts a 32-byte key")
}

/// JSON with object keys sorted at every depth, so the bytes MACed do not
/// depend on how the event was built or which `serde_json` features are on.
fn canonical_json(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                // A string's JSON form: serde_json escapes it the one way.
                out.extend_from_slice(Value::String(key.clone()).to_string().as_bytes());
                out.push(b':');
                canonical_json(&map[key], out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                canonical_json(item, out);
            }
            out.push(b']');
        }
        scalar => out.extend_from_slice(scalar.to_string().as_bytes()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    const KEY: [u8; 32] = [7; 32];

    /// A sink that keeps lines in memory, shared with the test.
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<String>>>);

    impl AuditSink for Lines {
        fn write_line(&mut self, line: &str) -> io::Result<()> {
            self.0.lock().push(line.to_string());
            Ok(())
        }
    }

    fn chain_of(n: usize) -> Vec<String> {
        let lines = Lines::default();
        let chain = AuditChain::new(KEY, Box::new(lines.clone()));
        for i in 0..n {
            chain
                .append(
                    "http",
                    json!({"method": "POST", "path": format!("/vms/{i}"), "n": i}),
                )
                .unwrap();
        }
        let out = lines.0.lock().clone();
        out
    }

    fn check(lines: &[String]) -> Result<u64, VerifyError> {
        verify(lines.iter().map(String::as_str), &KEY)
    }

    #[test]
    fn an_untouched_chain_verifies() {
        assert_eq!(check(&chain_of(5)), Ok(5));
        assert_eq!(check(&[]), Ok(0));
    }

    #[test]
    fn fractional_request_timings_verify_after_json_round_trip() {
        let lines = Lines::default();
        let chain = AuditChain::new(KEY, Box::new(lines.clone()));
        for nanos in (1..100_000).step_by(97) {
            chain
                .append("http", json!({"elapsed_ms": nanos as f64 / 1_000_000.0}))
                .unwrap();
        }
        let records = lines.0.lock();
        assert_eq!(check(&records), Ok(records.len() as u64));
    }

    #[test]
    fn an_edited_event_is_caught_where_it_was_edited() {
        let mut lines = chain_of(5);
        lines[2] = lines[2].replace("/vms/2", "/vms/9");
        assert_eq!(check(&lines), Err(VerifyError::BadMac { line: 3 }));
    }

    #[test]
    fn a_deleted_record_is_caught() {
        let mut lines = chain_of(5);
        lines.remove(2);
        assert_eq!(
            check(&lines),
            Err(VerifyError::Sequence {
                line: 3,
                expected: 2,
                found: 3
            })
        );
    }

    #[test]
    fn swapped_records_are_caught() {
        let mut lines = chain_of(5);
        lines.swap(1, 2);
        assert!(check(&lines).is_err());
    }

    /// Renumbering after a deletion does not help: the next record names the
    /// deleted one's MAC.
    #[test]
    fn a_deletion_disguised_by_renumbering_is_caught() {
        let mut lines = chain_of(4);
        lines.remove(1);
        let mut renumbered: AuditRecord = serde_json::from_str(&lines[1]).unwrap();
        renumbered.seq = 1;
        lines[1] = serde_json::to_string(&renumbered).unwrap();
        assert_eq!(check(&lines), Err(VerifyError::Broken { line: 2 }));
    }

    /// Splicing: replace a record with the one at the same position in
    /// *another* chain under the same key -- a rotated log, an earlier run --
    /// and point its `prev` at the record now before it. Its `seq` is genuine
    /// and its MAC was genuinely computed, so only `prev` being inside the MAC
    /// catches this. A chain that compared `prev` fields without
    /// authenticating them would pass it.
    #[test]
    fn a_record_spliced_in_from_another_chain_is_caught() {
        let mut lines = chain_of(4);
        let other = {
            let sink = Lines::default();
            let chain = AuditChain::new(KEY, Box::new(sink.clone()));
            for i in 0..4 {
                chain
                    .append("http", json!({"path": "/elsewhere", "n": i}))
                    .unwrap();
            }
            let out = sink.0.lock().clone();
            out
        };
        let before: AuditRecord = serde_json::from_str(&lines[1]).unwrap();
        let mut spliced: AuditRecord = serde_json::from_str(&other[2]).unwrap();
        spliced.prev = before.mac.clone();
        lines[2] = serde_json::to_string(&spliced).unwrap();
        assert_eq!(check(&lines), Err(VerifyError::BadMac { line: 3 }));
    }

    #[test]
    fn a_chain_written_under_another_key_does_not_verify() {
        let lines = chain_of(3);
        assert_eq!(
            verify(lines.iter().map(String::as_str), &[8; 32]),
            Err(VerifyError::BadMac { line: 1 })
        );
    }

    /// The MAC is over canonical JSON, so key order in the stored line does
    /// not matter -- and floats, unicode and nesting survive the round trip.
    #[test]
    fn key_order_and_value_encoding_do_not_break_verification() {
        let lines = Lines::default();
        let chain = AuditChain::new(KEY, Box::new(lines.clone()));
        chain
            .append(
                "mcp",
                json!({"z": 1.5, "a": {"y": [1, "é", null], "b": true}, "m": -3}),
            )
            .unwrap();
        let line = lines.0.lock()[0].clone();
        // Re-serialise the event with its keys in a different order.
        let mut record: Value = serde_json::from_str(&line).unwrap();
        let event = record["event"].take();
        let mut reordered = serde_json::Map::new();
        for key in ["m", "a", "z"] {
            reordered.insert(key.to_string(), event[key].clone());
        }
        record["event"] = Value::Object(reordered);
        let rewritten = record.to_string();
        assert_eq!(check(&[rewritten]), Ok(1));
    }

    /// `from_env`: nothing asked for is `None`, half a request is an error,
    /// an unusable key is an error, and a whole one opens a chain.
    ///
    /// One test, because the environment is process-wide: split across tests
    /// running in parallel, the cases would race each other.
    #[test]
    fn from_env_opens_only_what_was_asked_for_and_refuses_half_a_request() {
        let dir = std::env::temp_dir().join(format!("hv2-audit-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("audit.jsonl");
        let key_file = dir.join("key.hex");
        let _ = std::fs::remove_file(&log);

        std::env::remove_var("HV2_AUDIT_CHAIN");
        std::env::remove_var("HV2_AUDIT_KEY_FILE");
        assert!(AuditChain::from_env().unwrap().is_none());

        std::env::set_var("HV2_AUDIT_CHAIN", &log);
        assert_eq!(
            AuditChain::from_env().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );

        std::fs::write(&key_file, "not hex").unwrap();
        std::env::set_var("HV2_AUDIT_KEY_FILE", &key_file);
        assert_eq!(
            AuditChain::from_env().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        std::fs::write(&key_file, format!("{}\n", "ab".repeat(32))).unwrap();
        let chain = AuditChain::from_env().unwrap().expect("a chain");
        chain.append("env", json!({"ok": true})).unwrap();
        drop(chain);
        let text = std::fs::read_to_string(&log).unwrap();
        assert_eq!(verify(text.lines(), &[0xab; 32]), Ok(1));

        std::env::remove_var("HV2_AUDIT_CHAIN");
        std::env::remove_var("HV2_AUDIT_KEY_FILE");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_chain_resumes_across_restarts_and_refuses_an_edited_one() {
        let dir = std::env::temp_dir().join(format!("hv2-audit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.jsonl");
        let _ = std::fs::remove_file(&path);

        AuditChain::open_file(&path, KEY, false)
            .unwrap()
            .append("http", json!({"n": 0}))
            .unwrap();
        // A second process picks up where the first stopped.
        let resumed = AuditChain::open_file(&path, KEY, false).unwrap();
        assert_eq!(resumed.append("http", json!({"n": 1})).unwrap().seq, 1);
        drop(resumed);

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(verify(text.lines(), &KEY), Ok(2));

        // Someone edits the log; the next process refuses to extend it.
        std::fs::write(&path, text.replace("\"n\":0", "\"n\":5")).unwrap();
        let err = AuditChain::open_file(&path, KEY, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exclusive_file_rejects_another_writer_and_resumes_after_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let first = AuditChain::open_exclusive_file(&path, KEY).unwrap();
        first.append("http", json!({"n":0})).unwrap();
        assert!(AuditChain::open_exclusive_file(&path, KEY).is_err());
        drop(first);
        let second = AuditChain::open_exclusive_file(&path, KEY).unwrap();
        assert_eq!(second.append("http", json!({"n":1})).unwrap().seq, 1);
        drop(second);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(verify(text.lines(), &KEY), Ok(2));
        std::fs::write(&path, text.replace("\"n\":0", "\"n\":3")).unwrap();
        assert!(AuditChain::open_exclusive_file(&path, KEY).is_err());
    }

    #[test]
    fn exclusive_file_refuses_partial_and_oversized_records_without_extending_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let chain = AuditChain::open_exclusive_file(&path, KEY).unwrap();
        chain.append("http", json!({"n":0})).unwrap();
        drop(chain);
        let text = std::fs::read_to_string(&path).unwrap();
        for damaged in [
            text.trim_end().to_owned(),
            format!("{text}{{"),
            "a".repeat(1_048_577),
        ] {
            std::fs::write(&path, &damaged).unwrap();
            assert!(AuditChain::open_exclusive_file(&path, KEY).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), damaged);
        }
    }
}
