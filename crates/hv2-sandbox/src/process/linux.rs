//! Linux confinement: namespaces, cgroup v2, and resource limits.
//!
//! # What is enforced, and by what
//!
//! | Control | Mechanism |
//! | --- | --- |
//! | [`Control::Memory`] | `memory.max` in a cgroup v2 the workload is placed in before `exec` |
//! | [`Control::ProcessCount`] | `pids.max` in the same cgroup, plus `RLIMIT_NPROC` |
//! | [`Control::CpuTime`] | `RLIMIT_CPU`, which the kernel turns into `SIGKILL` |
//! | [`Control::WallClock`] | this crate, killing the process group |
//! | [`Control::NetworkIsolation`] | `CLONE_NEWNET`: an empty network namespace with only loopback, down |
//! | [`Control::ProcessIsolation`] | `CLONE_NEWPID` and `CLONE_NEWIPC`: the workload is PID 1 in its own namespace and cannot see or signal anything outside |
//! | [`Control::NoNewPrivileges`] | `prctl(PR_SET_NO_NEW_PRIVS)` |
//!
//! [`Control::FilesystemIsolation`] is **not** implemented here and is
//! reported as unavailable with that reason. Doing it properly means
//! `pivot_root` with a prepared root, and a `chroot` that looks like isolation
//! while a retained directory descriptor walks out of it would be exactly the
//! kind of claim this crate exists to stop making. A caller that needs it uses
//! the microVM sandbox, which gets it from having a different kernel.
//!
//! # Ordering
//!
//! Everything happens in the child between `fork` and `exec`, and the order is
//! load-bearing:
//!
//! 1. **Join the cgroup**, while still the original user. After
//!    `CLONE_NEWUSER` the process no longer has permission to write the file.
//! 2. **Resource limits and `no_new_privs`**, which need no privileges.
//! 3. **`unshare`** every namespace in one call, so the kernel creates the user
//!    namespace first and grants the capabilities the others need.
//! 4. **Write the id maps**, which is only possible from inside the new user
//!    namespace and only after `setgroups` is denied.
//! 5. **Fork again**, if a PID namespace was created: `unshare(CLONE_NEWPID)`
//!    puts the *next* child in the new namespace, not the caller. Without this
//!    step the workload would run in the host's PID namespace while the code
//!    claimed otherwise.
//!
//! # Async-signal-safety
//!
//! The code between `fork` and `exec` runs in a child that may share memory
//! with a multi-threaded parent, so it must not allocate or take locks. Every
//! string it needs is built in the parent and captured; it uses only raw
//! `libc` calls.

use std::ffi::CString;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    Control, Controls, FilesystemPolicy, NetworkPolicy, SandboxCommand, SandboxError,
    SandboxOutput, SandboxSpec,
};

use super::driver;

/// Distinguishes the cgroups this process creates from each other.
static CGROUP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Probe what this kernel actually allows.
///
/// Every answer comes from trying the thing. A kernel with unprivileged user
/// namespaces disabled, or a container with no writable cgroup delegation,
/// enforces less than the same code on the next machine, and a caller has to
/// be told which machine it is on.
pub(super) fn probe() -> Controls {
    let mut controls = Controls::none()
        // Always available: these need no privileges and no configuration.
        .with(Control::CpuTime)
        .with(Control::WallClock)
        .without(
            Control::FilesystemIsolation,
            "not implemented by the process backend: a chroot that a retained descriptor \
             can walk out of is not isolation, and pivot_root needs a prepared root; use \
             the microVM sandbox",
        );

    controls = match can_unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) {
        Ok(()) => controls.with(Control::NetworkIsolation),
        Err(e) => controls.without(
            Control::NetworkIsolation,
            format!("a network namespace could not be created: {e}"),
        ),
    };

    controls = match can_unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWPID | libc::CLONE_NEWIPC) {
        Ok(()) => controls.with(Control::ProcessIsolation),
        Err(e) => controls.without(
            Control::ProcessIsolation,
            format!("a PID namespace could not be created: {e}"),
        ),
    };

    controls = match can_set_no_new_privs() {
        Ok(()) => controls.with(Control::NoNewPrivileges),
        Err(e) => controls.without(
            Control::NoNewPrivileges,
            format!("PR_SET_NO_NEW_PRIVS was refused: {e}"),
        ),
    };

    match CgroupScope::probe() {
        Ok(available) => {
            controls = if available.memory {
                controls.with(Control::Memory)
            } else {
                controls.without(
                    Control::Memory,
                    "the memory controller is not delegated to this cgroup; enable it in the \
                     parent's cgroup.subtree_control",
                )
            };
            controls = if available.pids {
                controls.with(Control::ProcessCount)
            } else {
                controls.without(
                    Control::ProcessCount,
                    "the pids controller is not delegated to this cgroup; enable it in the \
                     parent's cgroup.subtree_control",
                )
            };
        }
        Err(e) => {
            let reason = format!("no writable cgroup v2 hierarchy: {e}");
            controls = controls
                .without(Control::Memory, reason.clone())
                .without(Control::ProcessCount, reason);
        }
    }

    controls
}

