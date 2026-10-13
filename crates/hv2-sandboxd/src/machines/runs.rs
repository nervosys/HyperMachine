//! Runs: a command started in a machine and left running, whose output is
//! kept and can be read afterwards.
//!
//! `POST /machines/{name}/exec` waits for a command and answers with what it
//! printed. That is right for a command that takes a second and wrong for one
//! that takes an hour: nothing is seen until it ends, a caller that goes away
//! loses it, and it cannot be told anything or stopped. A run is the other
//! shape:
//!
//! - `POST /machines/{name}/runs` `{"cmd", "cwd", "env", "user", "stdin"}`
//!   starts one and answers at once with its ID.
//! - `GET  /machines/{name}/runs` lists them, newest last, without output.
//! - `GET  /machines/{name}/runs/{id}?stdout=N&stderr=N` is one run, and what
//!   it printed from those offsets on. The reply says where to read from
//!   next, so a caller follows a run by asking again.
//! - `POST /machines/{name}/runs/{id}/stdin` `{"data", "close"}` writes to it.
//! - `POST /machines/{name}/runs/{id}/signal` `{"signal"}` signals it.
//!
//! # Where the output is
//!
//! On the node, in memory. The guest agent hands a program's output over as
//! it is polled, and a task per run polls and keeps it: up to [`MAX_LOG`]
//! bytes of each stream, the oldest dropped first and counted, so an offset
//! from before what was dropped is answered with where the kept part starts.
//! A run's output outlives the run and the machine's stop, and is gone when
//! the node restarts or the machine is deleted. At most [`MAX_RUNS`] are
//! kept for a machine; the oldest finished one makes room.
//!
//! # What ends a run
//!
//! Its program exiting, or being signalled. A machine that stops takes its
//! programs with it, and their runs end as `lost`, saying so, with whatever
//! they had printed.
//!
//! # What is not kept
//!
//! The environment a run was given. It is where a caller puts a key, and a
//! run's record is returned to whoever can ask about the machine.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use axum::http::HeaderMap;
use serde_json::{json, Value};

use super::super::{
    api_error, now_ms, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State,
    StatusCode,
};
use super::{live, named, running, AgentVM};

/// The most of each of a run's output streams that is kept.
pub(crate) const MAX_LOG: usize = 4 << 20;

/// The most runs kept for one machine.
pub(crate) const MAX_RUNS: usize = 256;

/// How long the guest agent has to answer one request about a run.
const AGENT_TIMEOUT: Duration = Duration::from_secs(15);

/// One stream of a run's output: the newest [`MAX_LOG`] bytes of it, and how
/// many were dropped from the front to keep it so.
#[derive(Debug, Default)]
struct Log {
    kept: String,
    dropped: u64,
}

impl Log {
    /// Every byte the program wrote to this stream, kept or not.
    fn total(&self) -> u64 {
        self.dropped + self.kept.len() as u64
    }

    fn append(&mut self, text: &str) {
        self.kept.push_str(text);
        if self.kept.len() > MAX_LOG {
            let mut cut = self.kept.len() - MAX_LOG;
            // Text is cut between characters, never inside one.
            while !self.kept.is_char_boundary(cut) {
                cut += 1;
            }
            self.kept.drain(..cut);
            self.dropped += cut as u64;
        }
    }

    /// What was written from `offset` on, the offset that text really starts
    /// at, and the offset to ask from next.
    ///
    /// The text starts later than asked when what was asked for has been
    /// dropped, or when the offset falls inside a character.
    fn from(&self, offset: u64) -> (u64, &str, u64) {
        let total = self.total();
        let offset = offset.min(total);
        let mut at = offset.saturating_sub(self.dropped) as usize;
        while !self.kept.is_char_boundary(at) {
            at += 1;
        }
        (self.dropped + at as u64, &self.kept[at..], total)
    }
}

/// How a run stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RunState {
    Running,
    /// Its program ended: with an exit code, or by a signal.
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
    /// The node can no longer reach it, and why. Its program may have been
    /// taken down with its machine.
    Lost(String),
}

pub(super) struct Run {
    id: u64,
    cmd: String,
    pid: u32,
    started_ms: u64,
    ended_ms: Option<u64>,
    state: RunState,
    stdout: Log,
    stderr: Log,
}

impl Run {
    /// What it printed on standard output from `offset` on: where that text
    /// really starts, the text, and every byte it has written so far.
    pub(super) fn stdout_from(&self, offset: u64) -> (u64, &str, u64) {
        self.stdout.from(offset)
    }

