//! What the model actually sees, and how it is kept inside a budget.
//!
//! The working view is the only part of this crate that is allowed to forget,
//! and it can only do so once what it is forgetting is safely in the log. That
//! ordering is the whole of the safety argument, so [`WorkingView::evict`]
//! persists before it selects, every time, rather than trusting a caller to
//! have done it.

use crate::index::{EvictionIndex, Headline};
use crate::log::{Event, EventDraft, EventLog, Role, Seq};
use crate::Result;

/// How many entries at the end of the view are never evicted.
///
/// Without a tail the view can evict its way to the immediately preceding
/// turn, which reads to the model as amnesia mid-conversation -- the one thing
/// no amount of searching recovers gracefully, because it does not know it
/// needs to search.
pub const DEFAULT_TAIL: usize = 6;

/// Characters per token, for estimating what a view costs.
///
/// An estimate and named as one. Real tokenization depends on the model, and
/// a budget enforced against a number that is 15% off is still a budget; a
/// budget enforced against nothing is not.
pub const CHARS_PER_TOKEN: usize = 4;

/// The size a view is kept under.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    /// The context window, in tokens.
    pub capacity: usize,
    /// Fraction of it the view may occupy before eviction runs.
    pub ratio: f64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            capacity: 128_000,
            ratio: 0.6,
        }
    }
}

impl Budget {
    /// A budget of `capacity` tokens with eviction at `ratio` of it.
    pub fn new(capacity: usize, ratio: f64) -> Self {
        Self {
            capacity,
            ratio: ratio.clamp(0.05, 1.0),
        }
    }

    /// The point at which eviction runs.
    pub fn threshold(&self) -> usize {
        (self.capacity as f64 * self.ratio) as usize
    }

    /// Rough token cost of a string.
    pub fn estimate_tokens(text: &str) -> usize {
        text.len().div_ceil(CHARS_PER_TOKEN)
    }
}

/// One item in the view.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewEntry {
    /// Where this lives in the log, once it has been persisted.
    ///
    /// `None` means it is live and not yet addressable, which is precisely
    /// why it cannot be evicted yet.
    pub seq: Option<Seq>,
    /// Who produced it.
    pub role: Role,
    /// Its kind, carried through to the log.
    pub kind: String,
    /// What the model sees. Replaced by a pointer when folded.
    pub text: String,
    /// The full content, kept until persisted so the log gets the whole thing
    /// even after folding shortens what is displayed.
    stored_text: String,
    /// Whether the payload has been replaced by its address.
    pub folded: bool,
    /// Part of the turn in progress, and so never evicted.
    pub active: bool,
}

impl ViewEntry {
    /// An entry that has not been persisted yet.
    pub fn new(role: Role, kind: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            seq: None,
            role,
            kind: kind.into(),
            stored_text: text.clone(),
            text,
            folded: false,
            active: false,
        }
    }

    /// The same entry marked as part of the turn in progress.
    #[must_use]
    pub fn active(mut self) -> Self {
        self.active = true;
        self
    }

    /// What this entry costs against the budget.
    pub fn cost(&self) -> usize {
        Budget::estimate_tokens(&self.text)
    }

    /// Replace the payload with its address.
    ///
    /// Folding is not eviction: the entry stays in the view, in order, saying
    /// what it was and where to read it. That distinction is what lets a model
    /// notice a large result it now wants and go get it, instead of not
    /// knowing it ever happened.
    fn fold(&mut self) {
        let Some(seq) = self.seq else {
            return;
        };
        if self.folded {
            return;
        }
        let bytes = self.stored_text.len();
        self.text = format!(
            "[{} {} folded, {bytes} bytes -- expand {seq}]",
            self.kind, seq
        );
        self.folded = true;
    }
}

/// What one call to [`WorkingView::evict`] did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Eviction {
    /// Entries whose payloads were replaced by pointers.
    pub folded: usize,
    /// Entries removed from the view entirely.
    pub evicted: usize,
    /// The span that left, if anything did.
    pub span: Option<(Seq, Seq)>,
    /// Estimated tokens the view holds afterwards.
    pub cost_after: usize,
    /// Whether the view is under its threshold afterwards.
    ///
    /// `false` is a real outcome, not a bug: a view whose protected tail alone
    /// exceeds the budget cannot be brought under it, and saying so beats
    /// evicting the tail and pretending.
    pub within_budget: bool,
}

/// The bounded view, and the index of everything that has left it.
#[derive(Debug)]
pub struct WorkingView {
    entries: Vec<ViewEntry>,
    index: EvictionIndex,
    budget: Budget,
    tail: usize,
    session: String,
}

