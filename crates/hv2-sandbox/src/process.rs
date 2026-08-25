//! A sandbox made of operating-system process confinement.
//!
//! One type, [`ProcessSandbox`], with a different implementation behind it per
//! platform. What differs between platforms is not just the mechanism but *how
//! much is enforced*, and that difference is reported rather than smoothed
//! over: on Linux this is namespaces and cgroups, on Windows a job object, on
//! other Unixes resource limits and nothing else.
//!
//! # Probing, not assuming
//!
//! [`ProcessSandbox::new`] probes the host by trying each control in a
//! throwaway child and keeping what worked. Two machines running this same
//! binary can report different [`Controls`]: a kernel with unprivileged user
//! namespaces disabled, or a container with no writable cgroup delegation,
//! genuinely enforces less. Code that reported its intentions would be the
//! same class of claim this crate exists to replace.
//!
//! Probing costs a few forks, once, so [`ProcessSandbox::new`] is meant to be
//! called at startup and the result kept.

use crate::{Control, Controls, Sandbox, SandboxCommand, SandboxError, SandboxOutput, SandboxSpec};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(windows)]
mod windows;

#[cfg(all(unix, not(target_os = "linux")))]
mod unix_fallback;

/// Runs a program in a confined child process.
#[derive(Debug, Clone)]
pub struct ProcessSandbox {
    controls: Controls,
}

impl ProcessSandbox {
    /// Probe this host and build a sandbox that reports what it found.
    pub fn new() -> Self {
        Self { controls: probe() }
    }

    /// Build a sandbox that claims exactly `controls`, without probing.
    ///
    /// For tests, and for a deployment that has already probed and does not
    /// want to pay for it again. Claiming more than the host enforces makes
    /// this type lie, which is the one thing it exists not to do.
    pub fn with_controls(controls: Controls) -> Self {
        Self { controls }
    }
}

impl Default for ProcessSandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Sandbox for ProcessSandbox {
    fn name(&self) -> &str {
        "process"
    }

    fn controls(&self) -> Controls {
        self.controls.clone()
    }

    fn run(
        &self,
        command: &SandboxCommand,
        spec: &SandboxSpec,
    ) -> Result<SandboxOutput, SandboxError> {
        if command.program.trim().is_empty() {
            return Err(SandboxError::InvalidSpec("no program to run".to_string()));
        }
        if let Some(bytes) = spec.memory_bytes {
            if bytes == 0 {
                return Err(SandboxError::InvalidSpec(
                    "a memory limit of zero would refuse every allocation, including the \
                     program's own startup"
                        .to_string(),
                ));
            }
        }

        // Reconcile before anything is started. A workload that has already
        // begun cannot be un-started, and discovering the sandbox is weaker
        // than promised afterwards is too late to matter.
        let unenforced = spec.reconcile(&self.controls)?;

        let mut output = run_confined(command, spec)?;
        output.unenforced = unenforced;
        Ok(output)
    }
}

/// Probe what this host enforces.
fn probe() -> Controls {
    #[cfg(target_os = "linux")]
    {
        linux::probe()
    }
    #[cfg(windows)]
    {
        windows::probe()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        unix_fallback::probe()
    }
    #[cfg(not(any(unix, windows)))]
    {
        Controls::none()
    }
}

/// Run `command` under `spec` on this platform.
#[allow(unused_variables)]
fn run_confined(
    command: &SandboxCommand,
    spec: &SandboxSpec,
) -> Result<SandboxOutput, SandboxError> {
    #[cfg(target_os = "linux")]
    {
        linux::run(command, spec)
    }
    #[cfg(windows)]
    {
        windows::run(command, spec)
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        unix_fallback::run(command, spec)
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(SandboxError::Runtime(format!(
            "no process sandbox backend for {}",
            std::env::consts::OS
        )))
    }
}

