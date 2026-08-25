//! The seam between the `sandbox.*` tools and real confinement.
//!
//! # What this is for
//!
//! An agent that needs to run a program on the host — a build, a linter, a
//! conversion — has had no way to ask for one, and the honest reason is that
//! there was nothing to run it *under*. Adding a "run this command" tool
//! without confinement would have handed every agent the host account the
//! server runs as.
//!
//! [`hv2_sandbox`] made confinement real, so the tool can exist. This module
//! is the seam: [`SandboxHost`] is what [`McpServer`](crate::mcp::McpServer)
//! dispatches `sandbox.*` against, and [`LocalSandboxHost`] runs the workload
//! here under a [`ProcessSandbox`].
//!
//! # Two rules this surface keeps
//!
//! **A tool call never gets weaker confinement than it asked for.** The
//! underlying sandbox refuses a spec this host cannot enforce; nothing here
//! translates that refusal into a run. An agent may opt into best-effort, and
//! then the response says which controls were dropped.
//!
//! **The default is the strict one.** A request that names no limits gets
//! [`SandboxSpec::untrusted`], not an unconfined process. An agent that wants
//! the host's network or a longer deadline says so, and that shows up in the
//! audit log as something it asked for.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use hv2_sandbox::{
    Control, NetworkPolicy, ProcessSandbox, Sandbox, SandboxCommand, SandboxError, SandboxSpec,
};

/// Default memory ceiling for a request that names none, in bytes.
const DEFAULT_MEMORY_BYTES: u64 = 512 * 1024 * 1024;

/// Default deadline for a request that names none.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest deadline this surface will accept.
///
/// A tool call is a request/response exchange, and an agent that wants a job
/// running for an hour wants a job runner, not a tool call that never returns.
const MAX_TIMEOUT: Duration = Duration::from_secs(600);

/// A program an agent wants run, and how confined it should be.
///
/// Unknown fields are rejected rather than ignored. A caller that misspells
/// `allow_network` would otherwise get the strict default and believe it had
/// asked for the network — or, worse in the other direction, believe it had
/// asked for a limit it did not get.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxRequest {
    /// Program to execute. Run directly, never through a shell.
    pub program: String,
    /// Arguments, already split. Nothing here parses a command line.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment for the workload.
    ///
    /// This is the whole environment, not additions to the server's: a
    /// sandboxed workload that inherited the parent's would receive every
    /// credential the server holds, which is not a limit anyone asked to
    /// remove.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory, or the server's own if absent.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Bytes written to the workload's standard input.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Memory ceiling in bytes. Defaults to 512 MiB.
    #[serde(default)]
    pub memory_bytes: Option<u64>,
    /// Deadline in seconds. Defaults to 30, capped at 600.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    /// Whether the workload may reach the network.
    ///
    /// `false` — the default — needs a host that can actually isolate the
    /// network, and the request is refused where it cannot. That refusal is
    /// the point: quietly granting the network to a caller that asked for
    /// none is the failure this whole layer exists to prevent.
    #[serde(default)]
    pub allow_network: bool,
    /// Run under whatever subset of the requested confinement this host can
    /// enforce, instead of refusing.
    #[serde(default)]
    pub best_effort: bool,
}

impl SandboxRequest {
    /// A request to run `program` with the strict defaults.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            working_dir: None,
            stdin: None,
            memory_bytes: None,
            timeout_seconds: None,
            allow_network: false,
            best_effort: false,
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

    /// Validate and turn this into the sandbox's own types.
    ///
    /// # Errors
    ///
    /// Returns a message fit for an agent to read when the request cannot mean
    /// anything: no program, or a deadline of zero, which would be a limit
    /// nothing could satisfy rather than a limit at all.
    pub fn build(&self) -> Result<(SandboxCommand, SandboxSpec), String> {
        if self.program.trim().is_empty() {
            return Err("program must not be empty".to_string());
        }
        let timeout = match self.timeout_seconds {
            None => DEFAULT_TIMEOUT,
            Some(0) => {
                return Err("timeout_seconds must be at least 1; zero is not a deadline".to_string())
            }
            Some(seconds) => Duration::from_secs(seconds).min(MAX_TIMEOUT),
        };
        if let Some(0) = self.memory_bytes {
            return Err(
                "memory_bytes must be at least 1; zero would refuse the program's own startup"
                    .to_string(),
            );
        }

        let mut command = SandboxCommand::new(&self.program)
            .args(self.args.clone())
            .stdin(self.stdin.clone().unwrap_or_default().into_bytes());
        command.env = self.env.clone();
        if let Some(dir) = &self.working_dir {
            command = command.working_dir(dir);
        }
        if self.stdin.is_none() {
            command.stdin = None;
        }

        // Start from the strict spec and relax only what was asked for, so a
        // field nobody set can never mean "unconfined".
        let mut spec =
            SandboxSpec::untrusted(self.memory_bytes.unwrap_or(DEFAULT_MEMORY_BYTES), timeout);
        if self.allow_network {
            spec.network = NetworkPolicy::Host;
        }
        if self.best_effort {
            spec = spec.best_effort();
        }

        Ok((command, spec))
    }
}