impl WorkingView {
    /// A view for `session` under `budget`.
    pub fn new(session: impl Into<String>, budget: Budget) -> Self {
        Self {
            entries: Vec::new(),
            index: EvictionIndex::default(),
            budget,
            tail: DEFAULT_TAIL,
            session: session.into(),
        }
    }

    /// The same view protecting a different number of trailing entries.
    #[must_use]
    pub fn with_tail(mut self, tail: usize) -> Self {
        self.tail = tail;
        self
    }

    /// Add an entry to the end of the view.
    pub fn push(&mut self, entry: ViewEntry) {
        self.entries.push(entry);
    }

    /// The entries currently visible, oldest first.
    pub fn entries(&self) -> &[ViewEntry] {
        &self.entries
    }

    /// The index of everything that has been evicted.
    pub fn index(&self) -> &EvictionIndex {
        &self.index
    }

    /// Estimated tokens the view currently holds.
    pub fn cost(&self) -> usize {
        self.entries.iter().map(ViewEntry::cost).sum()
    }

    /// Whether the view is over the point at which eviction runs.
    pub fn over_budget(&self) -> bool {
        self.cost() > self.budget.threshold()
    }

    /// Write every unpersisted entry to the log, in order.
    ///
    /// Returns how many were written. Called by [`Self::evict`] before it
    /// removes anything, which is the only reason eviction is safe.
    ///
    /// # Errors
    ///
    /// Propagates any [`crate::ContextError`] from the log. A failure here
    /// leaves the view untouched, so nothing is dropped on the strength of a
    /// write that did not happen.
    pub fn persist(&mut self, log: &mut EventLog) -> Result<usize> {
        let mut written = 0;
        for entry in &mut self.entries {
            if entry.seq.is_some() {
                continue;
            }
            let seq = log.append(EventDraft::new(
                self.session.clone(),
                entry.role,
                entry.kind.clone(),
                entry.stored_text.clone(),
            ))?;
            entry.seq = Some(seq);
            written += 1;
        }
        Ok(written)
    }

    /// Bring the view back under budget, if it can be.
    ///
    /// The order is fixed and each step is cheaper than the next is
    /// destructive:
    ///
    /// 1. persist everything, so every entry has an address;
    /// 2. protect the active turn, the recent tail, and the newest tool
    ///    result;
    /// 3. fold unprotected tool payloads down to their addresses;
    /// 4. if that was not enough, evict the oldest unprotected span, leaving
    ///    `headline` behind in the index.
    ///
    /// `headline` is called with the span about to leave and returns what to
    /// record about it. It is a callback because the useful version of this
    /// text is written by whatever is driving the session -- the model, in
    /// the arrangement this crate is built for -- and a summary generated in
    /// here would be a mechanical restatement of the first line.
    ///
    /// # Errors
    ///
    /// Propagates any [`crate::ContextError`] raised while persisting.
    pub fn evict(
        &mut self,
        log: &mut EventLog,
        mut headline: impl FnMut(&[ViewEntry]) -> Headline,
    ) -> Result<Eviction> {
        let mut report = Eviction::default();

        // 1. Nothing can leave the view until it is in the log.
        self.persist(log)?;

        if !self.over_budget() {
            report.cost_after = self.cost();
            report.within_budget = true;
            return Ok(report);
        }

        // 2. Everything from here down is unprotected.
        let protected_from = self.protected_from();

        // 3. Fold before evicting: a folded entry keeps its place in the
        //    order and stays one expand away, which is strictly less lossy
        //    than removing it.
        for entry in &mut self.entries[..protected_from] {
            if entry.role == Role::Tool && !entry.folded {
                entry.fold();
                report.folded += 1;
            }
        }

        if !self.over_budget() {
            report.cost_after = self.cost();
            report.within_budget = true;
            return Ok(report);
        }

        // 4. Evict the oldest span, up to but never into the protected
        //    region, and only as much of it as the budget requires.
        let mut end = 0;
        while end < protected_from
            && self.cost_of(end..self.entries.len()) > self.budget.threshold()
        {
            end += 1;
        }

        if end > 0 {
            let span: Vec<ViewEntry> = self.entries.drain(..end).collect();
            let from = span.first().and_then(|e| e.seq).unwrap_or_default();
            let to = span.last().and_then(|e| e.seq).unwrap_or(from);
            self.index.push(headline(&span));
            report.evicted = span.len();
            report.span = Some((from, to));
        }

        report.cost_after = self.cost();
        report.within_budget = !self.over_budget();
        Ok(report)
    }

