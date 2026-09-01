//! Computing over what was retrieved, without putting it in the context.
//!
//! # What this layer is for
//!
//! Retrieval alone does not solve much. If the answer to "how many of the 400
//! results are still failing" is 400 results in the context, the log has just
//! become a slower way to fill the window. The point of the third layer is
//! that the agent can *compute* over what it retrieved somewhere else, and
//! only the answer comes back.
//!
//! # What it is, and what it is not
//!
//! [`ContextRuntime`] is the interface; there are two backends, and they trade
//! against each other rather than one superseding the other.
//!
//! [`SandboxRuntime`], here, is a program run under [`hv2_sandbox`] with a
//! workspace directory that survives between calls, so a script can write a
//! file in one call and read it in the next. It is **not** a resident
//! namespace: every call is a fresh process, so files persist and variables do
//! not. That is the cost of the thing it buys -- because each call is its own
//! process, each call is confined afresh, under a spec the caller can change
//! per call.
//!
//! [`crate::resident::ResidentRuntime`] is the other side of that trade. It
//! keeps a Python interpreter alive across calls, as the paper this follows
//! does, so a tool result stays a live object a later call can operate on
//! without re-fetching -- and it can only be confined once, when it starts,
//! because a process already running cannot be re-confined. Which controls
//! that leaves out is documented there and reported on every call.
//!
//! # Fail-closed
//!
//! The event log is not reachable from inside. Not by policy -- by not being
//! there: the runtime is handed its workspace and nothing else, and the memory
//! surface ([`crate::SessionEnvironment::search`], [`crate::SessionEnvironment::expand`])
//! is mediated by the harness outside the sandbox. Exposing the log to
//! arbitrary code inside would need a filesystem policy the process backend
//! does not implement, and a control that is not enforced must not be
//! described as one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hv2_sandbox::{Control, NetworkPolicy, Sandbox, SandboxCommand, SandboxSpec};

use crate::{ContextError, Result};

/// Default ceiling on one call's memory.
pub const DEFAULT_MEMORY_BYTES: u64 = 512 * 1024 * 1024;

/// Default ceiling on one call's wall-clock time.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// How much of a call's output is carried back.
///
/// A runtime whose whole purpose is to keep large results out of the context
/// must not hand back an unbounded string. Anything longer is truncated and
/// says so, so a caller can tell a short answer from a cut one.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// What one call produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOutput {
    /// What the program printed. This is the part that enters the view.
    pub stdout: String,
    /// What it printed to standard error.
    pub stderr: String,
    /// Exit status, or `None` if a signal ended it.
    pub exit_code: Option<i32>,
    /// Whether the output was cut at [`MAX_OUTPUT_BYTES`].
    pub truncated: bool,
    /// Controls the host could not enforce, when best-effort was asked for.
    ///
    /// Empty is the normal case. Non-empty means the program ran with less
    /// confinement than the caller asked for, and the caller can find out.
    pub unenforced: Vec<Control>,
}

impl RuntimeOutput {
    /// Whether the program exited successfully.
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// One call into the runtime.
///
/// A struct rather than a list of arguments because of the last field: whether
/// to run with less confinement than asked for is a decision made per call, by
/// whoever is making it, and it has to be visible at the call site rather than
/// baked into how the runtime was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCall {
    /// Program to run, directly rather than through a shell.
    pub program: String,
    /// Arguments, already split.
    pub args: Vec<String>,
    /// Text written to the program's standard input.
    pub stdin: Option<String>,
    /// Run with whatever confinement this host can enforce, instead of
    /// refusing when it cannot enforce all of it.
    ///
    /// Off by default, and not a formality: a Windows host cannot isolate the
    /// network with a job object, so a call that needs to happen there has to
    /// say so, and [`RuntimeOutput::unenforced`] then reports exactly what was
    /// given up.
    pub best_effort: bool,
}

impl RuntimeCall {
    /// A call with no arguments, no input, and no relaxation.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            stdin: None,
            best_effort: false,
        }
    }

    /// Add arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Write text to the program's standard input.
    #[must_use]
    pub fn stdin(mut self, text: impl Into<String>) -> Self {
        self.stdin = Some(text.into());
        self
    }

    /// Run with whatever confinement this host can enforce.
    #[must_use]
    pub fn best_effort(mut self) -> Self {
        self.best_effort = true;
        self
    }
}

