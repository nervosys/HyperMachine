//! The three layers, wired together into the surface an agent actually uses.
//!
//! # The four operations
//!
//! Everything an agent does with its own history is one of these, and they are
//! deliberately the only ones:
//!
//! - **locate** -- [`SessionEnvironment::search`] finds addresses;
//! - **materialize** -- [`SessionEnvironment::expand`] turns an address back
//!   into exactly what was recorded there;
//! - **compute** -- [`SessionEnvironment::exec`] runs a program over what was
//!   retrieved, confined, with only its output coming back;
//! - **expose** -- [`SessionEnvironment::observe`] puts something in the view,
//!   and [`SessionEnvironment::compact`] takes it out again when the budget
//!   says so.
//!
//! There is no operation that edits or deletes an event, on purpose.

use crate::index::Headline;
use crate::log::{Event, EventDraft, EventLog, Role, Seq};
use crate::runtime::{ContextRuntime, RuntimeCall, RuntimeOutput};
use crate::search::{Filter, Hit, SearchIndex};
use crate::view::{Budget, Eviction, ViewEntry, WorkingView};
use crate::{ContextError, Result};

/// Which session an event belongs to.
///
/// Sessions share one log so a later one can search an earlier one, which is
/// most of the reason to keep a log at all. The id is what keeps them apart
/// when that is wanted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub String);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SessionId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for SessionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// A log, an index over it, a bounded view of it, and somewhere to compute.
pub struct SessionEnvironment {
    session: SessionId,
    log: EventLog,
    search: SearchIndex,
    view: WorkingView,
    runtime: Option<Box<dyn ContextRuntime>>,
    /// Highest address already in the search index.
    ///
    /// The index is rebuilt forward from here after anything appends, so a
    /// caller can never search a log that has moved on without it.
    indexed_through: Seq,
}

impl std::fmt::Debug for SessionEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionEnvironment")
            .field("session", &self.session)
            .field("events", &self.log.len())
            .field("view_cost", &self.view.cost())
            .field("runtime", &self.runtime.as_ref().map(|r| r.name()))
            .finish()
    }
}

impl SessionEnvironment {
    /// Open the environment rooted at `root` for `session`.
    ///
    /// Replays the whole log into the search index, so a session opened months
    /// later can find what an earlier one recorded. That is the case the log
    /// exists for; paying for it at open time is the point.
    ///
    /// # Errors
    ///
    /// Propagates [`ContextError::Io`] and [`ContextError::Corrupt`] from
    /// opening the log, and [`ContextError::LostPayload`] if an externalized
    /// payload cannot be read while indexing.
    pub fn open(
        root: impl AsRef<std::path::Path>,
        session: impl Into<SessionId>,
        budget: Budget,
    ) -> Result<Self> {
        let session = session.into();
        let log = EventLog::open(root)?;
        let view = WorkingView::new(session.0.clone(), budget);

        let mut env = Self {
            session,
            log,
            search: SearchIndex::new(),
            view,
            runtime: None,
            indexed_through: Seq(0),
        };
        env.reindex()?;
        Ok(env)
    }

    /// Install the confined runtime `exec` will use.
    ///
    /// Without one, [`Self::exec`] refuses. The alternative -- falling back to
    /// running the program unconfined on the host -- would turn a missing
    /// capability into an unannounced privilege.
    pub fn set_runtime(&mut self, runtime: Box<dyn ContextRuntime>) {
        self.runtime = Some(runtime);
    }

    /// The name of the installed runtime, if there is one.
    ///
    /// `None` is the answer an agent needs before it plans around
    /// [`Self::exec`]: there is nowhere to compute, and finding that out from
    /// a refused call halfway through is worse.
    pub fn runtime_name(&self) -> Option<&str> {
        self.runtime.as_ref().map(|runtime| runtime.name())
    }

    /// Which session this is.
    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// The log, for reading.
    pub fn log(&self) -> &EventLog {
        &self.log
    }

    /// The bounded view.
    pub fn view(&self) -> &WorkingView {
        &self.view
    }

    /// The bounded view, mutably.
    pub fn view_mut(&mut self) -> &mut WorkingView {
        &mut self.view
    }

    /// Lower the externalization threshold, for tests that would otherwise
    /// have to produce 8 KiB of text to reach the path that matters.
    pub fn set_externalize_threshold(&mut self, bytes: usize) {
        let store = self.log.store().clone().with_threshold(bytes);
        self.log.set_store(store);
    }