    /// Index of the first protected entry.
    fn protected_from(&self) -> usize {
        let len = self.entries.len();
        let mut boundary = len.saturating_sub(self.tail);

        // The active turn is protected wherever it starts, which may be
        // earlier than the tail.
        if let Some(first_active) = self.entries.iter().position(|entry| entry.active) {
            boundary = boundary.min(first_active);
        }

        // The newest tool result is usually what the next turn is about, so it
        // is protected past the tail -- but only while it is still recent.
        // Protecting it unconditionally means one tool result at the head of a
        // long view pins every entry behind it and eviction silently stops
        // doing anything, which is worse than evicting the wrong thing because
        // nothing reports it. A result in the older half of the view is not
        // what the session is working on any more.
        if let Some(newest_tool) = self
            .entries
            .iter()
            .rposition(|entry| entry.role == Role::Tool)
        {
            if newest_tool * 2 >= len {
                boundary = boundary.min(newest_tool);
            }
        }

        boundary
    }

    fn cost_of(&self, range: std::ops::Range<usize>) -> usize {
        self.entries[range].iter().map(ViewEntry::cost).sum()
    }

    /// Bring an evicted span back into view, newest-last.
    ///
    /// This is what makes eviction reversible in practice rather than only in
    /// principle: the model finds an address in the index or by search, and
    /// asks for it back.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::ContextError::NoSuchSeq`] for an address the log
    /// does not hold, and [`crate::ContextError::LostPayload`] if the bytes
    /// behind an externalized payload are gone.
    pub fn expand(&mut self, log: &EventLog, lo: Seq, hi: Seq) -> Result<Vec<Event>> {
        let events = log.range(lo, hi)?.to_vec();
        for event in &events {
            let text = log.store().materialize(&event.payload)?;
            let mut entry = ViewEntry::new(event.role, event.kind.clone(), text);
            entry.seq = Some(event.seq);
            self.entries.push(entry);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Status;

    fn log() -> (tempfile::TempDir, EventLog) {
        let dir = tempfile::tempdir().unwrap();
        let log = EventLog::open(dir.path()).unwrap();
        (dir, log)
    }

    fn headline_for(span: &[ViewEntry]) -> Headline {
        let from = span.first().and_then(|e| e.seq).unwrap_or_default();
        let to = span.last().and_then(|e| e.seq).unwrap_or_default();
        Headline::new(
            "a span of work",
            "checked",
            "carry on",
            Status::Done,
            from,
            to,
        )
    }

    fn filler(n: usize) -> String {
        "word ".repeat(n)
    }

    #[test]
    fn nothing_is_evicted_before_it_is_addressable() {
        // The ordering the safety argument depends on. If eviction could
        // outrun persistence, the record would have a hole in it exactly
        // where the view stopped being able to answer.
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(100, 0.5)).with_tail(1);
        for i in 0..20 {
            view.push(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("{i} {}", filler(20)),
            ));
        }

        let report = view.evict(&mut log, headline_for).unwrap();
        assert!(report.evicted > 0, "got: {report:?}");
        assert_eq!(
            log.len(),
            20,
            "every entry must be in the log, including the ones that left"
        );
        for entry in view.entries() {
            assert!(entry.seq.is_some());
        }
    }