    /// The last `most` bytes it printed on standard error, cut between
    /// characters.
    pub(super) fn stderr_tail(&self, most: usize) -> &str {
        let kept = &self.stderr.kept;
        let mut at = kept.len().saturating_sub(most);
        while !kept.is_char_boundary(at) {
            at += 1;
        }
        &kept[at..]
    }

    /// Whether its program is still going.
    pub(super) fn is_running(&self) -> bool {
        self.state == RunState::Running
    }

    /// The run without its output.
    pub(super) fn describe(&self) -> Value {
        let (state, code, signal, reason) = match &self.state {
            RunState::Running => ("running", None, None, None),
            RunState::Exited { code, signal } => ("exited", *code, *signal, None),
            RunState::Lost(why) => ("lost", None, None, Some(why.as_str())),
        };
        json!({
            "runID": self.id,
            "cmd": self.cmd,
            "pid": self.pid,
            "state": state,
            "exitCode": code,
            "signal": signal,
            "reason": reason,
            "startedAt": self.started_ms,
            "endedAt": self.ended_ms,
            "stdoutBytes": self.stdout.total(),
            "stderrBytes": self.stderr.total(),
        })
    }

    fn end(&mut self, state: RunState) {
        if self.state == RunState::Running {
            self.state = state;
            self.ended_ms = Some(now_ms());
        }
    }
}

#[derive(Default)]
struct MachineRuns {
    last_id: u64,
    runs: BTreeMap<u64, Run>,
}

impl MachineRuns {
    /// Make room for one more: the oldest finished run goes. With every one
    /// still running there is no room, and the caller is told.
    fn make_room(&mut self) -> Result<(), String> {
        while self.runs.len() >= MAX_RUNS {
            let oldest_finished = self
                .runs
                .values()
                .find(|run| run.state != RunState::Running)
                .map(|run| run.id);
            match oldest_finished {
                Some(id) => {
                    self.runs.remove(&id);
                }
                None => {
                    return Err(format!(
                        "this machine has {MAX_RUNS} runs still running, the most it keeps"
                    ))
                }
            }
        }
        Ok(())
    }
}

/// Every machine's runs on this node, by machine ID.
static RUNS: StdMutex<Option<HashMap<String, MachineRuns>>> = StdMutex::new(None);

fn runs<T>(f: impl FnOnce(&mut HashMap<String, MachineRuns>) -> T) -> T {
    let mut guard = RUNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// Forget a machine's runs, when it is deleted.
pub(crate) fn forget(machine_id: &str) {
    runs(|all| all.remove(machine_id));
}

/// Look at one run of a machine, if it is kept.
pub(super) fn with_run<T>(machine_id: &str, id: u64, f: impl FnOnce(&Run) -> T) -> Option<T> {
    runs(|all| all.get(machine_id)?.runs.get(&id).map(f))
}

/// A signal by its name, with or without `SIG`, or by its number.
fn signal_number(name: &str) -> Option<i32> {
    let upper = name.trim().to_ascii_uppercase();
    let short = upper.strip_prefix("SIG").unwrap_or(&upper);
    Some(match short {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "KILL" => 9,
        "USR1" => 10,
        "USR2" => 12,
        "TERM" => 15,
        "CONT" => 18,
        "STOP" => 19,
        other => other.parse().ok().filter(|n| (1..=64).contains(n))?,
    })
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewRun {
    /// A shell command line, run by the guest's `/bin/sh -c`.
    pub(super) cmd: String,
    pub(super) cwd: Option<String>,
    /// Set for the command, beside what the guest's own environment has.
    /// Not kept, and not returned.
    #[serde(default)]
    pub(super) env: BTreeMap<String, String>,
    /// The guest user to run it as; root unless given.
    pub(super) user: Option<String>,
    /// Written to its standard input, which is then closed. Leave it out to
    /// keep standard input open for `.../stdin`.
    pub(super) stdin: Option<String>,
}

// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn agent_of(state: &AppState, headers: &HeaderMap, name: &str) -> Result<Arc<AgentVM>, Response> {
    match running(state, headers, name)? {
        (vm, true) => Ok(vm),
        (_, false) => Err(api_error(
            StatusCode::CONFLICT,
            format!(
                "machine {name} has no guest agent answering, and a run is started through \
                 one; see its exec"
            ),
        )),
    }
}

/// `POST /machines/{name}/runs`.
pub(crate) async fn start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(req): Json<NewRun>,
) -> Response {
    match launch(&state, &headers, &name, req).await {
        Ok((_, _, described)) => (StatusCode::CREATED, Json(described)).into_response(),
        Err(response) => response,
    }
}

