//! Context as an environment.
//!
//! # The problem this addresses
//!
//! An agent's history is normally kept by putting it back in the prompt. That
//! makes every past turn compete for the same budget as the current one, and it
//! forces the decision about what matters to be made at *write* time -- when a
//! tool returns, something has to choose what to keep, before anyone knows what
//! the next question will be. Whatever it discards is gone.
//!
//! The alternative implemented here, from Scroll (arXiv:2608.21690), is to keep
//! the history *outside* the context as something the agent can query. Nothing
//! is summarised away at write time. What the agent sees next is chosen at read
//! time, by the agent, from a record that stayed complete.
//!
//! # The three layers
//!
//! - [`EventLog`] -- append-only, ground truth. Every event gets a [`Seq`], an
//!   address that never changes and never repeats. Nothing is ever edited or
//!   removed.
//! - [`PayloadStore`] -- large payloads live outside the log behind a
//!   [`Handle`], so a 40 MB tool result does not make the log expensive to
//!   scan. The log keeps a preview and the handle.
//! - A confined runtime -- see [`runtime`]. The agent computes over what it
//!   retrieves inside a sandbox rather than in its own context, and only what
//!   it prints comes back. Two backends: [`SandboxRuntime`] confines every
//!   call and keeps only files between them, and [`ResidentRuntime`] keeps a
//!   Python namespace alive so a retrieved result stays an object a later call
//!   can operate on. What each can and cannot confine differs, and each says
//!   which.
//!
//! [`WorkingView`] is what the model actually sees, and it is bounded. When it
//! exceeds its budget, [`WorkingView::evict`] moves the oldest completed spans
//! out and leaves [`Headline`]s in an [`EvictionIndex`] that point at the exact
//! addresses they came from.
//!
//! # The invariant everything else rests on
//!
//! **Eviction changes the view and never the record.** Anything evicted is
//! still in the log, still at the same address, still byte-for-byte what it
//! was. A summary that replaces its source is a lossy compression nobody can
//! undo; a headline that *points at* its source is a table of contents. This
//! crate is built so that the second is the only thing that can happen -- there
//! is no operation on [`EventLog`] that removes or rewrites an event.

#![forbid(unsafe_code)]

pub mod index;
pub mod log;
pub mod payload;
pub mod resident;
pub mod runtime;
pub mod search;
pub mod session;
pub mod view;

pub use index::{EvictionIndex, Headline, IndexBlock, IndexEntry, Status};
pub use log::{Event, EventDraft, EventLog, Role, Seq};
pub use payload::{Handle, Payload, PayloadStore};
pub use resident::{ResidentRuntime, ResidentSpec};
pub use runtime::{ContextRuntime, RuntimeCall, RuntimeOutput, SandboxRuntime};
pub use search::{Filter, Hit, SearchIndex};
pub use session::{SessionEnvironment, SessionId};
pub use view::{Budget, Eviction, ViewEntry, WorkingView};

/// Anything that can go wrong reaching the environment.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    /// The address does not exist in this log.
    ///
    /// Distinct from an empty result: asking for `#9000` in a log of 40 events
    /// is a bug in the caller, and answering it with "nothing found" hides
    /// that.
    #[error("no event at {0}; the log holds {1} event(s)")]
    NoSuchSeq(Seq, u64),

    /// A range whose end precedes its start.
    #[error("range {0}..={1} runs backwards")]
    BackwardRange(Seq, Seq),

    /// The payload behind a handle could not be read back.
    ///
    /// The log entry survives -- this says the externalized bytes did not, so
    /// a caller can tell a missing file apart from a missing event.
    #[error("payload {0} is unreadable: {1}")]
    LostPayload(String, String),

    /// Storage refused a read or a write.
    #[error("context storage: {0}")]
    Io(String),

    /// A record on disk could not be parsed.
    ///
    /// Reported with its line so a truncated write can be found rather than
    /// guessed at.
    #[error("event log line {0} is not a valid record: {1}")]
    Corrupt(u64, String),

    /// The confined runtime refused, failed to start, or was never installed.
    #[error("context runtime: {0}")]
    Runtime(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, ContextError>;
