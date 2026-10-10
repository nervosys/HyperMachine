//! Lightweight confinement for agent workloads.
//!
//! # Why this crate exists
//!
//! Two things in this repository looked like sandboxes and confined nothing.
//! `hv2_agent`'s `Sandbox` says so in its own documentation: it is a policy
//! object whose limits take effect only where a caller consults them.
//! `hv2_core::container` is 3,866 lines of namespace, cgroup and seccomp data
//! structures whose `start` fabricates a PID and marks the container running.
//! Between them they made not one confinement syscall.
//!
//! This crate makes some. A [`Sandbox`] runs a program under limits the
//! operating system enforces, and — this is the part that matters — it reports
//! exactly which limits *this host* can enforce, and refuses to run when asked
//! for one it cannot.
//!
//! # The rule this crate is built around
//!
//! **A sandbox that silently drops a control is worse than no sandbox**, because
//! a caller who asked for no network and got one believes the opposite of the
//! truth. So:
//!
//! - [`Sandbox::controls`] reports what the backend actually enforces here.
//! - [`SandboxSpec`] asks for controls.
//! - Running with a spec asking for a control the backend lacks is
//!   [`SandboxError::Unsupported`], not a quiet downgrade.
//! - A caller who genuinely wants best-effort says so, once, with
//!   [`SandboxSpec::best_effort`], and can read back what was dropped.
//!
//! **For integrators: the refusal is the value, so make sure it arrives.** An
//! [`SandboxError::Unsupported`] names the controls this host cannot enforce,
//! and [`Controls::reason`] says why, in terms an operator can act on. Both
//! are easy to lose downstream of this crate without touching it: an
//! application that sanitises error messages before showing them (a common
//! and sensible habit) turns a precise refusal into "Operation failed", and the
//! operator is left knowing only that something went wrong. The first
//! consumer of this crate hit exactly that. Check what your error path does to
//! the message, and pass the refusal and its reasons through a channel that
//! keeps them, verbatim.
//!
//! # Backends
//!
//! | Backend | Where | Enforces |
//! | --- | --- | --- |
//! | [`ProcessSandbox`] on Linux | this crate | user/PID/mount/net/IPC namespaces, `pivot_root` onto a named root, cgroup v2 memory and PID caps, `RLIMIT_*`, `no_new_privs`; a network of one relayed port |
//! | [`ProcessSandbox`] on Windows | this crate | job object memory, process count, and CPU-time caps, kill-on-close; no network, by an AppContainer |
//! | [`ProcessSandbox`] on macOS | this crate | `RLIMIT_CPU`; no network or one port of it, confinement to granted paths and denied paths, by the system's sandbox |
//! | microVM | `hv2-agent` | a whole guest, reached over vsock |
//!
//! The microVM backend lives in `hv2-agent` because it needs a VM and a guest
//! agent; it implements the same [`Sandbox`] trait so a caller can choose
//! isolation strength without changing how it asks.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

pub mod process;
pub mod request;

pub use process::ProcessSandbox;

/// One thing a sandbox can enforce.
///
/// These are deliberately coarse. A caller asks for an outcome — "no network" —
/// rather than for a mechanism, because the mechanism differs per platform and
/// a caller that named one would be writing Linux into its own logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Control {
    /// A hard ceiling on the memory the workload may commit.
    Memory,
    /// A ceiling on how many processes or threads it may create.
    ProcessCount,
    /// A ceiling on CPU time consumed, as distinct from wall clock.
    CpuTime,
    /// A wall-clock deadline, after which the workload is killed.
    WallClock,
    /// No network access at all.
    NetworkIsolation,
    /// A filesystem view that does not include the host's.
    FilesystemIsolation,
    /// Processes inside cannot see or signal processes outside.
    ProcessIsolation,
    /// The workload cannot gain privileges it did not start with.
    NoNewPrivileges,
    /// The workload reaches the paths it was granted and no others of the
    /// caller's, without the caller building a root for it.
    ///
    /// Weaker than [`Control::FilesystemIsolation`] in one way that differs by
    /// platform, and so a control of its own. On Linux it is the same
    /// mechanism with an empty root made for the run, and nothing else is
    /// visible. On Windows it is an AppContainer, which also reads what
    /// Windows lets every packaged application read: the system directories.
    PathConfinement,
    /// A path the caller names is closed to the workload, whatever else
    /// would let it in.
    ///
    /// Linux covers the path with a mount of its own. Windows runs the
    /// workload in an AppContainer and makes the path stop inheriting the
    /// entries that let a container in, for the length of the run. An
    /// access-denied entry for the container's SID was tried first and does
    /// not refuse it.
    PathDenial,
    /// The workload is kept from the desktop it was started on: the
    /// clipboard, other programs' windows, the display's and the system's
    /// settings, and logging the user off.
    ///
    /// On Windows this is the job object's user-interface restrictions. A
    /// Linux process has no such boundary here: a workload that can reach a
    /// display server's socket can use it.
    UiIsolation,
    /// The workload's whole network is one port on the host's loopback,
    /// where the caller has a proxy listening, and the proxy decides what
    /// is reached beyond it.
    ///
    /// This crate keeps the workload to that port; it does not decide what
    /// the proxy lets through. On Linux the workload has a network namespace
    /// of its own in which connections to the port are handed to the host;
    /// on macOS the sandbox profile allows that one address.
    NetworkProxy,
}

