//! Reading a sandbox's checkpoint from outside, without touching the
//! sandbox: `GET /sandboxes/{id}/checkpoints/{name}/files?path=` lists a
//! directory in it, `.../file?path=` reads a file, and `.../archive?path=`
//! downloads a directory's tree as a gzipped tar.
//!
//! A sandbox's filesystem is in its memory, and a checkpoint is that memory,
//! layered over its template's. There is no disk image to open, as there is
//! for a machine's snapshot. So each request restores the checkpoint into a
//! guest of its own -- not registered, no network, no volumes, never routed
//! to -- stops every process in it but the agent, reads, and stops it.
//!
//! - **The sandbox itself is untouched,** running or paused, and the
//!   checkpoint is not changed: what the reader's guest writes is in its own
//!   memory, and gone when it stops.
//! - **What it sees** is the checkpoint as taken. The checkpoint's own
//!   programs are stopped before anything is read, so they cannot change a
//!   file meanwhile.
//! - **Not read:** a volume's files, which are on the volume and read through
//!   it; the reader's guest has none mounted.
//! - At most two readers at a time on a node; each is a guest the
//!   sandbox's size, over and above `--capacity`, for the length of one
//!   request.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use hv2_agent::{is_unsupported_op, AgentError, AgentVM};
use hv2_guest_agent::{DirEntry, DirEntryKind, OpResult, Operation, SignalName};
use serde::Deserialize;
use serde_json::{json, Value};

use super::guest_setup::{guest_has_no_shell, no_shell_message};
use super::{api_error, new_vm, AppState, TEMPLATE_CID, TEMPLATE_MAC};

/// The largest file read out of a checkpoint, as for a machine's snapshot.
const MAX_FILE: u64 = 64 << 20;

/// The largest archive downloaded: the compressed tar, as built in the
/// reader's guest, whose memory it takes.
const MAX_ARCHIVE: u64 = 64 << 20;

/// The most file content an archive built on the host reads from the guest,
/// before compression. Larger than [`MAX_ARCHIVE`] because the tar is
/// compressed and a tree of text shrinks several-fold, but bounded: the cost
/// of the walk is vsock round trips and host CPU, and a highly compressible
/// tree would otherwise let a small download cost unbounded work.
const MAX_ARCHIVE_INPUT: u64 = 256 << 20;

/// The largest single file put in a host-built archive. The agent's read
/// returns a whole file, so this bounds the host's memory for one.
const MAX_ARCHIVE_FILE: u64 = 64 << 20;

/// The most entries (directories, files, links) a host-built archive holds.
const MAX_ARCHIVE_ENTRIES: usize = 200_000;

/// How long building one archive on the host may take.
const ARCHIVE_DEADLINE: Duration = Duration::from_secs(180);

/// What an archive of `/` leaves out, as `ARCHIVE` does.
const SKIPPED_AT_ROOT: [&str; 4] = ["proc", "sys", "dev", "run"];

/// Where the reader's guest builds an archive. In memory, like all of it.
const ARCHIVE_AT: &str = "/dev/shm/.hv2-checkpoint-archive.tgz";

/// The most entries a listing returns.
const MAX_ENTRIES: usize = 10_000;

/// How long reading one file or listing one directory may take.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Readers at a time on this node.
static READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Stop every process but PID 1 (init), the agent (this shell's parent) and
/// this shell. Kernel threads ignore the signal.
const FREEZE: &str = r#"for p in /proc/[0-9]*; do
  pid=${p#/proc/}
  [ "$pid" = 1 ] || [ "$pid" = $$ ] || [ "$pid" = "$PPID" ] || kill -STOP "$pid" 2>/dev/null
done
exit 0"#;

/// `$1`'s entries, each as its type, mode, owner, group and size, then its
/// name, every field ended by NUL. Exit 4: no such path; 5: not a directory.
const LIST: &str = r#"[ -e "$1" ] || [ -L "$1" ] || exit 4
[ -d "$1" ] && [ ! -L "$1" ] || exit 5
cd -- "$1" || exit 5
for f in * .*; do
  case $f in .|..) continue;; esac
  [ -e "$f" ] || [ -L "$f" ] || continue
  printf '%s\0%s\0' "$(stat -c '%F|%a|%u|%g|%s' -- "$f")" "$f"
done"#;

/// `$1`'s size, if it is a regular file. Exit 4: no such path; 5: not a
/// regular file (a symlink is not followed).
const SIZE: &str = r#"[ -e "$1" ] || [ -L "$1" ] || exit 4
[ -f "$1" ] && [ ! -L "$1" ] || exit 5
stat -c %s -- "$1""#;

/// Build `ARCHIVE_AT` from the directory `$1`, its contents at the top of the
/// tar. Exit 4: no such path; 5: not a directory; 6: nothing was written.
/// Only plain `tar`, `gzip` and shell, which a minimal userland has: no long
/// options. A file that changed or vanished while it was read is not a
/// failure -- the archive is still whole for what it holds. An archive of `/`
/// leaves out what the kernel makes up: `/proc`, `/sys`, `/dev` (which holds
/// the archive itself) and `/run`.
const ARCHIVE: &str = r#"[ -e "$1" ] || [ -L "$1" ] || exit 4
[ -d "$1" ] && [ ! -L "$1" ] || exit 5
cd -- "$1" || exit 5
mkdir -p /dev/shm 2>/dev/null
rm -f "$ARCHIVE_AT"
if [ "$(pwd -P)" = / ]; then
  set --
  for f in * .[!.]* ..?*; do
    case $f in proc|sys|dev|run) continue;; esac
    [ -e "$f" ] || [ -L "$f" ] || continue
    set -- "$@" "$f"
  done
else
  set -- .
fi
tar -cf - "$@" 2>/dev/null | gzip -c > "$ARCHIVE_AT"
[ -s "$ARCHIVE_AT" ] || exit 6
stat -c %s -- "$ARCHIVE_AT""#;

#[derive(Debug, Deserialize)]
pub(crate) struct At {
    path: String,
}

/// Why `path` cannot be looked up in a checkpoint, when it cannot.
fn refused_path(path: &str) -> Option<&'static str> {
    if !path.starts_with('/') {
        Some("path must be absolute")
    } else if path.contains('\0') {
        Some("path cannot contain NUL")
    } else if path.len() > 4096 {
        Some("path is at most 4096 bytes")
    } else {
        None
    }
}

/// A refusal, as the status and message a handler answers with.
type Refused = (StatusCode, String);

fn refused(status: StatusCode, why: impl std::fmt::Display) -> Refused {
    (status, why.to_string())
}

/// A guest restored from a checkpoint, its own programs stopped.
struct Reader {
    vm: AgentVM,
    _permit: tokio::sync::SemaphorePermit<'static>,
}

impl Reader {
    async fn sh(&self, script: &str, path: &str) -> Result<(Option<i32>, String), Refused> {
        let args = [
            "-c".to_string(),
            script.to_string(),
            "sh".to_string(),
            path.to_string(),
        ];
        let ran = self
            .vm
            .exec_in_guest("/bin/sh", &args, READ_TIMEOUT)
            .await
            .map_err(|e| {
                // An image with no shell is a supported one: say so, not 502.
                if guest_has_no_shell(&e.to_string()) {
                    refused(
                        StatusCode::BAD_REQUEST,
                        no_shell_message(false, &e.to_string()),
                    )
                } else {
                    refused(
                        StatusCode::BAD_GATEWAY,
                        format!("reading the checkpoint: {e}"),
                    )
                }
            })?;
        Ok((ran.exit_code, ran.stdout))
    }