/// Shared plumbing for the backends that drive a `std::process::Child`.
///
/// Wall-clock enforcement and output collection are identical everywhere; only
/// the confinement differs, so they live here rather than being written twice
/// and drifting.
#[cfg(any(unix, windows))]
pub(crate) mod driver {
    use super::*;
    use std::io::{Read, Write};
    use std::process::Child;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Feed stdin, wait for the child, and enforce the wall-clock deadline.
    ///
    /// `kill` is how this platform stops the whole workload — for a job object
    /// that is terminating the job, not the one process, so a child that
    /// spawned grandchildren does not leave them running.
    pub(crate) fn wait_with_deadline(
        mut child: Child,
        stdin: Option<&[u8]>,
        deadline: Option<Duration>,
        kill: impl FnOnce(),
    ) -> Result<SandboxOutput, SandboxError> {
        if let Some(data) = stdin {
            if let Some(mut pipe) = child.stdin.take() {
                // A workload that never reads stdin would otherwise block this
                // write forever; ignoring the error lets it decide.
                let _ = pipe.write_all(data);
            }
        } else {
            // Close it, or a program that reads stdin waits for a write that is
            // never coming and then hits the deadline for the wrong reason.
            drop(child.stdin.take());
        }

        // Both pipes must be drained while the child runs, or a chatty workload
        // fills a pipe buffer and blocks forever. Threads do that; this one
        // holds the deadline.
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let (out_tx, out_rx) = mpsc::channel();
        let (err_tx, err_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                let _ = pipe.read_to_end(&mut buf);
            }
            let _ = out_tx.send(buf);
        });
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = stderr_pipe.as_mut() {
                let _ = pipe.read_to_end(&mut buf);
            }
            let _ = err_tx.send(buf);
        });

        let (status, killed) = match deadline {
            None => (
                child
                    .wait()
                    .map_err(|e| SandboxError::Runtime(format!("waiting for workload: {e}")))?,
                false,
            ),
            Some(limit) => {
                let start = std::time::Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break (status, false),
                        Ok(None) => {}
                        Err(e) => {
                            return Err(SandboxError::Runtime(format!("waiting for workload: {e}")))
                        }
                    }
                    if start.elapsed() >= limit {
                        kill();
                        // Reap it, so the deadline does not leave a zombie
                        // behind every time it fires.
                        let status = child.wait().map_err(|e| {
                            SandboxError::Runtime(format!("reaping killed workload: {e}"))
                        })?;
                        break (status, true);
                    }
                    std::thread::sleep(POLL);
                }
            }
        };

        let stdout = out_rx.recv().unwrap_or_default();
        let stderr = err_rx.recv().unwrap_or_default();

        Ok(SandboxOutput {
            exit_code: status.code(),
            signal: signal_of(&status),
            stdout,
            stderr,
            killed_by: killed.then_some(Control::WallClock),
            unenforced: Vec::new(),
        })
    }

    /// How often the deadline is checked. Small enough that a limit means what
    /// it says, large enough not to spin a core doing it.
    const POLL: Duration = Duration::from_millis(5);

    #[cfg(unix)]
    fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }

    #[cfg(not(unix))]
    fn signal_of(_status: &std::process::ExitStatus) -> Option<i32> {
        // Windows has no signals; a terminated process reports an exit code.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NetworkPolicy;
    use std::time::Duration;

    /// A program that exists on every platform CI runs on, printing its
    /// argument.
    fn echo() -> SandboxCommand {
        #[cfg(windows)]
        {
            SandboxCommand::new("cmd.exe").args(["/C", "echo", "hello"])
        }
        #[cfg(not(windows))]
        {
            SandboxCommand::new("/bin/echo").args(["hello"])
        }
    }

    #[test]
    fn a_probe_reports_something_about_every_control() {
        let sandbox = ProcessSandbox::new();
        let controls = sandbox.controls();

        // Every control is either enforced or explained. Silence about one
        // would leave a caller unable to tell "not supported" from "nobody
        // checked".
        for control in Control::ALL {
            assert!(
                controls.enforces(control) || controls.reason(control).is_some(),
                "{control} is neither enforced nor explained"
            );
        }
    }

    #[test]
    fn an_unconfined_program_runs_and_its_output_comes_back() {
        let sandbox = ProcessSandbox::new();
        let output = sandbox
            .run(&echo(), &SandboxSpec::unconfined())
            .expect("an unconfined run should work anywhere");

        assert!(output.succeeded(), "got: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("hello"),
            "stdout was {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(output.unenforced.is_empty());
    }

    #[test]
    fn a_control_this_host_lacks_refuses_the_run() {
        // Built to claim nothing, so every spec that asks for something is
        // refused. This is the behaviour that matters most: silently running
        // unconfined is how a caller comes to believe the opposite of the
        // truth.
        let sandbox = ProcessSandbox::with_controls(Controls::none());
        let spec = SandboxSpec {
            network: NetworkPolicy::Denied,
            ..SandboxSpec::default()
        };

        let err = sandbox
            .run(&echo(), &spec)
            .expect_err("a sandbox enforcing nothing must refuse to pretend");
        assert!(
            matches!(err, SandboxError::Unsupported { .. }),
            "got: {err}"
        );
    }

    #[test]
    fn best_effort_runs_and_reports_what_it_could_not_enforce() {
        let sandbox = ProcessSandbox::with_controls(Controls::none());
        let spec = SandboxSpec {
            network: NetworkPolicy::Denied,
            no_new_privileges: true,
            ..SandboxSpec::default()
        }
        .best_effort();

        let output = sandbox.run(&echo(), &spec).expect("best effort runs");
        assert_eq!(
            output.unenforced,
            vec![Control::NetworkIsolation, Control::NoNewPrivileges],
            "a caller that opted into best-effort still has to be able to find out what it got"
        );
    }

    #[test]
    fn a_program_that_does_not_exist_is_a_spawn_error() {
        let sandbox = ProcessSandbox::new();
        let err = sandbox
            .run(
                &SandboxCommand::new("this-program-does-not-exist-anywhere"),
                &SandboxSpec::unconfined(),
            )
            .expect_err("a missing program is not an exit code");
        assert!(matches!(err, SandboxError::Spawn { .. }), "got: {err}");
    }

    #[test]
    fn an_empty_program_is_refused_before_anything_is_spawned() {
        let sandbox = ProcessSandbox::new();
        assert!(matches!(
            sandbox.run(&SandboxCommand::new("   "), &SandboxSpec::unconfined()),
            Err(SandboxError::InvalidSpec(_))
        ));
    }

    #[test]
    fn a_zero_memory_limit_is_refused_rather_than_starving_the_program() {
        let sandbox = ProcessSandbox::new();
        let spec = SandboxSpec {
            memory_bytes: Some(0),
            ..SandboxSpec::unconfined()
        };
        assert!(matches!(
            sandbox.run(&echo(), &spec),
            Err(SandboxError::InvalidSpec(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_process_limit_stops_the_workload_spawning_past_it() {
        // The evidence that this is a kernel limit and not a policy object:
        // the workload asks Windows for a second process and Windows refuses.
        // Nothing in this crate is consulted at that moment.
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::ProcessCount) {
            eprintln!("skipping: this host has no usable job objects");
            return;
        }

        let command = SandboxCommand::new("cmd.exe")
            .args(["/C", "cmd.exe /C echo nested"])
            .env("SystemRoot", r"C:\Windows")
            .env("PATH", r"C:\Windows\System32");
        let spec = SandboxSpec {
            // One process: the shell itself, and nothing it tries to start.
            max_processes: Some(1),
            wall_clock: Some(Duration::from_secs(10)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };

        let output = sandbox.run(&command, &spec).expect("run");
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("nested"),
            "the nested process ran, so the limit did not bind: {output:?}"
        );
        assert!(!output.succeeded(), "got: {output:?}");
    }

    #[test]
    fn a_workload_that_overruns_its_deadline_is_killed_and_says_so() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::WallClock) {
            eprintln!("skipping: this host does not enforce a wall-clock deadline");
            return;
        }

        // The environment is empty by design, so anything the workload needs
        // to find its own tools has to be handed to it — which is the point,
        // and is why this reads as more setup than a plain spawn would.
        #[cfg(windows)]
        let command = SandboxCommand::new("cmd.exe")
            .args(["/C", "ping", "-n", "30", "127.0.0.1"])
            .env("SystemRoot", "C:\\Windows")
            .env("PATH", "C:\\Windows\\System32");
        #[cfg(not(windows))]
        let command = SandboxCommand::new("/bin/sleep").args(["30"]);

        let spec = SandboxSpec {
            wall_clock: Some(Duration::from_millis(300)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };

        let start = std::time::Instant::now();
        let output = sandbox.run(&command, &spec).expect("run");

        assert!(
            start.elapsed() >= Duration::from_millis(250),
            "the workload exited on its own, so this proves nothing about the deadline: {output:?}"
        );
        assert_eq!(
            output.killed_by,
            Some(Control::WallClock),
            "a killed workload has to say it was killed, or its empty output reads as success"
        );
        assert!(!output.succeeded());
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the deadline did not actually bind"
        );
    }
}
