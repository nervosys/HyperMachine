//! A resident namespace: one interpreter, alive across calls.
//!
//! # What this adds over [`crate::runtime::SandboxRuntime`]
//!
//! `SandboxRuntime` runs a fresh process per call, so a script can leave a
//! *file* behind but not a *value*. Re-deriving a 40 MB parse on every call is
//! the cost, and carrying the result through the model's context to avoid it is
//! exactly what this crate exists to stop.
//!
//! [`ResidentRuntime`] keeps a Python interpreter running between calls and
//! executes each call's code in one namespace that outlives it. A result
//! computed in one call is still an object in the next:
//!
//! ```text
//! exec: rows = [json.loads(l) for l in open("result.jsonl")]   -> (nothing printed)
//! exec: print(sum(1 for r in rows if r["status"] == "FAILED")) -> 58
//! ```
//!
//! The second call did not re-read the file, and neither call put a row in the
//! context. That is the shape Scroll (arXiv:2608.21690) describes.
//!
//! # Confinement, and how it differs
//!
//! `SandboxRuntime` confines *every call*, because every call is a new process.
//! A resident process cannot be re-confined per call — it is already running,
//! and whatever was applied to it was applied when it started. So confinement
//! here is decided **once, at spawn**, and what is applied is:
//!
//! | Control | Here | How |
//! |---|---|---|
//! | Wall clock | **enforced**, per call | The host kills the kernel when a call overruns [`ResidentSpec::deadline`]. |
//! | Memory | enforced where the interpreter can | The kernel lowers its own `RLIMIT_AS`, soft *and* hard, before reading its first frame. Unprivileged processes cannot raise a hard limit back, so code arriving later cannot undo it. No `resource` module (Windows) means not enforced, and it says so. |
//! | Network, filesystem, process isolation, no-new-privileges | **not enforced** | Applying these means starting the process inside a namespace or a job object, and [`hv2_sandbox::Sandbox::run`] runs a program to completion — there is no way through it to *start* a long-lived confined child. This backend therefore starts the interpreter itself, and gets none of them. |
//!
//! That is not hidden. [`ResidentRuntime::spawn`] **refuses** unless the asked-for
//! confinement can be applied, exactly as `hv2_sandbox` refuses; a caller that
//! wants it anyway sets [`ResidentSpec::best_effort`], and then every
//! [`crate::RuntimeOutput`] it gets back carries the given-up controls in
//! [`crate::RuntimeOutput::unenforced`] — on every call, not once at startup
//! where it can be missed.
//!
//! # Protocol
//!
//! The framing this repository already uses between a host and something it
//! drives: a four-byte little-endian length, then that many bytes of JSON. See
//! [`hv2_guest_agent`](../../hv2_guest_agent/index.html) for the same
//! convention over vsock. Both ends refuse a length over
//! [`MAX_FRAME_BYTES`] *before* allocating, because the length is written by
//! the other end.
//!
//! ```text
//! host -> kernel   len u32 | {"version":1,"id":7,"code":"x = 1"}
//! kernel -> host   len u32 | {"version":1,"reply":{"kind":"ready","python":"3.13.0","applied":["memory"]}}
//! kernel -> host   len u32 | {"version":1,"reply":{"kind":"result","id":7,"ok":true,...}}
//! ```
//!
//! # When the deadline fires
//!
//! The kernel is killed, and the namespace dies with it. There is no way to
//! interrupt one call in a resident interpreter without interrupting the
//! interpreter. This runtime therefore does not restart quietly: the call
//! returns [`crate::ContextError::Runtime`], and every later call says the
//! namespace is gone. Silently starting a new kernel would leave an agent
//! believing its variables were still there.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hv2_sandbox::{Control, NetworkPolicy, SandboxSpec};
use serde::{Deserialize, Serialize};

use crate::runtime::{
    clamp, ContextRuntime, RuntimeCall, RuntimeOutput, DEFAULT_MEMORY_BYTES, DEFAULT_TIMEOUT,
    MAX_OUTPUT_BYTES,
};
use crate::{ContextError, Result};

/// Protocol version, in every frame in both directions.
///
/// The kernel script is written out by the host that runs it, so the two ends
/// cannot normally disagree — but a stale script left in a workspace by an
/// older build can, and a mismatch has to be a refusal rather than a misread
/// field.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest frame either end will send or accept.
///
/// Comfortably above [`MAX_OUTPUT_BYTES`] so a legitimate reply always fits,
/// and far below anything that would matter if the other end lied about it.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Interpreters tried, in order, when none was named.
///
/// `python3` first because that is the name that means version 3 everywhere it
/// exists. `python` second, and not as a synonym: on Windows `python3` is
/// usually an App Execution Alias that prints an advertisement and exits
/// non-zero, while `python` is the real interpreter. Each candidate is *run*
/// and asked its version rather than merely being found on `PATH`, which is
/// the only way to tell those two apart.
pub const INTERPRETER_CANDIDATES: [&str; 2] = ["python3", "python"];