/// Fork a child that tries `flags` and report whether it worked.
///
/// Testing in a child rather than in this process: `unshare` cannot be undone,
/// and a probe that changed the caller's namespaces would be a side effect
/// nobody asked for.
fn can_unshare(flags: libc::c_int) -> std::io::Result<()> {
    // SAFETY: the child does nothing but call unshare and _exit, neither of
    // which allocates or touches shared state.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            let rc = unsafe { libc::unshare(flags) };
            unsafe { libc::_exit(if rc == 0 { 0 } else { 1 }) };
        }
        _ => {
            let mut status = 0;
            // SAFETY: `pid` is our child and `status` is a valid out-pointer.
            unsafe { libc::waitpid(pid, &mut status, 0) };
            if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "unprivileged namespaces appear to be unavailable",
                ))
            }
        }
    }
}

/// Whether `PR_SET_NO_NEW_PRIVS` is accepted.
///
/// Setting it on this process would be permanent and inherited, so it is tried
/// in a child like everything else.
fn can_set_no_new_privs() -> std::io::Result<()> {
    // SAFETY: as in `can_unshare`.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
            unsafe { libc::_exit(if rc == 0 { 0 } else { 1 }) };
        }
        _ => {
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
            if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::other("prctl refused"))
            }
        }
    }
}

/// Which cgroup controllers this process can actually use.
struct Delegated {
    memory: bool,
    pids: bool,
}

/// A cgroup created for one sandboxed run, removed when the run ends.
struct CgroupScope {
    path: PathBuf,
}

impl CgroupScope {
    /// The cgroup v2 directory this process is in.
    ///
    /// `/proc/self/cgroup` on a unified hierarchy has exactly one line,
    /// `0::<path>`, and that path is relative to the cgroup2 mount.
    fn current() -> std::io::Result<PathBuf> {
        let content = std::fs::read_to_string("/proc/self/cgroup")?;
        let relative = content
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or_else(|| {
                std::io::Error::other(
                    "no unified (0::) entry in /proc/self/cgroup; this looks like cgroup v1",
                )
            })?
            .trim()
            .trim_start_matches('/')
            .to_string();

        let root = Path::new("/sys/fs/cgroup");
        if !root.exists() {
            return Err(std::io::Error::other("/sys/fs/cgroup is not mounted"));
        }
        Ok(root.join(relative))
    }

