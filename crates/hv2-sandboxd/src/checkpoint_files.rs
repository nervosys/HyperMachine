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

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use hv2_agent::{is_unsupported_op, AgentError, AgentVM};
use hv2_guest_agent::{DirEntry, DirEntryKind};
use serde::Deserialize;
use serde_json::{json, Value};

use super::guest_setup::{guest_has_no_shell, no_shell_message};
use super::{api_error, new_vm, AppState, TEMPLATE_CID, TEMPLATE_MAC};

/// The largest file read out of a checkpoint, as for a machine's snapshot.
const MAX_FILE: u64 = 64 << 20;

/// The largest archive downloaded: the compressed tar, as built in the
/// reader's guest, whose memory it takes.
const MAX_ARCHIVE: u64 = 64 << 20;

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
        // The agent can signal only what it started, so stopping the
        // checkpoint's programs takes a shell. A guest with none is read
        // unfrozen: its programs are whatever its agent started.
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
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/gzip".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}.tar.gz\"", archive_name(&name)),
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
    fn a_path_is_absolute() {
        assert!(refused_path("etc").is_some());
        assert!(refused_path("/a\0b").is_some());
        assert!(refused_path(&format!("/{}", "a".repeat(4096))).is_some());
        assert!(refused_path("/").is_none());
        assert!(refused_path("/etc/hostname").is_none());
    }
}