/// Start a run in the machine `name`: the machine's ID, the run's, and the
/// run as it is described. What [`start`] does, for a caller in this crate
/// that starts runs of its own.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
pub(super) async fn launch(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    name: &str,
    req: NewRun,
) -> Result<(String, u64, Value), Response> {
    if req.cmd.trim().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "cmd is empty"));
    }
    let machine = named(state, headers, name)?;
    let vm = agent_of(state, headers, name)?;
    let Some(boot_ms) = live(|l| l.get(&machine.id).map(|m| m.started_ms)) else {
        return Err(api_error(
            StatusCode::CONFLICT,
            format!("machine {name} is not running"),
        ));
    };
    if let Err(full) = runs(|all| all.entry(machine.id.clone()).or_default().make_room()) {
        return Err(api_error(StatusCode::CONFLICT, full));
    }
    let pid = match vm
        .start_in_guest_as(
            "/bin/sh",
            &["-c".to_string(), req.cmd.clone()],
            req.cwd.as_deref(),
            &req.env,
            None,
            req.user.as_deref(),
            AGENT_TIMEOUT,
        )
        .await
    {
        Ok(pid) => pid,
        Err(e) => {
            return Err(api_error(
                StatusCode::BAD_GATEWAY,
                format!("starting the run: {e}"),
            ))
        }
    };
    let (id, described) = runs(|all| {
        let machine_runs = all.entry(machine.id.clone()).or_default();
        machine_runs.last_id += 1;
        let run = Run {
            id: machine_runs.last_id,
            cmd: req.cmd.clone(),
            pid,
            started_ms: now_ms(),
            ended_ms: None,
            state: RunState::Running,
            stdout: Log::default(),
            stderr: Log::default(),
        };
        let described = run.describe();
        machine_runs.runs.insert(run.id, run);
        (machine_runs.last_id, described)
    });
    tokio::spawn(follow(
        machine.id.clone(),
        id,
        Arc::clone(&vm),
        pid,
        boot_ms,
    ));
    if let Some(data) = req.stdin {
        if let Err(e) = vm
            .write_stdin_in_guest(pid, &data, true, AGENT_TIMEOUT)
            .await
        {
            // The run is started and stays: the caller is told what did not
            // happen, and can read or signal it by its ID.
            return Err(api_error(
                StatusCode::BAD_GATEWAY,
                format!("run {id} started, and its standard input could not be written: {e}"),
            ));
        }
    }
    Ok((machine.id, id, described))
}

/// Signal a run of the machine `name` that is still running.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
pub(super) async fn signal_run(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
    id: u64,
    number: i32,
) -> Result<(), Response> {
    let (vm, pid) = reachable(state, headers, name, id)?;
    vm.signal_in_guest(pid, number, AGENT_TIMEOUT)
        .await
        .map_err(|e| api_error(StatusCode::BAD_GATEWAY, format!("signalling run {id}: {e}")))
}

/// Poll the run's program and keep what it prints, until it ends or cannot
/// be reached.
async fn follow(machine_id: String, id: u64, vm: Arc<AgentVM>, pid: u32, boot_ms: u64) {
    let mut failures = 0u32;
    let mut quiet = 0u32;
    loop {
        // Quick while it is printing, slower when it is not.
        let pause = if quiet < 10 { 150 } else { 750 };
        tokio::time::sleep(Duration::from_millis(pause)).await;
        // Forgotten with its machine: nothing left to keep output for.
        let known = runs(|all| {
            all.get(&machine_id)
                .is_some_and(|m| m.runs.contains_key(&id))
        });
        if !known {
            return;
        }
        let same_boot = live(|l| l.get(&machine_id).is_some_and(|m| m.started_ms == boot_ms));
        if !same_boot {
            finish(
                &machine_id,
                id,
                RunState::Lost("its machine stopped".into()),
            );
            return;
        }
        match vm.poll_in_guest(pid, AGENT_TIMEOUT).await {
            Ok(out) => {
                failures = 0;
                quiet = if out.stdout.is_empty() && out.stderr.is_empty() {
                    quiet.saturating_add(1)
                } else {
                    0
                };
                let ended = runs(|all| {
                    let Some(run) = all.get_mut(&machine_id).and_then(|m| m.runs.get_mut(&id))
                    else {
                        return true;
                    };
                    run.stdout.append(&out.stdout);
                    run.stderr.append(&out.stderr);
                    if !out.running {
                        run.end(RunState::Exited {
                            code: out.exit_code,
                            signal: out.signal,
                        });
                    }
                    !out.running
                });
                if ended {
                    return;
                }
            }
            Err(e) => {
                failures += 1;
                // A guest busy enough to miss a few is not a lost run.
                if failures >= 20 {
                    finish(
                        &machine_id,
                        id,
                        RunState::Lost(format!("its guest agent stopped answering: {e}")),
                    );
                    return;
                }
            }
        }
    }
}

