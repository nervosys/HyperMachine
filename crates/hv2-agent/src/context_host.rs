//! The memory surface, as something an agent can call.
//!
//! # Why this is a host rather than a field on the server
//!
//! Same inversion as [`crate::vm_host::VmHost`] and
//! [`crate::sandbox_host::SandboxHost`]: the MCP server describes the surface
//! and dispatches against a trait, and something else decides what is behind
//! it. A deployment that keeps no session record installs nothing and the
//! tools refuse, which is the honest answer -- rather than an in-memory stub
//! that accepts every write and loses it, so an agent recording something
//! important gets a success and no record.
//!
//! # What the surface is
//!
//! Four operations, from Scroll (arXiv:2608.21690): locate an address, read it
//! back exactly, compute over what was read somewhere confined, and decide
//! what stays in the view. Nothing here edits or deletes an event, because the
//! whole arrangement depends on an address meaning the same thing forever.

use std::sync::Arc;

use async_trait::async_trait;
use hv2_context::{
    Budget, ContextRuntime, Filter, Headline, Role, RuntimeCall, SandboxRuntime, Seq,
    SessionEnvironment, Status, ViewEntry,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// A request to find addresses.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    /// What to look for.
    pub query: String,
    /// How many hits to return. Defaults to 8.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Restrict to one session.
    #[serde(default)]
    pub session: Option<String>,
    /// Restrict to one kind.
    #[serde(default)]
    pub kind: Option<String>,
}

/// One search result: where it is, not what it says.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    /// The address, which is what `context.expand` takes.
    pub seq: u64,
    /// Relevance within this result set.
    pub score: f64,
    /// The event's kind.
    pub kind: String,
    /// Who produced it.
    pub role: String,
    /// Which session it came from.
    pub session: String,
    /// The opening of the content, so hits can be told apart without reading
    /// each one.
    pub preview: String,
}

/// A request to read a span back.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpandRequest {
    /// First address, inclusive.
    pub from: u64,
    /// Last address, inclusive. Defaults to `from`.
    #[serde(default)]
    pub to: Option<u64>,
    /// Put the span back in the working view as well as returning it.
    ///
    /// Defaults to true. Setting it false is how an agent reads something it
    /// intends to compute over rather than keep.
    #[serde(default = "yes")]
    pub into_view: bool,
}

fn yes() -> bool {
    true
}

/// One recovered event, exactly as it was recorded.
#[derive(Debug, Clone, Serialize)]
pub struct ExpandedEvent {
    /// Its address.
    pub seq: u64,
    /// Who produced it.
    pub role: String,
    /// Its kind.
    pub kind: String,
    /// Milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// The whole content, externalized payloads included.
    pub text: String,
}

/// A request to append to the record without showing it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordRequest {
    /// Who this is from: `user`, `assistant`, `tool` or `system`.
    pub role: String,
    /// A short type used for filtering later: `tool_result`, `plan`, `note`.
    pub kind: String,
    /// The content. Large content is externalized and still searchable whole.
    pub text: String,
}

/// A request to run a program over what was retrieved.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    /// Program to run, directly rather than through a shell.
    pub program: String,
    /// Arguments, already split.
    #[serde(default)]
    pub args: Vec<String>,
    /// Text written to the program's standard input.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Run with whatever confinement this host can enforce, rather than
    /// refusing when it cannot enforce all of it.
    ///
    /// Off by default. Needed on hosts that cannot provide one of the defaults
    /// -- a Windows job object does not isolate the network -- and what was
    /// given up comes back in `unenforced` rather than going unmentioned.
    #[serde(default)]
    pub best_effort: bool,
}

/// A request to bring the view back inside its budget.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompactRequest {
    /// What the span being evicted was about.
    pub task: String,
    /// What is known to be true afterwards. Checked, not hoped.
    pub state: String,
    /// What the next step was going to be.
    pub next_action: String,
    /// How it ended: `done`, `failed`, `abandoned` or `in_progress`.
    #[serde(default)]
    pub status: Option<String>,
}

/// What one call produced.
#[derive(Debug, Clone, Serialize)]
pub struct ExecResult {
    /// What the program printed. This is what enters the view.
    pub stdout: String,
    /// What it printed to standard error.
    pub stderr: String,
    /// Exit status, or absent if a signal ended it.
    pub exit_code: Option<i32>,
    /// Whether the output was cut for length.
    pub truncated: bool,
    /// Confinement the host could not enforce. Empty is the normal case.
    pub unenforced: Vec<String>,
}