/// Somewhere an agent can run a program over what it retrieved.
pub trait ContextRuntime: Send + Sync {
    /// A short name for this backend.
    fn name(&self) -> &str;

    /// The directory that survives between calls.
    fn workspace(&self) -> &Path;

    /// Run `call`.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::Runtime`] if the program could not be run under
    /// the confinement asked for. A non-zero exit is a [`RuntimeOutput`], not
    /// an error: the program ran and said something, and that is the answer.
    fn exec(&self, call: &RuntimeCall) -> Result<RuntimeOutput>;
}

/// A [`ContextRuntime`] backed by a one-shot sandbox and a durable workspace.
pub struct SandboxRuntime {
    sandbox: Box<dyn Sandbox>,
    workspace: PathBuf,
    spec: SandboxSpec,
}

impl std::fmt::Debug for SandboxRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxRuntime")
            .field("sandbox", &self.sandbox.name())
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

impl SandboxRuntime {
    /// A runtime confining every call with `sandbox`, working in `workspace`.
    ///
    /// The default spec denies the network and refuses new privileges, because
    /// this runs code an agent wrote about data it retrieved, and neither of
    /// those is a capability that job needs.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::Io`] if the workspace cannot be created.
    pub fn new(sandbox: Box<dyn Sandbox>, workspace: impl Into<PathBuf>) -> Result<Self> {
        let workspace = workspace.into();
        std::fs::create_dir_all(&workspace)
            .map_err(|e| ContextError::Io(format!("creating {}: {e}", workspace.display())))?;

        let spec = SandboxSpec {
            memory_bytes: Some(DEFAULT_MEMORY_BYTES),
            wall_clock: Some(DEFAULT_TIMEOUT),
            network: NetworkPolicy::Denied,
            no_new_privileges: true,
            ..SandboxSpec::default()
        };

        Ok(Self {
            sandbox,
            workspace,
            spec,
        })
    }

    /// The same runtime under a different spec.
    ///
    /// Whatever is passed here is what gets enforced, and a spec asking for
    /// something this host cannot do is refused at call time rather than
    /// quietly dropped -- that is [`hv2_sandbox`]'s rule and this does not
    /// weaken it.
    #[must_use]
    pub fn with_spec(mut self, spec: SandboxSpec) -> Self {
        self.spec = spec;
        self
    }

    /// The spec each call runs under.
    pub fn spec(&self) -> &SandboxSpec {
        &self.spec
    }

    /// What the underlying sandbox enforces on this host.
    pub fn controls(&self) -> hv2_sandbox::Controls {
        self.sandbox.controls()
    }
}