    /// Create a fresh child cgroup.
    fn create() -> std::io::Result<Self> {
        let seq = CGROUP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = Self::current()?.join(format!("hv2-sandbox-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// Find out which controllers a fresh cgroup can actually use.
    fn probe() -> std::io::Result<Delegated> {
        let scope = Self::create()?;
        let memory = scope.write("memory.max", "max").is_ok();
        let pids = scope.write("pids.max", "max").is_ok();
        Ok(Delegated { memory, pids })
    }

    fn write(&self, file: &str, value: &str) -> std::io::Result<()> {
        let mut handle = std::fs::OpenOptions::new()
            .write(true)
            .open(self.path.join(file))?;
        handle.write_all(value.as_bytes())
    }

    /// Path of the file a process writes to join this cgroup.
    fn procs_path(&self) -> PathBuf {
        self.path.join("cgroup.procs")
    }
}

impl Drop for CgroupScope {
    fn drop(&mut self) {
        // Only succeeds once every process in it has exited, which by this
        // point they have. A leftover directory is harmless but untidy, and
        // ignoring the error is right: there is nothing to do about it.
        let _ = std::fs::remove_dir(&self.path);
    }
}

/// Run `command` under `spec`.
pub(super) fn run(
    command: &SandboxCommand,
    spec: &SandboxSpec,
) -> Result<SandboxOutput, SandboxError> {
    if let FilesystemPolicy::Isolated { .. } = spec.filesystem {
        return Err(SandboxError::InvalidSpec(
            "the process backend does not isolate the filesystem; use the microVM sandbox"
                .to_string(),
        ));
    }

    // Set up the cgroup in the parent, where errors can still be reported
    // before anything has been started.
    let cgroup = if spec.memory_bytes.is_some() || spec.max_processes.is_some() {
        let scope = CgroupScope::create().map_err(|e| SandboxError::ConfinementFailed {
            control: Control::Memory,
            source: e,
        })?;
        if let Some(bytes) = spec.memory_bytes {
            scope.write("memory.max", &bytes.to_string()).map_err(|e| {
                SandboxError::ConfinementFailed {
                    control: Control::Memory,
                    source: e,
                }
            })?;
        }
        if let Some(max) = spec.max_processes {
            scope.write("pids.max", &max.to_string()).map_err(|e| {
                SandboxError::ConfinementFailed {
                    control: Control::ProcessCount,
                    source: e,
                }
            })?;
        }
        Some(scope)
    } else {
        None
    };

    // Everything the child needs, built here: the code between fork and exec
    // must not allocate.
    let procs_file = cgroup
        .as_ref()
        .map(|scope| path_to_cstring(&scope.procs_path()))
        .transpose()
        .map_err(|e| SandboxError::Runtime(format!("cgroup path is not usable: {e}")))?;

    let mut clone_flags = 0;
    if spec.network == NetworkPolicy::Denied {
        clone_flags |= libc::CLONE_NEWUSER | libc::CLONE_NEWNET;
    }
    if spec.isolate_processes {
        clone_flags |= libc::CLONE_NEWUSER | libc::CLONE_NEWPID | libc::CLONE_NEWIPC;
    }
    let new_pid_ns = clone_flags & libc::CLONE_NEWPID != 0;
    let new_user_ns = clone_flags & libc::CLONE_NEWUSER != 0;

    // SAFETY: getuid and getgid have no preconditions and cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let uid_map = format!("0 {uid} 1\n").into_bytes();
    let gid_map = format!("0 {gid} 1\n").into_bytes();

    let cpu_seconds = spec.cpu_time.map(|d| d.as_secs().max(1));
    let max_processes = spec.max_processes;
    let no_new_privs = spec.no_new_privileges;

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

    // SAFETY: the closure runs after fork and before exec. It allocates
    // nothing, takes no locks, and calls only async-signal-safe functions.
    unsafe {
        builder.pre_exec(move || {
            confine(
                procs_file.as_ref(),
                cpu_seconds,
                max_processes,
                no_new_privs,
                clone_flags,
                new_user_ns,
                new_pid_ns,
                &uid_map,
                &gid_map,
            )
        });
    }

    let child = builder.spawn().map_err(|e| SandboxError::Spawn {
        program: command.program.clone(),
        source: e,
    })?;
    let pid = child.id() as libc::pid_t;

    let output =
        driver::wait_with_deadline(child, command.stdin.as_deref(), spec.wall_clock, || {
            // SIGKILL the whole process group, not the one process: a workload
            // that spawned children would otherwise leave them running past its
            // own deadline. A PID namespace makes this exact anyway, since killing
            // its PID 1 takes the namespace with it.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
            }
        });

    drop(cgroup);
    output
}

/// Everything the child does between `fork` and `exec`.
///
/// Returns an `io::Error` to abort the spawn, which `std` reports to the
/// parent — so a confinement step that fails means no workload runs, rather
/// than one running unconfined.
#[allow(clippy::too_many_arguments)]
fn confine(
    procs_file: Option<&CString>,
    cpu_seconds: Option<u64>,
    max_processes: Option<u32>,
    no_new_privs: bool,
    clone_flags: libc::c_int,
    new_user_ns: bool,
    new_pid_ns: bool,
    uid_map: &[u8],
    gid_map: &[u8],
) -> std::io::Result<()> {
    // 1. Join the cgroup while still the original user. After CLONE_NEWUSER
    //    this file is no longer writable by us.
    if let Some(path) = procs_file {
        // "0" means "the process doing the writing", which saves formatting a
        // pid here where formatting would allocate.
        write_file(path, b"0")?;
    }

    // 2. Limits that need no privileges.
    if let Some(seconds) = cpu_seconds {
        set_rlimit(libc::RLIMIT_CPU, seconds)?;
    }
    if let Some(max) = max_processes {
        // Belt and braces alongside pids.max: RLIMIT_NPROC is per-user rather
        // than per-cgroup, so it is the weaker of the two and never the only
        // one relied on.
        set_rlimit(libc::RLIMIT_NPROC, u64::from(max))?;
    }
    if no_new_privs {
        // SAFETY: prctl with this option takes no pointers.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    // 3. One unshare for every namespace: the kernel creates the user
    //    namespace first and grants the capabilities the rest need.
    if clone_flags != 0 {
        // SAFETY: unshare takes only flags.
        if unsafe { libc::unshare(clone_flags) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    // 4. Map ourselves to root inside the new user namespace. Without this the
    //    process runs as the overflow uid and most things fail confusingly.
    if new_user_ns {
        // setgroups must be denied before gid_map may be written.
        let _ = write_path(c"/proc/self/setgroups", b"deny");
        write_path(c"/proc/self/uid_map", uid_map)?;
        write_path(c"/proc/self/gid_map", gid_map)?;
    }

    // 5. unshare(CLONE_NEWPID) puts the *next* child in the new namespace, not
    //    this process. Forking here is what makes the workload actually PID 1
    //    in it; without this the claim would be false.
    if new_pid_ns {
        // SAFETY: this child is single-threaded, so a second fork is safe.
        let pid = unsafe { libc::fork() };
        match pid {
            -1 => return Err(std::io::Error::last_os_error()),
            0 => { /* the workload continues to exec */ }
            _ => {
                // The intermediate process waits and mirrors the outcome, so
                // the parent's Child handle reports what the workload did.
                let mut status = 0;
                // SAFETY: `pid` is our child.
                unsafe { libc::waitpid(pid, &mut status, 0) };
                if libc::WIFSIGNALED(status) {
                    let signal = libc::WTERMSIG(status);
                    // Die the same way, so a killed workload is not reported
                    // as one that exited.
                    unsafe {
                        libc::signal(signal, libc::SIG_DFL);
                        libc::raise(signal);
                    }
                }
                unsafe { libc::_exit(libc::WEXITSTATUS(status)) };
            }
        }
    }

    // Become a process group leader so the deadline can kill the whole group.
    // SAFETY: setpgid on self with group 0 has no preconditions.
    unsafe { libc::setpgid(0, 0) };

    Ok(())
}

/// `setrlimit`, both soft and hard.
fn set_rlimit(resource: libc::__rlimit_resource_t, value: u64) -> std::io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: `limit` is fully initialised and outlives the call.
    if unsafe { libc::setrlimit(resource, &limit) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Write `data` to `path`, using only async-signal-safe calls.
fn write_file(path: &CString, data: &[u8]) -> std::io::Result<()> {
    // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` is open for writing and `data` outlives the call.
    let written = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
    // SAFETY: `fd` is open and closed exactly once.
    unsafe { libc::close(fd) };
    if written < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// [`write_file`] for a path known at compile time.
fn write_path(path: &std::ffi::CStr, data: &[u8]) -> std::io::Result<()> {
    // SAFETY: as in `write_file`.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let written = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
    unsafe { libc::close(fd) };
    if written < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Turn a path into a NUL-terminated string, in the parent where allocating is
/// allowed.
fn path_to_cstring(path: &Path) -> std::io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::other("path contains an interior NUL"))
}