/// What a compaction did.
#[derive(Debug, Clone, Serialize)]
pub struct CompactResult {
    /// Entries whose payloads were replaced by their addresses.
    pub folded: usize,
    /// Entries removed from the view. All are still in the log.
    pub evicted: usize,
    /// First address of the evicted span.
    pub span_from: Option<u64>,
    /// Last address of the evicted span.
    pub span_to: Option<u64>,
    /// Estimated tokens the view holds now.
    pub cost_after: usize,
    /// Whether the view is inside its budget.
    ///
    /// `false` means it could not be: everything left is protected. A caller
    /// that treats this as success will keep growing.
    pub within_budget: bool,
}

/// A description of the record, for an agent deciding whether to search it.
#[derive(Debug, Clone, Serialize)]
pub struct ContextStatus {
    /// Which session this environment is for.
    pub session: String,
    /// How many events the log holds, across all sessions.
    pub events: u64,
    /// Estimated tokens the view holds.
    pub view_cost: usize,
    /// Entries currently visible.
    pub view_entries: usize,
    /// Blocks in the eviction index.
    pub index_blocks: usize,
    /// Whether a confined runtime is installed.
    pub runtime: Option<String>,
}

/// Somewhere the memory surface is actually implemented.
#[async_trait]
pub trait ContextHost: Send + Sync {
    /// Find addresses.
    async fn search(&self, request: SearchRequest) -> Result<Vec<SearchHit>, String>;

    /// Read a span back exactly.
    async fn expand(&self, request: ExpandRequest) -> Result<Vec<ExpandedEvent>, String>;

    /// Append to the record without putting it in the view.
    async fn record(&self, request: RecordRequest) -> Result<u64, String>;

    /// Compute over what was retrieved, confined.
    async fn exec(&self, request: ExecRequest) -> Result<ExecResult, String>;

    /// Bring the view back inside its budget.
    async fn compact(&self, request: CompactRequest) -> Result<CompactResult, String>;

    /// What the model would see right now.
    async fn render(&self) -> Result<String, String>;

    /// A description of the record.
    async fn status(&self) -> Result<ContextStatus, String>;
}

/// A [`ContextHost`] over a [`SessionEnvironment`] on this machine.
pub struct LocalContextHost {
    env: Mutex<SessionEnvironment>,
}

impl std::fmt::Debug for LocalContextHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalContextHost").finish_non_exhaustive()
    }
}

impl LocalContextHost {
    /// Open the environment rooted at `root` for `session`.
    ///
    /// No runtime is installed, so `context.exec` refuses until
    /// [`Self::with_sandbox_runtime`] or [`Self::with_runtime`] provides one.
    ///
    /// # Errors
    ///
    /// Returns the environment's error as a string if the log cannot be
    /// opened or replayed.
    pub fn open(
        root: impl AsRef<std::path::Path>,
        session: impl Into<String>,
        budget: Budget,
    ) -> Result<Self, String> {
        let env =
            SessionEnvironment::open(root, session.into(), budget).map_err(|e| e.to_string())?;
        Ok(Self {
            env: Mutex::new(env),
        })
    }

    /// Install a runtime confined by `sandbox`, working under `workspace`.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace cannot be created.
    pub fn with_sandbox_runtime(
        self,
        sandbox: Box<dyn hv2_sandbox::Sandbox>,
        workspace: impl Into<std::path::PathBuf>,
    ) -> Result<Self, String> {
        let runtime = SandboxRuntime::new(sandbox, workspace).map_err(|e| e.to_string())?;
        Ok(self.with_runtime(Box::new(runtime)))
    }

    /// Install an already-built runtime.
    #[must_use]
    pub fn with_runtime(self, runtime: Box<dyn ContextRuntime>) -> Self {
        // `Mutex::blocking_lock` would panic inside a runtime thread, and this
        // is a builder called before the host is shared, so `get_mut` is both
        // correct and free.
        let mut env = self.env;
        env.get_mut().set_runtime(runtime);
        Self { env }
    }
}

/// Parse a role name, naming the alternatives when it is not one.
fn parse_role(name: &str) -> Result<Role, String> {
    match name {
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" => Ok(Role::Tool),
        "system" => Ok(Role::System),
        other => Err(format!(
            "unknown role {other:?}; expected user, assistant, tool or system"
        )),
    }
}

