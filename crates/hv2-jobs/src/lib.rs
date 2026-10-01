//! A durable job queue: batch work on the host, each job a program run under
//! [`hv2_sandbox::ProcessSandbox`], queued in a directory any number of
//! workers and clients share. No VM, no daemon, no database.
//!
//! # The store
//!
//! ```text
//! STORE/jobs/<id>/spec.json      what to run; written once
//! STORE/jobs/<id>/state.json     where it is; replaced atomically
//! STORE/jobs/<id>/stdout.log     what it printed, appended as it runs
//! STORE/jobs/<id>/stderr.log
//! STORE/jobs/<id>/cancel         present once a cancel is asked for
//! STORE/queue/<id>               present while it waits
//! STORE/running/<id>             present while a worker holds it
//! STORE/claims/<id>-<attempt>    who decided attempt <attempt>'s fate
//! ```
//!
//! **Every decision is an exclusive create.** Starting attempt N of a job,
//! cancelling it before attempt N starts, and requeueing attempt N after its
//! worker is lost are each decided by creating `claims/<id>-<N>` (or, for a
//! requeue, `claims/<id>-<N>-lost`) with `create_new` -- `O_EXCL` on Unix,
//! `CREATE_NEW` on Windows -- which exactly one of any number of racing
//! processes can do. Whoever creates it acts; everyone else moves on.
//!
//! Not a rename: on Windows, Rust's `rename` opens the source and renames
//! through that handle, so two processes that both opened `queue/<id>` before
//! either moved it both "succeed". A test with eight workers racing for
//! twenty jobs claimed them 64 times before this was changed.
//!
//! **A worker holds a lease.** While a job runs, its worker rewrites
//! `state.json` with a heartbeat. A job whose heartbeat is older than the
//! lease has lost its worker -- killed, crashed, the machine rebooted -- and
//! is put back in the queue by the next store operation that notices, until
//! `max_attempts` runs out. The lease names the attempt, so a worker that was
//! only slow, and finds its job requeued, stops rather than running it
//! alongside its successor.
//!
//! **Nothing is lost on a crash.** Every state change is a whole-file write
//! renamed into place, so a reader sees the old state or the new one, never
//! half of either.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[cfg(feature = "http")]
pub mod http;
pub mod worker;
pub mod schedule;
pub mod dispatch;

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// No job with that ID.
    #[error("no job {0}")]
    NotFound(String),
    /// The spec cannot be run.
    #[error("invalid job spec: {0}")]
    InvalidSpec(String),
    /// The job is not in a state that allows this.
    #[error("{0}")]
    Conflict(String),
    /// The store could not be read or written.
    #[error("job store: {0}")]
    Io(#[from] std::io::Error),
    /// A file in the store is not what this version writes.
    #[error("job store: {0}")]
    Corrupt(String),
}

/// A shorthand for store results.
pub type Result<T> = std::result::Result<T, JobError>;

/// Confinement for a job, as `hm sandbox run`'s flags have it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSettings {
    /// Memory ceiling, e.g. `"8G"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_time_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_clock_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_processes: Option<u32>,
    /// `"deny"` (the default) or `"host"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    /// `"host"` (the default) or `"isolated:ROOT"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fs: Option<String>,
    /// Host paths mounted read-only inside an isolated filesystem.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ro: Vec<PathBuf>,
    #[serde(default)]
    pub isolate_processes: bool,
    #[serde(default)]
    pub no_new_privileges: bool,
    /// Refuse to run if a control cannot be enforced, rather than dropping
    /// it and recording which were dropped.
    #[serde(default)]
    pub strict: bool,
}

/// How a cancel asks a job to stop before it is killed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GracefulStop {
    /// A file the worker creates when the job is cancelled. The program is
    /// expected to notice it, finish what it can and exit.
    pub create_file: PathBuf,
    /// How long it has, after the file appears, before it is killed.
    pub grace_secs: u64,
}