fn finish(machine_id: &str, id: u64, state: RunState) {
    runs(|all| {
        if let Some(run) = all.get_mut(machine_id).and_then(|m| m.runs.get_mut(&id)) {
            run.end(state);
        }
    });
}

/// `GET /machines/{name}/runs`: every run kept for the machine, oldest
/// first, without output.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let listed: Vec<Value> = runs(|all| {
        all.get(&machine.id)
            .map(|m| m.runs.values().map(Run::describe).collect())
            .unwrap_or_default()
    });
    Json(listed).into_response()
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct OutputQuery {
    /// Read standard output from this byte on.
    stdout: Option<u64>,
    /// And standard error from this one.
    stderr: Option<u64>,
}

/// `GET /machines/{name}/runs/{id}`.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, u64)>,
    axum::extract::Query(query): axum::extract::Query<OutputQuery>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let found = runs(|all| {
        let run = all.get(&machine.id)?.runs.get(&id)?;
        let mut reply = run.describe();
        for (key, log, asked) in [
            ("stdout", &run.stdout, query.stdout),
            ("stderr", &run.stderr, query.stderr),
        ] {
            let (offset, text, next) = log.from(asked.unwrap_or(0));
            reply[key] = json!({
                "offset": offset,
                "text": text,
                "next": next,
                // Bytes the program wrote that are no longer kept.
                "dropped": log.dropped,
            });
        }
        Some(reply)
    });
    match found {
        Some(reply) => Json(reply).into_response(),
        None => api_error(
            StatusCode::NOT_FOUND,
            format!("machine {name} has no run {id}"),
        ),
    }
}

/// The guest agent and PID of a run that is still running, in a machine
/// that still is, or why it cannot be reached.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
pub(super) fn reachable(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
    id: u64,
) -> Result<(Arc<AgentVM>, u32), Response> {
    let machine = named(state, headers, name)?;
    let found = runs(|all| {
        all.get(&machine.id)
            .and_then(|m| m.runs.get(&id))
            .map(|run| (run.pid, run.state.clone()))
    });
    match found {
        None => Err(api_error(
            StatusCode::NOT_FOUND,
            format!("machine {name} has no run {id}"),
        )),
        Some((pid, RunState::Running)) => Ok((agent_of(state, headers, name)?, pid)),
        Some(_) => Err(api_error(
            StatusCode::CONFLICT,
            format!("run {id} has ended"),
        )),
    }
}