    /// What the agent's `stat` says about `path` (not following a symlink):
    /// `None` if the agent predates the operation, so the caller may use a
    /// shell; a refusal if there is no such path.
    async fn stat(&self, path: &str) -> Result<Option<DirEntry>, Refused> {
        match self.vm.stat_in_guest(path, false, READ_TIMEOUT).await {
            Ok(entry) => {
                tracing::debug!("checkpoint reader: using agent op (stat)");
                Ok(Some(entry))
            }
            Err(e) if is_unsupported_op(&e) => {
                tracing::debug!("checkpoint reader: falling back to shell (stat)");
                Ok(None)
            }
            Err(e) => Err(agent_refusal(path, &e)),
        }
    }

    /// `path`'s entries, as the listing answers: through the agent's own
    /// operations, or for an agent that predates them, `LIST` in a shell.
    async fn list_entries(&self, path: &str) -> Result<Vec<Value>, Refused> {
        match self.stat(path).await? {
            Some(entry) => {
                // As `LIST`: a symlink to a directory is not one.
                if entry.kind != DirEntryKind::Dir {
                    return Err(refused(
                        StatusCode::BAD_REQUEST,
                        format!("{path:?} is not a directory"),
                    ));
                }
                let (listed, _) = self
                    .vm
                    .list_dir_in_guest(path, MAX_ENTRIES as u32, READ_TIMEOUT)
                    .await
                    .map_err(|e| agent_refusal(path, &e))?;
                Ok(agent_entries(listed))
            }
            None => match self.sh(LIST, path).await? {
                (Some(0), out) => Ok(entries(&out)),
                (code, _) => Err(refusal(code, path, "a directory")),
            },
        }
    }

    /// `path`'s size, if it is a regular file (not through a symlink).
    async fn file_size(&self, path: &str) -> Result<u64, Refused> {
        match self.stat(path).await? {
            Some(entry) if entry.kind == DirEntryKind::File => Ok(entry.size),
            Some(_) => Err(refused(
                StatusCode::BAD_REQUEST,
                format!("{path:?} is not a regular file"),
            )),
            None => match self.sh(SIZE, path).await? {
                (Some(0), out) => out.trim().parse::<u64>().map_err(|e| {
                    refused(
                        StatusCode::BAD_GATEWAY,
                        format!("the size of {path:?}: {e}"),
                    )
                }),
                (code, _) => Err(refusal(code, path, "a regular file")),
            },
        }
    }

    async fn close(self) {
        let _ = self.vm.stop().await;
    }
}

/// Restore `sandbox_id`'s checkpoint `name` into a reader.
async fn open(state: &AppState, sandbox_id: &str, name: &str) -> Result<Reader, Refused> {
    let Some(checkpoint) = state
        .checkpoints
        .lock()
        .get(sandbox_id)
        .and_then(|held| held.get(name).cloned())
    else {
        return Err(refused(
            StatusCode::NOT_FOUND,
            format!("{sandbox_id} has no checkpoint {name:?}"),
        ));
    };
    // Its template and size: a checkpoint is the sandbox's guest, restored
    // as that guest, whether the sandbox runs now or is paused.
    let record = state
        .sandboxes
        .lock()
        .get(sandbox_id)
        .map(|live| live.record.clone())
        .or_else(|| {
            state
                .paused
                .lock()
                .get(sandbox_id)
                .map(|paused| paused.record.clone())
        });
    let Some(record) = record else {
        return Err(refused(
            StatusCode::NOT_FOUND,
            format!("no sandbox {sandbox_id}"),
        ));
    };
    let permit = READERS.acquire().await.map_err(|_| {
        refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "checkpoint readers unavailable",
        )
    })?;
    // The size it was taken at, which the sandbox may no longer be.
    let opts = checkpoint.sizes.applied(&state.opts);
    let initrd = state
        .initrds
        .read()
        .get(&record.template_id)
        .cloned()
        .unwrap_or_else(|| state.opts.initrd.clone());
    // The devices it was taken with, so it restores; the network device is
    // left with nothing behind it.
    let (vm, _nic) = new_vm(
        &opts,
        Some(&initrd),
        &format!("{sandbox_id}-checkpoint-reader"),
        TEMPLATE_CID,
        state.opts.network.then_some(TEMPLATE_MAC),
        None,
    )
    .await
    .map_err(|e| refused(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let reader = Reader {
        vm,
        _permit: permit,
    };
    let ready = async {
        reader
            .vm
            .launch_from_snapshot(&checkpoint.path)
            .await
            .map_err(|e| format!("restoring it: {e}"))?;
        reader
            .vm
            .after_restore(state.opts.ready_timeout)
            .await
            .map_err(|e| format!("its agent: {e}"))?;
        // Stop the checkpoint's programs through the agent's `signal_process`;
        // only an agent that predates it needs a shell, and a guest with
        // neither is read unfrozen.
        match freeze_by_agent(&reader.vm).await {
            Ok(()) => tracing::debug!("checkpoint reader: using agent op (freeze)"),
            Err(FreezeError::Unsupported) => {
                tracing::debug!("checkpoint reader: falling back to shell (freeze)");
                match reader
                    .vm
                    .exec_in_guest("/bin/sh", &["-c".into(), FREEZE.into()], READ_TIMEOUT)
                    .await
                {
                    Ok(frozen) if frozen.exit_code != Some(0) => {
                        return Err(format!("stopping its programs: {}", frozen.stderr.trim()));
                    }
                    Ok(_) => {}
                    Err(e) if guest_has_no_shell(&e.to_string()) => {
                        tracing::debug!("checkpoint reader: no shell, reading unfrozen");
                    }
                    Err(e) => return Err(format!("stopping its programs: {e}")),
                }
            }
            Err(FreezeError::Failed(e)) => return Err(format!("stopping its programs: {e}")),
        }
        Ok(())
    }
    .await;
    if let Err(e) = ready {
        reader.close().await;
        return Err(refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("opening checkpoint {name:?}: {e}"),
        ));
    }
    Ok(reader)
}

/// Why the agent could not freeze the guest.
enum FreezeError {
    /// The agent predates `signal_process`: the shell may do it.
    Unsupported,
    Failed(String),
}

/// The pids in `/proc`'s listing a freeze signals: every number but 1. (The
/// agent's own pid is among them; it refuses, and that refusal is ignored.)
fn freeze_targets(listed: &[DirEntry]) -> Vec<u32> {
    listed
        .iter()
        .filter_map(|entry| entry.name.parse::<u32>().ok())
        .filter(|pid| *pid > 1)
        .collect()
}

/// Whether an agent's failure is its refusal to signal pid 1 or itself.
fn is_signal_refusal(error: &AgentError) -> bool {
    matches!(error, AgentError::Script(m) if m.starts_with("refusing to signal"))
}