impl Control {
    /// Every control, for a backend that wants to describe a full set.
    pub const ALL: [Control; 12] = [
        Control::Memory,
        Control::ProcessCount,
        Control::CpuTime,
        Control::WallClock,
        Control::NetworkIsolation,
        Control::FilesystemIsolation,
        Control::ProcessIsolation,
        Control::NoNewPrivileges,
        Control::PathConfinement,
        Control::PathDenial,
        Control::UiIsolation,
        Control::NetworkProxy,
    ];
}

impl fmt::Display for Control {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Memory => "memory limit",
            Self::ProcessCount => "process count limit",
            Self::CpuTime => "CPU time limit",
            Self::WallClock => "wall-clock deadline",
            Self::NetworkIsolation => "network isolation",
            Self::FilesystemIsolation => "filesystem isolation",
            Self::ProcessIsolation => "process isolation",
            Self::NoNewPrivileges => "no-new-privileges",
            Self::PathConfinement => "path confinement",
            Self::PathDenial => "path denial",
            Self::UiIsolation => "UI isolation",
            Self::NetworkProxy => "network through a proxy",
        };
        f.write_str(name)
    }
}

/// The controls a backend enforces on this host, right now.
///
/// Determined by probing rather than by assuming: a Linux kernel without
/// unprivileged user namespaces, or a container without a writable cgroup
/// delegation, enforces less than the same code on the next machine. A backend
/// that reported its intentions instead of its abilities would be the same
/// class of lie this crate exists to replace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Controls {
    enforced: Vec<Control>,
    /// Why each unavailable control is unavailable, for a caller that has to
    /// tell an operator what to change.
    unavailable: BTreeMap<Control, String>,
}

impl Controls {
    /// A set enforcing nothing.
    pub fn none() -> Self {
        Self::default()
    }

    /// Record that `control` is enforced.
    pub fn with(mut self, control: Control) -> Self {
        if !self.enforced.contains(&control) {
            self.enforced.push(control);
            self.enforced.sort();
        }
        self.unavailable.remove(&control);
        self
    }

    /// Record that `control` is not available, and why.
    pub fn without(mut self, control: Control, reason: impl Into<String>) -> Self {
        self.enforced.retain(|c| *c != control);
        self.unavailable.insert(control, reason.into());
        self
    }

    /// Whether `control` is enforced here.
    pub fn enforces(&self, control: Control) -> bool {
        self.enforced.contains(&control)
    }

    /// Every enforced control, in a stable order.
    pub fn enforced(&self) -> &[Control] {
        &self.enforced
    }

    /// Why `control` is unavailable, when the backend knows.
    pub fn reason(&self, control: Control) -> Option<&str> {
        self.unavailable.get(&control).map(String::as_str)
    }
}

/// What the workload may reach on the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkPolicy {
    /// No network. Requires [`Control::NetworkIsolation`].
    #[default]
    Denied,
    /// The host's network, unrestricted. Requires no control, and is a
    /// deliberate choice a caller has to write down.
    Host,
    /// Nothing but `port` on the host's loopback, at `127.0.0.1`. Requires
    /// [`Control::NetworkProxy`].
    ///
    /// The caller listens there, and what it lets through is the workload's
    /// network: a proxy that allows some names and refuses the rest, for
    /// one. Nothing tells the workload to use it; a caller sets `HTTP_PROXY`
    /// and its like in [`SandboxCommand::env`]. A workload that ignores them
    /// reaches nothing, names included: there is no resolver to ask.
    Proxy {
        /// The port the caller's proxy listens on. Not zero.
        port: u16,
    },
}