    /// Record an event without putting it in the view.
    ///
    /// For everything worth keeping that the model does not need to see now:
    /// the full text of a result it only skimmed, a note from the harness, an
    /// artifact from a previous session.
    ///
    /// # Errors
    ///
    /// Propagates any [`ContextError`] from the log.
    pub fn record(
        &mut self,
        role: Role,
        kind: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<Seq> {
        let seq = self
            .log
            .append(EventDraft::new(self.session.0.clone(), role, kind, text))?;
        self.reindex()?;
        Ok(seq)
    }

    /// Put something in the view. It reaches the log at the next
    /// [`Self::compact`].
    pub fn observe(&mut self, entry: ViewEntry) {
        self.view.push(entry);
    }

    /// Find addresses. The `locate` half of the surface.
    ///
    /// Returns at most `k` hits, best first. Content does not come back here
    /// -- only previews and addresses -- because a search that returned full
    /// payloads would fill the context with the thing it was asked to find a
    /// way around.
    pub fn search(&self, query: &str, k: usize, filter: &Filter) -> Vec<Hit> {
        self.search.search(query, k, filter)
    }

    /// Read a span back exactly as it was recorded, and put it in the view.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::NoSuchSeq`] for an unknown address,
    /// [`ContextError::BackwardRange`] for a reversed span, and
    /// [`ContextError::LostPayload`] if externalized bytes are gone.
    pub fn expand(&mut self, lo: Seq, hi: Seq) -> Result<Vec<Event>> {
        self.view.expand(&self.log, lo, hi)
    }

    /// Read a span without putting it in the view.
    ///
    /// The variant to use when the answer is going to be computed rather than
    /// read: pull the text out, hand it to [`Self::exec`], and let the program
    /// decide what is worth showing.
    ///
    /// # Errors
    ///
    /// As [`Self::expand`].
    pub fn materialize(&self, lo: Seq, hi: Seq) -> Result<Vec<String>> {
        self.log
            .range(lo, hi)?
            .iter()
            .map(|event| self.log.store().materialize(&event.payload))
            .collect()
    }

    /// Run a program in the confined runtime. The `compute` half.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::Runtime`] if no runtime is installed, or if the
    /// one installed refused to run the program under its confinement.
    pub fn exec(&self, call: &RuntimeCall) -> Result<RuntimeOutput> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ContextError::Runtime(
                "no runtime installed; call set_runtime. Refusing rather than running \
                 the program unconfined on the host"
                    .into(),
            )
        })?;
        runtime.exec(call)
    }

    /// Persist, fold and evict until the view is inside its budget.
    ///
    /// `headline` is called with each span about to leave, and what it returns
    /// is what the index will say about it.
    ///
    /// # Errors
    ///
    /// Propagates any [`ContextError`] from persisting the view.
    pub fn compact(&mut self, headline: impl FnMut(&[ViewEntry]) -> Headline) -> Result<Eviction> {
        let eviction = self.view.evict(&mut self.log, headline)?;
        self.reindex()?;
        Ok(eviction)
    }

    /// What the model sees: the index of what has left, then the view.
    ///
    /// The index goes first and is never omitted. A view rendered without it
    /// looks like the whole session, and an agent reading one has no way to
    /// know there is anything to go and look for.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if !self.view.index().is_empty() {
            out.push_str("evicted (still addressable):\n");
            out.push_str(&self.view.index().render());
            out.push('\n');
        }
        for entry in self.view.entries() {
            let address = entry
                .seq
                .map_or_else(|| "live".to_string(), |seq| seq.to_string());
            out.push_str(&format!(
                "{address} {} {}: {}\n",
                entry.role, entry.kind, entry.text
            ));
        }
        out
    }

    /// Index everything appended since the last time.
    fn reindex(&mut self) -> Result<()> {
        while self.indexed_through.0 < self.log.len() {
            let seq = self.indexed_through.next();
            let event = self.log.get(seq)?.clone();
            let full_text = self.log.store().materialize(&event.payload)?;
            self.search.index(&event, &full_text);
            self.indexed_through = seq;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Status;
    use crate::payload::EXTERNALIZE_THRESHOLD;

    fn env(dir: &tempfile::TempDir) -> SessionEnvironment {
        SessionEnvironment::open(dir.path(), "s1", Budget::new(200, 0.5)).unwrap()
    }

    fn headline_for(span: &[ViewEntry]) -> Headline {
        let from = span.first().and_then(|e| e.seq).unwrap_or_default();
        let to = span.last().and_then(|e| e.seq).unwrap_or_default();
        Headline::new(
            "earlier work",
            "recorded",
            "continue",
            Status::Done,
            from,
            to,
        )
    }

    #[test]
    fn what_was_evicted_can_be_found_again_and_read_exactly() {
        // The end-to-end claim: the view forgot it, search found where it
        // went, and expanding it produced the original text rather than a
        // description of it.
        let dir = tempfile::tempdir().unwrap();
        let mut env = env(&dir);

        env.observe(ViewEntry::new(
            Role::Tool,
            "tool_result",
            "the vsock device refuses a descriptor chain that cycles",
        ));
        for i in 0..30 {
            env.observe(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("unrelated turn {i} {}", "padding ".repeat(20)),
            ));
        }

        let eviction = env.compact(headline_for).unwrap();
        assert!(eviction.evicted > 0, "got: {eviction:?}");
        assert!(
            !env.render().contains("descriptor chain that cycles"),
            "the view should have let go of it"
        );

        let hits = env.search("descriptor chain cycles", 3, &Filter::default());
        let seq = hits.first().expect("search must still find it").seq;
        let events = env.expand(seq, seq).unwrap();
        assert_eq!(events.len(), 1);
        assert!(env
            .render()
            .contains("the vsock device refuses a descriptor chain that cycles"));
    }

    #[test]
    fn a_later_session_can_search_an_earlier_one() {
        // The reason sessions share a log. Without it, every session starts
        // from nothing and the record is only useful to whoever wrote it.
        let dir = tempfile::tempdir().unwrap();
        {
            let mut first =
                SessionEnvironment::open(dir.path(), "monday", Budget::default()).unwrap();
            first
                .record(Role::Tool, "tool_result", "the kvm backend never set CPUID")
                .unwrap();
        }

        let later = SessionEnvironment::open(dir.path(), "tuesday", Budget::default()).unwrap();
        let hits = later.search("cpuid", 5, &Filter::default());
        assert_eq!(hits.len(), 1, "got: {hits:?}");
        assert_eq!(hits[0].session, "monday");

        // ...and can restrict itself to its own history when it wants to.
        assert!(later
            .search("cpuid", 5, &Filter::default().session("tuesday"))
            .is_empty());
    }

    #[test]
    fn a_large_result_is_searchable_by_its_whole_content_not_its_preview() {
        // The trap this avoids: index the preview and a 40 MB result becomes
        // findable only by its first 240 bytes, which is where nothing
        // interesting ever is.
        let dir = tempfile::tempdir().unwrap();
        let mut env = env(&dir);
        env.set_externalize_threshold(64);

        let mut text = "opening line\n".repeat(40);
        text.push_str("the needle is orthogonal_persistence\n");
        assert!(text.len() > 64);
        env.record(Role::Tool, "tool_result", text).unwrap();

        let hits = env.search("orthogonal_persistence", 3, &Filter::default());
        assert_eq!(hits.len(), 1, "got: {hits:?}");
    }

    #[test]
    fn expanding_an_externalized_payload_returns_all_of_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = env(&dir);
        let big = "x".repeat(EXTERNALIZE_THRESHOLD + 100);
        let seq = env.record(Role::Tool, "tool_result", big.clone()).unwrap();

        assert_eq!(env.materialize(seq, seq).unwrap(), vec![big]);
    }

    #[test]
    fn exec_refuses_rather_than_running_on_the_host() {
        // A missing runtime must not silently become "run it here".
        let dir = tempfile::tempdir().unwrap();
        let env = env(&dir);
        let err = env
            .exec(&RuntimeCall::new("echo").args(["hello"]))
            .unwrap_err();
        assert!(matches!(err, ContextError::Runtime(_)), "got: {err}");
        assert!(err.to_string().contains("unconfined"), "got: {err}");
    }

    #[test]
    fn the_render_says_what_is_missing_from_it() {
        // An agent reading a view with no notice of eviction has no reason to
        // go looking, which makes the log useless exactly when it matters.
        let dir = tempfile::tempdir().unwrap();
        let mut env = env(&dir);
        for i in 0..30 {
            env.observe(ViewEntry::new(
                Role::Assistant,
                "turn",
                format!("turn {i} {}", "padding ".repeat(20)),
            ));
        }
        env.compact(headline_for).unwrap();

        let rendered = env.render();
        assert!(
            rendered.starts_with("evicted (still addressable):"),
            "{rendered}"
        );
        assert!(rendered.contains("earlier work"), "{rendered}");
    }

    #[test]
    fn recording_keeps_the_search_index_current() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = env(&dir);
        env.record(Role::System, "note", "first fact").unwrap();
        assert_eq!(env.search("first", 5, &Filter::default()).len(), 1);
        env.record(Role::System, "note", "second fact").unwrap();
        assert_eq!(env.search("second", 5, &Filter::default()).len(), 1);
    }

    #[test]
    fn compaction_indexes_what_it_persisted() {
        // Entries reach the log through the view rather than through record,
        // and an index that missed them would make the newest history the
        // only unsearchable part.
        let dir = tempfile::tempdir().unwrap();
        let mut env = env(&dir);
        env.observe(ViewEntry::new(
            Role::Assistant,
            "turn",
            "a distinctive phrase: sublimation",
        ));
        env.compact(headline_for).unwrap();

        assert_eq!(env.search("sublimation", 5, &Filter::default()).len(), 1);
    }
}
