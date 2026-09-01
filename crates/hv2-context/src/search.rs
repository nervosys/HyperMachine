//! Finding an address again.
//!
//! An agent that cannot locate an old event cannot use the log, and the whole
//! arrangement collapses back to "put it all in the prompt". Search is what
//! makes eviction affordable, so it is ranked rather than substring-matched:
//! a substring search over a long session returns the fifty places a word
//! appears, in no useful order, which is the same as returning nothing.
//!
//! # Indexed at write time, read at read time
//!
//! Terms are counted when an event is appended, while its full text is in
//! hand. That matters for externalized payloads: index them from the preview
//! and a 40 MB tool result becomes findable only by its first 240 bytes. The
//! text itself is not kept here -- only the counts -- so the index stays small
//! and [`crate::EventLog::materialize`] remains the only way to get content
//! back.

use std::collections::HashMap;

use crate::log::{Event, Role, Seq};
use crate::payload::{truncate_on_char_boundary, PREVIEW_BYTES};

/// BM25 term-frequency saturation. The conventional value; above it, repeated
/// terms keep adding score long past the point of meaning anything.
const K1: f64 = 1.2;

/// BM25 length normalization. Also conventional: 0 ignores length entirely,
/// 1 divides it out completely, and neither is right for a log holding both
/// one-line notes and long tool output.
const B: f64 = 0.75;

/// What a search matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// Where the event lives, which is what a caller does something with.
    pub seq: Seq,
    /// BM25 score. Comparable within one result set and not across them.
    pub score: f64,
    /// The event's kind, so a caller can tell hits apart without expanding.
    pub kind: String,
    /// Who produced it.
    pub role: Role,
    /// Which session it came from.
    pub session: String,
    /// When it happened.
    pub timestamp_ms: u64,
    /// The opening of the content -- the preview for an externalized payload.
    pub preview: String,
}

/// Which events a search is allowed to return.
///
/// Every field is an `and`, and every unset field is "no restriction". A
/// filter that matches nothing returns nothing rather than falling back to
/// matching everything, which is the failure mode that makes a scoped search
/// silently unscoped.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Restrict to one session.
    pub session: Option<String>,
    /// Restrict to one kind.
    pub kind: Option<String>,
    /// Restrict to one role.
    pub role: Option<Role>,
    /// Earliest timestamp, inclusive.
    pub since_ms: Option<u64>,
    /// Latest timestamp, inclusive.
    pub until_ms: Option<u64>,
}

impl Filter {
    /// Restrict to one session.
    #[must_use]
    pub fn session(mut self, session: impl Into<String>) -> Self {
        self.session = Some(session.into());
        self
    }

    /// Restrict to one kind.
    #[must_use]
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Restrict to one role.
    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.role = Some(role);
        self
    }

    /// Restrict to a closed time range.
    #[must_use]
    pub fn between(mut self, since_ms: u64, until_ms: u64) -> Self {
        self.since_ms = Some(since_ms);
        self.until_ms = Some(until_ms);
        self
    }

    fn admits(&self, doc: &Doc) -> bool {
        if let Some(ref session) = self.session {
            if &doc.session != session {
                return false;
            }
        }
        if let Some(ref kind) = self.kind {
            if &doc.kind != kind {
                return false;
            }
        }
        if let Some(role) = self.role {
            if doc.role != role {
                return false;
            }
        }
        if let Some(since) = self.since_ms {
            if doc.timestamp_ms < since {
                return false;
            }
        }
        if let Some(until) = self.until_ms {
            if doc.timestamp_ms > until {
                return false;
            }
        }
        true
    }
}

/// One indexed event: everything a hit needs, and none of the content.
#[derive(Debug, Clone)]
struct Doc {
    seq: Seq,
    session: String,
    kind: String,
    role: Role,
    timestamp_ms: u64,
    preview: String,
    length: u32,
}

/// A BM25 index over the log.
#[derive(Debug, Default)]
pub struct SearchIndex {
    docs: Vec<Doc>,
    /// term -> (document position, term frequency)
    postings: HashMap<String, Vec<(u32, u32)>>,
    total_length: u64,
}