/// What the workload may see of the filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FilesystemPolicy {
    /// Whatever the host process can see. Requires no control.
    #[default]
    Host,
    /// Only `root` and whatever is mounted into it. Requires
    /// [`Control::FilesystemIsolation`].
    ///
    /// A backend mounts this root; it does not build one. A root that does not
    /// exist is refused rather than created, because a sandbox that invented an
    /// empty directory would run a workload that could find none of its own
    /// tools and report that as the workload's failure.
    ///
    /// Nothing is mounted on the caller's behalf beyond `read_only` — `/dev`
    /// included, which a shell wants the moment anything redirects to
    /// `/dev/null`. List it if the workload needs it.
    Isolated {
        /// Directory that becomes the workload's root.
        ///
        /// Absolute. Writable from inside unless the host says otherwise, so a
        /// caller that wants nothing written puts nothing writable in it.
        root: PathBuf,
        /// Host paths mounted read-only inside it, each at the same path it has
        /// on the host — `/usr` becomes `/usr` — so a workload finds its tools
        /// where they expect to be and no mapping has to be kept in step.
        ///
        /// Mount points are created inside `root` where they are missing. Each
        /// path must exist on the host, or the run is refused. Read-only here
        /// means the whole subtree, including anything the host has mounted
        /// under the path, and `nosuid` with it.
        read_only: Vec<PathBuf>,
    },
}

/// Paths a workload must be able to reach, where its containment would
/// otherwise hide them.
///
/// A floor, not a ceiling. Containment that hides part of the filesystem --
/// a Windows AppContainer, which cannot read the user's files -- is opened
/// for exactly these paths and what is under them. Containment that hides
/// nothing, such as [`FilesystemPolicy::Host`] on Linux, already reaches
/// them, and a grant there changes nothing: it does not take anything else
/// away. To take the rest away, isolate the filesystem.
///
/// Each path must be absolute and exist, or the run is refused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PathGrants {
    /// Readable, and executable, with everything under them.
    pub read_only: Vec<PathBuf>,
    /// Readable and writable, with everything under them.
    pub read_write: Vec<PathBuf>,
    /// Closed to the workload, with everything under them, whatever else
    /// would let it in: a grant above them, or the host's filesystem.
    /// Requires [`Control::PathDenial`].
    ///
    /// A denial covers everything under it, so a granted path under a denied
    /// one refuses the run rather than picking a winner.
    pub denied: Vec<PathBuf>,
}

impl PathGrants {
    /// Whether nothing is granted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.read_only.is_empty() && self.read_write.is_empty()
    }
}

/// Limits and policy for one sandboxed run.
#[derive(Debug, Clone, Default)]
pub struct SandboxSpec {
    /// Hard memory ceiling, in bytes.
    pub memory_bytes: Option<u64>,
    /// Ceiling on processes and threads.
    pub max_processes: Option<u32>,
    /// Ceiling on CPU time consumed.
    pub cpu_time: Option<Duration>,
    /// Wall-clock deadline, after which the workload is killed.
    pub wall_clock: Option<Duration>,
    /// Network policy.
    pub network: NetworkPolicy,
    /// Filesystem policy.
    pub filesystem: FilesystemPolicy,
    /// Paths the workload must reach whatever its containment hides.
    pub grants: PathGrants,
    /// Whether [`Self::grants`] is all of the caller's filesystem the
    /// workload reaches. Requires [`Control::PathConfinement`].
    ///
    /// Without this a grant only opens; with it, what is not granted is
    /// closed. Has no effect on top of [`FilesystemPolicy::Isolated`], whose
    /// root already decides what is there.
    pub confine_paths: bool,
    /// Whether processes inside are hidden from the host's process table.
    pub isolate_processes: bool,
    /// Whether the workload is barred from gaining privileges.
    pub no_new_privileges: bool,
    /// Whether the workload is kept from the desktop it was started on.
    /// Requires [`Control::UiIsolation`].
    pub isolate_ui: bool,
    /// Run with whatever subset of the above this host can enforce, instead of
    /// refusing.
    ///
    /// Off by default, because the default has to be the safe one: a caller
    /// that has not thought about it must not silently get less confinement
    /// than it asked for.
    pub best_effort: bool,
}