/// How long to wait for the kernel's ready frame.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// Name of the kernel script inside the workspace.
const KERNEL_SCRIPT: &str = ".hv2-resident-kernel.py";

/// The guest half, with its ceilings still to be substituted in.
const KERNEL_SOURCE: &str = include_str!("kernel.py");

/// What confinement, deadline and interpreter a kernel starts with.
///
/// Every field is decided before the process exists, because none of them can
/// be changed afterwards — which is the substantive difference between this
/// backend and a one-shot sandbox, and the reason it is a spawn-time struct
/// rather than a per-call one.
#[derive(Debug, Clone)]
pub struct ResidentSpec {
    /// Confinement asked for. Reconciled at spawn against what can actually be
    /// applied to a long-lived child, and refused unless [`Self::best_effort`].
    pub confinement: SandboxSpec,
    /// How long one call may run before the kernel is killed.
    ///
    /// Not optional. A resident process that hangs holds the only namespace the
    /// agent has, and a caller with no deadline waits on it forever.
    pub deadline: Duration,
    /// Interpreter to run, or probe [`INTERPRETER_CANDIDATES`] when absent.
    pub interpreter: Option<PathBuf>,
    /// Start even though some asked-for confinement cannot be applied.
    ///
    /// The relaxation is then reported on every call, not just at startup.
    pub best_effort: bool,
}

impl Default for ResidentSpec {
    /// The same confinement [`crate::runtime::SandboxRuntime`] asks for.
    ///
    /// Deliberately *not* weakened to what this backend can deliver: a default
    /// that asked for less would make the gap invisible. As written, the
    /// default refuses on every host, and the refusal names what is missing.
    fn default() -> Self {
        Self {
            confinement: SandboxSpec {
                memory_bytes: Some(DEFAULT_MEMORY_BYTES),
                network: NetworkPolicy::Denied,
                no_new_privileges: true,
                ..SandboxSpec::default()
            },
            deadline: DEFAULT_TIMEOUT,
            interpreter: None,
            best_effort: false,
        }
    }
}

impl ResidentSpec {
    /// A spec asking only for what this backend can actually apply.
    ///
    /// Memory where the interpreter can lower its own limit, a wall-clock
    /// deadline the host enforces, and nothing else. Useful precisely because
    /// it is honest about being weaker than a one-shot sandbox.
    pub fn attainable() -> Self {
        Self {
            confinement: SandboxSpec {
                memory_bytes: Some(DEFAULT_MEMORY_BYTES),
                network: NetworkPolicy::Host,
                ..SandboxSpec::default()
            },
            ..Self::default()
        }
    }

    /// Set the per-call deadline.
    #[must_use]
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// Run a named interpreter instead of probing.
    #[must_use]
    pub fn interpreter(mut self, path: impl Into<PathBuf>) -> Self {
        self.interpreter = Some(path.into());
        self
    }

    /// Start with whatever confinement can be applied, and report the rest.
    #[must_use]
    pub fn best_effort(mut self) -> Self {
        self.best_effort = true;
        self
    }
}

/// A [`ContextRuntime`] holding one interpreter alive across calls.
pub struct ResidentRuntime {
    workspace: PathBuf,
    interpreter: PathBuf,
    python_version: String,
    deadline: Duration,
    enforced: Vec<Control>,
    unenforced: Vec<Control>,
    next_id: AtomicU64,
    kernel: Mutex<Kernel>,
}

impl std::fmt::Debug for ResidentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResidentRuntime")
            .field("workspace", &self.workspace)
            .field("interpreter", &self.interpreter)
            .field("python", &self.python_version)
            .field("deadline", &self.deadline)
            .field("enforced", &self.enforced)
            .field("unenforced", &self.unenforced)
            .field("alive", &self.is_alive())
            .finish()
    }
}

/// The live child and the ends of its pipes.
struct Kernel {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Vec<u8>>,
    stderr: Arc<Mutex<String>>,
    /// Why the namespace is gone, once it is. `None` while it is alive.
    gone: Option<String>,
}