/// Stop every process but init and the agent with `signal_process`, with no
/// shell in the guest. A process that exits between the listing and its
/// signal is not a failure.
async fn freeze_by_agent(vm: &AgentVM) -> Result<(), FreezeError> {
    let (listed, truncated) = match vm.list_dir_in_guest("/proc", 0, READ_TIMEOUT).await {
        Ok(listed) => listed,
        Err(e) if is_unsupported_op(&e) => return Err(FreezeError::Unsupported),
        Err(e) => return Err(FreezeError::Failed(format!("listing /proc: {e}"))),
    };
    if truncated {
        return Err(FreezeError::Failed(
            "/proc lists more processes than one reply carries".into(),
        ));
    }
    for pid in freeze_targets(&listed) {
        match vm
            .signal_process_in_guest(pid, SignalName::Stop, READ_TIMEOUT)
            .await
        {
            Ok(_) => {}
            Err(e) if is_unsupported_op(&e) => return Err(FreezeError::Unsupported),
            Err(e) if is_signal_refusal(&e) => {}
            Err(e) => return Err(FreezeError::Failed(format!("signalling pid {pid}: {e}"))),
        }
    }
    Ok(())
}

/// What `stat -c %F` says, as a machine snapshot's listing says it.
fn kind(described: &str) -> &'static str {
    match described {
        "directory" => "directory",
        "symbolic link" => "symlink",
        d if d.starts_with("regular") => "file",
        _ => "other",
    }
}

/// A checkpoint's entry kind as a listing names it, as `kind` does for
/// `stat -c %F`.
fn agent_kind(kind: DirEntryKind) -> &'static str {
    match kind {
        DirEntryKind::Dir => "directory",
        DirEntryKind::Symlink => "symlink",
        DirEntryKind::File => "file",
        DirEntryKind::Other => "other",
    }
}

/// The agent's directory listing as `LIST`'s entries are: the shell lists
/// `*` and then the dot files, each sorted, and gives the mode as `stat -c %a`
/// does (octal, no leading zeros).
fn agent_entries(listed: Vec<DirEntry>) -> Vec<Value> {
    let (dotted, plain): (Vec<DirEntry>, Vec<DirEntry>) = listed
        .into_iter()
        .partition(|entry| entry.name.starts_with('.'));
    plain
        .into_iter()
        .chain(dotted)
        .take(MAX_ENTRIES)
        .map(|entry| {
            let kind = agent_kind(entry.kind);
            json!({
                "name": entry.name,
                "type": kind,
                "mode": format!("{:o}", entry.mode),
                "uid": entry.uid,
                "gid": entry.gid,
                "size": if kind == "directory" { None } else { Some(entry.size) },
            })
        })
        .collect()
}

/// Whether the guest's error says the path (or a parent of it) is not there.
fn is_missing(message: &str) -> bool {
    message.contains("No such file") || message.contains("Not a directory")
}

/// An agent operation's failure on `path` as a refusal.
fn agent_refusal(path: &str, error: &AgentError) -> Refused {
    if is_missing(&error.to_string()) {
        refused(
            StatusCode::NOT_FOUND,
            format!("no {path:?} in the checkpoint"),
        )
    } else {
        refused(
            StatusCode::BAD_GATEWAY,
            format!("reading {path:?}: {error}"),
        )
    }
}

/// `LIST`'s output as entries: NUL-ended fields, two to an entry.
fn entries(out: &str) -> Vec<Value> {
    let fields: Vec<&str> = out.split('\0').collect();
    fields
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| {
            let mut parts = pair[0].split('|');
            let (described, mode, uid, gid, size) = (
                parts.next()?,
                parts.next()?,
                parts.next()?.parse::<u32>().ok()?,
                parts.next()?.parse::<u32>().ok()?,
                parts.next()?.parse::<u64>().ok()?,
            );
            let kind = kind(described);
            Some(json!({
                "name": pair[1],
                "type": kind,
                "mode": mode,
                "uid": uid,
                "gid": gid,
                "size": if kind == "directory" { None } else { Some(size) },
            }))
        })
        .take(MAX_ENTRIES)
        .collect()
}

/// What a script's exit status says about `path`, when it is not success.
fn refusal(code: Option<i32>, path: &str, wanted: &str) -> Refused {
    match code {
        Some(4) => refused(
            StatusCode::NOT_FOUND,
            format!("no {path:?} in the checkpoint"),
        ),
        Some(5) => refused(StatusCode::BAD_REQUEST, format!("{path:?} is not {wanted}")),
        other => refused(
            StatusCode::BAD_GATEWAY,
            format!("reading {path:?} ended with {other:?}"),
        ),
    }
}