/// What to run, and under what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSpec {
    /// Earliest start time, in milliseconds since the Unix epoch. Omitted
    /// jobs are immediately eligible; workers poll and may start later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The program and its arguments. Run directly, not through a shell.
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<PathBuf>,
    /// Variables for the job, over the worker's minimal base
    /// ([`hv2_sandbox::HOST_BASE_ENV`]). `HM_JOB_ID` is always set.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub sandbox: SandboxSettings,
    /// Only a worker with every one of these labels takes the job.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// How many times it may be started, counting restarts after a worker
    /// is lost. 1 by default: a program that is not safe to run twice is
    /// not run twice unless the spec says so.
    #[serde(default = "one")]
    pub max_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graceful_stop: Option<GracefulStop>,
}

fn one() -> u32 {
    1
}

impl JobSpec {
    /// Refuse a spec that could never run.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(JobError::InvalidSpec(m));
        if self.command.first().is_none_or(|p| p.trim().is_empty()) {
            return bad("command is empty".into());
        }
        if self.max_attempts == 0 {
            return bad("max_attempts is at least 1".into());
        }
        for (k, v) in &self.env {
            if k.is_empty() || k.contains('=') || k.contains('\0') || v.contains('\0') {
                return bad(format!("env: {k:?} is not a usable variable"));
            }
        }
        for label in &self.labels {
            if label.is_empty()
                || !label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            {
                return bad(format!("label {label:?}: letters, digits, - _ . only"));
            }
        }
        if let Some(net) = &self.sandbox.net {
            if net != "deny" && net != "host" {
                return bad(format!("sandbox.net is deny or host, not {net:?}"));
            }
        }
        if let Some(fs) = &self.sandbox.fs {
            if fs != "host" && fs.strip_prefix("isolated:").is_none_or(|r| r.is_empty()) {
                return bad(format!("sandbox.fs is host or isolated:ROOT, not {fs:?}"));
            }
        }
        if let Some(memory) = &self.sandbox.memory {
            parse_size(memory).map_err(JobError::InvalidSpec)?;
        }
        Ok(())
    }
}