impl SandboxSpec {
    /// A spec asking for nothing, which every backend can satisfy.
    pub fn unconfined() -> Self {
        Self {
            network: NetworkPolicy::Host,
            ..Self::default()
        }
    }

    /// A spec for running something untrusted: no network, no privilege
    /// escalation, processes hidden, and the given memory and time ceilings.
    ///
    /// Filesystem policy is left at [`FilesystemPolicy::Host`] deliberately —
    /// isolating it needs a root directory only the caller can choose, and
    /// guessing one would either fail or quietly do nothing.
    pub fn untrusted(memory_bytes: u64, wall_clock: Duration) -> Self {
        Self {
            memory_bytes: Some(memory_bytes),
            max_processes: Some(64),
            cpu_time: Some(wall_clock),
            wall_clock: Some(wall_clock),
            network: NetworkPolicy::Denied,
            filesystem: FilesystemPolicy::Host,
            grants: PathGrants::default(),
            confine_paths: false,
            isolate_processes: true,
            no_new_privileges: true,
            // Left off: only Windows has it, and a spec for untrusted code
            // that Linux refused outright would not be used.
            isolate_ui: false,
            best_effort: false,
        }
    }

    /// Accept whatever this host can enforce rather than refusing.
    pub fn best_effort(mut self) -> Self {
        self.best_effort = true;
        self
    }

    /// The controls this spec asks for.
    pub fn required(&self) -> Vec<Control> {
        let mut wanted = Vec::new();
        if self.memory_bytes.is_some() {
            wanted.push(Control::Memory);
        }
        if self.max_processes.is_some() {
            wanted.push(Control::ProcessCount);
        }
        if self.cpu_time.is_some() {
            wanted.push(Control::CpuTime);
        }
        if self.wall_clock.is_some() {
            wanted.push(Control::WallClock);
        }
        match self.network {
            NetworkPolicy::Denied => wanted.push(Control::NetworkIsolation),
            NetworkPolicy::Proxy { .. } => wanted.push(Control::NetworkProxy),
            NetworkPolicy::Host => {}
        }
        if matches!(self.filesystem, FilesystemPolicy::Isolated { .. }) {
            wanted.push(Control::FilesystemIsolation);
        }
        if self.isolate_processes {
            wanted.push(Control::ProcessIsolation);
        }
        if self.no_new_privileges {
            wanted.push(Control::NoNewPrivileges);
        }
        if self.confine_paths && matches!(self.filesystem, FilesystemPolicy::Host) {
            wanted.push(Control::PathConfinement);
        }
        if !self.grants.denied.is_empty() {
            wanted.push(Control::PathDenial);
        }
        if self.isolate_ui {
            wanted.push(Control::UiIsolation);
        }
        wanted.sort();
        wanted
    }

    /// Controls this spec asks for that `controls` does not enforce.
    pub fn missing_from(&self, controls: &Controls) -> Vec<Control> {
        self.required()
            .into_iter()
            .filter(|c| !controls.enforces(*c))
            .collect()
    }

    /// A copy of this spec with `dropped` controls removed.
    ///
    /// Backends are handed this rather than the caller's spec, so a backend
    /// never attempts a control its own probe said was unavailable. Without
    /// it, [`Self::best_effort`] is broken in the worst way: it promises to
    /// run with whatever the host can enforce and then fails trying to apply
    /// something the host already said it could not — which is what running
    /// this on a kernel with no cgroup delegation showed.
    pub fn without_controls(&self, dropped: &[Control]) -> Self {
        let mut spec = self.clone();
        for control in dropped {
            match control {
                Control::Memory => spec.memory_bytes = None,
                Control::ProcessCount => spec.max_processes = None,
                Control::CpuTime => spec.cpu_time = None,
                Control::WallClock => spec.wall_clock = None,
                Control::NetworkIsolation => {
                    if spec.network == NetworkPolicy::Denied {
                        spec.network = NetworkPolicy::Host;
                    }
                }
                Control::FilesystemIsolation => spec.filesystem = FilesystemPolicy::Host,
                Control::ProcessIsolation => spec.isolate_processes = false,
                Control::NoNewPrivileges => spec.no_new_privileges = false,
                Control::PathConfinement => spec.confine_paths = false,
                Control::PathDenial => spec.grants.denied.clear(),
                Control::UiIsolation => spec.isolate_ui = false,
                // Without it the port is one among everything else on the
                // host's network, which is the host's network.
                Control::NetworkProxy => {
                    if matches!(spec.network, NetworkPolicy::Proxy { .. }) {
                        spec.network = NetworkPolicy::Host;
                    }
                }
            }
        }
        spec
    }