impl SearchIndex {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many events are indexed.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Index one event.
    ///
    /// `full_text` is the whole content, which for an externalized payload is
    /// not what the event carries. Passing the preview instead is not an
    /// error the type system can catch and makes large results findable only
    /// by their opening lines, so callers should pass what they stored.
    pub fn index(&mut self, event: &Event, full_text: &str) {
        let position = self.docs.len() as u32;
        let mut counts: HashMap<String, u32> = HashMap::new();
        let mut length = 0u32;

        for term in tokenize(&event.kind).chain(tokenize(full_text)) {
            *counts.entry(term).or_insert(0) += 1;
            length += 1;
        }

        for (term, tf) in counts {
            self.postings.entry(term).or_default().push((position, tf));
        }

        self.total_length += u64::from(length);
        self.docs.push(Doc {
            seq: event.seq,
            session: event.session.clone(),
            kind: event.kind.clone(),
            role: event.role,
            timestamp_ms: event.timestamp_ms,
            // Bounded here as well as in the store. An inline payload keeps
            // its whole content in the log, so echoing `visible_text` would
            // hand back the entire payload for every hit -- filling the
            // context with the thing search exists to avoid reading.
            preview: truncate_on_char_boundary(event.payload.visible_text(), PREVIEW_BYTES),
            length,
        });
    }

    /// The `k` best matches for `query`, best first.
    ///
    /// An empty query returns nothing. It would otherwise return the `k`
    /// shortest documents, since every one of them scores zero and the tie
    /// break is arbitrary -- an answer with no relationship to the question.
    pub fn search(&self, query: &str, k: usize, filter: &Filter) -> Vec<Hit> {
        let terms: Vec<String> = tokenize(query).collect();
        if terms.is_empty() || self.docs.is_empty() || k == 0 {
            return Vec::new();
        }

        let doc_count = self.docs.len() as f64;
        let average_length = self.total_length as f64 / doc_count;
        let mut scores: HashMap<u32, f64> = HashMap::new();

        for term in &terms {
            let Some(postings) = self.postings.get(term) else {
                continue;
            };
            // Document frequency is counted over the whole index rather than
            // the filtered subset: a term's rarity is a property of the
            // corpus, and recomputing it per filter would make the same
            // document score differently depending on what else was asked for.
            let df = postings.len() as f64;
            let idf = ((doc_count - df + 0.5) / (df + 0.5) + 1.0).ln();

            for &(position, tf) in postings {
                let doc = &self.docs[position as usize];
                if !filter.admits(doc) {
                    continue;
                }
                let tf = f64::from(tf);
                let norm = 1.0 - B + B * f64::from(doc.length) / average_length;
                *scores.entry(position).or_insert(0.0) +=
                    idf * (tf * (K1 + 1.0)) / (tf + K1 * norm);
            }
        }

        let mut hits: Vec<Hit> = scores
            .into_iter()
            .map(|(position, score)| {
                let doc = &self.docs[position as usize];
                Hit {
                    seq: doc.seq,
                    score,
                    kind: doc.kind.clone(),
                    role: doc.role,
                    session: doc.session.clone(),
                    timestamp_ms: doc.timestamp_ms,
                    preview: doc.preview.clone(),
                }
            })
            .collect();

        // Ties break towards the newer event: with two equally good matches,
        // the later one is the one that reflects what the session has since
        // learned.
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.seq.cmp(&a.seq))
        });
        hits.truncate(k);
        hits
    }
}

