//! A sandbox's commands, kept with their output after they end, to be
//! listed and read again: `GET /sandboxes/{id}/commands` and
//! `GET /sandboxes/{id}/commands/{n}?stdout=N&stderr=N`.
//!
//! Every command run through the API: `POST /sandboxes/{id}/exec`, and each
//! process started through envd's `process.Process/Start`, which is what
//! E2B's SDK runs `commands.run` with. Each is kept as it was started -- its
//! command line, user, tag and when -- and how it ended, with the newest
//! [`MAX_STREAM`] bytes of each stream it printed and a count of what went
//! before. Not its environment, which is where a caller puts a key.
//!
//! - **The newest [`MAX_COMMANDS`]** are kept; the oldest finished one makes
//!   room.
//! - **Kept through a pause**, here or, through a snapshot store, on the
//!   node that resumes it; and through a reboot, a resize and a restored
//!   checkpoint. Gone when the sandbox ends.
//! - **A command running when its sandbox pauses** is recorded as ended
//!   then, with what it had printed: the node stops watching it there,
//!   though the guest's process goes on after the resume.
//!
//! # Secrets
//!
//! A command line can carry one. Before it is kept, [`redact`] masks the
//! common shapes of one: `NAME=VALUE` for a name like `*TOKEN*`, `*SECRET*`,
//! `*PASSWORD*` or `*KEY*`; the value after `--password`, `--token` and their
//! kind; what follows `Bearer` or `Basic`; and the credentials in a URL. It
//! cannot know every secret: a sandbox created with `"commandHistory": false`,
//! or every sandbox on a node started with `--no-command-history`, keeps no
//! history at all. Output is kept as printed, not scanned.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{api_error, now_ms, AppState};

mod output;

/// The most commands kept for one sandbox.
pub(crate) const MAX_COMMANDS: usize = 64;

/// The most of each of a command's streams that is kept.
pub(crate) const MAX_STREAM: usize = 64 << 10;

/// One stream of a command's output: its newest bytes, and how many went
/// before them.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Stream {
    kept: String,
    dropped: u64,
}

impl Stream {
    fn total(&self) -> u64 {
        self.dropped + self.kept.len() as u64
    }

    /// `text`, after `gap` bytes that were written and never seen.
    fn append(&mut self, text: &str, gap: u64) {
        if gap > 0 {
            // The kept part is one unbroken piece of the stream.
            self.dropped = self.total() + gap;
            self.kept.clear();
        }
        self.kept.push_str(text);
        if self.kept.len() > MAX_STREAM {
            let mut cut = self.kept.len() - MAX_STREAM;
            while !self.kept.is_char_boundary(cut) {
                cut += 1;
            }
            self.kept.drain(..cut);
            self.dropped += cut as u64;
        }
    }

    /// What was written from `offset` on, where that really starts, and the
    /// offset to ask from next.
    fn from(&self, offset: u64) -> (u64, &str, u64) {
        let total = self.total();
        let mut at = offset.min(total).saturating_sub(self.dropped) as usize;
        while !self.kept.is_char_boundary(at) {
            at += 1;
        }
        (self.dropped + at as u64, &self.kept[at..], total)
    }
}

/// One command, as it was started and as it ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Command {
    n: u64,
    /// `exec`, or `process` for one started through envd.
    source: String,
    /// The command line, its arguments after it with spaces between.
    cmd: String,
    cwd: Option<String>,
    user: Option<String>,
    tag: Option<String>,
    pid: Option<u32>,
    pty: bool,
    started_at: u64,
    ended_at: Option<u64>,
    /// `running`, `exited`, `signalled`, or `lost` with `error` saying why.
    state: String,
    exit_code: Option<i32>,
    signal: Option<i32>,
    error: Option<String>,
    stdout: Stream,
    stderr: Stream,
}

impl Command {
    fn describe(&self) -> Value {
        json!({
            "n": self.n,
            "source": self.source,
            "cmd": self.cmd,
            "cwd": self.cwd,
            "user": self.user,
            "tag": self.tag,
            "pid": self.pid,
            "pty": self.pty,
            "state": self.state,
            "exitCode": self.exit_code,
            "signal": self.signal,
            "error": self.error,
            "startedAt": self.started_at,
            "endedAt": self.ended_at,
            "stdoutBytes": self.stdout.total(),
            "stderrBytes": self.stderr.total(),
        })
    }