    /// Check this spec against what a backend can do.
    ///
    /// Returns the controls that will not be enforced. Errors unless
    /// [`Self::best_effort`] was set, so the default outcome of asking for
    /// confinement a host cannot provide is a refusal rather than a workload
    /// running with less protection than its caller believes.
    pub fn reconcile(&self, controls: &Controls) -> Result<Vec<Control>, SandboxError> {
        let missing = self.missing_from(controls);
        if missing.is_empty() || self.best_effort {
            return Ok(missing);
        }
        Err(SandboxError::Unsupported {
            controls: missing
                .iter()
                .map(|c| match controls.reason(*c) {
                    Some(why) => format!("{c} ({why})"),
                    None => c.to_string(),
                })
                .collect(),
        })
    }
}

/// A program to run in a sandbox.
#[derive(Debug, Clone)]
pub struct SandboxCommand {
    /// Program to execute. Run directly, never through a shell.
    pub program: String,
    /// Arguments, already split.
    pub args: Vec<String>,
    /// Working directory, or the host process's if absent.
    pub working_dir: Option<PathBuf>,
    /// Environment for the workload.
    ///
    /// This is the whole environment, not additions to the host's: inheriting
    /// by default would hand a sandboxed workload every credential in the
    /// parent's environment, which is not a limit anyone asked to remove.
    pub env: BTreeMap<String, String>,
    /// Bytes written to the workload's standard input.
    pub stdin: Option<Vec<u8>>,
}

impl SandboxCommand {
    /// A command with no arguments and an empty environment.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_dir: None,
            env: BTreeMap::new(),
            stdin: None,
        }
    }

    /// Add arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Provide standard input.
    pub fn stdin(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(data.into());
        self
    }
}

/// What a sandboxed program did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxOutput {
    /// Exit status, or `None` when a signal ended the program.
    ///
    /// Separate from `signal` because a program killed by SIGKILL did not exit
    /// 0, and one field for both reports a kill as a success.
    pub exit_code: Option<i32>,
    /// Signal that ended the program, if one did.
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Whether the sandbox killed it for exceeding a limit, and which.
    pub killed_by: Option<Control>,
    /// Controls the caller asked for that this host did not enforce.
    ///
    /// Always empty unless the spec set [`SandboxSpec::best_effort`]. A caller
    /// that opted into best-effort has to be able to find out what it actually
    /// got, or the opt-in is just a way to hide the problem.
    pub unenforced: Vec<Control>,
}

impl SandboxOutput {
    /// Whether the program ran to completion and exited zero.
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && self.killed_by.is_none()
    }
}

/// Host variables a program needs to find its tools and a place to write,
/// and nothing else: [`SandboxCommand::env`] starts empty, and a caller that
/// wants a usable environment without handing over every credential in its
/// own starts from these. `SystemRoot` and the rest are Windows's; without
/// `SystemRoot` many programs there cannot load at all.
pub const HOST_BASE_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "TERM",
    "TMPDIR",
    "SystemRoot",
    "SystemDrive",
    "windir",
    "USERPROFILE",
    "TEMP",
    "TMP",
    "PATHEXT",
    "COMSPEC",
];

/// [`HOST_BASE_ENV`], with this process's values, for the variables it has.
pub fn host_base_env() -> BTreeMap<String, String> {
    HOST_BASE_ENV
        .iter()
        .filter_map(|name| std::env::var(name).ok().map(|v| ((*name).to_string(), v)))
        .collect()
}

/// Which of a workload's output streams a chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Called with each chunk of a workload's output as it arrives.
pub type OutputSink = Arc<dyn Fn(OutputStream, &[u8]) + Send + Sync>;

/// How a caller watches and steers a run while it happens, rather than only
/// reading what it did afterwards.
///
/// For work that runs for hours: [`Sandbox::run`] hands back everything the
/// workload printed once it has finished, which is too late to watch and, for
/// a chatty workload, too much to hold.
#[derive(Clone, Default)]
pub struct RunIo {
    /// Receives output as it arrives, in chunks, from reader threads.
    ///
    /// When set, output is not also collected: [`SandboxOutput::stdout`] and
    /// [`SandboxOutput::stderr`] come back empty, so a long run's output is
    /// held nowhere but wherever the sink puts it.
    pub on_output: Option<OutputSink>,
    /// Set to stop the workload -- all of it, as the wall-clock deadline
    /// does. A cancelled run returns normally, with the exit status the kill
    /// left; the caller that set the flag knows why.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl fmt::Debug for RunIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunIo")
            .field("on_output", &self.on_output.is_some())
            .field("cancel", &self.cancel)
            .finish()
    }
}