impl Kernel {
    /// Kill the child and remember why, so later calls can say so.
    fn bury(&mut self, why: impl Into<String>) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if self.gone.is_none() {
            self.gone = Some(why.into());
        }
    }

    /// What the kernel itself printed to stderr, for explaining a death.
    fn diagnosis(&self) -> String {
        self.stderr
            .lock()
            .map(|text| text.trim().to_string())
            .unwrap_or_default()
    }
}

impl Drop for Kernel {
    /// On the [`Kernel`] rather than on the runtime, so that every way out of
    /// `spawn_with` -- including the refusals, which happen after the child
    /// exists -- takes the interpreter with it. A backend that leaked one
    /// process per refused spawn would be worse than one that never started.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ResidentRuntime {
    /// Whether this host can run a resident kernel at all, and with what.
    ///
    /// The answer a caller needs *before* planning around this runtime. A host
    /// with no Python 3 gets a sentence saying which names were tried and what
    /// each of them did, rather than a spawn failure halfway through a session.
    ///
    /// # Errors
    ///
    /// The reason no interpreter was usable, as prose.
    pub fn available() -> std::result::Result<PathBuf, String> {
        find_interpreter(&INTERPRETER_CANDIDATES)
    }

    /// Start a kernel in `workspace` under [`ResidentSpec::default`].
    ///
    /// # Errors
    ///
    /// [`ContextError::Runtime`] if no interpreter is available, if the kernel
    /// does not answer, or if confinement was asked for that cannot be applied
    /// to a resident process and [`ResidentSpec::best_effort`] was not set.
    /// [`ContextError::Io`] if the workspace cannot be created.
    pub fn spawn(workspace: impl Into<PathBuf>) -> Result<Self> {
        Self::spawn_with(workspace, ResidentSpec::default())
    }

    /// Start a kernel in `workspace` under `spec`.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`].
    pub fn spawn_with(workspace: impl Into<PathBuf>, spec: ResidentSpec) -> Result<Self> {
        let workspace = workspace.into();
        std::fs::create_dir_all(&workspace)
            .map_err(|e| ContextError::Io(format!("creating {}: {e}", workspace.display())))?;

        let interpreter = match spec.interpreter.clone() {
            Some(named) => verify_interpreter(&named).map_err(|why| {
                ContextError::Runtime(format!("resident runtime unavailable, because {why}"))
            })?,
            None => find_interpreter(&INTERPRETER_CANDIDATES).map_err(|why| {
                ContextError::Runtime(format!("resident runtime unavailable, because {why}"))
            })?,
        };

        let script = workspace.join(KERNEL_SCRIPT);
        std::fs::write(&script, kernel_source(spec.confinement.memory_bytes))
            .map_err(|e| ContextError::Io(format!("writing {}: {e}", script.display())))?;

        // -I: ignore PYTHONPATH, PYTHONHOME and the user site directory, so
        // what the kernel imports does not depend on the environment of
        // whoever started the host. Not confinement -- it removes a way for
        // the host's environment to change what runs, nothing more.
        let mut child = Command::new(&interpreter)
            .arg("-I")
            .arg(&script)
            .current_dir(&workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ContextError::Runtime(format!(
                    "starting {} as a resident kernel: {e}",
                    interpreter.display()
                ))
            })?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr_pipe = child.stderr.take().expect("stderr was piped");