impl ContextRuntime for SandboxRuntime {
    fn name(&self) -> &str {
        self.sandbox.name()
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn exec(&self, call: &RuntimeCall) -> Result<RuntimeOutput> {
        let mut command = SandboxCommand::new(&call.program)
            .args(call.args.iter().cloned())
            .working_dir(&self.workspace);
        if let Some(ref stdin) = call.stdin {
            command = command.stdin(stdin.as_bytes().to_vec());
        }

        // Relaxing per call rather than per runtime: the spec this was built
        // with stays the thing being asked for, and what a best-effort call
        // gave up comes back in `unenforced` rather than being decided once,
        // out of sight, at construction.
        let spec = if call.best_effort {
            self.spec.clone().best_effort()
        } else {
            self.spec.clone()
        };

        let output = self
            .sandbox
            .run(&command, &spec)
            .map_err(|e| ContextError::Runtime(e.to_string()))?;

        let (stdout, cut_out) = clamp(&output.stdout);
        let (stderr, cut_err) = clamp(&output.stderr);

        Ok(RuntimeOutput {
            stdout,
            stderr,
            exit_code: output.exit_code,
            truncated: cut_out || cut_err,
            unenforced: output.unenforced.clone(),
        })
    }
}

/// Cut output to [`MAX_OUTPUT_BYTES`], reporting whether anything was lost.
///
/// Shared with [`crate::resident`] rather than written twice: two copies of a
/// ceiling drift, and this one is what keeps a result out of the context.
pub(crate) fn clamp(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX_OUTPUT_BYTES {
        return (text.into_owned(), false);
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_sandbox::ProcessSandbox;

    /// A shell that exists on this host, and the flag that runs a command.
    fn shell() -> (&'static str, &'static str) {
        if cfg!(windows) {
            ("cmd.exe", "/C")
        } else {
            ("/bin/sh", "-c")
        }
    }

    fn runtime(dir: &tempfile::TempDir) -> SandboxRuntime {
        // Denying the network needs a control not every host has; these tests
        // are about the runtime, not about confinement, which hv2-sandbox
        // tests directly.
        let spec = SandboxSpec {
            wall_clock: Some(Duration::from_secs(20)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };
        SandboxRuntime::new(Box::new(ProcessSandbox::new()), dir.path().join("work"))
            .unwrap()
            .with_spec(spec)
    }

    fn exec(rt: &SandboxRuntime, script: &str) -> RuntimeOutput {
        let (program, flag) = shell();
        rt.exec(&RuntimeCall::new(program).args([flag, script]))
            .unwrap()
    }

    #[test]
    fn only_what_the_program_prints_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(&dir);
        let out = exec(&rt, "echo visible");
        assert!(out.stdout.contains("visible"), "got: {out:?}");
        assert!(out.succeeded(), "got: {out:?}");
    }

    #[test]
    fn the_workspace_survives_between_calls() {
        // The whole difference between this and a bare sandbox run. Without
        // it, an agent has no way to build up a result across calls except by
        // carrying it through its own context, which is what this avoids.
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(&dir);

        exec(&rt, "echo 41 > tally.txt");
        let out = exec(
            &rt,
            if cfg!(windows) {
                "type tally.txt"
            } else {
                "cat tally.txt"
            },
        );

        assert!(out.stdout.contains("41"), "got: {out:?}");
    }

    #[test]
    fn a_failing_program_is_an_output_and_not_an_error() {
        // The program ran and said something. Turning that into an Err would
        // discard what it said, which is usually the diagnosis.
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(&dir);
        let out = exec(&rt, "echo trouble 1>&2 & exit 3");
        assert!(!out.succeeded(), "got: {out:?}");
    }

    #[test]
    fn oversized_output_is_cut_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(&dir);
        let (program, flag) = shell();
        let script = if cfg!(windows) {
            format!(
                "for /L %i in (1,1,{}) do @echo {}",
                (MAX_OUTPUT_BYTES / 64) + 200,
                "x".repeat(60)
            )
        } else {
            format!(
                "for i in $(seq 1 {}); do echo {}; done",
                (MAX_OUTPUT_BYTES / 64) + 200,
                "x".repeat(60)
            )
        };

        let out = rt
            .exec(&RuntimeCall::new(program).args([flag.to_string(), script]))
            .unwrap();
        assert!(out.truncated, "got {} bytes", out.stdout.len());
        assert!(out.stdout.len() <= MAX_OUTPUT_BYTES);
    }

    #[test]
    fn best_effort_is_a_per_call_decision_and_reports_what_it_dropped() {
        // On a host that cannot isolate the network, the strict default
        // refuses and the same call marked best-effort runs and says so. Both
        // halves matter: the refusal is the honest default, and the report is
        // what stops the relaxation from being silent.
        let dir = tempfile::tempdir().unwrap();
        let strict = SandboxRuntime::new(
            Box::new(ProcessSandbox::with_controls(hv2_sandbox::Controls::none())),
            dir.path().join("work"),
        )
        .unwrap();
        let (program, flag) = shell();

        let refused = strict.exec(&RuntimeCall::new(program).args([flag, "echo hi"]));
        assert!(refused.is_err(), "a host enforcing nothing must refuse");

        let out = strict
            .exec(
                &RuntimeCall::new(program)
                    .args([flag, "echo hi"])
                    .best_effort(),
            )
            .unwrap();
        assert!(
            !out.unenforced.is_empty(),
            "a caller that relaxed confinement has to be able to find out what it got"
        );
    }

    #[test]
    fn the_workspace_is_created_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(&dir);
        assert!(rt.workspace().is_dir());
        assert!(rt.workspace().ends_with("work"));
    }
}