/// What a sandboxed program did, as the tool surface reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxRun {
    /// Exit status, or `null` when a signal ended the program.
    ///
    /// Kept apart from `signal` because a program killed by SIGKILL did not
    /// exit 0, and one field for both reports a kill as a success.
    pub exit_code: Option<i32>,
    /// Signal that ended the program, if one did.
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Whether the sandbox killed the workload for exceeding a limit, and
    /// which one.
    pub killed_by: Option<String>,
    /// Controls the request asked for that this host did not enforce.
    ///
    /// Always empty unless the request set `best_effort`. An agent that opted
    /// into best-effort has to be able to find out what it actually got, or
    /// the opt-in is only a way to hide the problem.
    pub unenforced: Vec<String>,
}

/// What a host can confine, for an agent deciding what to ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxCapabilities {
    /// Which backend is installed.
    pub backend: String,
    /// Controls this host enforces, named.
    pub enforced: Vec<String>,
    /// Controls it does not, and why — so an operator learns what to change
    /// rather than only that something failed.
    pub unavailable: BTreeMap<String, String>,
}

/// Something that can run a program under confinement on an agent's behalf.
#[async_trait]
pub trait SandboxHost: Send + Sync {
    /// What this host can enforce.
    ///
    /// An agent should be able to ask before it asks for a run, so a refusal
    /// is a thing it can plan around rather than an error it discovers.
    async fn capabilities(&self) -> SandboxCapabilities;

    /// Run a program under confinement.
    ///
    /// A non-zero exit is a [`SandboxRun`], not an error: the program ran, and
    /// its output is what explains the failure. An error means it did not run,
    /// or would not have been confined the way the request asked.
    async fn run(&self, request: SandboxRequest) -> Result<SandboxRun, String>;
}

/// A [`SandboxHost`] that runs workloads on this machine.
pub struct LocalSandboxHost {
    sandbox: Arc<dyn Sandbox>,
}

impl LocalSandboxHost {
    /// Probe this host and build a sandbox from what it found.
    ///
    /// Probing costs a few process spawns, once. Doing it here rather than per
    /// call means a server reports the same capabilities to every agent for as
    /// long as it runs.
    pub fn new() -> Self {
        Self {
            sandbox: Arc::new(ProcessSandbox::new()),
        }
    }

    /// Use an already-built sandbox — a [`MicroVmSandbox`](crate::MicroVmSandbox)
    /// for a deployment that wants a VM boundary rather than a process one, or
    /// a pre-probed [`ProcessSandbox`].
    pub fn with_sandbox(sandbox: Arc<dyn Sandbox>) -> Self {
        Self { sandbox }
    }
}

impl Default for LocalSandboxHost {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxHost for LocalSandboxHost {
    async fn capabilities(&self) -> SandboxCapabilities {
        let controls = self.sandbox.controls();
        let mut unavailable = BTreeMap::new();
        for control in Control::ALL {
            if !controls.enforces(control) {
                unavailable.insert(
                    control.to_string(),
                    controls
                        .reason(control)
                        .unwrap_or("not reported by this backend")
                        .to_string(),
                );
            }
        }

        SandboxCapabilities {
            backend: self.sandbox.name().to_string(),
            enforced: controls.enforced().iter().map(|c| c.to_string()).collect(),
            unavailable,
        }
    }