/// Split text into lowercase alphanumeric terms.
///
/// Single characters are dropped: they match nearly everything and rank
/// nothing. Deliberately not stemmed -- an agent searching its own log is
/// usually looking for an identifier it can spell exactly, and stemming turns
/// `parses` and `parser` into the same term.
fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|term| term.len() > 1)
        .map(str::to_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::Payload;

    fn event(seq: u64, kind: &str, text: &str) -> Event {
        Event {
            seq: Seq(seq),
            session: "s1".into(),
            role: Role::Tool,
            kind: kind.into(),
            timestamp_ms: seq * 1000,
            payload: Payload::Inline { text: text.into() },
        }
    }

    fn index_of(events: &[Event]) -> SearchIndex {
        let mut index = SearchIndex::new();
        for event in events {
            index.index(event, event.payload.visible_text());
        }
        index
    }

    #[test]
    fn the_best_match_comes_first() {
        let events = [
            event(1, "note", "the vsock device carries the connection state"),
            event(2, "note", "nothing here is about devices at all"),
            event(3, "note", "vsock vsock vsock, the device under test"),
        ];
        let hits = index_of(&events).search("vsock device", 3, &Filter::default());

        assert_eq!(hits[0].seq, Seq(3), "got: {hits:?}");
        assert!(hits.iter().all(|h| h.seq != Seq(2)), "got: {hits:?}");
    }

    #[test]
    fn a_rare_term_outweighs_a_common_one() {
        // Every event mentions the file; one mentions the symbol. A search for
        // both should find the one, which is what IDF is for.
        let mut events: Vec<Event> = (1..=8)
            .map(|i| event(i, "note", "changes in kvm.rs today"))
            .collect();
        events.push(event(9, "note", "kvm.rs: apply_supported_cpuid added"));

        let hits = index_of(&events).search("kvm apply_supported_cpuid", 3, &Filter::default());
        assert_eq!(hits[0].seq, Seq(9), "got: {hits:?}");
    }

    #[test]
    fn a_filter_that_matches_nothing_returns_nothing() {
        // The failure this guards against is a scoped search quietly running
        // unscoped, which returns plausible results from the wrong session.
        let events = [event(1, "note", "the answer is here")];
        let hits = index_of(&events).search(
            "answer",
            10,
            &Filter::default().session("a-different-session"),
        );
        assert!(hits.is_empty(), "got: {hits:?}");
    }

    #[test]
    fn filters_compose_as_and() {
        let mut a = event(1, "tool_result", "matching text");
        a.session = "s1".into();
        let mut b = event(2, "plan", "matching text");
        b.session = "s1".into();
        let mut c = event(3, "tool_result", "matching text");
        c.session = "s2".into();

        let index = index_of(&[a, b, c]);
        let hits = index.search(
            "matching",
            10,
            &Filter::default().session("s1").kind("tool_result"),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].seq, Seq(1));
    }

    #[test]
    fn a_time_range_excludes_both_ends_correctly() {
        let events: Vec<Event> = (1..=5).map(|i| event(i, "note", "same text")).collect();
        let index = index_of(&events);

        // Timestamps are seq * 1000, and the range is inclusive.
        let hits = index.search("same", 10, &Filter::default().between(2000, 4000));
        let mut seqs: Vec<u64> = hits.iter().map(|h| h.seq.0).collect();
        seqs.sort_unstable();
        assert_eq!(seqs, vec![2, 3, 4]);
    }

    #[test]
    fn an_empty_query_returns_nothing_rather_than_anything() {
        let events = [event(1, "note", "content")];
        assert!(index_of(&events)
            .search("", 5, &Filter::default())
            .is_empty());
        assert!(index_of(&events)
            .search("   ", 5, &Filter::default())
            .is_empty());
    }

    #[test]
    fn a_hit_carries_a_preview_and_never_a_payload() {
        // Small payloads stay inline, so their whole content is in the log.
        // A hit that echoed it would make searching a long session as
        // expensive as re-reading it.
        let long = "findable ".repeat(2000);
        let events = [event(1, "note", &long)];
        let hits = index_of(&events).search("findable", 1, &Filter::default());
        assert_eq!(
            hits[0].preview.len(),
            PREVIEW_BYTES,
            "got: {}",
            hits[0].preview.len()
        );
    }

    #[test]
    fn the_kind_is_searchable_as_well_as_the_content() {
        // An agent looking for "what did the commit step say" knows the kind
        // and not the words.
        let events = [
            event(1, "commit", "message body"),
            event(2, "note", "unrelated"),
        ];
        let hits = index_of(&events).search("commit", 5, &Filter::default());
        assert_eq!(hits[0].seq, Seq(1), "got: {hits:?}");
    }
}