/// `512`, `64K`, `512M`, `4G`, `1T`, binary units, a trailing `B`/`iB` allowed.
pub fn parse_size(text: &str) -> std::result::Result<u64, String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let t = if lower.ends_with("ib") {
        &t[..t.len() - 2]
    } else if lower.ends_with('b') {
        &t[..t.len() - 1]
    } else {
        t
    };
    let (digits, shift) = match t.chars().last() {
        Some('K' | 'k') => (&t[..t.len() - 1], 10),
        Some('M' | 'm') => (&t[..t.len() - 1], 20),
        Some('G' | 'g') => (&t[..t.len() - 1], 30),
        Some('T' | 't') => (&t[..t.len() - 1], 40),
        _ => (t, 0),
    };
    let n: u64 = digits
        .trim()
        .parse()
        .map_err(|_| format!("{text:?} is not a size like 512M or 4G"))?;
    n.checked_shl(shift)
        .filter(|v| v >> shift == n)
        .ok_or_else(|| format!("{text:?} is too large"))
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl Phase {
    /// Whether the job is over.
    pub fn is_final(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    /// The name used on the command line and in the API.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Everything known about a job but its spec and output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobState {
    pub id: String,
    pub state: Phase,
    pub submitted_ms: u64,
    /// Times it has been started.
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    /// The limit that killed it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub killed_by: Option<String>,
    /// Controls asked for and dropped because this host cannot enforce them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unenforced: Vec<String>,
    #[serde(default)]
    pub cancel_requested: bool,
    /// Why it failed, when it failed without running to an exit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A job store: a directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
    /// A running job whose heartbeat is older than this has lost its worker.
    pub lease: Duration,
}

/// How often a worker renews its lease. The lease is several of these, so
/// one slow write does not requeue a job that is fine.
pub const HEARTBEAT: Duration = Duration::from_secs(5);
/// The default lease.
pub const DEFAULT_LEASE: Duration = Duration::from_secs(30);

impl Store {
    /// Open (creating if need be) the store at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for dir in ["jobs", "queue", "running", "claims"] {
            std::fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self {
            root,
            lease: DEFAULT_LEASE,
        })
    }

    /// The store `HM_JOBS_DIR` names, or `~/.hypermachine/jobs`.
    pub fn default_root() -> PathBuf {
        std::env::var_os("HM_JOBS_DIR").map_or_else(
            || {
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map_or_else(|| PathBuf::from("."), PathBuf::from);
                home.join(".hypermachine").join("jobs")
            },
            PathBuf::from,
        )
    }

    /// Where the store is.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn job_dir(&self, id: &str) -> PathBuf {
        self.root.join("jobs").join(id)
    }

    /// Where job `id` writes `stream` ("stdout" or "stderr").
    pub fn log_path(&self, id: &str, stream: &str) -> PathBuf {
        self.job_dir(id).join(format!("{stream}.log"))
    }

    fn check_id(id: &str) -> Result<()> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(JobError::NotFound(id.to_string()));
        }
        Ok(())
    }

    /// Queue `spec`; returns the new job's ID.
    pub fn submit(&self, spec: &JobSpec) -> Result<String> {
        spec.validate()?;
        let now = now_ms();
        // Sortable by time, and unique without coordination.
        let id = format!(
            "j{now:013}-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let dir = self.job_dir(&id);
        std::fs::create_dir_all(&dir)?;
        write_json(&dir.join("spec.json"), spec)?;
        std::fs::File::create(dir.join("stdout.log"))?;
        std::fs::File::create(dir.join("stderr.log"))?;
        let state = JobState {
            id: id.clone(),
            state: Phase::Queued,
            submitted_ms: now,
            attempts: 0,
            worker: None,
            started_ms: None,
            heartbeat_ms: None,
            finished_ms: None,
            exit_code: None,
            signal: None,
            killed_by: None,
            unenforced: Vec::new(),
            cancel_requested: false,
            message: None,
        };
        self.put_state(&state)?;
        // Last: a job is visible to workers only once it is whole.
        std::fs::File::create(self.root.join("queue").join(&id))?;
        Ok(id)
    }

    /// Job `id`'s spec.
    pub fn spec(&self, id: &str) -> Result<JobSpec> {
        Self::check_id(id)?;
        read_json(&self.job_dir(id).join("spec.json"), id)
    }

    /// Job `id`'s state.
    pub fn state(&self, id: &str) -> Result<JobState> {
        Self::check_id(id)?;
        read_json(&self.job_dir(id).join("state.json"), id)
    }

    pub(crate) fn put_state(&self, state: &JobState) -> Result<()> {
        write_json(&self.job_dir(&state.id).join("state.json"), state)
    }

    /// Every job, oldest first.
    pub fn list(&self) -> Result<Vec<JobState>> {
        self.reap_lost();
        let mut ids: Vec<String> = std::fs::read_dir(self.root.join("jobs"))?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        ids.sort();
        Ok(ids.iter().filter_map(|id| self.state(id).ok()).collect())
    }

    /// Ask job `id` to stop. A queued job is cancelled at once; a running one
    /// is stopped by its worker, gracefully first if its spec says how.
    pub fn cancel(&self, id: &str) -> Result<JobState> {
        let mut state = self.state(id)?;
        if state.state.is_final() {
            return Err(JobError::Conflict(format!(
                "job {id} is already {}",
                state.state.as_str()
            )));
        }
        std::fs::File::create(self.job_dir(id).join("cancel"))?;
        // Queued: take the next attempt's claim ourselves, so no worker can.
        if state.state == Phase::Queued && self.decide(&format!("{id}-{}", state.attempts + 1)) {
            state.state = Phase::Cancelled;
            state.cancel_requested = true;
            state.finished_ms = Some(now_ms());
            self.put_state(&state)?;
            let _ = std::fs::remove_file(self.root.join("queue").join(id));
            return Ok(state);
        }
        // Running, or a worker won the race: it sees the marker and stops.
        state.cancel_requested = true;
        Ok(state)
    }

    /// Whether a cancel has been asked for.
    pub fn cancel_requested(&self, id: &str) -> bool {
        self.job_dir(id).join("cancel").exists()
    }

    /// Create `claims/<name>` exclusively: `true` for the one caller that did.
    fn decide(&self, name: &str) -> bool {
        let dir = self.root.join("claims");
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
            .is_ok()
    }

    /// Claim the oldest queued job a worker with `labels` can run, which is
    /// then running and held by `worker` until it finishes it.
    pub fn claim(&self, worker: &str, labels: &[String]) -> Result<Option<Claim>> {
        self.claim_at(worker, labels, now_ms())
    }

    fn claim_at(&self, worker: &str, labels: &[String], eligible_ms: u64) -> Result<Option<Claim>> {
        self.reap_lost();
        let mut queued: Vec<String> = std::fs::read_dir(self.root.join("queue"))?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        queued.sort();
        for id in queued {
            let Ok(state) = self.state(&id) else { continue };
            if state.state != Phase::Queued {
                // A stale entry: the job moved on while listed.
                if state.state.is_final() {
                    let _ = std::fs::remove_file(self.root.join("queue").join(&id));
                }
                continue;
            }
            let Ok(spec) = self.spec(&id) else { continue };
            if spec.not_before_ms.is_some_and(|due| due > eligible_ms) {
                continue;
            }
            if !spec.labels.iter().all(|l| labels.contains(l)) {
                continue;
            }
            let attempt = state.attempts + 1;
            if !self.decide(&format!("{id}-{attempt}")) {
                continue; // Another worker won it, or it was cancelled.
            }
            let token = format!("{worker}#{attempt}");
            let mut state = state;
            let now = now_ms();
            state.state = Phase::Running;
            state.attempts = attempt;
            state.worker = Some(token.clone());
            state.started_ms = Some(now);
            state.heartbeat_ms = Some(now);
            state.message = None;
            self.put_state(&state)?;
            std::fs::File::create(self.root.join("running").join(&id))?;
            let _ = std::fs::remove_file(self.root.join("queue").join(&id));
            return Ok(Some(Claim { id, spec, token }));
        }
        Ok(None)
    }

    /// Renew the lease `token` holds on job `id`. An error means the lease is
    /// gone -- the job was requeued, and may be running elsewhere.
    pub fn heartbeat(&self, id: &str, token: &str) -> Result<()> {
        let mut state = self.state(id)?;
        if state.state != Phase::Running || state.worker.as_deref() != Some(token) {
            return Err(JobError::Conflict(format!(
                "{token} does not hold job {id}"
            )));
        }
        state.heartbeat_ms = Some(now_ms());
        self.put_state(&state)
    }

    /// Record how job `id` ended, and release it.
    pub fn finish(&self, id: &str, update: impl FnOnce(&mut JobState)) -> Result<JobState> {
        let mut state = self.state(id)?;
        update(&mut state);
        state.finished_ms = Some(now_ms());
        self.put_state(&state)?;
        let _ = std::fs::remove_file(self.root.join("running").join(id));
        Ok(state)
    }

    /// Put back in the queue every running job whose worker has gone quiet
    /// for longer than the lease -- or fail it, once it has had its attempts.
    /// Safe to call from anywhere at any time: an exclusive claim decides who
    /// acts on each lost attempt.
    pub fn reap_lost(&self) -> Vec<String> {
        let mut reaped = Vec::new();
        let Ok(entries) = std::fs::read_dir(self.root.join("running")) else {
            return reaped;
        };
        let cutoff =
            now_ms().saturating_sub(u64::try_from(self.lease.as_millis()).unwrap_or(u64::MAX));
        for entry in entries.flatten() {
            let Some(id) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok(mut state) = self.state(&id) else {
                continue;
            };
            if state.state != Phase::Running {
                // Finished, and its entry not yet removed.
                if state.state.is_final() {
                    let _ = std::fs::remove_file(entry.path());
                }
                continue;
            }
            if state.heartbeat_ms.unwrap_or(0) > cutoff {
                continue;
            }
            if !self.decide(&format!("{id}-{}-lost", state.attempts)) {
                continue;
            }
            let Ok(spec) = self.spec(&id) else { continue };
            let lost = format!(
                "its worker {} stopped renewing its lease",
                state.worker.as_deref().unwrap_or("?")
            );
            let requeue = !(state.cancel_requested || self.cancel_requested(&id))
                && state.attempts < spec.max_attempts;
            if requeue {
                state.state = Phase::Queued;
                state.worker = None;
                state.heartbeat_ms = None;
                state.message = Some(format!("{lost}; requeued"));
            } else {
                state.state = if state.cancel_requested || self.cancel_requested(&id) {
                    Phase::Cancelled
                } else {
                    Phase::Failed
                };
                state.finished_ms = Some(now_ms());
                state.message = Some(if state.state == Phase::Failed {
                    format!(
                        "{lost}, and it has had its {} attempt(s)",
                        spec.max_attempts
                    )
                } else {
                    lost
                });
            }
            if self.put_state(&state).is_ok() {
                let _ = std::fs::remove_file(entry.path());
                if requeue {
                    let _ = std::fs::File::create(self.root.join("queue").join(&id));
                }
            }
            reaped.push(id);
        }
        reaped
    }
}

