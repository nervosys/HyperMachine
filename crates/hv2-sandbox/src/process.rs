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

use crate::{
    Control, Controls, FilesystemPolicy, OutputSink, OutputStream, RunIo, Sandbox, SandboxCommand,
    SandboxError, SandboxOutput, SandboxSpec,
};

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
        self.run_with(command, spec, &RunIo::default())
    }

    fn run_with(
        &self,
        command: &SandboxCommand,
        spec: &SandboxSpec,
        io: &RunIo,
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

        for path in spec.grants.read_only.iter().chain(&spec.grants.read_write) {
            if !path.is_absolute() || !path.exists() {
                return Err(SandboxError::InvalidSpec(format!(
                    "granted path {} must be absolute and exist",
                    path.display()
                )));
            }
        }
        let mounted: &[std::path::PathBuf] = match &spec.filesystem {
            FilesystemPolicy::Isolated { read_only, .. } => read_only,
            FilesystemPolicy::Host => &[],
        };
        for denied in &spec.grants.denied {
            if !denied.is_absolute() || !denied.exists() {
                return Err(SandboxError::InvalidSpec(format!(
                    "denied path {} must be absolute and exist",
                    denied.display()
                )));
            }
            // A denial covers everything under it. Which of the two a caller
            // meant by naming both is not something to guess.
            let granted = spec
                .grants
                .read_only
                .iter()
                .chain(&spec.grants.read_write)
                .chain(mounted)
                .find(|path| path.starts_with(denied));
            if let Some(path) = granted {
                return Err(SandboxError::InvalidSpec(format!(
                    "granted path {} is under denied path {}, which closes everything under it",
                    path.display(),
                    denied.display()
                )));
            }
        }
        // A root of the caller's choosing holds what the caller mounts in it.
        // A read-only grant is such a mount. A read-write one is too where
        // the backend can mount one writable, which is Linux; elsewhere it
        // has nowhere to go, and running without it would be the quiet
        // downgrade this crate refuses to make.
        let widened;
        let spec = match &spec.filesystem {
            FilesystemPolicy::Isolated { root, read_only } if !spec.grants.is_empty() => {
                if !spec.grants.read_write.is_empty() && !cfg!(target_os = "linux") {
                    return Err(SandboxError::InvalidSpec(
                        "a read-write grant cannot be given inside an isolated root on this \
                         platform; make the path part of the root"
                            .to_string(),
                    ));
                }
                let mut mounts = read_only.clone();
                mounts.extend(spec.grants.read_only.iter().cloned());
                widened = SandboxSpec {
                    filesystem: FilesystemPolicy::Isolated {
                        root: root.clone(),
                        read_only: mounts,
                    },
                    grants: crate::PathGrants {
                        read_only: Vec::new(),
                        read_write: spec.grants.read_write.clone(),
                        denied: spec.grants.denied.clone(),
                    },
                    ..spec.clone()
                };
                &widened
            }
            _ => spec,
        };

        // Reconcile before anything is started. A workload that has already
        // begun cannot be un-started, and discovering the sandbox is weaker
        // than promised afterwards is too late to matter.
        let unenforced = spec.reconcile(&self.controls)?;

        // Hand the backend only what this host said it enforces. Passing the
        // caller's spec through would have the backend try to apply a control
        // its own probe reported unavailable, which turns best-effort from
        // "run with what we have" into "fail anyway, later and less clearly".
        let effective = spec.without_controls(&unenforced);

        let mut output = run_confined(command, &effective, io)?;
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
    io: &RunIo,
) -> Result<SandboxOutput, SandboxError> {
    #[cfg(target_os = "linux")]
    {
        linux::run(command, spec, io)
    }
    #[cfg(windows)]
    {
        windows::run(command, spec, io)
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        unix_fallback::run(command, spec, io)
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

    /// How a workload ended.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Exit {
        pub(crate) code: Option<i32>,
        pub(crate) signal: Option<i32>,
    }

    /// A started workload: its three pipes and how to wait for it.
    ///
    /// What [`wait_with_deadline`] needs of a process, and no more, so a
    /// backend that cannot start its workload through `std::process` -- one
    /// that needs a process attribute the standard library has no way to
    /// pass -- can still hand it one.
    pub(crate) struct Spawned {
        pub(crate) stdin: Option<Box<dyn Write + Send>>,
        pub(crate) stdout: Option<Box<dyn Read + Send>>,
        pub(crate) stderr: Option<Box<dyn Read + Send>>,
        /// The exit, if the workload has ended.
        pub(crate) try_wait: Box<dyn FnMut() -> std::io::Result<Option<Exit>>>,
        /// Block until it ends.
        pub(crate) wait: Box<dyn FnMut() -> std::io::Result<Exit>>,
    }

    impl From<Child> for Spawned {
        fn from(mut child: Child) -> Self {
            let exit = |status: std::process::ExitStatus| Exit {
                code: status.code(),
                signal: signal_of(&status),
            };
            let stdin = child
                .stdin
                .take()
                .map(|p| Box::new(p) as Box<dyn Write + Send>);
            let stdout = child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>);
            let stderr = child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>);
            let child = std::rc::Rc::new(std::cell::RefCell::new(child));
            let polled = std::rc::Rc::clone(&child);
            Self {
                stdin,
                stdout,
                stderr,
                try_wait: Box::new(move || Ok(polled.borrow_mut().try_wait()?.map(exit))),
                wait: Box::new(move || child.borrow_mut().wait().map(exit)),
            }
        }
    }

    /// Feed stdin, wait for the child, and enforce the wall-clock deadline.
    ///
    /// `kill` is how this platform stops the whole workload — for a job object
    /// that is terminating the job, not the one process, so a child that
    /// spawned grandchildren does not leave them running.
    pub(crate) fn wait_with_deadline(
        child: impl Into<Spawned>,
        stdin: Option<&[u8]>,
        deadline: Option<Duration>,
        io: &RunIo,
        kill: impl FnOnce(),
    ) -> Result<SandboxOutput, SandboxError> {
        let mut child: Spawned = child.into();
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

        let sink = io.on_output.clone();
        std::thread::spawn(move || {
            let _ = out_tx.send(drain(
                stdout_pipe.as_mut(),
                OutputStream::Stdout,
                sink.as_ref(),
            ));
        });
        let sink = io.on_output.clone();
        std::thread::spawn(move || {
            let _ = err_tx.send(drain(
                stderr_pipe.as_mut(),
                OutputStream::Stderr,
                sink.as_ref(),
            ));
        });

        let cancelled = || {
            io.cancel
                .as_ref()
                .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
        };
        let (status, killed) = if deadline.is_none() && io.cancel.is_none() {
            (
                (child.wait)()
                    .map_err(|e| SandboxError::Runtime(format!("waiting for workload: {e}")))?,
                false,
            )
        } else {
            let start = std::time::Instant::now();
            let mut kill = Some(kill);
            loop {
                match (child.try_wait)() {
                    Ok(Some(status)) => break (status, false),
                    Ok(None) => {}
                    Err(e) => {
                        return Err(SandboxError::Runtime(format!("waiting for workload: {e}")))
                    }
                }
                let expired = deadline.is_some_and(|limit| start.elapsed() >= limit);
                if expired || cancelled() {
                    if let Some(kill) = kill.take() {
                        kill();
                    }
                    // Reap it, so a kill does not leave a zombie behind every
                    // time it fires.
                    let status = (child.wait)().map_err(|e| {
                        SandboxError::Runtime(format!("reaping killed workload: {e}"))
                    })?;
                    break (status, expired);
                }
                std::thread::sleep(POLL);
            }
        };

        let stdout = out_rx.recv().unwrap_or_default();
        let stderr = err_rx.recv().unwrap_or_default();

        Ok(SandboxOutput {
            exit_code: status.code,
            signal: status.signal,
            stdout,
            stderr,
            killed_by: killed.then_some(Control::WallClock),
            unenforced: Vec::new(),
        })
    }

    /// Read `pipe` to its end: into a buffer returned at the end, or, with a
    /// sink, into the sink as each chunk arrives, keeping none of it.
    fn drain(
        pipe: Option<&mut impl Read>,
        stream: OutputStream,
        sink: Option<&OutputSink>,
    ) -> Vec<u8> {
        let mut kept = Vec::new();
        let Some(pipe) = pipe else { return kept };
        let Some(sink) = sink else {
            let _ = pipe.read_to_end(&mut kept);
            return kept;
        };
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => sink(stream, &chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        kept
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
    use std::sync::Arc;
    use std::time::Duration;

    /// Under this variable, [`slow_helper`] is the workload: it prints
    /// `first`, waits this many milliseconds, prints `second`, then waits as
    /// long again before exiting.
    const SLOW_HELPER_MS: &str = "HV2_SANDBOX_SLOW_HELPER_MS";

    #[test]
    fn slow_helper() {
        use std::io::Write;
        let Ok(ms) = std::env::var(SLOW_HELPER_MS) else {
            return;
        };
        let pause = Duration::from_millis(ms.parse().expect("milliseconds"));
        println!("first");
        std::io::stdout().flush().unwrap();
        std::thread::sleep(pause);
        eprintln!("second");
        std::thread::sleep(pause);
    }

    /// This test binary, re-run as [`slow_helper`] with `pause_ms`.
    fn slow_command(pause_ms: u64) -> SandboxCommand {
        let exe = std::env::current_exe().expect("this test binary's own path");
        let command = SandboxCommand::new(exe.to_string_lossy())
            .args(["--exact", "process::tests::slow_helper", "--nocapture"])
            .env(SLOW_HELPER_MS, pause_ms.to_string());
        #[cfg(windows)]
        let command = command
            .env("SystemRoot", r"C:\Windows")
            .env("PATH", r"C:\Windows\System32");
        command
    }

    /// A grant that cannot be honoured refuses the run, on every platform and
    /// before anything starts: a path that is not absolute or not there, and
    /// a read-write grant inside a root of the caller's choosing where the
    /// platform has nowhere to put it, which is everywhere but Linux.
    #[test]
    fn a_grant_that_cannot_be_honoured_refuses_the_run() {
        let here = std::env::temp_dir();
        let run = |spec: SandboxSpec| ProcessSandbox::new().run(&SandboxCommand::new("x"), &spec);
        let grants = |read_only: Vec<std::path::PathBuf>, read_write| crate::PathGrants {
            read_only,
            read_write,
            denied: Vec::new(),
        };
        for bad in [
            std::path::PathBuf::from("relative"),
            here.join("hv2-not-there"),
        ] {
            let refused = run(SandboxSpec {
                grants: grants(vec![bad], Vec::new()),
                ..SandboxSpec::unconfined()
            });
            assert!(
                matches!(&refused, Err(SandboxError::InvalidSpec(why)) if why.contains("granted path")),
                "{refused:?}"
            );
        }
        #[cfg(not(target_os = "linux"))]
        {
            let refused = run(SandboxSpec {
                filesystem: FilesystemPolicy::Isolated {
                    root: here.clone(),
                    read_only: Vec::new(),
                },
                grants: grants(Vec::new(), vec![here.clone()]),
                best_effort: true,
                ..SandboxSpec::unconfined()
            });
            assert!(
                matches!(&refused, Err(SandboxError::InvalidSpec(why)) if why.contains("read-write grant")),
                "{refused:?}"
            );
        }
    }

    /// A denied path that cannot be honoured refuses the run, on every
    /// platform and before anything starts: one that is not absolute or not
    /// there, and one with a granted path under it.
    #[test]
    fn a_denial_that_cannot_be_honoured_refuses_the_run() {
        let here = std::env::temp_dir();
        let run = |grants: crate::PathGrants| {
            ProcessSandbox::new().run(
                &SandboxCommand::new("x"),
                &SandboxSpec {
                    grants,
                    best_effort: true,
                    ..SandboxSpec::unconfined()
                },
            )
        };
        for bad in [
            std::path::PathBuf::from("relative"),
            here.join("hv2-not-there"),
        ] {
            let refused = run(crate::PathGrants {
                denied: vec![bad],
                ..crate::PathGrants::default()
            });
            assert!(
                matches!(&refused, Err(SandboxError::InvalidSpec(why)) if why.contains("denied path")),
                "{refused:?}"
            );
        }
        // The same path both ways, and a grant beneath a denial.
        let parent = here.parent().expect("the temporary directory has a parent");
        for denied in [here.clone(), parent.to_path_buf()] {
            let refused = run(crate::PathGrants {
                read_only: vec![here.clone()],
                read_write: Vec::new(),
                denied: vec![denied],
            });
            assert!(
                matches!(&refused, Err(SandboxError::InvalidSpec(why)) if why.contains("is under denied path")),
                "{refused:?}"
            );
        }

        // Asking for one is asking for the control that enforces it, and
        // giving that control up gives the denial up where it can be seen.
        let spec = SandboxSpec {
            grants: crate::PathGrants {
                denied: vec![here],
                ..crate::PathGrants::default()
            },
            ..SandboxSpec::unconfined()
        };
        assert_eq!(spec.required(), vec![Control::PathDenial]);
        assert!(spec
            .without_controls(&[Control::PathDenial])
            .grants
            .denied
            .is_empty());
    }

    /// Output reaches the sink while the workload is still running -- the
    /// point of streaming -- and is not also collected.
    #[test]
    fn output_is_streamed_as_it_arrives_not_after_the_exit() {
        use std::sync::Mutex;
        type Seen = Vec<(std::time::Instant, OutputStream, Vec<u8>)>;
        let seen: Arc<Mutex<Seen>> = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = Arc::clone(&seen);
        let io = RunIo {
            on_output: Some(Arc::new(move |stream, bytes: &[u8]| {
                sink_seen
                    .lock()
                    .unwrap()
                    .push((std::time::Instant::now(), stream, bytes.to_vec()));
            })),
            cancel: None,
        };
        let output = ProcessSandbox::new()
            .run_with(&slow_command(1500), &SandboxSpec::unconfined(), &io)
            .expect("run");
        let ended = std::time::Instant::now();
        assert_eq!(output.exit_code, Some(0));
        assert!(
            output.stdout.is_empty() && output.stderr.is_empty(),
            "collected as well"
        );

        let seen = seen.lock().unwrap();
        let text = |want: OutputStream| -> String {
            seen.iter()
                .filter(|(_, s, _)| *s == want)
                .map(|(_, _, b)| String::from_utf8_lossy(b).into_owned())
                .collect()
        };
        assert!(text(OutputStream::Stdout).contains("first"), "{seen:?}");
        assert!(text(OutputStream::Stderr).contains("second"), "{seen:?}");
        let first_at = seen
            .iter()
            .find(|(_, _, b)| String::from_utf8_lossy(b).contains("first"))
            .map(|(t, _, _)| *t)
            .unwrap();
        assert!(
            ended.duration_since(first_at) >= Duration::from_millis(2000),
            "`first` arrived only {:?} before the run ended",
            ended.duration_since(first_at)
        );
    }

    /// Setting the cancel flag stops the workload, and the run returns.
    #[test]
    fn a_cancelled_run_stops_the_workload_and_returns() {
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let io = RunIo {
            on_output: None,
            cancel: Some(Arc::clone(&cancel)),
        };
        let flag = Arc::clone(&cancel);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let output = ProcessSandbox::new()
            .run_with(&slow_command(60_000), &SandboxSpec::unconfined(), &io)
            .expect("run");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
        assert!(!output.succeeded(), "{output:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("second"),
            "it ran on past the cancel"
        );
    }

    /// The trait's default, for a backend with no streaming of its own:
    /// the output still reaches the sink, once the run is over.
    #[test]
    fn the_default_run_with_hands_collected_output_to_the_sink() {
        struct Canned;
        impl Sandbox for Canned {
            fn name(&self) -> &str {
                "canned"
            }
            fn controls(&self) -> Controls {
                Controls::none()
            }
            fn run(
                &self,
                _: &SandboxCommand,
                _: &SandboxSpec,
            ) -> Result<SandboxOutput, SandboxError> {
                Ok(SandboxOutput {
                    exit_code: Some(0),
                    signal: None,
                    stdout: b"out".to_vec(),
                    stderr: b"err".to_vec(),
                    killed_by: None,
                    unenforced: Vec::new(),
                })
            }
        }
        let got = Arc::new(std::sync::Mutex::new(Vec::new()));
        let g = Arc::clone(&got);
        let io = RunIo {
            on_output: Some(Arc::new(move |s, b: &[u8]| {
                g.lock().unwrap().push((s, b.to_vec()));
            })),
            cancel: None,
        };
        let out = Canned
            .run_with(&SandboxCommand::new("x"), &SandboxSpec::unconfined(), &io)
            .unwrap();
        assert!(out.stdout.is_empty());
        assert_eq!(
            *got.lock().unwrap(),
            vec![
                (OutputStream::Stdout, b"out".to_vec()),
                (OutputStream::Stderr, b"err".to_vec())
            ]
        );
    }

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

    /// Turns this test binary into an allocator when the memory-limit test
    /// re-executes it.
    ///
    /// The workload has to fit under the cap it is testing. PowerShell, which
    /// this used to run, spends most of a 256 MiB budget starting up, and on a
    /// cold CI runner it spent the whole wall-clock deadline doing so -- a
    /// timeout that said nothing either way about the memory limit.
    #[cfg(windows)]
    const ALLOC_HELPER_BYTES: &str = "HV2_SANDBOX_ALLOC_HELPER_BYTES";

    #[cfg(windows)]
    #[test]
    fn alloc_helper() {
        // Does nothing when run as an ordinary test. Under the variable it is
        // the workload: ask Windows to commit the requested bytes and say
        // which answer came back. `try_reserve_exact` rather than a plain
        // allocation, so a refusal is a value this process can print instead
        // of an abort with nothing in it to assert on.
        let Ok(requested) = std::env::var(ALLOC_HELPER_BYTES) else {
            return;
        };
        let bytes: usize = requested.parse().expect("a byte count");
        let mut buffer: Vec<u8> = Vec::new();
        match buffer.try_reserve_exact(bytes) {
            Ok(()) => println!("COMMITTED {bytes}"),
            Err(err) => eprintln!("REFUSED {bytes}: {err}"),
        }
    }

    /// Run [`alloc_helper`] confined to `cap` bytes, asking it for `ask`.
    #[cfg(windows)]
    fn allocate_confined(sandbox: &ProcessSandbox, cap: u64, ask: usize) -> SandboxOutput {
        let exe = std::env::current_exe().expect("this test binary's own path");
        let command = SandboxCommand::new(exe.to_string_lossy())
            .args(["--exact", "process::tests::alloc_helper", "--nocapture"])
            .env(ALLOC_HELPER_BYTES, ask.to_string())
            .env("SystemRoot", r"C:\Windows")
            .env("PATH", r"C:\Windows\System32");
        let spec = SandboxSpec {
            memory_bytes: Some(cap),
            wall_clock: Some(Duration::from_secs(60)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };
        sandbox.run(&command, &spec).expect("run")
    }

    /// Turns this test binary into a memory hog when the Linux memory-limit
    /// test re-executes it.
    ///
    /// It *touches* what it allocates, which is the whole point on Linux:
    /// `memory.max` does not refuse a mapping, it accounts pages as they are
    /// faulted in and kills the cgroup when the charge exceeds the cap. A
    /// helper that only called `malloc` would sail past any limit.
    #[cfg(target_os = "linux")]
    const HOG_HELPER_MIB: &str = "HV2_SANDBOX_HOG_HELPER_MIB";

    #[cfg(target_os = "linux")]
    #[test]
    fn hog_helper() {
        let Ok(requested) = std::env::var(HOG_HELPER_MIB) else {
            return;
        };
        let mib: usize = requested.parse().expect("a megabyte count");
        let mut held: Vec<Vec<u8>> = Vec::new();
        for _ in 0..mib {
            // One megabyte at a time, written to, so every page is charged to
            // the cgroup rather than left as an untouched reservation.
            let mut chunk = vec![0u8; 1024 * 1024];
            for page in chunk.chunks_mut(4096) {
                page[0] = 1;
            }
            held.push(chunk);
        }
        println!("COMMITTED {mib}");
    }

    /// Run [`hog_helper`] under `cap` bytes, asking it to touch `ask` MiB.
    #[cfg(target_os = "linux")]
    fn hog_confined(sandbox: &ProcessSandbox, cap: u64, ask: usize) -> SandboxOutput {
        let exe = std::env::current_exe().expect("this test binary's own path");
        let command = SandboxCommand::new(exe.to_string_lossy())
            .args(["--exact", "process::tests::hog_helper", "--nocapture"])
            .env(HOG_HELPER_MIB, ask.to_string());
        let spec = SandboxSpec {
            memory_bytes: Some(cap),
            wall_clock: Some(Duration::from_secs(60)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };
        sandbox.run(&command, &spec).expect("run")
    }

    /// The Linux half of the memory limit, which had no test at all.
    ///
    /// `linux.rs` implements namespaces, `pivot_root` and the cgroup v2 caps,
    /// and every confinement test in this module that touches memory or
    /// process count was `#[cfg(windows)]`. So `Control::Memory` was reported
    /// as enforced here on the strength of a successful write to `memory.max`
    /// and nothing else -- the same shape of evidence the Windows test was
    /// written to replace.
    ///
    /// Skips honestly where the controller is not delegated, which is most
    /// unprivileged hosts; see `CgroupScope::create` for why that happens and
    /// what it now does about it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_memory_limit_kills_a_workload_that_touches_past_it() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::Memory) {
            eprintln!("skipping: the memory controller is not delegated to this cgroup");
            return;
        }

        // Well over the cap, and touched, so the charge is real.
        let over = hog_confined(&sandbox, 64 * 1024 * 1024, 512);
        assert!(
            !String::from_utf8_lossy(&over.stdout).contains("COMMITTED"),
            "a workload 8x over its cap should not have finished: {over:?}"
        );
        assert!(
            over.signal.is_some() || over.exit_code.is_some_and(|c| c != 0),
            "it should have been killed or have failed, not succeeded: {over:?}"
        );

        // The other direction, and the reason this is evidence rather than a
        // coincidence: the same helper under a cap above what it asks for is
        // allowed to finish. A one-sided test also passes on a machine that
        // simply could not allocate.
        let under = hog_confined(&sandbox, 256 * 1024 * 1024, 16);
        assert!(
            String::from_utf8_lossy(&under.stdout).contains("COMMITTED 16"),
            "16 MiB under a 256 MiB cap should have been allowed: {under:?}"
        );
    }

    /// The pids controller, same reasoning.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_limit_stops_the_workload_spawning_past_it_on_linux() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::ProcessCount) {
            eprintln!("skipping: the pids controller is not delegated to this cgroup");
            return;
        }

        // `sh` itself is one process; the `sh -c` it tries to start is the
        // second, and pids.max=1 is what refuses it. Nothing in this crate is
        // consulted at that moment, which is the point.
        let command = SandboxCommand::new("/bin/sh").args(["-c", "/bin/sh -c 'echo nested'"]);
        let spec = SandboxSpec {
            max_processes: Some(1),
            wall_clock: Some(Duration::from_secs(30)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };
        let out = sandbox.run(&command, &spec).expect("run");
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("nested"),
            "the nested shell should not have run under pids.max=1: {out:?}"
        );
    }

    /// The limit is the workload's, not the user's. With `RLIMIT_NPROC` set
    /// beside `pids.max`, a caller whose user already owned more threads than
    /// the limit -- counted host-wide -- had every spawn refused with EAGAIN.
    /// The caller here makes sure of that by holding 80 threads itself.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_limit_counts_the_workload_not_everything_the_user_runs() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::ProcessCount) {
            eprintln!("skipping: the pids controller is not delegated to this cgroup");
            return;
        }

        let release = std::sync::Arc::new(std::sync::Barrier::new(81));
        let held: Vec<_> = (0..80)
            .map(|_| {
                let release = std::sync::Arc::clone(&release);
                std::thread::spawn(move || {
                    release.wait();
                })
            })
            .collect();

        // One fork, well inside the limit of 8. It has to fork: the per-user
        // count was checked at fork, so a workload that only execs would pass
        // with the bug in place -- which is how this test's first draft did.
        let command = SandboxCommand::new("/bin/sh").args(["-c", "/bin/true && echo ran"]);
        let spec = SandboxSpec {
            max_processes: Some(8),
            wall_clock: Some(Duration::from_secs(30)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };
        let result = sandbox.run(&command, &spec);

        release.wait();
        for thread in held {
            thread.join().unwrap();
        }
        let out = result.expect("a limit of 8 must not count the caller's own 80 threads");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ran");
    }

    #[cfg(windows)]
    #[test]
    fn a_memory_limit_stops_the_workload_allocating_past_it() {
        // The process-count test proves the kernel refuses a second process.
        // This proves the other half of the job object: the workload asks for
        // memory past the cap and Windows refuses that too. Without it,
        // Control::Memory was reported as enforced on the strength of
        // SetInformationJobObject returning success.
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::Memory) {
            eprintln!("skipping: this host has no usable job objects");
            return;
        }

        const ASK: usize = 512 * 1024 * 1024;

        let capped = allocate_confined(&sandbox, 128 * 1024 * 1024, ASK);
        assert!(
            String::from_utf8_lossy(&capped.stderr).contains("REFUSED"),
            "the allocation should have been refused by the job limit: {capped:?}"
        );

        // The other direction, and the reason this is evidence rather than a
        // coincidence: the same request under a cap above it is granted. A
        // one-sided test also passes on a machine that simply had no memory to
        // spare, which would say nothing about whether the cap did anything.
        let roomy = allocate_confined(&sandbox, 2 * 1024 * 1024 * 1024, ASK);
        assert!(
            String::from_utf8_lossy(&roomy.stdout).contains("COMMITTED"),
            "the same request under a larger cap should have been granted: {roomy:?}"
        );
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

    /// Run `sh -c script` confined, returning trimmed stdout.
    ///
    /// These ask the workload what it can see, which is the only question that
    /// settles whether an isolation claim is true. A test that read
    /// `controls()` back would pass on a backend that reported the set and
    /// applied none of it.
    #[cfg(target_os = "linux")]
    fn confined_shell(sandbox: &ProcessSandbox, script: &str, spec: &SandboxSpec) -> String {
        let command = SandboxCommand::new("/bin/sh").args(["-c", script]);
        let output = sandbox.run(&command, spec).expect("run");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_isolated_workload_is_pid_1_and_cannot_see_the_host() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::ProcessIsolation) {
            eprintln!(
                "skipping: {}",
                sandbox
                    .controls()
                    .reason(Control::ProcessIsolation)
                    .unwrap_or("process isolation unavailable")
            );
            return;
        }

        let mut spec = SandboxSpec::untrusted(64 * 1024 * 1024, Duration::from_secs(10));
        spec.best_effort = true;

        // unshare(CLONE_NEWPID) moves the *next* child into the namespace, not
        // the caller. Without the second fork this reads as the host's pid,
        // and the isolation claim would be false in a way nothing else shows.
        assert_eq!(
            confined_shell(&sandbox, "echo $$", &spec),
            "1",
            "the workload should be PID 1 in its own namespace"
        );

        // A PID namespace alone does not hide the process table: /proc is
        // inherited from the host's namespace unless it is remounted. Before
        // that remount this counted 48 on the machine it was first run on.
        let visible: usize = confined_shell(&sandbox, "ls /proc | grep -c '^[0-9]'", &spec)
            .parse()
            .expect("a count");
        let on_host = std::fs::read_dir("/proc")
            .expect("/proc")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .chars()
                    .all(|c| c.is_ascii_digit())
            })
            .count();

        assert!(
            visible <= 5,
            "the workload can see {visible} processes; the host has {on_host}"
        );
        assert!(
            on_host > visible,
            "the host should have more processes than the sandbox, or this proves nothing"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_network_isolated_workload_has_only_loopback() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::NetworkIsolation) {
            eprintln!("skipping: this host cannot isolate the network");
            return;
        }

        let mut spec = SandboxSpec::untrusted(64 * 1024 * 1024, Duration::from_secs(10));
        spec.best_effort = true;

        // An empty network namespace has loopback and nothing else, and
        // loopback comes up down. Anything more means the workload kept the
        // host's interfaces.
        let interfaces = confined_shell(&sandbox, "ls /sys/class/net | wc -l", &spec);
        assert_eq!(
            interfaces, "1",
            "a network-isolated workload should see only loopback"
        );
    }

    /// A scratch directory holding a root to pivot into and a file outside it.
    ///
    /// Both halves matter: the file outside is the one the workload must not be
    /// able to reach, and it has to be a real file on the host or its absence
    /// inside proves nothing.
    #[cfg(target_os = "linux")]
    struct ScratchRoot {
        base: std::path::PathBuf,
    }

    #[cfg(target_os = "linux")]
    impl ScratchRoot {
        fn new(label: &str) -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("a clock after 1970")
                .as_nanos();
            let base = std::env::temp_dir().join(format!(
                "hv2-sandbox-{label}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(base.join("root")).expect("a scratch root");
            let scratch = Self { base };
            std::fs::write(scratch.outside(), b"host-only\n").expect("a file outside the root");
            scratch
        }

        fn root(&self) -> std::path::PathBuf {
            self.base.join("root")
        }

        /// A file that exists on the host and is not under the root.
        fn outside(&self) -> std::path::PathBuf {
            self.base.join("host-only-secret")
        }

        /// The policy that gives the workload this root, with enough of the
        /// host mounted read-only for `/bin/sh` to run inside it.
        fn policy(&self) -> crate::FilesystemPolicy {
            let read_only = ["/bin", "/usr", "/lib", "/lib64", "/sbin"]
                .iter()
                .map(std::path::PathBuf::from)
                .filter(|path| path.exists())
                .collect();
            crate::FilesystemPolicy::Isolated {
                root: self.root(),
                read_only,
            }
        }

        /// A spec asking for the filesystem control and nothing else that could
        /// be unavailable here and turn a failure into a different story.
        fn spec(&self) -> SandboxSpec {
            SandboxSpec {
                filesystem: self.policy(),
                network: NetworkPolicy::Host,
                wall_clock: Some(Duration::from_secs(20)),
                ..SandboxSpec::default()
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for ScratchRoot {
        fn drop(&mut self) {
            // The mounts lived in the workload's own mount namespace and died
            // with it, so nothing here is still mounted in ours.
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// The reason to skip, or `None` when this host can isolate a filesystem.
    #[cfg(target_os = "linux")]
    fn filesystem_isolation_unavailable(sandbox: &ProcessSandbox) -> Option<String> {
        let controls = sandbox.controls();
        if controls.enforces(Control::FilesystemIsolation) {
            return None;
        }
        Some(
            controls
                .reason(Control::FilesystemIsolation)
                .unwrap_or("filesystem isolation unavailable")
                .to_string(),
        )
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_filesystem_isolated_workload_cannot_see_a_file_outside_its_root() {
        // The claim, stated as the workload's own view: this is the assertion
        // that would fail if `pivot_root` were swapped for a `chroot` that left
        // the old root mounted, or if the pivot silently did nothing. Reading
        // `controls()` back would pass in both of those cases.
        let sandbox = ProcessSandbox::new();
        if let Some(why) = filesystem_isolation_unavailable(&sandbox) {
            eprintln!("skipping: {why}");
            return;
        }

        let scratch = ScratchRoot::new("outside");
        assert!(
            scratch.outside().exists(),
            "the file has to exist on the host, or its absence inside proves nothing"
        );

        let script = format!(
            "if [ -e '{}' ]; then echo VISIBLE; else echo HIDDEN; fi",
            scratch.outside().display()
        );
        assert_eq!(
            confined_shell(&sandbox, &script, &scratch.spec()),
            "HIDDEN",
            "the workload reached a host file outside the root it was given"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_filesystem_isolated_workload_can_read_a_file_inside_its_root() {
        // The other direction, and the reason the test above is evidence rather
        // than a coincidence: a sandbox that showed the workload an empty or
        // broken filesystem would also hide the outside file, and would be
        // useless. The root has to be the one the spec named.
        let sandbox = ProcessSandbox::new();
        if let Some(why) = filesystem_isolation_unavailable(&sandbox) {
            eprintln!("skipping: {why}");
            return;
        }

        let scratch = ScratchRoot::new("inside");
        std::fs::write(scratch.root().join("inside-marker"), b"sandbox\n").expect("a file inside");

        assert_eq!(
            confined_shell(&sandbox, "cat /inside-marker", &scratch.spec()),
            "sandbox",
            "the directory the spec named should be the workload's root"
        );
    }

    /// The workload is root in its own user namespace, and must be root with
    /// no capabilities: the ones that namespace gives are what mounted its
    /// filesystem, and would unmake it. This is the test that failed before
    /// they were dropped: the remount succeeded and the write reached the host.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_workload_cannot_remount_a_read_only_grant_writable() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::PathConfinement) {
            eprintln!(
                "skipping: {}",
                sandbox
                    .controls()
                    .reason(Control::PathConfinement)
                    .unwrap_or("path confinement unavailable")
            );
            return;
        }

        let scratch = ScratchRoot::new("remount");
        let readable = scratch.base.join("readable");
        std::fs::create_dir_all(&readable).expect("a directory to read");
        let mut read_only: Vec<std::path::PathBuf> = ["/bin", "/usr", "/lib", "/lib64", "/sbin"]
            .iter()
            .map(std::path::PathBuf::from)
            .filter(|path| path.exists())
            .collect();
        read_only.push(readable.clone());
        let spec = SandboxSpec {
            grants: crate::PathGrants {
                read_only,
                read_write: Vec::new(),
                denied: Vec::new(),
            },
            confine_paths: true,
            network: NetworkPolicy::Host,
            wall_clock: Some(Duration::from_secs(20)),
            ..SandboxSpec::default()
        };

        // No redirect of mount's complaint: there is no /dev/null in there to
        // send it to, and a redirect that fails would skip the command.
        let script = format!(
            "command -v mount || echo NO-MOUNT-PROGRAM; \
             if mount -o remount,rw,bind '{readable}'; then echo REMOUNTED; else echo KEPT; fi; \
             if echo x > '{readable}/made'; then echo WROTE; else echo REFUSED; fi",
            readable = readable.display(),
        );
        let said = confined_shell(&sandbox, &script, &spec);
        assert!(
            !said.contains("NO-MOUNT-PROGRAM"),
            "without a mount program inside, nothing was attempted: {said}"
        );
        assert!(said.ends_with("KEPT\nREFUSED"), "{said}");
        assert!(
            !readable.join("made").exists(),
            "the write reached the host through a read-only grant"
        );
    }

    /// Every capability set is empty, in any user namespace the crate makes,
    /// and the bounding set with them, so nothing the workload runs gets one
    /// back.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_workload_in_a_user_namespace_holds_no_capabilities() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::ProcessIsolation) {
            eprintln!(
                "skipping: {}",
                sandbox
                    .controls()
                    .reason(Control::ProcessIsolation)
                    .unwrap_or("process isolation unavailable")
            );
            return;
        }
        // Process isolation is what gives it a /proc of its own to read.
        let spec = SandboxSpec {
            isolate_processes: true,
            network: NetworkPolicy::Host,
            wall_clock: Some(Duration::from_secs(20)),
            ..SandboxSpec::default()
        };
        let said = confined_shell(
            &sandbox,
            "while read name value; do case $name in Cap*) echo $name $value;; esac; done \
             < /proc/self/status",
            &spec,
        );
        let sets: Vec<&str> = said.lines().collect();
        assert_eq!(sets.len(), 5, "{said}");
        for set in sets {
            assert!(set.ends_with(" 0000000000000000"), "{said}");
        }
    }

    /// A denied path is closed on the host's own filesystem, with nothing
    /// else taken away: the directory cannot be entered, the file reads as
    /// empty, a write to it goes nowhere, and what is beside them is as it
    /// was.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_denied_path_is_closed_on_the_hosts_filesystem() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::PathDenial) {
            eprintln!(
                "skipping: {}",
                sandbox
                    .controls()
                    .reason(Control::PathDenial)
                    .unwrap_or("path denial unavailable")
            );
            return;
        }

        let scratch = ScratchRoot::new("deny-host");
        let secrets = scratch.base.join("secrets");
        std::fs::create_dir_all(&secrets).expect("a directory to deny");
        std::fs::write(secrets.join("key"), b"private\n").expect("a file under it");
        let token = scratch.base.join("token");
        std::fs::write(&token, b"private\n").expect("a file to deny");

        let spec = SandboxSpec {
            grants: crate::PathGrants {
                denied: vec![secrets.clone(), token.clone()],
                ..crate::PathGrants::default()
            },
            network: NetworkPolicy::Host,
            wall_clock: Some(Duration::from_secs(20)),
            ..SandboxSpec::default()
        };
        let script = format!(
            "if cat '{secrets}/key'; then echo READ; else echo CLOSED; fi; \
             if ls '{secrets}'; then echo LISTED; else echo CLOSED; fi; \
             echo \"[$(cat '{token}')]\"; \
             echo overwritten > '{token}'; \
             cat '{outside}'",
            secrets = secrets.display(),
            token = token.display(),
            outside = scratch.outside().display(),
        );
        assert_eq!(
            confined_shell(&sandbox, &script, &spec),
            "CLOSED\nCLOSED\n[]\nhost-only",
            "the denied directory and file should be closed and their neighbour open"
        );
        assert_eq!(
            std::fs::read_to_string(&token).expect("the host's file"),
            "private\n",
            "a write to a denied file reached the host"
        );
        assert_eq!(
            std::fs::read_to_string(secrets.join("key")).expect("the host's file"),
            "private\n"
        );
    }

    /// A denied path under a grant is closed inside a workload confined to
    /// its grants, and the rest of the grant is not.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_denied_path_is_carved_out_of_a_grant() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::PathDenial) {
            eprintln!(
                "skipping: {}",
                sandbox
                    .controls()
                    .reason(Control::PathDenial)
                    .unwrap_or("path denial unavailable")
            );
            return;
        }

        let scratch = ScratchRoot::new("deny-grant");
        let work = scratch.root();
        std::fs::create_dir_all(work.join("private")).expect("a directory to deny");
        std::fs::write(work.join("private/key"), b"private\n").expect("a file under it");
        std::fs::write(work.join("notes"), b"open\n").expect("a file beside it");
        let mut read_only: Vec<std::path::PathBuf> = ["/bin", "/usr", "/lib", "/lib64", "/sbin"]
            .iter()
            .map(std::path::PathBuf::from)
            .filter(|path| path.exists())
            .collect();
        read_only.sort();

        let spec = SandboxSpec {
            grants: crate::PathGrants {
                read_only,
                read_write: vec![work.clone()],
                // The second is not under any grant, so is closed already and
                // must not stop the run.
                denied: vec![work.join("private"), scratch.outside()],
            },
            confine_paths: true,
            network: NetworkPolicy::Host,
            wall_clock: Some(Duration::from_secs(20)),
            ..SandboxSpec::default()
        };
        let script = format!(
            "cat '{work}/notes'; \
             if cat '{work}/private/key'; then echo READ; else echo CLOSED; fi; \
             if echo x > '{work}/private/made'; then echo WROTE; else echo CLOSED; fi; \
             if echo y > '{work}/made'; then echo WROTE; else echo CLOSED; fi",
            work = work.display(),
        );
        assert_eq!(
            confined_shell(&sandbox, &script, &spec),
            "open\nCLOSED\nCLOSED\nWROTE"
        );
        assert!(!work.join("private/made").exists());
        assert!(work.join("made").exists());
    }

    /// A read-write grant inside a root of the caller's choosing is mounted
    /// there writable, at the path it has on the host.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_read_write_grant_is_writable_inside_an_isolated_root() {
        let sandbox = ProcessSandbox::new();
        if let Some(why) = filesystem_isolation_unavailable(&sandbox) {
            eprintln!("skipping: {why}");
            return;
        }

        let scratch = ScratchRoot::new("rw-grant");
        let shared = scratch.base.join("shared");
        std::fs::create_dir_all(shared.join("private")).expect("a directory to share");
        std::fs::write(
            shared.join("private/key"),
            b"private
",
        )
        .expect("a file to deny");
        // A denied path under it is closed here as anywhere else.
        let spec = SandboxSpec {
            grants: crate::PathGrants {
                read_only: Vec::new(),
                read_write: vec![shared.clone()],
                denied: vec![shared.join("private")],
            },
            ..scratch.spec()
        };
        let script = format!(
            "if echo z > '{0}/made'; then echo WROTE; else echo REFUSED; fi;              if cat '{0}/private/key'; then echo READ; else echo CLOSED; fi",
            shared.display()
        );
        assert_eq!(
            confined_shell(&sandbox, &script, &spec),
            "WROTE
CLOSED"
        );
        assert_eq!(
            std::fs::read_to_string(shared.join("made")).expect("the file it made"),
            "z\n"
        );
    }

    /// Confined to its grants with no root of the caller's making: what was
    /// granted is there, read-only or writable as it was granted, and nothing
    /// else of the host is.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_workload_confined_to_its_grants_reaches_them_and_nothing_else() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::PathConfinement) {
            eprintln!(
                "skipping: {}",
                sandbox
                    .controls()
                    .reason(Control::PathConfinement)
                    .unwrap_or("path confinement unavailable")
            );
            return;
        }

        // The scratch root is not used as a root here: it is a directory to
        // grant, beside a file that is not granted.
        let scratch = ScratchRoot::new("confine");
        let writable = scratch.root();
        let mut read_only: Vec<std::path::PathBuf> = ["/bin", "/usr", "/lib", "/lib64", "/sbin"]
            .iter()
            .map(std::path::PathBuf::from)
            .filter(|path| path.exists())
            .collect();
        let readable = scratch.base.join("readable");
        std::fs::create_dir_all(&readable).expect("a directory to read");
        std::fs::write(readable.join("note"), b"granted\n").expect("a file to read");
        read_only.push(readable.clone());
        assert!(
            std::path::Path::new("/etc/passwd").exists(),
            "the file has to exist on the host, or its absence inside proves nothing"
        );

        let spec = SandboxSpec {
            grants: crate::PathGrants {
                read_only,
                read_write: vec![writable.clone()],
                denied: Vec::new(),
            },
            confine_paths: true,
            network: NetworkPolicy::Host,
            wall_clock: Some(Duration::from_secs(20)),
            ..SandboxSpec::default()
        };
        assert_eq!(
            spec.required(),
            vec![Control::WallClock, Control::PathConfinement]
        );

        let script = format!(
            "if [ -e /etc/passwd ]; then echo VISIBLE; else echo HIDDEN; fi; \
             if [ -e '{outside}' ]; then echo VISIBLE; else echo HIDDEN; fi; \
             cat '{readable}/note'; \
             if echo x > '{readable}/made'; then echo WROTE; else echo REFUSED; fi; \
             if echo y > '{writable}/made'; then echo WROTE; else echo REFUSED; fi",
            outside = scratch.outside().display(),
            readable = readable.display(),
            writable = writable.display(),
        );
        assert_eq!(
            confined_shell(&sandbox, &script, &spec),
            "HIDDEN\nHIDDEN\ngranted\nREFUSED\nWROTE",
            "ungranted paths should be absent, and each grant what it was granted as"
        );
        assert!(
            !readable.join("made").exists(),
            "a read-only grant was written"
        );
        assert_eq!(
            std::fs::read_to_string(writable.join("made")).expect("the file it made"),
            "y\n",
            "a file made in a read-write grant should be on the host"
        );

        // Without the flag the same grants open and close nothing: the host's
        // filesystem is the workload's.
        let open = SandboxSpec {
            confine_paths: false,
            ..spec
        };
        assert_eq!(
            confined_shell(
                &sandbox,
                "if [ -e /etc/passwd ]; then echo VISIBLE; else echo HIDDEN; fi",
                &open
            ),
            "VISIBLE"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_read_only_mount_refuses_writes_while_the_root_itself_accepts_them() {
        // A bind mount is created read-write whatever flags the first mount
        // call carried; making it read-only is a second call that can fail. If
        // its failure were ignored the workload would get a writable /usr while
        // `FilesystemPolicy::read_only` said otherwise. The writable half is
        // here so a workload that simply could not write anything — a wrong
        // uid, say — cannot pass this by failing twice.
        let sandbox = ProcessSandbox::new();
        if let Some(why) = filesystem_isolation_unavailable(&sandbox) {
            eprintln!("skipping: {why}");
            return;
        }

        let scratch = ScratchRoot::new("readonly");
        // No `2>/dev/null` on these redirects. An isolated root has no /dev
        // unless the caller mounts one, so suppressing stderr that way makes
        // *both* writes fail on the redirect and the test passes for a reason
        // that has nothing to do with the mount — which is what it did.
        let script = "if echo x > /usr/hv2-write-probe; then echo WROTE; else echo REFUSED; fi; \
                      if echo y > /writable-probe; then echo WROTE; else echo REFUSED; fi";

        assert_eq!(
            confined_shell(&sandbox, script, &scratch.spec()),
            "REFUSED\nWROTE",
            "a read-only bind should refuse the write and the root itself should not"
        );
        assert!(
            !std::path::Path::new("/usr/hv2-write-probe").exists(),
            "the write escaped into the host's /usr"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_new_root_and_a_new_pid_namespace_hold_at_the_same_time() {
        // The ordering is load-bearing and only this combination exercises it:
        // /proc has to be mounted after the pivot or it lands in the root that
        // is thrown away, and the pivot has to come after the id maps are
        // written or /proc/self/uid_map has no name any more. Each control
        // tested alone passes with the order wrong.
        let sandbox = ProcessSandbox::new();
        if let Some(why) = filesystem_isolation_unavailable(&sandbox) {
            eprintln!("skipping: {why}");
            return;
        }
        if !sandbox.controls().enforces(Control::ProcessIsolation) {
            eprintln!("skipping: this host cannot isolate processes");
            return;
        }

        let scratch = ScratchRoot::new("both");
        std::fs::write(scratch.root().join("inside-marker"), b"sandbox\n").expect("a file inside");
        let mut spec = scratch.spec();
        spec.isolate_processes = true;

        assert_eq!(
            confined_shell(&sandbox, "echo $$; cat /inside-marker", &spec),
            "1\nsandbox",
            "the workload should be PID 1 and in the root it was given, at the same time"
        );

        // A /proc mounted before the pivot would have been discarded with the
        // old root, and the shell would report the host's process table or
        // nothing at all.
        let visible: usize = confined_shell(&sandbox, "ls /proc | grep -c '^[0-9]'", &spec)
            .parse()
            .expect("a count");
        assert!(
            (1..=5).contains(&visible),
            "the workload sees {visible} processes through a /proc inside its own root"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_working_directory_is_resolved_inside_the_new_root() {
        // `std` applies `Command::current_dir` before the `pre_exec` closure
        // runs, so a working directory left to it would be resolved against the
        // host and then thrown away by the pivot — the workload would start
        // somewhere it did not ask for and nothing would say so.
        let sandbox = ProcessSandbox::new();
        if let Some(why) = filesystem_isolation_unavailable(&sandbox) {
            eprintln!("skipping: {why}");
            return;
        }

        let scratch = ScratchRoot::new("workdir");
        std::fs::create_dir_all(scratch.root().join("work")).expect("a directory inside the root");
        let command = SandboxCommand::new("/bin/sh")
            .args(["-c", "pwd"])
            .working_dir("/work");
        let output = sandbox.run(&command, &scratch.spec()).expect("run");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "/work",
            "got: {output:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_isolated_root_that_does_not_exist_is_refused_before_anything_runs() {
        // The failure has to land in the parent, where it is a refusal. A root
        // checked only in the child would surface as an opaque spawn failure
        // after the workload had already been committed to.
        let sandbox = ProcessSandbox::new();
        if let Some(why) = filesystem_isolation_unavailable(&sandbox) {
            eprintln!("skipping: {why}");
            return;
        }

        let spec = SandboxSpec {
            filesystem: crate::FilesystemPolicy::Isolated {
                root: std::path::PathBuf::from("/no/such/sandbox/root"),
                read_only: Vec::new(),
            },
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };
        let err = sandbox
            .run(&SandboxCommand::new("/bin/echo"), &spec)
            .expect_err("a root that is not there cannot be pivoted into");
        assert!(matches!(err, SandboxError::InvalidSpec(_)), "got: {err}");
    }

    #[test]
    fn a_host_that_cannot_isolate_the_filesystem_refuses_to_pretend_it_did() {
        // Runs everywhere, including the platforms where this control is never
        // available: asking for a root on a backend that cannot give one is a
        // refusal naming the control, not a workload quietly seeing the host.
        let sandbox = ProcessSandbox::with_controls(Controls::none());
        let spec = SandboxSpec {
            filesystem: crate::FilesystemPolicy::Isolated {
                root: std::env::temp_dir(),
                read_only: Vec::new(),
            },
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };

        let err = sandbox
            .run(&SandboxCommand::new("/bin/echo"), &spec)
            .expect_err("a backend enforcing nothing must refuse");
        assert!(
            err.to_string().contains("filesystem isolation"),
            "got: {err}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn best_effort_does_not_attempt_a_control_the_probe_rejected() {
        // Found by running on a real kernel: `untrusted` asks for a memory
        // ceiling, this host has no writable cgroup, and the backend tried to
        // create one anyway — so best-effort failed rather than running with
        // what was available. The backend must be handed only what the probe
        // said it enforces.
        let sandbox = ProcessSandbox::new();
        let mut spec = SandboxSpec::untrusted(64 * 1024 * 1024, Duration::from_secs(10));
        spec.best_effort = true;

        let command = SandboxCommand::new("/bin/echo").args(["ok"]);
        let output = sandbox.run(&command, &spec).expect("best effort must run");
        assert!(output.succeeded(), "got: {output:?}");

        for control in &output.unenforced {
            assert!(
                !sandbox.controls().enforces(*control),
                "{control} was reported unenforced but the host does enforce it"
            );
        }
    }

    /// A deadline that kills the workload has to kill what the workload
    /// started, or an agent platform accumulates orphans.
    ///
    /// The existing deadline test runs a single `sleep` and checks that it
    /// dies. That says nothing about a workload with children, which is the
    /// case that leaks: the shell is killed, the process it forked is
    /// reparented to init, and nothing is left holding a reference to it. Ten
    /// thousand agent runs later the host has ten thousand strays.
    ///
    /// Marked with a distinctive argument so the check can find exactly the
    /// process this test started and nothing else on the machine.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_deadline_kill_takes_the_workload_s_children_with_it() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::WallClock) {
            eprintln!("skipping: this host does not enforce a wall-clock deadline");
            return;
        }

        // The marker is the *duration*, not an extra argument: `sleep` treats
        // anything after the interval as another interval and exits, so a
        // workload tagged that way dies instantly and the deadline never
        // fires. An implausible number of seconds, derived from this process
        // so two test runs cannot collide, is both unique and something sleep
        // will accept.
        let marker = 30_000 + (std::process::id() % 10_000);
        // A shell that forks a child and then waits. The deadline fires while
        // both are alive.
        let script = format!("/bin/sleep {marker} & /bin/sleep {marker}");
        let command = SandboxCommand::new("/bin/sh").args(["-c", &script]);
        let spec = SandboxSpec {
            wall_clock: Some(Duration::from_millis(300)),
            network: NetworkPolicy::Host,
            ..SandboxSpec::default()
        };

        let out = sandbox.run(&command, &spec).expect("run");
        assert_eq!(
            out.killed_by,
            Some(Control::WallClock),
            "the workload should have been killed by its deadline: {out:?}"
        );

        // Give the kernel a moment to reap, then look for anything left.
        std::thread::sleep(Duration::from_millis(400));
        let survivors = std::process::Command::new("/bin/ps")
            .args(["-eo", "args"])
            .output()
            .expect("ps");
        let listing = String::from_utf8_lossy(&survivors.stdout);
        let strays: Vec<&str> = listing
            .lines()
            .filter(|l| l.contains(&marker.to_string()) && !l.contains("ps -eo"))
            .collect();
        assert!(
            strays.is_empty(),
            "the deadline killed the workload and left {} of its processes \
             behind: {strays:?}",
            strays.len()
        );
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