        // A reader thread rather than a blocking read, because that is what
        // makes the deadline enforceable: a pipe has no read timeout, so the
        // only way not to wait forever on a hung kernel is to have something
        // else doing the waiting.
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || read_frames(stdout, &tx));

        let stderr = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr);
        std::thread::spawn(move || drain(stderr_pipe, &sink));

        let mut kernel = Kernel {
            child,
            stdin,
            replies,
            stderr,
            gone: None,
        };

        let ready = match kernel.replies.recv_timeout(STARTUP_TIMEOUT) {
            Ok(frame) => frame,
            Err(_) => {
                let why = kernel.diagnosis();
                kernel.bury("never sent a ready frame");
                return Err(ContextError::Runtime(format!(
                    "the resident kernel did not announce itself within {}s{}",
                    STARTUP_TIMEOUT.as_secs(),
                    if why.is_empty() {
                        String::new()
                    } else {
                        format!("; it said: {why}")
                    }
                )));
            }
        };

        let (python_version, applied) = match parse_frame(&ready)? {
            Reply::Ready { python, applied } => (python, applied),
            Reply::Result { .. } => {
                kernel.bury("answered before it was asked");
                return Err(ContextError::Runtime(
                    "the resident kernel sent a result before its ready frame".into(),
                ));
            }
        };

        // What is enforced is what the two ends between them can show: the
        // deadline, because this side kills the kernel, and memory only if the
        // kernel reported having applied it.
        let mut enforced = vec![Control::WallClock];
        if applied.iter().any(|name| name == "memory") {
            enforced.push(Control::Memory);
        }

        let mut asked = spec.confinement.clone();
        asked.wall_clock = Some(spec.deadline);
        let unenforced: Vec<Control> = asked
            .required()
            .into_iter()
            .filter(|control| !enforced.contains(control))
            .collect();

        if !unenforced.is_empty() && !spec.best_effort {
            kernel.bury("refused before it ran anything");
            return Err(ContextError::Runtime(format!(
                "a resident kernel cannot be given {}: confinement is applied once, at spawn, \
                 and this backend starts the interpreter itself rather than through a sandbox. \
                 Set ResidentSpec::best_effort to start anyway and read `unenforced` on every \
                 call, or use SandboxRuntime, which confines each call and keeps no namespace",
                unenforced
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        Ok(Self {
            workspace,
            interpreter,
            python_version,
            deadline: spec.deadline,
            enforced,
            unenforced,
            next_id: AtomicU64::new(1),
            kernel: Mutex::new(kernel),
        })
    }

    /// The interpreter this kernel is running.
    pub fn interpreter(&self) -> &Path {
        &self.interpreter
    }

    /// The interpreter's own version string, as it reported it.
    pub fn python_version(&self) -> &str {
        &self.python_version
    }

    /// How long one call may run.
    pub fn deadline(&self) -> Duration {
        self.deadline
    }

    /// Controls actually applied to this kernel.
    pub fn enforced(&self) -> &[Control] {
        &self.enforced
    }

    /// Controls asked for at spawn that could not be applied.
    ///
    /// Also attached to every [`RuntimeOutput`] this runtime returns, so a
    /// caller reading results does not have to have remembered to ask here.
    pub fn unenforced(&self) -> &[Control] {
        &self.unenforced
    }

    /// Whether the namespace still exists.
    ///
    /// `false` after a call overran its deadline or the interpreter died. The
    /// runtime does not restart itself, because the value of this backend is
    /// state that survives, and a quiet restart returns an empty namespace to
    /// a caller with no way to tell.
    pub fn is_alive(&self) -> bool {
        self.kernel
            .lock()
            .map(|kernel| kernel.gone.is_none())
            .unwrap_or(false)
    }

    /// The code a call is asking the kernel to run.
    ///
    /// Either `stdin`, or the argument after `-c`. A [`RuntimeCall`] names a
    /// program, and a resident interpreter cannot honour that: it is already
    /// running, in a namespace whose whole point is that it persists, so
    /// `exec`ing something else would end it.
    fn code_of(&self, call: &RuntimeCall) -> Result<String> {
        let named = Path::new(&call.program);
        let is_interpreter = named == self.interpreter
            || named
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| matches!(stem, "python" | "python3"));
        if !is_interpreter {
            return Err(ContextError::Runtime(format!(
                "the resident runtime runs code in a namespace it already holds; it cannot \
                 start `{}`. Name the interpreter and pass the code as stdin or after -c, or \
                 use SandboxRuntime to run a program",
                call.program
            )));
        }

        if let Some(ref code) = call.stdin {
            return Ok(code.clone());
        }
        if let Some(position) = call.args.iter().position(|arg| arg == "-c") {
            if let Some(code) = call.args.get(position + 1) {
                return Ok(code.clone());
            }
        }
        Err(ContextError::Runtime(
            "a resident call carries its code in `stdin` or after `-c`, and this one has \
             neither"
                .into(),
        ))
    }
}

impl ContextRuntime for ResidentRuntime {
    fn name(&self) -> &str {
        "resident-python"
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn exec(&self, call: &RuntimeCall) -> Result<RuntimeOutput> {
        let code = self.code_of(call)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let mut kernel = self
            .kernel
            .lock()
            .map_err(|_| ContextError::Runtime("the resident kernel's lock is poisoned".into()))?;

        if let Some(ref why) = kernel.gone {
            return Err(ContextError::Runtime(format!(
                "the resident namespace is gone ({why}); every value from earlier calls went \
                 with it. Start a new ResidentRuntime, knowing that it starts empty"
            )));
        }

        let frame = encode(&Request {
            version: PROTOCOL_VERSION,
            id,
            code,
        })?;
        let written = kernel.stdin.write_all(&frame);
        let written = written.and_then(|()| kernel.stdin.flush());
        if let Err(e) = written {
            let why = kernel.diagnosis();
            kernel.bury(format!("its input pipe closed: {e}"));
            return Err(ContextError::Runtime(format!(
                "writing to the resident kernel: {e}{}",
                if why.is_empty() {
                    String::new()
                } else {
                    format!("; it said: {why}")
                }
            )));
        }

        let reply = match kernel.replies.recv_timeout(self.deadline) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => {
                kernel.bury(format!(
                    "a call overran the {} ms deadline and the kernel was killed",
                    self.deadline.as_millis()
                ));
                return Err(ContextError::Runtime(format!(
                    "the call did not finish within {} ms; the resident kernel was killed, and \
                     the namespace died with it. A resident interpreter cannot have one call \
                     interrupted without interrupting the interpreter",
                    self.deadline.as_millis()
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let why = kernel.diagnosis();
                kernel.bury("the interpreter exited");
                return Err(ContextError::Runtime(format!(
                    "the resident kernel exited{}",
                    if why.is_empty() {
                        String::new()
                    } else {
                        format!("; it said: {why}")
                    }
                )));
            }
        };

        // A frame this host cannot read means the two ends have stopped
        // agreeing, and every later reply would be read against the wrong
        // shape. End the kernel rather than carry on guessing.
        let reply = match parse_frame(&reply) {
            Ok(reply) => reply,
            Err(e) => {
                kernel.bury("sent a frame this host could not read");
                return Err(e);
            }
        };

        match reply {
            Reply::Result {
                id: answered,
                ok,
                stdout,
                stderr,
                truncated,
            } => {
                if answered != id {
                    kernel.bury("answered a call that was not the one asked");
                    return Err(ContextError::Runtime(format!(
                        "the resident kernel answered call {answered} to call {id}; the stream \
                         is out of step and the namespace can no longer be trusted"
                    )));
                }
                // Clamped again on this side. The kernel already truncates, but
                // the ceiling that protects the context has to hold even if the
                // thing on the other end of the pipe stops honouring it.
                let (stdout, cut_out) = clamp(stdout.as_bytes());
                let (stderr, cut_err) = clamp(stderr.as_bytes());
                Ok(RuntimeOutput {
                    stdout,
                    stderr,
                    // A raised exception is a failed call, not a failed
                    // runtime: the code ran and said what went wrong, and the
                    // traceback is in stderr where a caller expects it.
                    exit_code: Some(i32::from(!ok)),
                    truncated: truncated || cut_out || cut_err,
                    unenforced: self.unenforced.clone(),
                })
            }
            Reply::Ready { .. } => {
                kernel.bury("announced itself twice");
                Err(ContextError::Runtime(
                    "the resident kernel sent a second ready frame".into(),
                ))
            }
        }
    }
}

/// A call, on the wire.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Request {
    version: u32,
    id: u64,
    code: String,
}

/// A frame from the kernel.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Envelope {
    version: u32,
    reply: Reply,
}

/// What the kernel had to say.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Reply {
    /// Sent once, before the first call, saying what the kernel could apply to
    /// itself. Only this end can find that out, so only this end may report it.
    Ready {
        python: String,
        applied: Vec<String>,
    },
    /// One call finished. `ok` is false when the code raised.
    Result {
        id: u64,
        ok: bool,
        stdout: String,
        stderr: String,
        truncated: bool,
    },
}

/// Encode a value as a length-prefixed frame.
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value)
        .map_err(|e| ContextError::Runtime(format!("encoding a resident frame: {e}")))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(ContextError::Runtime(format!(
            "a frame of {} bytes exceeds the {MAX_FRAME_BYTES}-byte limit",
            body.len()
        )));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Read one frame body and check what it says it is.