/// A job a worker has claimed.
#[derive(Debug, Clone)]
pub struct Claim {
    pub id: String,
    pub spec: JobSpec,
    /// The lease this worker holds: its name and the attempt.
    pub token: String,
}

/// Write `value` as JSON to `path`, atomically: a temporary file beside it,
/// renamed over it, so a reader sees the old whole file or the new one.
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(value).map_err(|e| JobError::Corrupt(e.to_string()))?,
    )?;
    if let Err(e) = replace(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// `rename`, retried briefly on Windows, where replacing a file another
/// process has open for reading fails with a sharing violation for as long
/// as that read lasts.
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for _ in 0..50 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename kept failing")))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, id: &str) -> Result<T> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(JobError::NotFound(id.to_string()))
        }
        Err(e) => return Err(e.into()),
    };
    serde_json::from_slice(&bytes)
        .map_err(|e| JobError::Corrupt(format!("{}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(cmd: &[&str]) -> JobSpec {
        JobSpec {
            not_before_ms: None,
            name: None,
            command: cmd.iter().map(|s| s.to_string()).collect(),
            workdir: None,
            env: BTreeMap::new(),
            sandbox: SandboxSettings::default(),
            labels: Vec::new(),
            max_attempts: 1,
            graceful_stop: None,
        }
    }

    #[test]
    fn delayed_jobs_survive_reopen_and_do_not_block_ready_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut delayed = spec(&["delayed"]);
        delayed.not_before_ms = Some(u64::MAX);
        let delayed_id = store.submit(&delayed).unwrap();
        let ready_id = store.submit(&spec(&["ready"])).unwrap();
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.spec(&delayed_id).unwrap(), delayed);
        assert_eq!(
            store.claim_at("w", &[], u64::MAX - 1).unwrap().unwrap().id,
            ready_id
        );
        assert!(store.claim_at("w", &[], u64::MAX - 1).unwrap().is_none());
        assert_eq!(store.state(&delayed_id).unwrap().attempts, 0);
        assert_eq!(
            store.claim_at("w", &[], u64::MAX).unwrap().unwrap().id,
            delayed_id
        );
        assert!(store.claim_at("other", &[], u64::MAX).unwrap().is_none());
    }

    #[test]
    fn delayed_jobs_can_be_cancelled_before_becoming_eligible() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut delayed = spec(&["delayed"]);
        delayed.not_before_ms = Some(u64::MAX);
        let id = store.submit(&delayed).unwrap();
        assert_eq!(store.cancel(&id).unwrap().state, Phase::Cancelled);
        assert!(store.claim_at("w", &[], u64::MAX).unwrap().is_none());
        assert_eq!(store.state(&id).unwrap().attempts, 0);
    }

    #[test]
    fn a_spec_parses_from_the_documented_json_and_refuses_unknown_fields() {
        let json = r#"{"name":"arena","command":["awake","arena-loop","--dir","run"],
            "workdir":"/work","env":{"A":"1"},
            "sandbox":{"memory":"8G","wall_clock_secs":null,"net":"deny","fs":"host"},
            "labels":["gpu"],"max_attempts":3,
            "graceful_stop":{"create_file":"run/STOP","grace_secs":900}}"#;
        let s: JobSpec = serde_json::from_str(json).unwrap();
        assert_eq!(s.max_attempts, 3);
        assert_eq!(s.graceful_stop.as_ref().unwrap().grace_secs, 900);
        s.validate().unwrap();
        assert!(serde_json::from_str::<JobSpec>(r#"{"command":["x"],"retries":2}"#).is_err());
        let defaults: JobSpec = serde_json::from_str(r#"{"command":["x"]}"#).unwrap();
        assert_eq!(defaults.max_attempts, 1);
    }

    #[test]
    fn specs_that_could_never_run_are_refused() {
        assert!(spec(&[]).validate().is_err());
        assert!(spec(&[" "]).validate().is_err());
        let mut s = spec(&["x"]);
        s.max_attempts = 0;
        assert!(s.validate().is_err());
        let mut s = spec(&["x"]);
        s.labels = vec!["no spaces".into()];
        assert!(s.validate().is_err());
        let mut s = spec(&["x"]);
        s.sandbox.net = Some("maybe".into());
        assert!(s.validate().is_err());
        let mut s = spec(&["x"]);
        s.sandbox.fs = Some("isolated:".into());
        assert!(s.validate().is_err());
        let mut s = spec(&["x"]);
        s.sandbox.memory = Some("lots".into());
        assert!(s.validate().is_err());
    }

    #[test]
    fn submit_then_claim_then_finish() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let id = store.submit(&spec(&["echo", "hi"])).unwrap();
        assert_eq!(store.state(&id).unwrap().state, Phase::Queued);

        let c = store.claim("w1", &[]).unwrap().unwrap();
        assert_eq!(c.id, id);
        assert_eq!(c.spec.command, ["echo", "hi"]);
        assert_eq!(c.token, "w1#1");
        let st = store.state(&id).unwrap();
        assert_eq!(
            (st.state, st.attempts, st.worker.as_deref()),
            (Phase::Running, 1, Some("w1#1"))
        );
        assert!(store.claim("w2", &[]).unwrap().is_none(), "claimed twice");

        store.heartbeat(&id, &c.token).unwrap();
        assert!(store.heartbeat(&id, "w2#1").is_err());
        assert!(store.heartbeat(&id, "w1#2").is_err());
        let done = store
            .finish(&id, |s| {
                s.state = Phase::Succeeded;
                s.exit_code = Some(0);
            })
            .unwrap();
        assert_eq!(done.state, Phase::Succeeded);
        assert!(!dir.path().join("running").join(&id).exists());
    }

    #[test]
    fn many_workers_racing_for_one_job_get_it_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        for _ in 0..20 {
            store.submit(&spec(&["x"])).unwrap();
        }
        let won = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for w in 0..8 {
                let store = store.clone();
                let won = &won;
                s.spawn(move || {
                    while let Some(c) = store.claim(&format!("w{w}"), &[]).unwrap() {
                        won.lock().unwrap().push(c.id);
                    }
                });
            }
        });
        let mut won = won.into_inner().unwrap();
        assert_eq!(won.len(), 20);
        won.sort();
        won.dedup();
        assert_eq!(won.len(), 20, "a job was claimed twice");
    }

    #[test]
    fn labels_route_jobs_to_workers_that_have_them() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut gpu = spec(&["train"]);
        gpu.labels = vec!["gpu".into()];
        let gpu_id = store.submit(&gpu).unwrap();
        let cpu_id = store.submit(&spec(&["score"])).unwrap();
        let got = store.claim("cpu-only", &[]).unwrap().unwrap().id;
        assert_eq!(got, cpu_id);
        assert!(store.claim("cpu-only", &["cpu".into()]).unwrap().is_none());
        let got = store
            .claim("gpu-box", &["gpu".into(), "cpu".into()])
            .unwrap()
            .unwrap()
            .id;
        assert_eq!(got, gpu_id);
    }

    #[test]
    fn a_queued_job_cancels_at_once_and_is_never_claimed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let id = store.submit(&spec(&["x"])).unwrap();
        assert_eq!(store.cancel(&id).unwrap().state, Phase::Cancelled);
        assert!(store.claim("w", &[]).unwrap().is_none());
        assert!(matches!(store.cancel(&id), Err(JobError::Conflict(_))));
    }

    #[test]
    fn a_job_whose_worker_went_quiet_is_requeued_until_its_attempts_run_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.lease = Duration::from_millis(50);
        let mut s = spec(&["x"]);
        s.max_attempts = 2;
        let id = store.submit(&s).unwrap();

        store.claim("dies", &[]).unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(store.reap_lost(), std::slice::from_ref(&id));
        let st = store.state(&id).unwrap();
        assert_eq!((st.state, st.attempts), (Phase::Queued, 1));
        assert!(st.message.unwrap().contains("requeued"));

        store.claim("dies-too", &[]).unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(120));
        store.reap_lost();
        let st = store.state(&id).unwrap();
        assert_eq!((st.state, st.attempts), (Phase::Failed, 2));
        assert!(store.claim("w", &[]).unwrap().is_none());
    }

    #[test]
    fn a_slow_worker_whose_job_was_requeued_has_lost_its_lease() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.lease = Duration::from_millis(50);
        let mut s = spec(&["x"]);
        s.max_attempts = 2;
        let id = store.submit(&s).unwrap();
        let slow = store.claim("w", &[]).unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(120));
        store.reap_lost();
        // The same worker name, a new attempt: the old lease must not renew.
        let fresh = store.claim("w", &[]).unwrap().unwrap();
        assert_eq!(fresh.token, "w#2");
        assert!(store.heartbeat(&id, &slow.token).is_err());
        store.heartbeat(&id, &fresh.token).unwrap();
    }

    #[test]
    fn a_cancel_racing_a_worker_for_a_queued_job_has_one_winner() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(dir.path()).unwrap();
            let id = store.submit(&spec(&["x"])).unwrap();
            let (claimed, cancelled) = std::thread::scope(|sc| {
                let a = sc.spawn(|| store.claim("w", &[]).unwrap().is_some());
                let b = sc.spawn(|| store.cancel(&id).unwrap().state == Phase::Cancelled);
                (a.join().unwrap(), b.join().unwrap())
            });
            assert!(
                claimed != cancelled,
                "claimed {claimed}, cancelled {cancelled}"
            );
        }
    }

    #[test]
    fn a_live_worker_keeps_its_job() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.lease = Duration::from_millis(200);
        let id = store.submit(&spec(&["x"])).unwrap();
        let c = store.claim("alive", &[]).unwrap().unwrap();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(60));
            store.heartbeat(&id, &c.token).unwrap();
            assert!(store.reap_lost().is_empty());
        }
    }

    #[test]
    fn ids_cannot_reach_outside_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        for bad in ["../x", "a/b", "", "..", "a\\b"] {
            assert!(
                matches!(store.state(bad), Err(JobError::NotFound(_))),
                "{bad}"
            );
        }
    }
}