    async fn run(&self, request: SandboxRequest) -> Result<SandboxRun, String> {
        let (command, spec) = request.build()?;
        let sandbox = Arc::clone(&self.sandbox);

        // The sandbox blocks: it waits on a child process. Keeping that off
        // the async runtime is what stops one slow workload from stalling
        // every other tool call the server is serving.
        let output = tokio::task::spawn_blocking(move || sandbox.run(&command, &spec))
            .await
            .map_err(|e| format!("sandbox task failed: {e}"))?
            .map_err(describe)?;

        Ok(SandboxRun {
            exit_code: output.exit_code,
            signal: output.signal,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            killed_by: output.killed_by.map(|c| c.to_string()),
            unenforced: output.unenforced.iter().map(|c| c.to_string()).collect(),
        })
    }
}

/// Turn a sandbox error into something an agent can act on.
///
/// [`SandboxError::Unsupported`] carries the whole explanation already,
/// including why each control is unavailable, so it is passed through rather
/// than summarised — an agent deciding whether to retry with `best_effort`
/// needs the detail, not a category.
fn describe(error: SandboxError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_sandbox::Controls;

    /// A host that claims nothing, so every confinement request is refused.
    fn claims_nothing() -> LocalSandboxHost {
        LocalSandboxHost::with_sandbox(Arc::new(ProcessSandbox::with_controls(Controls::none())))
    }

    /// A program that exists everywhere CI runs, with the environment it needs
    /// to find its own tools — the environment is empty by design.
    fn echo() -> SandboxRequest {
        #[cfg(windows)]
        {
            let mut request = SandboxRequest::new("cmd.exe").args(["/C", "echo", "hello"]);
            request
                .env
                .insert("SystemRoot".to_string(), r"C:\Windows".to_string());
            request
                .env
                .insert("PATH".to_string(), r"C:\Windows\System32".to_string());
            request
        }
        #[cfg(not(windows))]
        {
            SandboxRequest::new("/bin/echo").args(["hello"])
        }
    }

    #[tokio::test]
    async fn a_request_that_names_no_limits_gets_the_strict_ones() {
        // The default has to be the safe one. A field nobody set must never
        // mean "unconfined", or the first careless caller runs a workload with
        // the server's own privileges.
        let (_, spec) = SandboxRequest::new("/bin/true").build().expect("build");

        assert_eq!(spec.memory_bytes, Some(DEFAULT_MEMORY_BYTES));
        assert_eq!(spec.wall_clock, Some(DEFAULT_TIMEOUT));
        assert_eq!(spec.network, NetworkPolicy::Denied);
        assert!(spec.no_new_privileges);
        assert!(spec.isolate_processes);
        assert!(!spec.best_effort);
    }

    #[test]
    fn network_access_has_to_be_asked_for_explicitly() {
        let mut request = SandboxRequest::new("/bin/true");
        request.allow_network = true;
        let (_, spec) = request.build().expect("build");
        assert_eq!(spec.network, NetworkPolicy::Host);
    }

    #[test]
    fn a_deadline_is_capped_rather_than_honoured_without_limit() {
        // A tool call is a request/response exchange. An agent that wants an
        // hour-long job wants a job runner.
        let mut request = SandboxRequest::new("/bin/true");
        request.timeout_seconds = Some(86_400);
        let (_, spec) = request.build().expect("build");
        assert_eq!(spec.wall_clock, Some(MAX_TIMEOUT));
    }

    #[test]
    fn a_request_that_cannot_mean_anything_is_refused() {
        assert!(SandboxRequest::new("  ").build().is_err());

        let mut zero_timeout = SandboxRequest::new("/bin/true");
        zero_timeout.timeout_seconds = Some(0);
        assert!(zero_timeout.build().is_err());

        let mut zero_memory = SandboxRequest::new("/bin/true");
        zero_memory.memory_bytes = Some(0);
        assert!(zero_memory.build().is_err());
    }

    #[tokio::test]
    async fn capabilities_explain_every_control_they_do_not_enforce() {
        let host = LocalSandboxHost::new();
        let capabilities = host.capabilities().await;

        assert_eq!(capabilities.backend, "process");
        for control in Control::ALL {
            let name = control.to_string();
            assert!(
                capabilities.enforced.contains(&name)
                    || capabilities.unavailable.contains_key(&name),
                "{name} is neither enforced nor explained"
            );
        }
        // An operator has to learn what to change, not only that it failed.
        for reason in capabilities.unavailable.values() {
            assert!(!reason.is_empty());
        }
    }

    #[tokio::test]
    async fn a_host_that_cannot_confine_refuses_rather_than_running() {
        // The rule the whole layer exists for: an agent that asked for no
        // network and got one believes the opposite of the truth.
        let err = claims_nothing()
            .run(echo())
            .await
            .expect_err("a host enforcing nothing must not run a strict request");
        assert!(err.contains("cannot enforce"), "got: {err}");
        assert!(err.contains("network isolation"), "got: {err}");
    }

    #[tokio::test]
    async fn best_effort_runs_and_names_what_it_dropped() {
        let mut request = echo();
        request.best_effort = true;

        let run = claims_nothing()
            .run(request)
            .await
            .expect("best effort runs");
        assert!(
            run.unenforced.contains(&"network isolation".to_string()),
            "an agent that opted into best-effort still has to know what it got: {run:?}"
        );
    }

    #[tokio::test]
    async fn a_program_runs_and_its_output_comes_back() {
        let host = LocalSandboxHost::new();
        let mut request = echo();
        // Whatever this host cannot enforce is not the subject of this test.
        request.best_effort = true;

        let run = host.run(request).await.expect("run");
        assert!(run.stdout.contains("hello"), "got: {run:?}");
        assert_eq!(run.exit_code, Some(0));
        assert!(run.killed_by.is_none());
    }

    #[tokio::test]
    async fn a_missing_program_is_an_error_not_an_exit_code() {
        let host = LocalSandboxHost::new();
        let mut request = SandboxRequest::new("this-program-does-not-exist-anywhere");
        request.best_effort = true;

        let err = host.run(request).await.expect_err("nothing ran");
        assert!(err.contains("could not start"), "got: {err}");
    }

    #[tokio::test]
    async fn a_failing_program_is_a_result_rather_than_an_error() {
        let host = LocalSandboxHost::new();

        #[cfg(windows)]
        let mut request = {
            let mut r = SandboxRequest::new("cmd.exe").args(["/C", "exit", "3"]);
            r.env
                .insert("SystemRoot".to_string(), r"C:\Windows".to_string());
            r
        };
        #[cfg(not(windows))]
        let mut request = SandboxRequest::new("/bin/false");
        request.best_effort = true;

        // Exiting non-zero is what the program did, not a failure to run it.
        // An Err here would throw away the output that explains it.
        let run = host.run(request).await.expect("the program ran");
        assert_ne!(run.exit_code, Some(0));
    }
}