/// Something that runs a program under enforced limits.
pub trait Sandbox: Send + Sync {
    /// A short name for this backend, for logs and errors.
    fn name(&self) -> &str;

    /// What this backend enforces on this host.
    ///
    /// Probed, not assumed. Two machines running this binary can return
    /// different sets.
    fn controls(&self) -> Controls;

    /// Run `command` under `spec` and wait for it to finish.
    ///
    /// # Errors
    ///
    /// [`SandboxError::Unsupported`] when the spec asks for a control this
    /// host cannot enforce and did not opt into best-effort. A non-zero exit
    /// is a [`SandboxOutput`], not an error: the program ran.
    fn run(
        &self,
        command: &SandboxCommand,
        spec: &SandboxSpec,
    ) -> Result<SandboxOutput, SandboxError>;

    /// [`Self::run`], watched and steerable as it runs: see [`RunIo`].
    ///
    /// The default runs to completion and then hands the collected output to
    /// the sink, and cannot cancel -- correct, but not live. A backend that
    /// can do better overrides it; [`ProcessSandbox`] does.
    ///
    /// # Errors
    ///
    /// As [`Self::run`].
    fn run_with(
        &self,
        command: &SandboxCommand,
        spec: &SandboxSpec,
        io: &RunIo,
    ) -> Result<SandboxOutput, SandboxError> {
        let mut output = self.run(command, spec)?;
        if let Some(sink) = &io.on_output {
            sink(OutputStream::Stdout, &std::mem::take(&mut output.stdout));
            sink(OutputStream::Stderr, &std::mem::take(&mut output.stderr));
        }
        Ok(output)
    }
}