fn parse_frame(body: &[u8]) -> Result<Reply> {
    let envelope: Envelope = serde_json::from_slice(body).map_err(|e| {
        ContextError::Runtime(format!(
            "the resident kernel sent a frame this host cannot read: {e}"
        ))
    })?;
    if envelope.version != PROTOCOL_VERSION {
        return Err(ContextError::Runtime(format!(
            "the resident kernel speaks protocol {} and this host speaks {PROTOCOL_VERSION}",
            envelope.version
        )));
    }
    Ok(envelope.reply)
}

/// Read length-prefixed frames from `stream` until it closes.
///
/// Bodies only: the length prefix is checked here, against
/// [`MAX_FRAME_BYTES`], before anything is allocated for it.
fn read_frames(mut stream: impl Read, out: &mpsc::Sender<Vec<u8>>) {
    loop {
        let mut header = [0u8; 4];
        if stream.read_exact(&mut header).is_err() {
            return;
        }
        let len = u32::from_le_bytes(header) as usize;
        if len > MAX_FRAME_BYTES {
            // The other end wrote this number. Believing it is how a peer gets
            // to choose how much memory this process uses.
            return;
        }
        let mut body = vec![0u8; len];
        if stream.read_exact(&mut body).is_err() {
            return;
        }
        if out.send(body).is_err() {
            return;
        }
    }
}