    #[test]
    fn an_evicted_span_can_be_read_back_word_for_word() {
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(100, 0.5)).with_tail(2);
        for i in 0..20 {
            view.push(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("entry {i} {}", filler(20)),
            ));
        }

        let report = view.evict(&mut log, headline_for).unwrap();
        let (from, to) = report.span.expect("something left the view");

        let recovered = log.range(from, to).unwrap();
        assert_eq!(recovered.len(), report.evicted);
        assert!(
            log.materialize(from).unwrap().starts_with("entry 0"),
            "an evicted event is not a summary of itself"
        );
    }

    #[test]
    fn folding_is_tried_before_eviction() {
        // A folded entry keeps its place in the order and stays one expand
        // away. Reaching for eviction first would throw away ordering
        // information that cost nothing to keep.
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(200, 0.5)).with_tail(2);
        view.push(ViewEntry::new(Role::User, "ask", "what is in the file"));
        view.push(ViewEntry::new(Role::Tool, "tool_result", filler(300)));
        view.push(ViewEntry::new(Role::Assistant, "turn", "short answer"));
        view.push(ViewEntry::new(
            Role::Tool,
            "tool_result",
            "a newer, smaller result",
        ));

        let report = view.evict(&mut log, headline_for).unwrap();
        assert_eq!(report.folded, 1, "got: {report:?}");
        assert_eq!(report.evicted, 0, "got: {report:?}");
        assert!(view.entries()[1].folded);
        assert!(
            view.entries()[1].text.contains("expand #2"),
            "a folded entry has to say where its content went: {:?}",
            view.entries()[1].text
        );
        assert!(
            !view.entries()[3].folded,
            "the newest tool result stays whole: it is what the next turn is about"
        );
    }

    #[test]
    fn a_folded_payload_reaches_the_log_whole() {
        // Folding shortens what is displayed and must not shorten what is
        // recorded, or the pointer would lead to the pointer.
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(200, 0.5)).with_tail(2);
        let big = filler(300);
        view.push(ViewEntry::new(Role::Tool, "tool_result", big.clone()));
        view.push(ViewEntry::new(Role::Assistant, "turn", "a"));
        view.push(ViewEntry::new(Role::Assistant, "turn", "b"));
        view.push(ViewEntry::new(Role::Assistant, "turn", "c"));
        view.evict(&mut log, headline_for).unwrap();

        assert_eq!(log.materialize(Seq(1)).unwrap(), big);
    }

    #[test]
    fn the_recent_tail_and_the_active_turn_survive() {
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(60, 0.5)).with_tail(3);
        for i in 0..15 {
            view.push(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("{i} {}", filler(20)),
            ));
        }
        view.push(ViewEntry::new(Role::User, "ask", "the current question").active());

        view.evict(&mut log, headline_for).unwrap();

        let entries = view.entries();
        assert!(
            entries.last().unwrap().text.contains("current question"),
            "the turn in progress must never be evicted"
        );
        assert!(
            entries.len() >= 3,
            "the tail is protected: {}",
            entries.len()
        );
    }

    #[test]
    fn an_old_tool_result_does_not_pin_the_whole_view() {
        // Protecting the newest tool result unconditionally makes one result
        // at the head of a long session block every eviction behind it. The
        // symptom is a view that quietly stops shrinking and an eviction
        // report saying it did nothing, over and over.
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(240, 0.5)).with_tail(2);
        view.push(ViewEntry::new(Role::Tool, "tool_result", filler(40)));
        for i in 0..20 {
            view.push(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("{i} {}", filler(20)),
            ));
        }

        let report = view.evict(&mut log, headline_for).unwrap();
        assert!(report.evicted > 0, "got: {report:?}");
        assert!(report.within_budget, "got: {report:?}");
    }

    #[test]
    fn a_view_that_cannot_fit_says_so_rather_than_evicting_its_tail() {
        // The honest outcome. Evicting into the protected region would keep
        // the number under the limit and lose the conversation.
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(20, 0.5)).with_tail(4);
        for i in 0..4 {
            view.push(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("{i} {}", filler(50)),
            ));
        }

        let report = view.evict(&mut log, headline_for).unwrap();
        assert!(!report.within_budget, "got: {report:?}");
        assert_eq!(view.entries().len(), 4, "nothing protected was dropped");
    }

    #[test]
    fn eviction_leaves_a_headline_pointing_at_what_left() {
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(100, 0.5)).with_tail(2);
        for i in 0..20 {
            view.push(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("{i} {}", filler(20)),
            ));
        }

        let report = view.evict(&mut log, headline_for).unwrap();
        let (from, _) = report.span.unwrap();
        let entry = view
            .index()
            .locate(from)
            .expect("the index must cover what was evicted");
        assert!(entry.covers(from));
    }

    #[test]
    fn expanding_brings_the_original_text_back_into_the_view() {
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(100, 0.5)).with_tail(2);
        for i in 0..20 {
            view.push(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("entry {i} {}", filler(20)),
            ));
        }
        let report = view.evict(&mut log, headline_for).unwrap();
        let (from, _) = report.span.unwrap();

        let events = view.expand(&log, from, from).unwrap();
        assert_eq!(events.len(), 1);
        assert!(view
            .entries()
            .last()
            .unwrap()
            .text
            .starts_with(&format!("entry {}", from.0 - 1)));
    }

    #[test]
    fn a_view_under_budget_is_left_alone() {
        let (_dir, mut log) = log();
        let mut view = WorkingView::new("s1", Budget::new(10_000, 0.6));
        view.push(ViewEntry::new(Role::User, "ask", "short"));

        let report = view.evict(&mut log, headline_for).unwrap();
        assert_eq!(
            report,
            Eviction {
                folded: 0,
                evicted: 0,
                span: None,
                cost_after: view.cost(),
                within_budget: true,
            }
        );
        assert_eq!(log.len(), 1, "persisting still happens");
    }
}