/// Why a sandboxed run could not happen, or could not be trusted.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// The spec asked for confinement this host cannot provide.
    #[error(
        "this host cannot enforce: {}. Pass SandboxSpec::best_effort() to run anyway, \
         and read SandboxOutput::unenforced to see what was dropped",
        .controls.join(", ")
    )]
    Unsupported { controls: Vec<String> },

    /// A confinement step failed after the workload had been committed to.
    ///
    /// Distinct from [`Self::Spawn`]: this is the dangerous case, where a
    /// process may exist without the limits it was supposed to have. Backends
    /// must kill the workload before returning this.
    #[error("confinement failed after the workload started ({control}): {source}")]
    ConfinementFailed {
        control: Control,
        #[source]
        source: std::io::Error,
    },

    /// The program could not be started.
    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    /// Something went wrong while the workload ran.
    #[error("sandbox failed: {0}")]
    Runtime(String),

    /// The spec itself is contradictory or unusable.
    #[error("invalid sandbox spec: {0}")]
    InvalidSpec(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_controls() -> Controls {
        Control::ALL
            .iter()
            .fold(Controls::none(), |c, ctl| c.with(*ctl))
    }

    #[test]
    fn a_spec_asks_for_exactly_the_controls_its_fields_imply() {
        let spec = SandboxSpec::untrusted(64 * 1024 * 1024, Duration::from_secs(5));
        let required = spec.required();

        assert!(required.contains(&Control::Memory));
        assert!(required.contains(&Control::NetworkIsolation));
        assert!(required.contains(&Control::ProcessIsolation));
        assert!(required.contains(&Control::NoNewPrivileges));
        // Not asked for: isolating the filesystem needs a root only the caller
        // can choose, so `untrusted` does not pretend to have chosen one.
        assert!(!required.contains(&Control::FilesystemIsolation));
    }

    /// A proxied network is a control of its own. Asking for it is not
    /// asking for no network, and a host without it gives the host's
    /// network when told to make do, never a quiet nothing.
    #[test]
    fn a_proxied_network_asks_for_its_own_control() {
        let spec = SandboxSpec {
            network: NetworkPolicy::Proxy { port: 3128 },
            ..SandboxSpec::unconfined()
        };
        assert_eq!(spec.required(), vec![Control::NetworkProxy]);
        assert_eq!(
            spec.without_controls(&[Control::NetworkProxy]).network,
            NetworkPolicy::Host
        );
        // Each network control drops only the policy that asked for it.
        assert_eq!(
            spec.without_controls(&[Control::NetworkIsolation]).network,
            NetworkPolicy::Proxy { port: 3128 }
        );
        let denied = SandboxSpec::default();
        assert_eq!(
            denied.without_controls(&[Control::NetworkProxy]).network,
            NetworkPolicy::Denied
        );
        let without = full_controls().without(Control::NetworkProxy, "not here");
        assert!(matches!(
            spec.reconcile(&without),
            Err(SandboxError::Unsupported { .. })
        ));
        assert_eq!(denied.reconcile(&without).expect("no proxy asked for"), []);
    }

    #[test]
    fn an_unconfined_spec_asks_for_nothing() {
        // Otherwise every backend refuses the one spec that should always run.
        assert!(SandboxSpec::unconfined().required().is_empty());
    }

    #[test]
    fn asking_for_a_control_the_host_lacks_is_refused_by_default() {
        // The whole point of the crate: a caller that asked for no network and
        // silently got one believes the opposite of the truth.
        let controls = Controls::none().with(Control::Memory);
        let spec = SandboxSpec {
            memory_bytes: Some(1024),
            network: NetworkPolicy::Denied,
            ..SandboxSpec::default()
        };

        let err = spec
            .reconcile(&controls)
            .expect_err("a missing control must refuse");
        assert!(err.to_string().contains("network isolation"), "got: {err}");
    }

    #[test]
    fn a_refusal_says_why_the_control_is_unavailable() {
        let controls = Controls::none().without(
            Control::NetworkIsolation,
            "unprivileged user namespaces are disabled",
        );
        let spec = SandboxSpec {
            network: NetworkPolicy::Denied,
            ..SandboxSpec::default()
        };

        // An operator needs to know what to change, not just that it failed.
        let err = spec.reconcile(&controls).expect_err("must refuse");
        assert!(
            err.to_string().contains("user namespaces are disabled"),
            "got: {err}"
        );
    }

    #[test]
    fn best_effort_reports_what_it_dropped_rather_than_hiding_it() {
        let controls = Controls::none().with(Control::Memory);
        let spec = SandboxSpec {
            memory_bytes: Some(1024),
            network: NetworkPolicy::Denied,
            no_new_privileges: true,
            ..SandboxSpec::default()
        }
        .best_effort();

        let dropped = spec.reconcile(&controls).expect("best effort runs");
        assert_eq!(
            dropped,
            vec![Control::NetworkIsolation, Control::NoNewPrivileges],
            "opting into best-effort must not mean opting out of knowing"
        );
    }

    #[test]
    fn a_fully_capable_host_drops_nothing() {
        let spec = SandboxSpec::untrusted(1024, Duration::from_secs(1));
        assert!(spec
            .reconcile(&full_controls())
            .expect("supported")
            .is_empty());
    }

    #[test]
    fn controls_report_a_stable_order_and_the_last_word_wins() {
        let controls = Controls::none()
            .with(Control::NetworkIsolation)
            .with(Control::Memory)
            .without(Control::Memory, "no cgroup delegation");

        // Added out of order, reported in order, and the later `without` wins.
        assert_eq!(controls.enforced(), &[Control::NetworkIsolation]);
        assert!(!controls.enforces(Control::Memory));
        assert!(controls.enforces(Control::NetworkIsolation));
        assert_eq!(
            controls.reason(Control::Memory),
            Some("no cgroup delegation")
        );
    }

    #[test]
    fn a_command_starts_with_an_empty_environment() {
        // Inheriting the parent's environment would hand a sandboxed workload
        // every credential the host process holds, which is not a limit anyone
        // asked to remove.
        let command = SandboxCommand::new("printenv");
        assert!(command.env.is_empty());
        assert_eq!(command.env("PATH", "/usr/bin").env["PATH"], "/usr/bin");
    }

    #[test]
    fn a_killed_program_is_not_reported_as_succeeding() {
        let killed = SandboxOutput {
            exit_code: Some(0),
            signal: Some(9),
            stdout: Vec::new(),
            stderr: Vec::new(),
            killed_by: Some(Control::WallClock),
            unenforced: Vec::new(),
        };
        assert!(
            !killed.succeeded(),
            "a workload the sandbox killed did not succeed, whatever exit code it left"
        );
    }
}