/// Parse a status name. Absent means the work is still open, which is the
/// safer default: an unfinished task recorded as done is not recoverable by
/// reading, because nothing about it looks wrong.
fn parse_status(name: Option<&str>) -> Result<Status, String> {
    match name {
        None | Some("in_progress") => Ok(Status::InProgress),
        Some("done") => Ok(Status::Done),
        Some("failed") => Ok(Status::Failed),
        Some("abandoned") => Ok(Status::Abandoned),
        Some(other) => Err(format!(
            "unknown status {other:?}; expected done, failed, abandoned or in_progress"
        )),
    }
}

#[async_trait]
impl ContextHost for LocalContextHost {
    async fn search(&self, request: SearchRequest) -> Result<Vec<SearchHit>, String> {
        let mut filter = Filter::default();
        if let Some(session) = request.session {
            filter = filter.session(session);
        }
        if let Some(kind) = request.kind {
            filter = filter.kind(kind);
        }

        let env = self.env.lock().await;
        Ok(env
            .search(&request.query, request.limit.unwrap_or(8), &filter)
            .into_iter()
            .map(|hit| SearchHit {
                seq: hit.seq.0,
                score: hit.score,
                kind: hit.kind,
                role: hit.role.to_string(),
                session: hit.session,
                preview: hit.preview,
            })
            .collect())
    }

    async fn expand(&self, request: ExpandRequest) -> Result<Vec<ExpandedEvent>, String> {
        let from = Seq(request.from);
        let to = Seq(request.to.unwrap_or(request.from));

        let mut env = self.env.lock().await;
        let events = if request.into_view {
            env.expand(from, to).map_err(|e| e.to_string())?
        } else {
            env.log()
                .range(from, to)
                .map_err(|e| e.to_string())?
                .to_vec()
        };

        let mut out = Vec::with_capacity(events.len());
        for event in events {
            let text = env
                .log()
                .store()
                .materialize(&event.payload)
                .map_err(|e| e.to_string())?;
            out.push(ExpandedEvent {
                seq: event.seq.0,
                role: event.role.to_string(),
                kind: event.kind,
                timestamp_ms: event.timestamp_ms,
                text,
            });
        }
        Ok(out)
    }

    async fn record(&self, request: RecordRequest) -> Result<u64, String> {
        let role = parse_role(&request.role)?;
        let mut env = self.env.lock().await;
        env.record(role, request.kind, request.text)
            .map(|seq| seq.0)
            .map_err(|e| e.to_string())
    }

    async fn exec(&self, request: ExecRequest) -> Result<ExecResult, String> {
        let mut call = RuntimeCall::new(request.program).args(request.args);
        if let Some(stdin) = request.stdin {
            call = call.stdin(stdin);
        }
        if request.best_effort {
            call = call.best_effort();
        }

        let env = self.env.lock().await;
        let output = env.exec(&call).map_err(|e| e.to_string())?;
        Ok(ExecResult {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
            truncated: output.truncated,
            unenforced: output.unenforced.iter().map(ToString::to_string).collect(),
        })
    }

    async fn compact(&self, request: CompactRequest) -> Result<CompactResult, String> {
        let status = parse_status(request.status.as_deref())?;
        let mut env = self.env.lock().await;

        let eviction = env
            .compact(|span: &[ViewEntry]| {
                let from = span.first().and_then(|e| e.seq).unwrap_or_default();
                let to = span.last().and_then(|e| e.seq).unwrap_or(from);
                Headline::new(
                    request.task.clone(),
                    request.state.clone(),
                    request.next_action.clone(),
                    status,
                    from,
                    to,
                )
            })
            .map_err(|e| e.to_string())?;

        Ok(CompactResult {
            folded: eviction.folded,
            evicted: eviction.evicted,
            span_from: eviction.span.map(|(from, _)| from.0),
            span_to: eviction.span.map(|(_, to)| to.0),
            cost_after: eviction.cost_after,
            within_budget: eviction.within_budget,
        })
    }

    async fn render(&self) -> Result<String, String> {
        Ok(self.env.lock().await.render())
    }