    fn end(&mut self, exit_code: Option<i32>, signal: Option<i32>, error: Option<&str>) {
        if self.state != "running" {
            return;
        }
        self.ended_at = Some(now_ms());
        self.exit_code = exit_code;
        self.signal = signal;
        self.error = error.map(str::to_string);
        self.state = if error.is_some() {
            "lost"
        } else if signal.is_some() {
            "signalled"
        } else {
            "exited"
        }
        .to_string();
    }
}

#[derive(Default)]
struct Sandbox {
    last: u64,
    commands: BTreeMap<u64, Command>,
}

/// Every sandbox's commands on this node, by sandbox ID.
static HISTORY: StdMutex<Option<HashMap<String, Sandbox>>> = StdMutex::new(None);

fn history<T>(f: impl FnOnce(&mut HashMap<String, Sandbox>) -> T) -> T {
    let mut guard = HISTORY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// Whether this node keeps command history at all.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Sandboxes created with `"commandHistory": false`, and their forks.
static OFF: StdMutex<Option<HashSet<String>>> = StdMutex::new(None);

fn off<T>(f: impl FnOnce(&mut HashSet<String>) -> T) -> T {
    let mut guard = OFF
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashSet::new))
}

/// Keep no history on this node: `--no-command-history`.
pub(crate) fn disable_node() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// Keep no history for `sandbox_id`, and forget what was kept.
pub(crate) fn turn_off(sandbox_id: &str) {
    off(|set| set.insert(sandbox_id.to_string()));
    history(|all| all.remove(sandbox_id));
}

/// Whether `sandbox_id` keeps no history, by its own choice or the node's.
pub(crate) fn is_off(sandbox_id: &str) -> bool {
    !ENABLED.load(Ordering::Relaxed) || off(|set| set.contains(sandbox_id))
}

/// A fork keeps no history if its source kept none.
pub(crate) fn inherit_off(source: &str, fork: &str) {
    if off(|set| set.contains(source)) {
        turn_off(fork);
    }
}

/// Names whose value is a secret, matched within a name, any case.
const SECRET_NAMES: [&str; 8] = [
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "KEY",
    "CREDENTIAL",
    "AUTH",
    "COOKIE",
];

/// Flags whose next word is a secret.
const SECRET_FLAGS: [&str; 10] = [
    "--password",
    "--passwd",
    "--pass",
    "--token",
    "--secret",
    "--api-key",
    "--apikey",
    "--access-key",
    "--secret-key",
    "--auth",
];

const MASK: &str = "[redacted]";

fn secret_name(name: &str) -> bool {
    let name = name.trim_start_matches('-').to_ascii_uppercase();
    !name.is_empty() && SECRET_NAMES.iter().any(|secret| name.contains(secret))
}

/// The quote `value` opens and does not close, if any: what is masked
/// runs on to it.
fn unclosed_quote(value: &str) -> Option<char> {
    let quote = value.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    (value.len() < 2 || !value.ends_with(quote)).then_some(quote)
}

/// `token`, masked if it is a secret by its shape or by the word before it.
fn mask(token: &str, previous: Option<&str>, in_quote: &mut Option<char>) -> String {
    // The rest of a quoted secret begun in an earlier word.
    if let Some(quote) = *in_quote {
        if token.ends_with(quote) {
            *in_quote = None;
        }
        return String::new();
    }
    // Credentials in a URL: scheme://user:password@host, or a token as user.
    if let Some(at_scheme) = token.find("://") {
        let after = at_scheme + 3;
        let host_end = token[after..].find('/').map_or(token.len(), |i| after + i);
        if let Some(at) = token[after..host_end].rfind('@') {
            return format!("{}{MASK}{}", &token[..after], &token[after + at..]);
        }
    }
    // NAME=VALUE, or --name=VALUE.
    if let Some((name, value)) = token.split_once('=') {
        if secret_name(name) && !value.is_empty() {
            *in_quote = unclosed_quote(value);
            // Shell punctuation after the value is the line's, not the
            // secret's: `TOKEN=x; next` stays two commands.
            let tail: String = value
                .chars()
                .rev()
                .take_while(|c| matches!(c, ';' | '&' | '|' | ')'))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return format!("{name}={MASK}{tail}");
        }
    }
    let previous = previous.map(|p| {
        p.trim_matches(|c| c == '\'' || c == '"')
            .to_ascii_lowercase()
    });
    let after_flag = previous
        .as_deref()
        .is_some_and(|p| SECRET_FLAGS.contains(&p));
    let after_scheme = previous
        .as_deref()
        .is_some_and(|p| p == "bearer" || p == "basic");
    if after_flag || after_scheme {
        *in_quote = unclosed_quote(token);
        // A quote that closes the whole argument is kept, so the line reads
        // as it was written.
        let closing: String = token
            .chars()
            .rev()
            .take_while(|c| *c == '\'' || *c == '"')
            .collect();
        return format!("{MASK}{closing}");
    }
    token.to_string()
}

