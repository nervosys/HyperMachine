//! Confinement on Unixes that are not Linux — macOS, the BSDs.
//!
//! There is no namespace or cgroup equivalent here that this crate implements,
//! so what is left is `setrlimit` and a wall-clock deadline. That is a real but
//! small set, and the point of this file is that it says so: every control it
//! cannot provide is reported as unavailable with a reason, so a caller asking
//! for network isolation on macOS is refused rather than handed a process with
//! full network access and a sandbox-shaped API around it.
//!
//! macOS has `sandbox_init`, which would cover more of this. It has been
//! deprecated since 10.8 with no supported replacement for third-party use,
//! and building on it would mean claiming isolation from an interface Apple
//! does not support. A caller needing more than resource limits here uses the
//! microVM sandbox.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use crate::{
    Control, Controls, FilesystemPolicy, RunIo, SandboxCommand, SandboxError, SandboxOutput,
    SandboxSpec,
};

use super::driver;

/// What resource limits give us here.
pub(super) fn probe() -> Controls {
    let unsupported = |what: &str| {
        format!(
            "{what} is not implemented for {}; use the microVM sandbox",
            std::env::consts::OS
        )
    };

    Controls::none()
        .with(Control::CpuTime)
        .with(Control::WallClock)
        // RLIMIT_AS is an address-space limit rather than a resident-memory
        // one, which is a different promise. Reporting it as a memory limit
        // would overstate what a caller gets.
        .without(
            Control::Memory,
            unsupported("a committed-memory limit (RLIMIT_AS bounds address space, not usage)"),
        )
        // RLIMIT_NPROC is the only process limit here, and it caps every
        // process the *user* owns, not the workload's. A user already running
        // more than the limit -- any desktop session -- has the spawn refused;
        // one running fewer gets a workload that may fork until the user's
        // total reaches it. Neither is "at most N processes".
        .without(
            Control::ProcessCount,
            format!(
                "a per-workload process limit is not available on {}: RLIMIT_NPROC counts \
                 every process the user owns, not the workload's; use the microVM sandbox",
                std::env::consts::OS
            ),
        )
        .without(Control::NetworkIsolation, unsupported("network isolation"))
        .without(
            Control::FilesystemIsolation,
            unsupported("filesystem isolation"),
        )
        .without(Control::ProcessIsolation, unsupported("process isolation"))
        .without(
            Control::NoNewPrivileges,
            unsupported("a no-new-privileges bit"),
        )
}

/// Run `command` under `spec`.
pub(super) fn run(
    command: &SandboxCommand,
    spec: &SandboxSpec,
    io: &RunIo,
) -> Result<SandboxOutput, SandboxError> {
    if let FilesystemPolicy::Isolated { .. } = spec.filesystem {
        return Err(SandboxError::InvalidSpec(format!(
            "the process backend cannot isolate the filesystem on {}",
            std::env::consts::OS
        )));
    }

    let cpu_seconds = spec.cpu_time.map(|d| d.as_secs().max(1));

    let mut builder = Command::new(&command.program);
    builder
        .args(&command.args)
        .env_clear()
        .envs(&command.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &command.working_dir {
        builder.current_dir(dir);
    }

    // SAFETY: the closure runs between fork and exec. It allocates nothing and
    // calls only async-signal-safe functions.
    unsafe {
        builder.pre_exec(move || {
            if let Some(seconds) = cpu_seconds {
                set_rlimit(libc::RLIMIT_CPU, seconds)?;
            }
            // Lead a process group, so the deadline can kill everything the
            // workload started rather than only the workload.
            libc::setpgid(0, 0);
            Ok(())
        });
    }

    let child = builder.spawn().map_err(|e| SandboxError::Spawn {
        program: command.program.clone(),
        source: e,
    })?;
    let pid = child.id() as libc::pid_t;

    driver::wait_with_deadline(child, command.stdin.as_deref(), spec.wall_clock, io, || {
        // SAFETY: signalling a process group we created.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
            libc::kill(pid, libc::SIGKILL);
        }
    })
}

fn set_rlimit(resource: libc::c_int, value: u64) -> std::io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: `limit` is fully initialised and outlives the call.
    if unsafe { libc::setrlimit(resource, &limit) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