/// Collect a stream into `sink`, bounded, so a chatty kernel cannot grow it
/// without limit.
fn drain(mut stream: impl Read, sink: &Mutex<String>) {
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    if let Ok(mut text) = sink.lock() {
        let (kept, _) = clamp(String::from_utf8_lossy(&buf).as_bytes());
        text.push_str(&kept);
    }
}

/// The kernel script with this spawn's ceilings substituted in.
fn kernel_source(memory_bytes: Option<u64>) -> String {
    KERNEL_SOURCE
        .replace("__MAX_FRAME__", &MAX_FRAME_BYTES.to_string())
        .replace("__MAX_OUTPUT__", &MAX_OUTPUT_BYTES.to_string())
        .replace("__MEMORY_BYTES__", &memory_bytes.unwrap_or(0).to_string())
}

/// Try each candidate and return the first that is really a Python 3.
///
/// # Errors
///
/// A sentence naming every candidate and what it did, because "python not
/// found" on a machine where `python3` is a Store stub sends whoever reads it
/// looking in the wrong place.
fn find_interpreter(candidates: &[&str]) -> std::result::Result<PathBuf, String> {
    let mut reasons = Vec::new();
    for candidate in candidates {
        match verify_interpreter(Path::new(candidate)) {
            Ok(path) => return Ok(path),
            Err(why) => reasons.push(why),
        }
    }
    Err(format!(
        "no usable Python 3 interpreter: {}",
        reasons.join("; ")
    ))
}