/// `command` with the common shapes of a secret masked; its spacing kept.
pub(crate) fn redact(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    let mut previous: Option<&str> = None;
    let mut in_quote = None;
    loop {
        let Some(start) = rest.find(|c: char| !c.is_whitespace()) else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..end];
        // A word inside a masked quote goes, and the space before it.
        let continuing = in_quote.is_some();
        let masked = mask(token, previous, &mut in_quote);
        if continuing {
            out.truncate(out.trim_end().len());
        }
        out.push_str(&masked);
        previous = Some(token);
        rest = &rest[end..];
    }
}

/// What a command is started as.
pub(crate) struct Begun<'a> {
    pub(crate) source: &'a str,
    pub(crate) cmd: String,
    pub(crate) cwd: Option<&'a str>,
    pub(crate) user: Option<&'a str>,
    pub(crate) tag: Option<&'a str>,
    pub(crate) pid: Option<u32>,
    pub(crate) pty: bool,
}

/// Record a command `sandbox_id` started, its line redacted; its number,
/// or 0 when the sandbox keeps no history.
pub(crate) fn begin(sandbox_id: &str, begun: Begun<'_>) -> u64 {
    if is_off(sandbox_id) {
        return 0;
    }
    let begun = Begun {
        cmd: redact(&begun.cmd),
        ..begun
    };
    history(|all| {
        let sandbox = all.entry(sandbox_id.to_string()).or_default();
        // The oldest that has ended makes room; one still running is kept,
        // and the oldest of all goes if every one is.
        while sandbox.commands.len() >= MAX_COMMANDS {
            let oldest = sandbox
                .commands
                .values()
                .find(|c| c.state != "running")
                .or_else(|| sandbox.commands.values().next())
                .map(|c| c.n);
            if let Some(n) = oldest {
                sandbox.commands.remove(&n);
            }
        }
        sandbox.last += 1;
        let n = sandbox.last;
        sandbox.commands.insert(
            n,
            Command {
                n,
                source: begun.source.to_string(),
                cmd: begun.cmd,
                cwd: begun.cwd.map(str::to_string),
                user: begun.user.map(str::to_string),
                tag: begun.tag.map(str::to_string),
                pid: begun.pid,
                pty: begun.pty,
                started_at: now_ms(),
                ended_at: None,
                state: "running".to_string(),
                exit_code: None,
                signal: None,
                error: None,
                stdout: Stream::default(),
                stderr: Stream::default(),
            },
        );
        n
    })
}

fn with<T>(sandbox_id: &str, n: u64, f: impl FnOnce(&mut Command) -> T) -> Option<T> {
    history(|all| all.get_mut(sandbox_id)?.commands.get_mut(&n).map(f))
}

/// It printed.
pub(crate) fn printed(sandbox_id: &str, n: u64, stdout: &str, stderr: &str, dropped: (u64, u64)) {
    with(sandbox_id, n, |c| {
        c.stdout.append(stdout, dropped.0);
        c.stderr.append(stderr, dropped.1);
    });
}

/// It ended.
pub(crate) fn ended(
    sandbox_id: &str,
    n: u64,
    exit_code: Option<i32>,
    signal: Option<i32>,
    error: Option<&str>,
) {
    with(sandbox_id, n, |c| c.end(exit_code, signal, error));
}

/// The sandbox ended: its commands go with it.
pub(crate) fn forget(sandbox_id: &str) {
    history(|all| all.remove(sandbox_id));
    off(|set| set.remove(sandbox_id));
}

/// Its commands, to go into a snapshot store with it when it pauses.
pub(crate) fn export(sandbox_id: &str) -> Vec<Command> {
    history(|all| {
        all.get(sandbox_id)
            .map(|s| s.commands.values().cloned().collect())
            .unwrap_or_default()
    })
}