/// `GET /sandboxes/{id}/checkpoints/{name}/files?path=`.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    Path((sandbox_id, name)): Path<(String, String)>,
    Query(at): Query<At>,
) -> Response {
    if let Some(why) = refused_path(&at.path) {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    let reader = match open(&state, &sandbox_id, &name).await {
        Ok(reader) => reader,
        Err((status, why)) => return api_error(status, why),
    };
    let listed = reader.list_entries(&at.path).await;
    reader.close().await;
    match listed {
        Ok(entries) => Json(json!({
            "path": at.path,
            "checkpoint": name,
            "entries": entries,
        }))
        .into_response(),
        Err((status, why)) => api_error(status, why),
    }
}

/// `GET /sandboxes/{id}/checkpoints/{name}/file?path=`: its bytes.
pub(crate) async fn file(
    State(state): State<Arc<AppState>>,
    Path((sandbox_id, name)): Path<(String, String)>,
    Query(at): Query<At>,
) -> Response {
    if let Some(why) = refused_path(&at.path) {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    let reader = match open(&state, &sandbox_id, &name).await {
        Ok(reader) => reader,
        Err((status, why)) => return api_error(status, why),
    };
    let read = async {
        let size = reader.file_size(&at.path).await?;
        if size > MAX_FILE {
            return Err(refused(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("{:?} is {size} bytes; at most {MAX_FILE} are read", at.path),
            ));
        }
        reader
            .vm
            .read_file_in_guest(&at.path, MAX_FILE, READ_TIMEOUT)
            .await
            .map_err(|e| {
                refused(
                    StatusCode::BAD_GATEWAY,
                    format!("reading {:?}: {e}", at.path),
                )
            })
    }
    .await;
    reader.close().await;
    match read {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
        Err((status, why)) => api_error(status, why),
    }
}

/// The guest's tree as the agent's operations show it: what the archive
/// walk reads, so the walk can be tested without a guest.
trait GuestTree {
    async fn stat(&self, path: &str) -> Result<DirEntry, AgentError>;
    async fn list(&self, path: &str) -> Result<(Vec<DirEntry>, bool), AgentError>;
    async fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, AgentError>;
    async fn link(&self, path: &str) -> Result<String, AgentError>;
    /// Several reads in one round trip, in order; `Ok(None)` if the guest's
    /// agent has no batch operation (the default: one operation per read).
    async fn batch(&self, ops: Vec<Operation>) -> Result<Option<Vec<OpResult>>, AgentError> {
        let _ = ops;
        Ok(None)
    }
}

impl GuestTree for AgentVM {
    async fn stat(&self, path: &str) -> Result<DirEntry, AgentError> {
        self.stat_in_guest(path, false, READ_TIMEOUT).await
    }
    async fn list(&self, path: &str) -> Result<(Vec<DirEntry>, bool), AgentError> {
        self.list_dir_in_guest(path, 0, READ_TIMEOUT).await
    }
    async fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, AgentError> {
        self.read_file_in_guest(path, limit, READ_TIMEOUT).await
    }
    async fn link(&self, path: &str) -> Result<String, AgentError> {
        self.read_link_in_guest(path, READ_TIMEOUT).await
    }
    async fn batch(&self, ops: Vec<Operation>) -> Result<Option<Vec<OpResult>>, AgentError> {
        match self.batch_in_guest(ops, READ_TIMEOUT).await {
            Ok(results) => Ok(Some(results)),
            Err(e) if is_unsupported_op(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// A file at most this large is read in a batch with the files after it in
/// the walk, rather than on a connection of its own; a larger one is read as
/// before, so a batch's reply stays small whatever the tree holds.
const BATCH_READ_FILE: u64 = 64 << 10;

/// The most file data one prefetch asks for, by the files' sizes.
const BATCH_READ_BYTES: u64 = 4 << 20;

/// The most operations one prefetch holds.
const BATCH_READ_OPS: usize = 256;

/// The most prefetched data kept waiting for the walk to reach it, in bytes
/// of the agent's base64 (a third more than the files).
const PREFETCH_HELD: usize = 8 << 20;

/// How far down the walk's stack one prefetch looks for files to read.
const PREFETCH_LOOKAHEAD: usize = 4096;

/// Reads of the walk's next files and links, made together in a batch ahead
/// of when the walk needs them: one round trip for dozens of files where
/// one each costs two to three milliseconds. What is read is exactly what
/// the walk then reads one at a time -- a result that is not a clean, whole
/// answer (the file changed, the answer is odd) is dropped and the walk
/// reads that path itself, so every limit and error is the walk's own.
struct Prefetch {
    /// False once the agent has no batch operation, or a batch failed.
    on: bool,
    held: HashMap<String, OpResult>,
    held_bytes: usize,
}

impl Prefetch {
    fn new() -> Self {
        Self {
            on: true,
            held: HashMap::new(),
            held_bytes: 0,
        }
    }

    /// The operation that reads `entry` at `path` ahead, if it is worth one.
    fn read_for(path: &str, entry: &DirEntry) -> Option<Operation> {
        match entry.kind {
            DirEntryKind::File if entry.size <= BATCH_READ_FILE => Some(Operation::ReadFile {
                path: path.to_string(),
                offset: 0,
                length: BATCH_READ_FILE,
            }),
            DirEntryKind::Symlink => Some(Operation::ReadLink {
                path: path.to_string(),
            }),
            _ => None,
        }
    }

    /// Take what was read for `path`, if anything.
    fn take(&mut self, path: &str) -> Option<OpResult> {
        let result = self.held.remove(path)?;
        if let OpResult::FileData { data, .. } = &result {
            self.held_bytes = self.held_bytes.saturating_sub(data.len());
        }
        Some(result)
    }

    /// Read `first` and the files and links below it on the walk's `stack`
    /// (the next to be walked are at its end) in one batch, as far as the
    /// caps allow.
    async fn fill<G: GuestTree>(
        &mut self,
        guest: &G,
        first: (&str, &DirEntry),
        stack: &[(String, String, DirEntry)],
    ) {
        let mut paths = vec![first.0.to_string()];
        let mut ops: Vec<Operation> = Vec::new();
        ops.extend(Self::read_for(first.0, first.1));
        let mut bytes = first.1.size;
        for (path, _, entry) in stack.iter().rev().take(PREFETCH_LOOKAHEAD) {
            if self.held.contains_key(path) {
                continue;
            }
            let Some(op) = Self::read_for(path, entry) else {
                continue;
            };
            if ops.len() >= BATCH_READ_OPS
                || bytes + entry.size > BATCH_READ_BYTES
                || self.held_bytes + (bytes as usize) / 3 * 4 > PREFETCH_HELD
            {
                break;
            }
            bytes += entry.size;
            paths.push(path.clone());
            ops.push(op);
        }
        // `paths` has `first` first whether or not it had an operation.
        if Self::read_for(first.0, first.1).is_none() {
            paths.remove(0);
        }
        if ops.is_empty() {
            return;
        }
        match guest.batch(ops).await {
            Ok(Some(results)) if results.len() == paths.len() => {
                for (path, result) in paths.into_iter().zip(results) {
                    if let OpResult::FileData { data, .. } = &result {
                        self.held_bytes += data.len();
                    }
                    self.held.insert(path, result);
                }
            }
            Ok(Some(_)) | Err(_) => {
                tracing::debug!("checkpoint reader: a prefetch failed, reading one at a time");
                self.on = false;
            }
            Ok(None) => {
                tracing::debug!(
                    "checkpoint reader: the guest's agent has no batch operation, \
                     using one op per connection (archive)"
                );
                self.on = false;
            }
        }
    }
}

/// A prefetched file read as the walk's own `read` would return it: `None`
/// if it is not a clean, whole answer within `limit`, and the walk reads
/// the path itself.
fn prefetched_file(result: OpResult, limit: u64) -> Option<Result<Vec<u8>, AgentError>> {
    match result {
        OpResult::FileData { data, size } if size <= limit.min(BATCH_READ_FILE) => {
            let bytes = hv2_guest_agent::b64::decode(&data)?;
            (bytes.len() as u64 >= size).then_some(Ok(bytes))
        }
        OpResult::Failed { message } => Some(Err(AgentError::Script(message))),
        _ => None,
    }
}

/// A prefetched link target as the walk's own `link` would return it.
fn prefetched_link(result: OpResult) -> Option<Result<String, AgentError>> {
    match result {
        OpResult::LinkTarget { target } => Some(Ok(target)),
        OpResult::Failed { message } => Some(Err(AgentError::Script(message))),
        _ => None,
    }
}

// A gzipped tar built on the host, and how many entries it left out.
struct Archived {
    bytes: Vec<u8>,
    skipped: u64,
}

/// A tar header for `entry`: its mode, owner and modification time (a time
/// before the epoch is written as the epoch).
fn tar_header(entry: &DirEntry, kind: tar::EntryType, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(entry.mode);
    header.set_uid(u64::from(entry.uid));
    header.set_gid(u64::from(entry.gid));
    header.set_size(size);
    header.set_mtime(u64::try_from(entry.mtime_secs).unwrap_or(0));
    header
}

fn too_big(what: impl std::fmt::Display) -> Refused {
    refused(StatusCode::PAYLOAD_TOO_LARGE, what)
}

/// Build the gzipped tar of the directory `root` on the host, by walking it
/// with the agent's `stat`, `list_dir`, `read_file` and `read_link` -- no
/// shell, `tar` or `gzip` in the guest. Contents are at the top of the tar
/// (names relative, as `tar -C dir .`), except that an archive of `/` names
/// its top-level entries bare and omits [`SKIPPED_AT_ROOT`].
///
/// Holds directories (mode, owner, time), regular files (mode, owner, time,
/// contents) and symlinks (target verbatim). **Not held, and counted in
/// `skipped`:** devices, sockets and FIFOs (nothing to read), a symlink whose
/// target is not UTF-8 (the agent will not report it lossily), and hard
/// links, which appear as the separate files they are; and a file the guest
/// lists but cannot read (`/proc/*/clear_refs`, an I/O error), which `tar`
/// in `ARCHIVE` also went on past. A path that vanishes
/// during the walk is left out without comment, as `ARCHIVE` does.
///
/// Refused with 413 over [`MAX_ARCHIVE`] compressed, [`MAX_ARCHIVE_INPUT`] of
/// file content, [`MAX_ARCHIVE_FILE`] for one file, [`MAX_ARCHIVE_ENTRIES`]
/// entries, or a directory with more entries than one listing carries.
///
/// `Ok(None)` when the agent predates one of the operations: use the shell.
async fn build_archive<G: GuestTree>(guest: &G, root: &str) -> Result<Option<Archived>, Refused> {
    let started = std::time::Instant::now();
    let top = match guest.stat(root).await {
        Ok(entry) => entry,
        Err(e) if is_unsupported_op(&e) => return Ok(None),
        Err(e) => return Err(agent_refusal(root, &e)),
    };
    // As `ARCHIVE`: a symlink to a directory is not a directory.
    if top.kind != DirEntryKind::Dir {
        return Err(refused(
            StatusCode::BAD_REQUEST,
            format!("{root:?} is not a directory"),
        ));
    }
    let at_root = root.trim_end_matches('/').is_empty();
    let io = |e: std::io::Error| {
        refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("building the archive: {e}"),
        )
    };
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    // (guest path, name in the tar -- empty for the top of an archive of `/`,
    // entry), walked depth-first with each directory's names in order.
    let mut stack = vec![(
        root.trim_end_matches('/').to_string(),
        if at_root { String::new() } else { ".".into() },
        top,
    )];
    let (mut entries, mut input, mut skipped) = (0usize, 0u64, 0u64);
    let mut ahead = Prefetch::new();
    while let Some((path, name, entry)) = stack.pop() {
        if started.elapsed() > ARCHIVE_DEADLINE {
            return Err(refused(
                StatusCode::GATEWAY_TIMEOUT,
                format!("the archive of {root:?} took over {ARCHIVE_DEADLINE:?} to build"),
            ));
        }
        entries += 1;
        if entries > MAX_ARCHIVE_ENTRIES {
            return Err(too_big(format!(
                "{root:?} holds more than {MAX_ARCHIVE_ENTRIES} entries"
            )));
        }
        let shown = if path.is_empty() { "/" } else { &path };
        match entry.kind {
            DirEntryKind::Dir => {
                if !name.is_empty() {
                    let archived = if name == "." { "./" } else { &name };
                    let mut header = tar_header(&entry, tar::EntryType::Directory, 0);
                    tar.append_data(&mut header, archived, std::io::empty())
                        .map_err(io)?;
                }
                let (children, truncated) = match guest.list(shown).await {
                    Ok(listed) => listed,
                    Err(e) if is_unsupported_op(&e) => return Ok(None),
                    Err(e) if is_missing(&e.to_string()) => continue,
                    Err(e) => return Err(agent_refusal(shown, &e)),
                };
                if truncated {
                    return Err(too_big(format!(
                        "{shown:?} holds more entries than one listing carries"
                    )));
                }
                for child in children.into_iter().rev() {
                    if at_root && name.is_empty() && SKIPPED_AT_ROOT.contains(&child.name.as_str())
                    {
                        continue;
                    }
                    let child_path = format!("{path}/{}", child.name);
                    let child_name = if name.is_empty() {
                        child.name.clone()
                    } else {
                        format!("{name}/{}", child.name)
                    };
                    stack.push((child_path, child_name, child));
                }
            }
            DirEntryKind::File => {
                let room = MAX_ARCHIVE_INPUT - input;
                let limit = MAX_ARCHIVE_FILE.min(room);
                if entry.size > limit {
                    return Err(too_big(format!(
                        "{shown:?} is {} bytes; an archive reads at most {MAX_ARCHIVE_FILE} of \
                         one file and {MAX_ARCHIVE_INPUT} in all",
                        entry.size
                    )));
                }
                if ahead.on && entry.size <= BATCH_READ_FILE && !ahead.held.contains_key(shown) {
                    ahead.fill(guest, (shown, &entry), &stack).await;
                }
                let read = match ahead.take(shown).and_then(|r| prefetched_file(r, limit)) {
                    Some(read) => read,
                    None => guest.read(shown, limit).await,
                };
                let data = match read {
                    Ok(data) => data,
                    Err(e) if is_missing(&e.to_string()) => continue,
                    // The guest could not read it (`/proc/*/clear_refs`, an
                    // I/O error): left out and counted, as `tar` in
                    // `ARCHIVE` went on past it. A failure to reach the
                    // agent is still the archive's failure.
                    Err(AgentError::Script(_)) => {
                        skipped += 1;
                        continue;
                    }
                    Err(e) => return Err(agent_refusal(shown, &e)),
                };
                input += data.len() as u64;
                let mut header = tar_header(&entry, tar::EntryType::Regular, data.len() as u64);
                tar.append_data(&mut header, &name, &data[..]).map_err(io)?;
            }
            DirEntryKind::Symlink => {
                if ahead.on && !ahead.held.contains_key(shown) {
                    ahead.fill(guest, (shown, &entry), &stack).await;
                }
                let read = match ahead.take(shown).and_then(prefetched_link) {
                    Some(read) => read,
                    None => guest.link(shown).await,
                };
                let target = match read {
                    Ok(target) => target,
                    Err(e) if is_unsupported_op(&e) => return Ok(None),
                    Err(e) if is_missing(&e.to_string()) => continue,
                    Err(e) if e.to_string().contains("UTF-8") => {
                        skipped += 1;
                        continue;
                    }
                    Err(e) => return Err(agent_refusal(shown, &e)),
                };
                let mut header = tar_header(&entry, tar::EntryType::Symlink, 0);
                tar.append_link(&mut header, &name, &target).map_err(io)?;
            }
            DirEntryKind::Other => skipped += 1,
        }
        if tar.get_ref().get_ref().len() as u64 > MAX_ARCHIVE {
            return Err(too_big(format!(
                "the archive of {root:?} is over {MAX_ARCHIVE} bytes compressed"
            )));
        }
    }
    let bytes = tar
        .into_inner()
        .and_then(flate2::write::GzEncoder::finish)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err(too_big(format!(
            "the archive of {root:?} is {} bytes; at most {MAX_ARCHIVE} are downloaded",
            bytes.len()
        )));
    }
    Ok(Some(Archived { bytes, skipped }))
}

/// `GET /sandboxes/{id}/checkpoints/{name}/archive?path=`: the directory's
/// tree as a gzipped tar, with the directory's contents at its top.
pub(crate) async fn archive(
    State(state): State<Arc<AppState>>,
    Path((sandbox_id, name)): Path<(String, String)>,
    Query(at): Query<At>,
) -> Response {
    if let Some(why) = refused_path(&at.path) {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    let reader = match open(&state, &sandbox_id, &name).await {
        Ok(reader) => reader,
        Err((status, why)) => return api_error(status, why),
    };
    let read = async {
        match build_archive(&reader.vm, &at.path).await? {
            Some(archived) => {
                tracing::debug!("checkpoint reader: using agent op (archive)");
                return Ok(archived);
            }
            None => tracing::debug!("checkpoint reader: falling back to shell (archive)"),
        }
        // The script's `$ARCHIVE_AT` is the constant above, put into its text.
        let script = ARCHIVE.replace("$ARCHIVE_AT", ARCHIVE_AT);
        let size = match reader.sh(&script, &at.path).await? {
            (Some(0), out) => out.trim().parse::<u64>().map_err(|e| {
                refused(
                    StatusCode::BAD_GATEWAY,
                    format!("the size of the archive of {:?}: {e}", at.path),
                )
            })?,
            (code, _) => return Err(refusal(code, &at.path, "a directory")),
        };
        if size > MAX_ARCHIVE {
            return Err(refused(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "the archive of {:?} is {size} bytes; at most {MAX_ARCHIVE} are downloaded",
                    at.path
                ),
            ));
        }
        reader
            .vm
            .read_file_in_guest(ARCHIVE_AT, MAX_ARCHIVE, READ_TIMEOUT)
            .await
            .map(|bytes| Archived { bytes, skipped: 0 })
            .map_err(|e| {
                refused(
                    StatusCode::BAD_GATEWAY,
                    format!("reading the archive of {:?}: {e}", at.path),
                )
            })
    }
    .await;
    reader.close().await;
    match read {
        Ok(Archived { bytes, skipped }) => (
            [
                (header::CONTENT_TYPE, "application/gzip".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}.tar.gz\"", archive_name(&name)),
                ),
                // Devices and the like the archive could not hold; 0 when none.
                (
                    header::HeaderName::from_static("x-hv2-archive-skipped"),
                    skipped.to_string(),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err((status, why)) => api_error(status, why),
    }
}

/// A checkpoint's name as a file name: anything outside letters, digits,
/// `-`, `_` and `.` becomes `_`, so it cannot close the header's quotes.
pub(crate) fn archive_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_is_read_from_nul_ended_fields() {
        let out = "directory|755|0|0|4096\0etc\0regular file|644|1000|1000|12\0a b\nc\0\
                   symbolic link|777|0|0|7\0link\0character special file|666|0|0|0\0null\0";
        let listed = entries(out);
        assert_eq!(listed.len(), 4);
        assert_eq!(listed[0]["type"], "directory");
        assert!(listed[0]["size"].is_null());
        assert_eq!(listed[1]["name"], "a b\nc");
        assert_eq!(listed[1]["type"], "file");
        assert_eq!(listed[1]["size"], 12);
        assert_eq!(listed[1]["uid"], 1000);
        assert_eq!(listed[2]["type"], "symlink");
        assert_eq!(listed[3]["type"], "other");
    }

    fn dir_entry(name: &str, kind: DirEntryKind, mode: u32) -> DirEntry {
        DirEntry {
            name: name.into(),
            kind,
            size: 12,
            mode,
            uid: 1000,
            gid: 100,
            mtime_secs: 0,
        }
    }

    #[test]
    fn agent_entries_read_as_the_shell_listing_does() {
        let listed = agent_entries(vec![
            dir_entry(".bashrc", DirEntryKind::File, 0o644),
            dir_entry("bin", DirEntryKind::Dir, 0o755),
            dir_entry("link", DirEntryKind::Symlink, 0o777),
            dir_entry("su", DirEntryKind::File, 0o4755),
            dir_entry("tty", DirEntryKind::Other, 0o666),
        ]);
        let names: Vec<_> = listed.iter().map(|e| e["name"].as_str().unwrap()).collect();
        // The dot files come after the rest, as `* .*` lists them.
        assert_eq!(names, vec!["bin", "link", "su", "tty", ".bashrc"]);
        assert_eq!(listed[0]["type"], "directory");
        assert!(listed[0]["size"].is_null());
        assert_eq!(listed[0]["mode"], "755");
        assert_eq!(listed[1]["type"], "symlink");
        assert_eq!(listed[2]["mode"], "4755");
        assert_eq!(listed[2]["size"], 12);
        assert_eq!(listed[2]["uid"], 1000);
        assert_eq!(listed[3]["type"], "other");
        assert_eq!(listed[4]["type"], "file");
    }

    #[test]
    fn a_missing_path_is_a_404_and_other_failures_are_not() {
        let missing = AgentError::Script("stat /x: No such file or directory (os error 2)".into());
        assert_eq!(agent_refusal("/x", &missing).0, StatusCode::NOT_FOUND);
        let under_file = AgentError::Script("stat /f/x: Not a directory (os error 20)".into());
        assert_eq!(agent_refusal("/f/x", &under_file).0, StatusCode::NOT_FOUND);
        let other = AgentError::Script("stat /y: Permission denied".into());
        assert_eq!(agent_refusal("/y", &other).0, StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn a_malformed_entry_is_skipped() {
        assert!(entries("nonsense\0name\0").is_empty());
        assert!(entries("").is_empty());
    }

    #[test]
    fn an_archive_is_named_safely() {
        assert_eq!(archive_name("before-upgrade_1.0"), "before-upgrade_1.0");
        assert_eq!(archive_name("a\"b\r\nc/d"), "a_b__c_d");
    }

    #[test]
    fn the_archive_script_names_its_file_once() {
        // The script is a template over one placeholder; a second spelling
        // would be left unreplaced and tar a file called "".
        let script = ARCHIVE.replace("$ARCHIVE_AT", ARCHIVE_AT);
        assert!(!script.contains("ARCHIVE_AT"), "{script}");
        assert!(script.contains(ARCHIVE_AT));
    }

    #[test]
    fn a_freeze_signals_every_numbered_process_but_init() {
        let listed = vec![
            dir_entry("1", DirEntryKind::Dir, 0o555),
            dir_entry("2", DirEntryKind::Dir, 0o555),
            dir_entry("self", DirEntryKind::Symlink, 0o777),
            dir_entry("cpuinfo", DirEntryKind::File, 0o444),
            dir_entry("341", DirEntryKind::Dir, 0o555),
            dir_entry("0", DirEntryKind::Dir, 0o555),
        ];
        assert_eq!(freeze_targets(&listed), vec![2, 341]);
        assert!(is_signal_refusal(&AgentError::Script(
            "refusing to signal the agent itself".into()
        )));
        assert!(!is_signal_refusal(&AgentError::Script(
            "signalling pid 5: Operation not permitted".into()
        )));
    }

    /// A guest tree in memory: path -> (entry, file bytes or link target).
    #[derive(Default)]
    struct Fake {
        nodes: std::collections::BTreeMap<String, (DirEntry, String)>,
        /// Paths whose `read_link` the agent would refuse as non-UTF-8.
        odd_links: Vec<String>,
        /// Whether the agent has a batch operation; if so the sizes of the
        /// batches it was sent, and how many reads went one at a time.
        batching: bool,
        batch_fails: bool,
        batches: std::sync::Mutex<Vec<usize>>,
        single_reads: std::sync::atomic::AtomicUsize,
        /// Paths that vanish between the listing and the read.
        vanished: Vec<String>,
        /// Paths the guest lists but cannot read (EINVAL, EIO).
        unreadable: Vec<String>,
    }

    impl Fake {
        fn add(&mut self, path: &str, kind: DirEntryKind, mode: u32, content: &str) {
            let mut entry = dir_entry(path.rsplit('/').next().unwrap(), kind, mode);
            entry.size = content.len() as u64;
            self.nodes
                .insert(path.to_string(), (entry, content.to_string()));
        }
    }

    impl GuestTree for Fake {
        async fn stat(&self, path: &str) -> Result<DirEntry, AgentError> {
            if path == "/" {
                return Ok(dir_entry("/", DirEntryKind::Dir, 0o755));
            }
            self.nodes.get(path).map(|(e, _)| e.clone()).ok_or_else(|| {
                AgentError::Script(format!("stat {path}: No such file or directory"))
            })
        }
        async fn list(&self, path: &str) -> Result<(Vec<DirEntry>, bool), AgentError> {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            Ok((
                self.nodes
                    .iter()
                    .filter(|(p, _)| {
                        p.strip_prefix(&prefix)
                            .is_some_and(|rest| !rest.contains('/'))
                    })
                    .map(|(_, (e, _))| e.clone())
                    .collect(),
                false,
            ))
        }
        async fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, AgentError> {
            self.single_reads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.vanished.iter().any(|p| p == path) {
                return Err(AgentError::Script(format!(
                    "reading {path}: No such file or directory (os error 2)"
                )));
            }
            if self.unreadable.iter().any(|p| p == path) {
                return Err(AgentError::Script(format!(
                    "reading {path}: Invalid argument (os error 22)"
                )));
            }
            let (entry, content) = &self.nodes[path];
            assert!(entry.size <= limit);
            Ok(content.clone().into_bytes())
        }
        async fn batch(&self, ops: Vec<Operation>) -> Result<Option<Vec<OpResult>>, AgentError> {
            if !self.batching {
                return Ok(None);
            }
            if self.batch_fails {
                return Err(AgentError::Timeout("the guest did not answer".into()));
            }
            self.batches.lock().unwrap().push(ops.len());
            let mut out = Vec::new();
            for op in ops {
                out.push(match op {
                    Operation::ReadFile { path, length, .. } => {
                        if self.vanished.contains(&path) {
                            OpResult::Failed {
                                message: format!(
                                    "reading {path}: No such file or directory (os error 2)"
                                ),
                            }
                        } else if self.unreadable.contains(&path) {
                            OpResult::Failed {
                                message: format!("reading {path}: Invalid argument (os error 22)"),
                            }
                        } else {
                            let (entry, content) = &self.nodes[&path];
                            assert!(entry.size <= length, "a batch read of a big file");
                            OpResult::FileData {
                                data: hv2_guest_agent::b64::encode(content.as_bytes()),
                                size: entry.size,
                            }
                        }
                    }
                    Operation::ReadLink { path } => match self.link(&path).await {
                        Ok(target) => OpResult::LinkTarget { target },
                        Err(e) => OpResult::Failed {
                            message: e.to_string(),
                        },
                    },
                    other => panic!("not a read: {other:?}"),
                });
            }
            Ok(Some(out))
        }
        async fn link(&self, path: &str) -> Result<String, AgentError> {
            if self.odd_links.iter().any(|p| p == path) {
                return Err(AgentError::Script(format!(
                    "reading the link {path}: its target is not valid UTF-8"
                )));
            }
            Ok(self.nodes[path].1.clone())
        }
    }

    /// Everything in a gzipped tar: (name, type, mode, link target or data).
    fn unpacked(bytes: &[u8]) -> Vec<(String, tar::EntryType, u32, String)> {
        use std::io::Read;
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        archive
            .entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                let name = entry.path().unwrap().to_string_lossy().into_owned();
                let kind = entry.header().entry_type();
                let mode = entry.header().mode().unwrap();
                let content = match entry.link_name().unwrap() {
                    Some(target) => target.to_string_lossy().into_owned(),
                    None => {
                        let mut s = String::new();
                        entry.read_to_string(&mut s).unwrap();
                        s
                    }
                };
                (name, kind, mode, content)
            })
            .collect()
    }

    fn tree() -> Fake {
        let mut g = Fake::default();
        g.add("/srv", DirEntryKind::Dir, 0o755, "");
        g.add("/srv/a.txt", DirEntryKind::File, 0o640, "hello");
        g.add("/srv/link", DirEntryKind::Symlink, 0o777, "../etc/passwd");
        g.add("/srv/sub", DirEntryKind::Dir, 0o700, "");
        g.add("/srv/sub/b", DirEntryKind::File, 0o600, "");
        g.add("/srv/tty", DirEntryKind::Other, 0o666, "");
        g.add("/proc", DirEntryKind::Dir, 0o555, "");
        g.add("/proc/1", DirEntryKind::Dir, 0o555, "");
        g.add("/bin", DirEntryKind::Dir, 0o755, "");
        g.add("/bin/sh", DirEntryKind::File, 0o755, "#!");
        g
    }

    #[tokio::test]
    async fn an_archive_holds_files_dirs_and_symlinks_with_contents_at_the_top() {
        let got = build_archive(&tree(), "/srv").await.unwrap().unwrap();
        assert_eq!(got.skipped, 1, "the device");
        let t = tar::EntryType::Regular;
        assert_eq!(
            unpacked(&got.bytes),
            vec![
                ("./".into(), tar::EntryType::Directory, 0o755, String::new()),
                ("a.txt".into(), t, 0o640, "hello".into()),
                (
                    "link".into(),
                    tar::EntryType::Symlink,
                    0o777,
                    "../etc/passwd".into()
                ),
                (
                    "sub".into(),
                    tar::EntryType::Directory,
                    0o700,
                    String::new()
                ),
                ("sub/b".into(), t, 0o600, String::new()),
            ]
        );
    }

    #[tokio::test]
    async fn an_archive_of_root_leaves_out_what_the_kernel_makes_up() {
        let got = build_archive(&tree(), "/").await.unwrap().unwrap();
        let names: Vec<_> = unpacked(&got.bytes).into_iter().map(|e| e.0).collect();
        assert_eq!(
            names,
            vec![
                "bin",
                "bin/sh",
                "srv",
                "srv/a.txt",
                "srv/link",
                "srv/sub",
                "srv/sub/b"
            ]
        );
    }

    #[tokio::test]
    async fn an_archive_refuses_what_is_not_a_directory_or_not_there() {
        let g = tree();
        let not_dir = build_archive(&g, "/srv/a.txt").await.err().unwrap();
        assert_eq!(not_dir.0, StatusCode::BAD_REQUEST);
        // A symlink to a directory is not one, as the script has it.
        let link = build_archive(&g, "/srv/link").await.err().unwrap();
        assert_eq!(link.0, StatusCode::BAD_REQUEST);
        let missing = build_archive(&g, "/nope").await.err().unwrap();
        assert_eq!(missing.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_archive_skips_a_link_the_agent_will_not_report() {
        let mut g = tree();
        g.odd_links.push("/srv/link".into());
        let got = build_archive(&g, "/srv").await.unwrap().unwrap();
        assert_eq!(got.skipped, 2);
        assert!(unpacked(&got.bytes).iter().all(|e| e.0 != "link"));
    }

    /// A tree of many small files in several directories, plus a link, a
    /// device and a file too big to batch.
    fn wide_tree() -> Fake {
        let mut g = Fake::default();
        g.add("/w", DirEntryKind::Dir, 0o755, "");
        for d in 0..4 {
            g.add(&format!("/w/d{d}"), DirEntryKind::Dir, 0o755, "");
            for f in 0..120 {
                g.add(
                    &format!("/w/d{d}/f{f:03}"),
                    DirEntryKind::File,
                    0o644,
                    &format!("file {d}/{f}"),
                );
            }
            g.add(
                &format!("/w/d{d}/zlink"),
                DirEntryKind::Symlink,
                0o777,
                "f000",
            );
        }
        g.add("/w/tty", DirEntryKind::Other, 0o666, "");
        g.add("/w/big", DirEntryKind::File, 0o644, "x");
        g.nodes.get_mut("/w/big").unwrap().0.size = BATCH_READ_FILE + 1;
        g
    }

    fn singles(g: &Fake) -> usize {
        g.single_reads.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[tokio::test]
    async fn batched_reads_give_the_same_archive_in_far_fewer_round_trips() {
        // The slow way: no batch operation.
        let plain = wide_tree();
        let want = build_archive(&plain, "/w").await.unwrap().unwrap();
        assert_eq!(singles(&plain), 4 * 120 + 1);

        let mut fast = wide_tree();
        fast.batching = true;
        let got = build_archive(&fast, "/w").await.unwrap().unwrap();
        assert_eq!(unpacked(&got.bytes), unpacked(&want.bytes));
        assert_eq!(got.skipped, 1);
        let batches = fast.batches.lock().unwrap().clone();
        // 480 files and 4 links in a handful of batches; the big file alone.
        assert_eq!(batches.iter().sum::<usize>(), 4 * 120 + 4);
        assert!(batches.len() <= 6, "{batches:?}");
        assert!(batches.iter().all(|n| *n <= BATCH_READ_OPS));
        assert_eq!(singles(&fast), 1, "only the file too big to batch");
    }

    #[tokio::test]
    async fn a_batch_that_fails_or_is_unsupported_falls_back_to_one_at_a_time() {
        let want = build_archive(&wide_tree(), "/w").await.unwrap().unwrap();
        // Failing: the walk reads one at a time and still finishes.
        let mut g = wide_tree();
        g.batching = true;
        g.batch_fails = true;
        let got = build_archive(&g, "/w").await.unwrap().unwrap();
        assert_eq!(unpacked(&got.bytes), unpacked(&want.bytes));
        assert_eq!(singles(&g), 4 * 120 + 1);
        assert!(g.batches.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_batched_walk_drops_a_vanished_file_and_a_non_utf8_link_as_before() {
        let mut g = wide_tree();
        g.batching = true;
        g.vanished.push("/w/d1/f007".into());
        g.odd_links.push("/w/d2/zlink".into());
        let got = build_archive(&g, "/w").await.unwrap().unwrap();
        let names: Vec<String> = unpacked(&got.bytes).into_iter().map(|e| e.0).collect();
        assert!(!names.contains(&"d1/f007".to_string()));
        assert!(names.contains(&"d1/f006".to_string()) && names.contains(&"d1/f008".to_string()));
        assert!(!names.contains(&"d2/zlink".to_string()));
        assert!(names.contains(&"d1/zlink".to_string()));
        assert_eq!(got.skipped, 2, "the device and the odd link");
    }

    #[tokio::test]
    async fn a_file_the_guest_cannot_read_is_counted_not_fatal() {
        for batching in [false, true] {
            let mut g = wide_tree();
            g.batching = batching;
            g.unreadable.push("/w/d1/f007".into());
            let got = build_archive(&g, "/w").await.unwrap().unwrap();
            let names: Vec<String> = unpacked(&got.bytes).into_iter().map(|e| e.0).collect();
            assert!(
                !names.contains(&"d1/f007".to_string()),
                "batching {batching}"
            );
            assert!(
                names.contains(&"d1/f008".to_string()),
                "batching {batching}"
            );
            assert_eq!(got.skipped, 2, "the device and the unreadable file");
        }
    }

    #[tokio::test]
    async fn a_batched_walk_still_refuses_what_is_over_a_cap() {
        let mut g = wide_tree();
        g.batching = true;
        g.nodes.get_mut("/w/big").unwrap().0.size = MAX_ARCHIVE_FILE + 1;
        let err = build_archive(&g, "/w").await.err().unwrap();
        assert_eq!(err.0, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn a_prefetched_answer_is_used_only_when_whole_and_within_the_limit() {
        let ok = |size: u64, text: &str| OpResult::FileData {
            data: hv2_guest_agent::b64::encode(text.as_bytes()),
            size,
        };
        assert_eq!(prefetched_file(ok(2, "hi"), 100).unwrap().unwrap(), b"hi");
        // Over the walk's limit: the walk's own read says so.
        assert!(prefetched_file(ok(2, "hi"), 1).is_none());
        // The file grew past what was asked for, or less came than its size.
        assert!(prefetched_file(ok(BATCH_READ_FILE + 1, "hi"), u64::MAX).is_none());
        assert!(prefetched_file(ok(5, "hi"), 100).is_none());
        assert!(prefetched_file(OpResult::Acknowledged, 100).is_none());
        let failed = prefetched_file(
            OpResult::Failed {
                message: "No such file".into(),
            },
            100,
        )
        .unwrap();
        assert!(failed.unwrap_err().to_string().contains("No such file"));
        assert_eq!(
            prefetched_link(OpResult::LinkTarget { target: "t".into() })
                .unwrap()
                .unwrap(),
            "t"
        );
        assert!(prefetched_link(OpResult::Acknowledged).is_none());
    }

    #[tokio::test]
    async fn an_archive_over_a_cap_is_a_413() {
        let mut g = tree();
        g.add("/srv/big", DirEntryKind::File, 0o644, "x");
        g.nodes.get_mut("/srv/big").unwrap().0.size = MAX_ARCHIVE_FILE + 1;
        let err = build_archive(&g, "/srv").await.err().unwrap();
        assert_eq!(err.0, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn an_agent_that_predates_an_operation_means_use_the_shell() {
        struct Old;
        impl GuestTree for Old {
            async fn stat(&self, _: &str) -> Result<DirEntry, AgentError> {
                Err(AgentError::Script(format!(
                    "{} stat (it closed the connection on the request)",
                    hv2_agent::UNSUPPORTED_OP_PREFIX
                )))
            }
            async fn list(&self, _: &str) -> Result<(Vec<DirEntry>, bool), AgentError> {
                unreachable!()
            }
            async fn read(&self, _: &str, _: u64) -> Result<Vec<u8>, AgentError> {
                unreachable!()
            }
            async fn link(&self, _: &str) -> Result<String, AgentError> {
                unreachable!()
            }
        }
        assert!(build_archive(&Old, "/").await.unwrap().is_none());
    }

    #[test]
    fn a_path_is_absolute() {
        assert!(refused_path("etc").is_some());
        assert!(refused_path("/a\0b").is_some());
        assert!(refused_path(&format!("/{}", "a".repeat(4096))).is_some());
        assert!(refused_path("/").is_none());
        assert!(refused_path("/etc/hostname").is_none());
    }
}