/// Write `data` to a run's standard input, and close it after if `close`.
pub(super) async fn write_stdin(
    vm: &AgentVM,
    pid: u32,
    data: &str,
    close: bool,
) -> Result<(), String> {
    vm.write_stdin_in_guest(pid, data, close, AGENT_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub(crate) struct Input {
    #[serde(default)]
    data: String,
    /// Close its standard input after writing.
    #[serde(default)]
    close: bool,
}

/// `POST /machines/{name}/runs/{id}/stdin`.
pub(crate) async fn stdin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, u64)>,
    Json(req): Json<Input>,
) -> Response {
    let (vm, pid) = match reachable(&state, &headers, &name, id) {
        Ok(found) => found,
        Err(response) => return response,
    };
    match vm
        .write_stdin_in_guest(pid, &req.data, req.close, AGENT_TIMEOUT)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(StatusCode::BAD_GATEWAY, format!("writing to run {id}: {e}")),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Signal {
    /// `INT`, `TERM`, `KILL` and the like, or a number.
    signal: String,
}

/// `POST /machines/{name}/runs/{id}/signal`.
pub(crate) async fn signal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, u64)>,
    Json(req): Json<Signal>,
) -> Response {
    let Some(number) = signal_number(&req.signal) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!(
                "{:?} is not a signal: a name such as INT, TERM or KILL, or a number",
                req.signal
            ),
        );
    };
    match signal_run(&state, &headers, &name, id, number).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_keeps_its_newest_bytes_and_counts_what_it_dropped() {
        let mut log = Log::default();
        log.append("hello ");
        log.append("world");
        assert_eq!(log.total(), 11);
        assert_eq!(log.from(0), (0, "hello world", 11));
        assert_eq!(log.from(6), (6, "world", 11));
        // Past the end: nothing yet, and the same place to ask from.
        assert_eq!(log.from(11), (11, "", 11));
        assert_eq!(log.from(500), (11, "", 11));

        // More than it keeps: the front goes, and is counted.
        let big = "x".repeat(MAX_LOG);
        log.append(&big);
        assert_eq!(log.kept.len(), MAX_LOG);
        assert_eq!(log.dropped, 11);
        assert_eq!(log.total(), 11 + MAX_LOG as u64);
        // An offset from before what was dropped starts where the kept part
        // does, and says so.
        let (offset, text, next) = log.from(3);
        assert_eq!(
            (offset, text.len(), next),
            (11, MAX_LOG, 11 + MAX_LOG as u64)
        );
        // Following from `next` after more is written gets only the new.
        log.append("tail");
        let (offset, text, _) = log.from(next);
        assert_eq!((offset, text), (next, "tail"));
    }

    #[test]
    fn a_log_is_never_cut_inside_a_character() {
        let mut log = Log::default();
        // Three bytes each; the cut would fall inside one.
        let euro = "€".repeat(MAX_LOG / 3 + 10);
        log.append(&euro);
        assert!(log.kept.len() <= MAX_LOG);
        assert!(log.kept.chars().all(|c| c == '€'));
        assert_eq!(log.total(), euro.len() as u64);
        // An offset inside a character is answered from the next one.
        let (offset, text, _) = log.from(log.dropped + 1);
        assert_eq!(offset, log.dropped + 3);
        assert!(text.starts_with('€'));
    }

    #[test]
    fn signals_are_named_or_numbered() {
        for (name, number) in [
            ("INT", 2),
            ("SIGINT", 2),
            ("term", 15),
            ("sigkill", 9),
            (" HUP ", 1),
            ("15", 15),
            ("64", 64),
        ] {
            assert_eq!(signal_number(name), Some(number), "{name}");
        }
        for wrong in ["", "SIG", "NOPE", "0", "65", "-9", "9.0"] {
            assert_eq!(signal_number(wrong), None, "{wrong}");
        }
    }

    fn run(id: u64, state: RunState) -> Run {
        Run {
            id,
            cmd: format!("run {id}"),
            pid: 100 + id as u32,
            started_ms: id,
            ended_ms: None,
            state,
            stdout: Log::default(),
            stderr: Log::default(),
        }
    }

    #[test]
    fn room_is_made_from_the_oldest_finished_run_and_never_a_running_one() {
        let mut machine = MachineRuns::default();
        for id in 1..=MAX_RUNS as u64 {
            // The first two are still running; the rest have ended.
            let state = if id <= 2 {
                RunState::Running
            } else {
                RunState::Exited {
                    code: Some(0),
                    signal: None,
                }
            };
            machine.runs.insert(id, run(id, state));
        }
        machine.make_room().unwrap();
        assert_eq!(machine.runs.len(), MAX_RUNS - 1);
        assert!(machine.runs.contains_key(&1) && machine.runs.contains_key(&2));
        assert!(
            !machine.runs.contains_key(&3),
            "the oldest finished one went"
        );

        // All running: there is no room, and nothing is thrown away to make it.
        let mut busy = MachineRuns::default();
        for id in 1..=MAX_RUNS as u64 {
            busy.runs.insert(id, run(id, RunState::Running));
        }
        let refused = busy.make_room().unwrap_err();
        assert!(refused.contains("still running"), "{refused}");
        assert_eq!(busy.runs.len(), MAX_RUNS);
    }

    #[test]
    fn a_run_ends_once_and_says_how() {
        let mut one = run(1, RunState::Running);
        assert_eq!(one.describe()["state"], "running");
        assert!(one.describe()["endedAt"].is_null());
        one.stdout.append("out");
        one.end(RunState::Exited {
            code: Some(3),
            signal: None,
        });
        let described = one.describe();
        assert_eq!(
            (&described["state"], &described["exitCode"]),
            (&json!("exited"), &json!(3))
        );
        assert_eq!(described["stdoutBytes"], 3);
        assert!(described["endedAt"].is_u64());
        // A later verdict does not replace the first.
        one.end(RunState::Lost("its machine stopped".into()));
        assert_eq!(one.describe()["state"], "exited");

        let mut lost = run(2, RunState::Running);
        lost.end(RunState::Lost("its machine stopped".into()));
        let described = lost.describe();
        assert_eq!(described["state"], "lost");
        assert_eq!(described["reason"], "its machine stopped");
        // The environment is not in a record: there is nowhere it was kept.
        assert!(described.get("env").is_none());
    }
}