/// Its commands, from a snapshot store, when another node paused it. One
/// this node already keeps for it stays as it is.
pub(crate) fn import(sandbox_id: &str, commands: Vec<Command>) {
    if commands.is_empty() {
        return;
    }
    history(|all| {
        let sandbox = all.entry(sandbox_id.to_string()).or_default();
        for command in commands {
            sandbox.last = sandbox.last.max(command.n);
            sandbox.commands.entry(command.n).or_insert(command);
        }
    });
}

/// Tells the history of each process a sandbox's envd starts.
pub(crate) struct Observer {
    pub(crate) sandbox_id: String,
}

impl hv2_api::envd_process::ProcessObserver for Observer {
    fn started(&self, started: &hv2_api::envd_process::Started<'_>) -> u64 {
        let mut cmd = started.cmd.to_string();
        for arg in started.args {
            cmd.push(' ');
            cmd.push_str(arg);
        }
        begin(
            &self.sandbox_id,
            Begun {
                source: "process",
                cmd,
                cwd: started.cwd,
                user: started.user,
                tag: started.tag,
                pid: Some(started.pid),
                pty: started.pty,
            },
        )
    }

    fn printed(&self, key: u64, stdout: &str, stderr: &str, dropped: (u64, u64)) {
        printed(&self.sandbox_id, key, stdout, stderr, dropped);
    }

    fn ended(&self, key: u64, exit_code: Option<i32>, signal: Option<i32>, error: Option<&str>) {
        ended(&self.sandbox_id, key, exit_code, signal, error);
    }
}

/// Whether `sandbox_id` is a sandbox here, running or paused.
/// Why `sandbox_id`'s history cannot be read, when it is off.
fn refused_off(sandbox_id: &str) -> Option<Response> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Some(api_error(
            StatusCode::CONFLICT,
            "this node keeps no command history (--no-command-history)",
        ));
    }
    off(|set| set.contains(sandbox_id)).then(|| {
        api_error(
            StatusCode::CONFLICT,
            format!("{sandbox_id} was created with commandHistory off, and keeps none"),
        )
    })
}

fn known(state: &AppState, sandbox_id: &str) -> bool {
    state.sandboxes.lock().contains_key(sandbox_id) || state.paused.lock().contains_key(sandbox_id)
}