    async fn status(&self) -> Result<ContextStatus, String> {
        let env = self.env.lock().await;
        Ok(ContextStatus {
            session: env.session().to_string(),
            events: env.log().len(),
            view_cost: env.view().cost(),
            view_entries: env.view().entries().len(),
            index_blocks: env.view().index().block_count(),
            runtime: env.runtime_name().map(ToString::to_string),
        })
    }
}

/// Convenience alias for an installed host.
pub type SharedContextHost = Arc<dyn ContextHost>;

#[cfg(test)]
mod tests {
    use super::*;

    fn host(dir: &tempfile::TempDir) -> LocalContextHost {
        LocalContextHost::open(dir.path(), "s1", Budget::new(400, 0.5)).unwrap()
    }

    #[tokio::test]
    async fn recorded_events_are_findable_and_readable() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(&dir);

        let seq = host
            .record(RecordRequest {
                role: "tool".into(),
                kind: "tool_result".into(),
                text: "the guest triple-faulted at the reset vector".into(),
            })
            .await
            .unwrap();

        let hits = host
            .search(SearchRequest {
                query: "triple-faulted reset".into(),
                limit: None,
                session: None,
                kind: None,
            })
            .await
            .unwrap();
        assert_eq!(hits.first().map(|h| h.seq), Some(seq), "got: {hits:?}");

        let events = host
            .expand(ExpandRequest {
                from: seq,
                to: None,
                into_view: false,
            })
            .await
            .unwrap();
        assert_eq!(
            events[0].text,
            "the guest triple-faulted at the reset vector"
        );
    }

    #[tokio::test]
    async fn reading_without_into_view_leaves_the_view_alone() {
        // The distinction that makes computing over history affordable: an
        // agent can pull 400 events out to run a script over them without any
        // of them landing in the context.
        let dir = tempfile::tempdir().unwrap();
        let host = host(&dir);
        let seq = host
            .record(RecordRequest {
                role: "tool".into(),
                kind: "tool_result".into(),
                text: "bulk output".into(),
            })
            .await
            .unwrap();

        host.expand(ExpandRequest {
            from: seq,
            to: None,
            into_view: false,
        })
        .await
        .unwrap();
        assert_eq!(host.status().await.unwrap().view_entries, 0);

        host.expand(ExpandRequest {
            from: seq,
            to: None,
            into_view: true,
        })
        .await
        .unwrap();
        assert_eq!(host.status().await.unwrap().view_entries, 1);
    }

    #[tokio::test]
    async fn exec_refuses_when_no_runtime_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(&dir);
        let err = host
            .exec(ExecRequest {
                program: "echo".into(),
                args: vec!["hi".into()],
                stdin: None,
                best_effort: false,
            })
            .await
            .unwrap_err();
        assert!(err.contains("unconfined"), "got: {err}");
    }

    #[tokio::test]
    async fn an_unknown_role_names_the_ones_that_exist() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(&dir);
        let err = host
            .record(RecordRequest {
                role: "operator".into(),
                kind: "note".into(),
                text: "x".into(),
            })
            .await
            .unwrap_err();
        assert!(
            err.contains("expected user, assistant, tool or system"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn an_absent_status_is_recorded_as_unfinished() {
        // Recording an unfinished task as done is the one error reading
        // cannot catch, because nothing about it looks wrong.
        assert_eq!(parse_status(None).unwrap(), Status::InProgress);
        assert_eq!(parse_status(Some("done")).unwrap(), Status::Done);
        assert!(parse_status(Some("finished")).is_err());
    }

    #[tokio::test]
    async fn compaction_reports_what_left_and_the_log_still_has_it() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(&dir);
        {
            let mut env = host.env.lock().await;
            for i in 0..30 {
                env.observe(ViewEntry::new(
                    Role::Assistant,
                    "turn",
                    format!("turn {i} {}", "padding ".repeat(20)),
                ));
            }
        }

        let result = host
            .compact(CompactRequest {
                task: "an earlier stretch of work".into(),
                state: "recorded".into(),
                next_action: "carry on".into(),
                status: Some("done".into()),
            })
            .await
            .unwrap();

        assert!(result.evicted > 0, "got: {result:?}");
        let from = result.span_from.unwrap();
        let recovered = host
            .expand(ExpandRequest {
                from,
                to: None,
                into_view: false,
            })
            .await
            .unwrap();
        assert!(recovered[0].text.contains("padding"), "got: {recovered:?}");
    }
}