/// Run `path` and check that it is a Python 3.
///
/// Running it, rather than looking for the file, is the point: a name on
/// `PATH` that exits non-zero when asked its version is not an interpreter, and
/// discovering that at spawn time instead would look like a protocol failure.
fn verify_interpreter(path: &Path) -> std::result::Result<PathBuf, String> {
    let shown = path.display();
    let output = Command::new(path)
        .args(["-I", "-c", "import sys; print(sys.version_info[0])"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("`{shown}` could not be run ({e})"))?;

    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim().lines().next().unwrap_or("").trim();
        return Err(format!(
            "`{shown}` exited {}{}",
            output
                .status
                .code()
                .map_or_else(|| "on a signal".to_string(), |c| c.to_string()),
            if said.is_empty() {
                String::new()
            } else {
                format!(" saying: {said}")
            }
        ));
    }

    let major = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if major != "3" {
        return Err(format!("`{shown}` reports major version {major:?}, not 3"));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A runtime with the confinement this backend can actually apply, or
    /// `None` with a printed reason on a host that cannot run one.
    ///
    /// Skipping loudly rather than passing: a test that quietly succeeds on a
    /// host without Python proves nothing and looks like it proved everything.
    fn kernel(dir: &tempfile::TempDir, spec: ResidentSpec) -> Option<ResidentRuntime> {
        if let Err(why) = ResidentRuntime::available() {
            eprintln!("skipping: {why}");
            return None;
        }
        Some(ResidentRuntime::spawn_with(dir.path().join("work"), spec).expect("spawn a kernel"))
    }

    fn call(code: &str) -> RuntimeCall {
        RuntimeCall::new("python3").stdin(code)
    }

    #[test]
    fn a_variable_set_in_one_call_is_still_there_in_the_next() {
        // The entire reason this backend exists, and the one thing
        // SandboxRuntime cannot do: its every call is a fresh process, so the
        // second half of this would raise NameError there.
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(&dir, ResidentSpec::attainable().best_effort()) else {
            return;
        };

        let first = rt.exec(&call("rows = [1, 2, 3, 4]")).unwrap();
        assert!(first.succeeded(), "got: {first:?}");
        assert!(first.stdout.is_empty(), "got: {first:?}");

        let second = rt.exec(&call("print(sum(rows))")).unwrap();
        assert_eq!(second.stdout.trim(), "10", "got: {second:?}");

        // ...and a function defined in one call, which is the case that breaks
        // if globals and locals are not the same namespace.
        rt.exec(&call("def double(n):\n    return n * 2\n"))
            .unwrap();
        let third = rt.exec(&call("print(double(sum(rows)))")).unwrap();
        assert_eq!(third.stdout.trim(), "20", "got: {third:?}");
    }

    #[test]
    fn a_call_that_overruns_its_deadline_is_killed_rather_than_waited_on() {
        // A resident process holds the only namespace the agent has. Without a
        // deadline enforced on this side, one infinite loop makes the runtime
        // -- and whatever is driving it -- hang forever with nothing to read.
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(
            &dir,
            ResidentSpec::attainable()
                .deadline(Duration::from_millis(400))
                .best_effort(),
        ) else {
            return;
        };

        let start = std::time::Instant::now();
        let err = rt
            .exec(&call("while True:\n    pass\n"))
            .expect_err("a call that never returns must not return Ok");
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the deadline did not bind: waited {:?}",
            start.elapsed()
        );
        assert!(err.to_string().contains("did not finish"), "got: {err}");

        // And the loss is reported rather than papered over: a quiet restart
        // would hand back an empty namespace to a caller still holding names.
        assert!(!rt.is_alive());
        let after = rt.exec(&call("print(1)")).unwrap_err();
        assert!(after.to_string().contains("gone"), "got: {after}");
    }

    #[test]
    fn output_beyond_the_cap_is_cut_and_says_so() {
        // A runtime whose job is keeping large results out of the context must
        // not hand one back. Truncated output that did not say it was
        // truncated would be worse than the size: a cut list reads as a
        // complete short one.
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(&dir, ResidentSpec::attainable().best_effort()) else {
            return;
        };

        let out = rt
            .exec(&call(&format!(
                "print('x' * {})",
                MAX_OUTPUT_BYTES + 100_000
            )))
            .unwrap();
        assert!(out.truncated, "got {} bytes", out.stdout.len());
        assert!(out.stdout.len() <= MAX_OUTPUT_BYTES);
    }

    #[test]
    fn a_host_without_python3_reports_unavailable_and_says_what_it_tried() {
        // The failure this replaces: a spawn that dies on a closed pipe, which
        // reads as a protocol bug rather than as a missing interpreter. Both
        // the probe and spawn have to answer in the same terms.
        let missing = "hv2-context-no-such-interpreter";
        let why = find_interpreter(&[missing, "hv2-context-nor-this-one"])
            .expect_err("nothing by these names can exist");
        assert!(why.contains("no usable Python 3"), "got: {why}");
        assert!(why.contains(missing), "got: {why}");

        let dir = tempfile::tempdir().unwrap();
        let err = ResidentRuntime::spawn_with(
            dir.path().join("work"),
            ResidentSpec::attainable()
                .interpreter(missing)
                .best_effort(),
        )
        .expect_err("a runtime with no interpreter must not appear to start");
        assert!(err.to_string().contains("unavailable"), "got: {err}");
        assert!(err.to_string().contains(missing), "got: {err}");
    }

    #[test]
    fn confinement_that_cannot_be_applied_refuses_the_spawn_and_names_it() {
        // The honest half of a resident backend. It cannot deny the network to
        // a process it started itself, so the default spec -- which asks --
        // must refuse rather than start something a caller believes is
        // isolated.
        let dir = tempfile::tempdir().unwrap();
        if let Err(why) = ResidentRuntime::available() {
            eprintln!("skipping: {why}");
            return;
        }

        let err = ResidentRuntime::spawn_with(dir.path().join("strict"), ResidentSpec::default())
            .expect_err("a kernel that cannot be confined as asked must refuse");
        assert!(err.to_string().contains("network isolation"), "got: {err}");

        // ...and the same spec, relaxed on purpose, runs and keeps saying what
        // it gave up. On every call: a caller reading results should not have
        // to have remembered a warning from startup.
        let rt = ResidentRuntime::spawn_with(
            dir.path().join("relaxed"),
            ResidentSpec::default().best_effort(),
        )
        .expect("best effort starts");
        assert!(rt.unenforced().contains(&Control::NetworkIsolation));
        let out = rt.exec(&call("print('ran')")).unwrap();
        assert!(out.stdout.contains("ran"), "got: {out:?}");
        assert_eq!(out.unenforced, rt.unenforced().to_vec());
        assert!(
            rt.enforced().contains(&Control::WallClock),
            "the deadline is enforced here and must be reported as such: {rt:?}"
        );
    }

    #[test]
    fn an_exception_is_an_output_and_not_an_error() {
        // The program ran and said what went wrong. Turning that into an Err
        // would discard the traceback, which is the diagnosis.
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(&dir, ResidentSpec::attainable().best_effort()) else {
            return;
        };

        let out = rt.exec(&call("raise ValueError('deliberate')")).unwrap();
        assert!(!out.succeeded(), "got: {out:?}");
        assert!(out.stderr.contains("deliberate"), "got: {out:?}");

        // ...and the namespace is still there afterwards, which is what makes
        // a failed call recoverable rather than the end of the session.
        assert!(rt.is_alive());
        assert!(rt.exec(&call("print('still here')")).unwrap().succeeded());
    }

    #[test]
    fn the_workspace_is_the_kernels_working_directory() {
        // Files and variables both persist here, and code that writes a file
        // expects to find it where the next call looks.
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(&dir, ResidentSpec::attainable().best_effort()) else {
            return;
        };

        rt.exec(&call("open('tally.txt', 'w').write('41')"))
            .unwrap();
        assert!(rt.workspace().join("tally.txt").is_file());
    }

    #[test]
    fn a_call_naming_something_other_than_the_interpreter_is_refused() {
        // A RuntimeCall names a program, and this backend cannot honour that:
        // exec'ing one would end the namespace every later call depends on.
        // Saying so beats appearing to run grep and running Python.
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(&dir, ResidentSpec::attainable().best_effort()) else {
            return;
        };

        let err = rt
            .exec(&RuntimeCall::new("grep").args(["-c", "FAILED"]))
            .expect_err("a resident kernel cannot become grep");
        assert!(err.to_string().contains("cannot start"), "got: {err}");
    }

    #[test]
    fn code_can_arrive_after_a_dash_c_as_well_as_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(&dir, ResidentSpec::attainable().best_effort()) else {
            return;
        };
        let out = rt
            .exec(&RuntimeCall::new("python3").args(["-c", "print('from argv')"]))
            .unwrap();
        assert!(out.stdout.contains("from argv"), "got: {out:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_memory_limit_the_kernel_set_on_itself_cannot_be_raised_back() {
        // Memory is the one OS control that survives into a resident process,
        // and only because the kernel lowers its own *hard* limit before it
        // reads a frame. Were it only the soft limit, code arriving later
        // would raise it in one line and the control reported here would be
        // fiction.
        let dir = tempfile::tempdir().unwrap();
        let Some(rt) = kernel(&dir, ResidentSpec::attainable().best_effort()) else {
            return;
        };
        if !rt.enforced().contains(&Control::Memory) {
            eprintln!("skipping: this interpreter could not limit its own address space");
            return;
        }

        let raised = rt
            .exec(&call(
                "import resource\n\
                 resource.setrlimit(resource.RLIMIT_AS, (resource.RLIM_INFINITY,) * 2)\n",
            ))
            .unwrap();
        assert!(!raised.succeeded(), "the limit was raised back: {raised:?}");

        let big = rt
            .exec(&call(&format!(
                "x = bytearray({})",
                DEFAULT_MEMORY_BYTES + 200 * 1024 * 1024
            )))
            .unwrap();
        assert!(
            big.stderr.contains("MemoryError"),
            "an allocation past the cap should have been refused: {big:?}"
        );
    }

    #[test]
    fn a_length_beyond_the_limit_is_refused_before_anything_is_allocated() {
        // The length prefix is written by the other end of a pipe this host
        // does not control the contents of. Believing it is how a peer gets to
        // choose how much memory this process uses.
        let mut buf = u32::MAX.to_le_bytes().to_vec();
        buf.extend_from_slice(b"{}");
        let (tx, rx) = mpsc::channel();
        read_frames(buf.as_slice(), &tx);
        drop(tx);
        assert!(rx.recv().is_err(), "an oversized frame must not be read");
    }

    #[test]
    fn a_frame_from_another_protocol_version_is_refused_rather_than_misread() {
        // A stale kernel script left in a workspace by an older build is the
        // realistic case. Reading its frames with today's field names is how a
        // mismatch turns into wrong answers instead of an error.
        let body = br#"{"version":99,"reply":{"kind":"ready","python":"3.9.0","applied":[]}}"#;
        let err = parse_frame(body).expect_err("a version mismatch is not readable");
        assert!(err.to_string().contains("protocol 99"), "got: {err}");
    }

    #[test]
    fn the_kernel_script_carries_this_hosts_ceilings() {
        // Both ends have to agree on the caps without asking each other, so
        // the numbers are substituted in rather than defaulted twice.
        let source = kernel_source(Some(123_456));
        assert!(source.contains(&MAX_OUTPUT_BYTES.to_string()));
        assert!(source.contains(&MAX_FRAME_BYTES.to_string()));
        assert!(source.contains("123456"));
        assert!(!source.contains("__MEMORY_BYTES__"));
    }
}