/// `GET /sandboxes/{id}/commands`: its commands, oldest first, without
/// their output.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
) -> Response {
    if !known(&state, &sandbox_id) {
        return api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"));
    }
    if let Some(refused) = refused_off(&sandbox_id) {
        return refused;
    }
    let listed: Vec<Value> = history(|all| {
        all.get(&sandbox_id)
            .map(|s| s.commands.values().map(Command::describe).collect())
            .unwrap_or_default()
    });
    Json(listed).into_response()
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Offsets {
    stdout: Option<u64>,
    stderr: Option<u64>,
}

/// `GET /sandboxes/{id}/commands/{n}`: one, with what it printed from the
/// offsets asked for.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    Path((sandbox_id, n)): Path<(String, u64)>,
    Query(offsets): Query<Offsets>,
) -> Response {
    if !known(&state, &sandbox_id) {
        return api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"));
    }
    if let Some(refused) = refused_off(&sandbox_id) {
        return refused;
    }
    let found = history(|all| {
        let command = all.get(&sandbox_id)?.commands.get(&n)?;
        let mut reply = command.describe();
        for (key, stream, asked) in [
            ("stdout", &command.stdout, offsets.stdout),
            ("stderr", &command.stderr, offsets.stderr),
        ] {
            let (offset, text, next) = stream.from(asked.unwrap_or(0));
            reply[key] = json!({
                "offset": offset,
                "text": output::redact(text),
                "next": next,
                "dropped": stream.dropped,
            });
        }
        Some(reply)
    });
    match found {
        Some(reply) => Json(reply).into_response(),
        None => api_error(
            StatusCode::NOT_FOUND,
            format!("sandbox {sandbox_id} has no command {n}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn begun(cmd: &str) -> Begun<'_> {
        Begun {
            source: "exec",
            cmd: cmd.to_string(),
            cwd: None,
            user: None,
            tag: None,
            pid: None,
            pty: false,
        }
    }

    #[test]
    fn the_shapes_of_a_secret_are_masked() {
        for (given, kept) in [
            ("TOKEN=abc123 make deploy", "TOKEN=[redacted] make deploy"),
            (
                "export GITHUB_TOKEN=ghp_x; git push",
                "export GITHUB_TOKEN=[redacted]; git push",
            ),
            ("api_key=k1 run", "api_key=[redacted] run"),
            (
                "app --password hunter2 --user me",
                "app --password [redacted] --user me",
            ),
            ("app --password=hunter2", "app --password=[redacted]"),
            ("app --token 'a b c' next", "app --token [redacted] next"),
            ("PASSWORD=\"two words\" run", "PASSWORD=[redacted] run"),
            (
                "curl -H \"Authorization: Bearer abc.def\" https://x",
                "curl -H \"Authorization: Bearer [redacted]\" https://x",
            ),
            (
                "git clone https://user:pw@git.example/r.git",
                "git clone https://[redacted]@git.example/r.git",
            ),
            (
                "git clone https://ghp_tok@github.com/o/r",
                "git clone https://[redacted]@github.com/o/r",
            ),
            // Not a secret: kept as it is, spacing too.
            ("echo  hello   world", "echo  hello   world"),
            ("ls -la /srv/app", "ls -la /srv/app"),
            (
                "curl https://example.com/a@b",
                "curl https://example.com/a@b",
            ),
        ] {
            assert_eq!(redact(given), kept, "{given}");
        }
    }

    #[test]
    fn a_sandbox_with_history_off_keeps_none_and_its_fork_neither() {
        let (id, fork) = ("sbx-history-off", "sbx-history-off-fork");
        let n = begin(id, begun("before"));
        assert!(n > 0);
        turn_off(id);
        assert!(
            export(id).is_empty(),
            "turning it off forgets what was kept"
        );
        assert_eq!(begin(id, begun("after")), 0);
        assert!(export(id).is_empty());
        inherit_off(id, fork);
        assert_eq!(begin(fork, begun("in the fork")), 0);
        forget(id);
        forget(fork);
        assert!(!is_off(id), "a sandbox that ended is forgotten, off or not");
    }

    #[test]
    fn a_command_line_is_kept_redacted() {
        let id = "sbx-history-redacted";
        begin(id, begun("deploy --token s3cr3t"));
        assert_eq!(export(id)[0].cmd, "deploy --token [redacted]");
        forget(id);
    }

    #[test]
    fn a_command_is_kept_as_it_ended() {
        let id = "sbx-history-a";
        let n = begin(id, begun("echo hi"));
        printed(id, n, "hi\n", "", (0, 0));
        ended(id, n, Some(0), None, None);
        let kept = export(id);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            (kept[0].state.as_str(), kept[0].exit_code),
            ("exited", Some(0))
        );
        assert_eq!(kept[0].stdout.from(0), (0, "hi\n", 3));
        // Ending twice changes nothing.
        ended(id, n, Some(9), None, Some("late"));
        assert_eq!(export(id)[0].exit_code, Some(0));
        forget(id);
        assert!(export(id).is_empty());
    }

    #[test]
    fn a_stream_keeps_its_newest_and_counts_the_rest() {
        let mut stream = Stream::default();
        stream.append(&"x".repeat(MAX_STREAM), 0);
        stream.append("tail", 0);
        assert_eq!(stream.total(), MAX_STREAM as u64 + 4);
        assert_eq!(stream.dropped, 4);
        assert!(stream.from(0).1.ends_with("tail"));
        stream.append("after", 100);
        assert_eq!(
            stream.from(0),
            (
                MAX_STREAM as u64 + 4 + 100,
                "after",
                MAX_STREAM as u64 + 109
            )
        );
    }

    #[test]
    fn the_oldest_finished_makes_room() {
        let id = "sbx-history-b";
        let first = begin(id, begun("sleep 1000"));
        for _ in 1..MAX_COMMANDS {
            let n = begin(id, begun("true"));
            ended(id, n, Some(0), None, None);
        }
        let next = begin(id, begun("true"));
        let kept: Vec<u64> = export(id).iter().map(|c| c.n).collect();
        assert_eq!(kept.len(), MAX_COMMANDS);
        // The running one stays; the oldest finished, number 2, went.
        assert!(kept.contains(&first) && kept.contains(&next) && !kept.contains(&2));
        forget(id);
    }

    #[test]
    fn a_history_crosses_a_store_and_numbers_go_on() {
        let id = "sbx-history-c";
        let n = begin(id, begun("make"));
        ended(id, n, None, Some(9), None);
        let carried = serde_json::to_value(export(id)).unwrap();
        forget(id);
        import(id, serde_json::from_value(carried).unwrap());
        let back = export(id);
        assert_eq!(back[0].state, "signalled");
        assert_eq!(begin(id, begun("next")), n + 1);
        forget(id);
    }
}
