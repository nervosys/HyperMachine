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
//! | [`Control::FilesystemIsolation`] | `CLONE_NEWNS` plus `pivot_root` onto the root the spec names |
//! | [`Control::ProcessIsolation`] | `CLONE_NEWPID` and `CLONE_NEWIPC`: the workload is PID 1 in its own namespace and cannot see or signal anything outside |
//! | [`Control::NoNewPrivileges`] | `prctl(PR_SET_NO_NEW_PRIVS)` |
//!
//! # Why `pivot_root` and not `chroot`
//!
//! `chroot` moves the root a path walk starts from and leaves the old root
//! reachable: a process holding a directory descriptor from before the call
//! `fchdir`s to it and walks out, and a process that can `chroot` again does the
//! classic double-chroot escape. That is isolation as a claim and not as a
//! boundary, which is the exact shape of thing this crate exists to stop
//! repeating.
//!
//! `pivot_root` moves the root *mount*. After
//! `pivot_root(".", ".")` and `umount2(".", MNT_DETACH)` — the two-step form
//! that needs no separate `put_old` directory — the old root is not mounted
//! anywhere in this mount namespace, so there is no path to it and no mount for
//! a stale descriptor to resolve against. The remaining way out would be a
//! descriptor the child inherited; every descriptor this crate and `std` open is
//! `O_CLOEXEC`, so `exec` closes all of them and the workload starts holding
//! nothing but its three standard streams.
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
//! 6. **`pivot_root`**, if a root was named. After this the host's filesystem
//!    has no name, so it has to come after everything that reads a host path —
//!    `/proc/self/uid_map` above, and the cgroup file above that.
//! 7. **Mount `/proc` and `/sys`** last, because they must land inside the new
//!    root rather than the old one.
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

/// Distinguishes the scratch directories this process creates — cgroups for a
/// run, and the throwaway roots the filesystem probe pivots into — from each
/// other and from another process's.
static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

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
        .with(Control::WallClock);

    controls = match can_isolate_filesystem() {
        Ok(()) => controls.with(Control::FilesystemIsolation),
        Err(e) => controls.without(
            Control::FilesystemIsolation,
            format!("the filesystem could not be isolated: {e}"),
        ),
    };

    controls = match can_isolate_network() {
        Ok(()) => controls.with(Control::NetworkIsolation),
        Err(e) => controls.without(
            Control::NetworkIsolation,
            format!("the network could not be isolated: {e}"),
        ),
    };

    controls = match can_isolate_processes() {
        Ok(()) => controls.with(Control::ProcessIsolation),
        Err(e) => controls.without(
            Control::ProcessIsolation,
            format!("processes could not be isolated: {e}"),
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

/// Whether the full process-isolation sequence works here.
///
/// A PID namespace alone is not what [`Control::ProcessIsolation`] claims.
/// Running this on a real kernel showed why: the workload was correctly PID 1
/// in its own namespace and `/proc` still listed 48 host processes, because
/// `/proc` was mounted in the *host's* PID namespace and inherited. "Cannot
/// signal" was true and "cannot see" was not.
///
/// So the probe rehearses the whole thing — namespaces, the second fork that
/// actually enters the PID namespace, and the `/proc` remount — and reports
/// the control only if all of it worked.
fn can_isolate_processes() -> std::io::Result<()> {
    // SAFETY: the child only makes syscalls and _exit; it never allocates or
    // touches state shared with this process.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            let flags =
                libc::CLONE_NEWUSER | libc::CLONE_NEWPID | libc::CLONE_NEWIPC | libc::CLONE_NEWNS;
            if unsafe { libc::unshare(flags) } != 0 {
                unsafe { libc::_exit(1) };
            }
            // The PID namespace only takes effect for the next child.
            let inner = unsafe { libc::fork() };
            if inner < 0 {
                unsafe { libc::_exit(1) };
            }
            if inner > 0 {
                let mut status = 0;
                unsafe { libc::waitpid(inner, &mut status, 0) };
                let code = if libc::WIFEXITED(status) {
                    libc::WEXITSTATUS(status)
                } else {
                    1
                };
                unsafe { libc::_exit(code) };
            }
            let ok = remount_namespaced_filesystems(true, false).is_ok();
            unsafe { libc::_exit(if ok { 0 } else { 1 }) };
        }
        _ => {
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
            if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "a PID namespace with its own /proc could not be created",
                ))
            }
        }
    }
}

/// Give the new namespaces the pseudo-filesystems that describe them.
///
/// `/proc` and `/sys` are not ordinary directories: each is a view of the
/// namespace it was *mounted in*. Inherit them and a workload reads the host
/// process table and the host network interfaces while being genuinely unable
/// to signal or reach either. Both halves of this were found by running on a
/// real kernel: `/proc` listed 48 host processes and `/sys/class/net` listed
/// the host interfaces, while `$$` was 1 and netlink showed only loopback.
/// Half an isolation claim is the kind this crate refuses to make.
fn remount_namespaced_filesystems(new_pid_ns: bool, new_net_ns: bool) -> std::io::Result<()> {
    if !new_pid_ns && !new_net_ns {
        return Ok(());
    }
    make_mounts_private()?;
    mount_namespaced_filesystems(new_pid_ns, new_net_ns)
}

/// Detach this mount namespace's propagation from the host's.
///
/// Without it every mount made below would be visible to the host mount
/// namespace, and `pivot_root` refuses to run at all while the root is shared.
fn make_mounts_private() -> std::io::Result<()> {
    // SAFETY: every pointer is a NUL-terminated literal that outlives the call.
    if unsafe {
        libc::mount(
            c"none".as_ptr(),
            c"/".as_ptr(),
            std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Mount `/proc` and `/sys` so they describe *these* namespaces.
///
/// Split from [`make_mounts_private`] because when a root is pivoted into,
/// these have to happen after the pivot — a `/proc` mounted before it would end
/// up in the root that is about to be thrown away — while making the mounts
/// private has to happen before.
fn mount_namespaced_filesystems(new_pid_ns: bool, new_net_ns: bool) -> std::io::Result<()> {
    if new_pid_ns {
        // SAFETY: as above.
        if unsafe {
            libc::mount(
                c"proc".as_ptr(),
                c"/proc".as_ptr(),
                c"proc".as_ptr(),
                0,
                std::ptr::null(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }

    if new_net_ns {
        // sysfs is bound to the network namespace it is mounted in, which is
        // what makes /sys/class/net show the host interfaces otherwise.
        // SAFETY: as above.
        if unsafe {
            libc::mount(
                c"sysfs".as_ptr(),
                c"/sys".as_ptr(),
                c"sysfs".as_ptr(),
                0,
                std::ptr::null(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }

    Ok(())
}

/// Whether the full network-isolation sequence works here.
///
/// Rehearses the sysfs remount as well as the namespace, for the same reason
/// [`can_isolate_processes`] rehearses `/proc`.
fn can_isolate_network() -> std::io::Result<()> {
    // SAFETY: the child only makes syscalls and _exit.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            let flags = libc::CLONE_NEWUSER | libc::CLONE_NEWNET | libc::CLONE_NEWNS;
            if unsafe { libc::unshare(flags) } != 0 {
                unsafe { libc::_exit(1) };
            }
            let ok = remount_namespaced_filesystems(false, true).is_ok();
            unsafe { libc::_exit(if ok { 0 } else { 1 }) };
        }
        _ => {
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
            if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "a network namespace with its own sysfs could not be created",
                ))
            }
        }
    }
}

/// One host path made visible read-only inside the new root.
///
/// Both paths are host paths, resolved and NUL-terminated in the parent,
/// because the bind happens before the pivot — at which point the host paths
/// still mean something — and the child may not allocate.
struct BindMount {
    source: CString,
    target: CString,
}

/// Everything the child needs to build and enter the new root.
struct FilesystemPlan {
    /// The directory that becomes `/`, as a host path.
    new_root: CString,
    /// Read-only binds to place inside it, in the order the caller gave.
    binds: Vec<BindMount>,
    /// Where to `chdir` once inside, interpreted in the *new* root.
    ///
    /// The caller's working directory cannot be handed to `Command::current_dir`
    /// when a root is pivoted into: `std` applies it before the `pre_exec`
    /// closure runs, so it would resolve against the host's filesystem and then
    /// be thrown away by the pivot. Silently landing the workload somewhere
    /// other than where it asked is the class of quiet substitution this crate
    /// refuses, so it is applied here instead, and a directory that does not
    /// exist inside the root fails the spawn.
    working_dir: Option<CString>,
}

/// The kernel's `struct mount_attr`, which `libc` does not declare.
///
/// Field order and width are the kernel's ABI; the size is passed to the
/// syscall so a future kernel that grew the struct still gets an unambiguous
/// answer about which version this is.
#[repr(C)]
#[derive(Default)]
struct MountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

/// `MOUNT_ATTR_RDONLY`.
const MOUNT_ATTR_RDONLY: u64 = 0x0000_0001;
/// `MOUNT_ATTR_NOSUID`.
const MOUNT_ATTR_NOSUID: u64 = 0x0000_0002;
/// `AT_RECURSIVE`: apply to every mount in the subtree, not just its top.
const AT_RECURSIVE: libc::c_long = 0x8000;

/// Make `target` and everything mounted under it read-only, and `nosuid`.
///
/// `nosuid` because a read-only mount of host paths is exactly where a setuid
/// binary would be, and a workload that was told it cannot gain privileges
/// should not be handed one — it costs nothing here and this is the only place
/// the mount flags are chosen.
fn set_subtree_read_only(target: &CString) -> std::io::Result<()> {
    let attr = MountAttr {
        attr_set: MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID,
        ..MountAttr::default()
    };
    // The integer arguments are widened here rather than left to the variadic
    // call: `syscall` reads each as a `long`, and a 32-bit argument leaves the
    // top half of the register undefined.
    // SAFETY: `target` is NUL-terminated and outlives the call, and `attr` is
    // fully initialised with its size passed explicitly.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_mount_setattr,
            libc::c_long::from(libc::AT_FDCWD),
            target.as_ptr(),
            AT_RECURSIVE,
            std::ptr::addr_of!(attr),
            std::mem::size_of::<MountAttr>() as libc::c_long,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Build the new root and enter it, leaving the old one unmounted.
///
/// Async-signal-safe: every path is a `CString` built in the parent, and this
/// makes only syscalls.
fn pivot_into(plan: &FilesystemPlan) -> std::io::Result<()> {
    // `pivot_root` requires the new root to be a mount point. Binding the
    // directory onto itself makes it one without the caller having to have
    // mounted anything.
    // SAFETY: every pointer is a NUL-terminated string owned by `plan`.
    if unsafe {
        libc::mount(
            plan.new_root.as_ptr(),
            plan.new_root.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REC,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }

    for bind in &plan.binds {
        // Recursive, and not by preference. Inside a user namespace the kernel
        // refuses to create a mount that would hide a mount the namespace's
        // creator cannot unmount, so a plain `MS_BIND` of a directory with
        // anything mounted underneath it fails outright with `EINVAL` — which
        // is how this was found: `/usr` on the kernel this was written against
        // has three submounts under `/usr/lib`.
        // SAFETY: as above.
        if unsafe {
            libc::mount(
                bind.source.as_ptr(),
                bind.target.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND | libc::MS_REC,
                std::ptr::null(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        // Now make the whole subtree read-only. `mount(MS_REMOUNT | MS_RDONLY)`
        // will not do: it changes the top mount only, so on the tree above it
        // would leave `/usr/lib/wsl/lib` writable inside a mount the caller was
        // told is read-only — the precise shape of half-true claim this crate
        // exists to stop making. `mount_setattr` with `AT_RECURSIVE` covers
        // every submount, and a kernel too old to have it (pre-5.12) is
        // reported as unable to isolate the filesystem rather than quietly
        // given the weaker version.
        set_subtree_read_only(&bind.target)?;
    }

    // SAFETY: `new_root` is NUL-terminated and owned by `plan`.
    if unsafe { libc::chdir(plan.new_root.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    // pivot_root(".", ".") is the form that needs no separate put_old
    // directory: the old root ends up stacked over the new one at "/", and the
    // umount below removes it. The working directory is what keeps "." naming
    // the old root across the call.
    // SAFETY: raw syscall with two NUL-terminated literals.
    if unsafe { libc::syscall(libc::SYS_pivot_root, c".".as_ptr(), c".".as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    // Until this succeeds the host root is still mounted and still reachable,
    // so a failure here is a failure of the whole control.
    // SAFETY: as above.
    if unsafe { libc::umount2(c".".as_ptr(), libc::MNT_DETACH) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    let destination = plan
        .working_dir
        .as_ref()
        .map_or(c"/".as_ptr(), |dir| dir.as_ptr());
    // SAFETY: as above.
    if unsafe { libc::chdir(destination) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

/// Whether this host can actually pivot into a root and lose the old one.
///
/// Rehearses the whole sequence in a throwaway child and then asks the two
/// questions that decide whether the claim is true: is a file inside the new
/// root visible, and is a file outside it gone. A probe that stopped at
/// "`pivot_root` returned 0" would report the control on a host where the old
/// root was still reachable, which is the failure mode this crate keeps
/// finding.
fn can_isolate_filesystem() -> std::io::Result<()> {
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let base =
        std::env::temp_dir().join(format!("hv2-sandbox-fsprobe-{}-{seq}", std::process::id()));
    let root = base.join("root");
    // A file inside the root the child should still see, and one outside it,
    // named by its absolute host path, that must have become unreachable.
    std::fs::create_dir_all(&root)?;
    std::fs::create_dir_all(root.join("ro"))?;
    let outside = base.join("outside");
    let read_only = base.join("read-only");
    std::fs::create_dir_all(&read_only)?;
    std::fs::write(&outside, b"host\n")?;
    std::fs::write(root.join("inside"), b"sandbox\n")?;

    let result = run_filesystem_probe(&root, &outside, &read_only);
    let _ = std::fs::remove_dir_all(&base);
    result
}

/// The forking half of [`can_isolate_filesystem`], split out so the temporary
/// directory is cleaned up on every path.
fn run_filesystem_probe(root: &Path, outside: &Path, read_only: &Path) -> std::io::Result<()> {
    let plan = FilesystemPlan {
        new_root: path_to_cstring(root)?,
        // The read-only bind is rehearsed too, because `FilesystemPolicy`
        // offers it and a host that can pivot but cannot make a subtree
        // read-only would enforce only half of what the control promises.
        binds: vec![BindMount {
            source: path_to_cstring(read_only)?,
            target: path_to_cstring(&root.join("ro"))?,
        }],
        working_dir: None,
    };
    let outside = path_to_cstring(outside)?;

    // SAFETY: getuid and getgid have no preconditions and cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let uid_map = format!("0 {uid} 1\n").into_bytes();
    let gid_map = format!("0 {gid} 1\n").into_bytes();

    // SAFETY: the child only makes syscalls and _exit.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            let code = filesystem_probe_child(&plan, &outside, &uid_map, &gid_map);
            unsafe { libc::_exit(code) };
        }
        _ => {
            let mut status = 0;
            // SAFETY: `pid` is our child.
            unsafe { libc::waitpid(pid, &mut status, 0) };
            if !libc::WIFEXITED(status) {
                return Err(std::io::Error::other("the probe child died unexpectedly"));
            }
            match libc::WEXITSTATUS(status) {
                0 => Ok(()),
                1 => Err(std::io::Error::other(
                    "unshare(CLONE_NEWUSER|CLONE_NEWNS) was refused; this kernel does not \
                     permit unprivileged user namespaces (check \
                     /proc/sys/kernel/unprivileged_userns_clone and \
                     /proc/sys/user/max_user_namespaces)",
                )),
                2 => Err(std::io::Error::other(
                    "the user namespace id maps could not be written",
                )),
                3 => Err(std::io::Error::other(
                    "the mount namespace could not be detached from the host's propagation",
                )),
                4 => Err(std::io::Error::other(
                    "a read-only bind mount, or pivot_root onto the prepared directory, \
                     was refused (a kernel older than 5.12 has no mount_setattr and cannot \
                     make a mounted subtree read-only)",
                )),
                5 => Err(std::io::Error::other(
                    "a file inside the new root was not visible after pivot_root",
                )),
                6 => Err(std::io::Error::other(
                    "a file outside the new root was still reachable after pivot_root, \
                     so the old root had not gone away",
                )),
                7 => Err(std::io::Error::other(
                    "a mount asked for read-only accepted a write",
                )),
                other => Err(std::io::Error::other(format!(
                    "the probe child exited {other}"
                ))),
            }
        }
    }
}

/// The probe's child. Returns the exit code its parent decodes above.
fn filesystem_probe_child(
    plan: &FilesystemPlan,
    outside: &CString,
    uid_map: &[u8],
    gid_map: &[u8],
) -> libc::c_int {
    // SAFETY: unshare takes only flags.
    if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) } != 0 {
        return 1;
    }
    let _ = write_path(c"/proc/self/setgroups", b"deny");
    if write_path(c"/proc/self/uid_map", uid_map).is_err()
        || write_path(c"/proc/self/gid_map", gid_map).is_err()
    {
        return 2;
    }
    if make_mounts_private().is_err() {
        return 3;
    }
    if pivot_into(plan).is_err() {
        return 4;
    }
    // SAFETY: both pointers are NUL-terminated and outlive the call.
    if unsafe { libc::access(c"/inside".as_ptr(), libc::F_OK) } != 0 {
        return 5;
    }
    // SAFETY: as above.
    if unsafe { libc::access(outside.as_ptr(), libc::F_OK) } == 0 {
        return 6;
    }
    // SAFETY: as above; the descriptor is closed on every path that opens one.
    let fd = unsafe { libc::open(c"/ro/probe".as_ptr(), libc::O_WRONLY | libc::O_CREAT, 0o600) };
    if fd >= 0 {
        // SAFETY: `fd` is open and closed exactly once.
        unsafe { libc::close(fd) };
        return 7;
    }
    0
}

/// Turn a [`FilesystemPolicy::Isolated`] into something the child can apply.
///
/// Everything that can be checked or created from the host side happens here,
/// where a failure is still a refusal to run rather than a half-confined
/// workload: the root must exist, every read-only source must exist, and the
/// mount points inside the root are created, because a bind mount onto a path
/// that is not there fails and `mkdir` after the pivot would be too late.
fn plan_filesystem(
    root: &Path,
    read_only: &[PathBuf],
    working_dir: Option<&Path>,
    need_proc: bool,
    need_sys: bool,
) -> Result<FilesystemPlan, SandboxError> {
    let invalid = |message: String| SandboxError::InvalidSpec(message);

    if !root.is_absolute() {
        return Err(invalid(format!(
            "the sandbox root {} must be an absolute path",
            root.display()
        )));
    }
    if !root.is_dir() {
        return Err(invalid(format!(
            "the sandbox root {} does not exist or is not a directory; this backend mounts \
             a root, it does not build one",
            root.display()
        )));
    }

    let mut binds = Vec::with_capacity(read_only.len());
    for source in read_only {
        if !source.is_absolute() {
            return Err(invalid(format!(
                "the read-only path {} must be absolute",
                source.display()
            )));
        }
        let metadata = std::fs::metadata(source).map_err(|e| {
            invalid(format!(
                "the read-only path {} cannot be mounted: {e}",
                source.display()
            ))
        })?;

        // Mirrored at the same path inside the root, so a workload finds
        // /usr/bin/sh where it expects it and the caller does not have to
        // describe a mapping it would then have to keep in step.
        let relative = source.strip_prefix("/").expect("checked absolute above");
        let target = root.join(relative);
        let created = if metadata.is_dir() {
            std::fs::create_dir_all(&target)
        } else {
            target
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(false)
                        .open(&target)
                        .map(drop)
                })
        };
        created.map_err(|e| {
            invalid(format!(
                "the mount point {} could not be created: {e}",
                target.display()
            ))
        })?;

        binds.push(BindMount {
            source: cstring_or_invalid(source)?,
            target: cstring_or_invalid(&target)?,
        });
    }

    // /proc and /sys are mounted after the pivot, so their mount points have to
    // exist inside the root by then.
    for (needed, name) in [(need_proc, "proc"), (need_sys, "sys")] {
        if needed {
            let path = root.join(name);
            std::fs::create_dir_all(&path).map_err(|e| {
                invalid(format!(
                    "the mount point {} could not be created: {e}",
                    path.display()
                ))
            })?;
        }
    }

    Ok(FilesystemPlan {
        new_root: cstring_or_invalid(root)?,
        binds,
        working_dir: working_dir.map(cstring_or_invalid).transpose()?,
    })
}

/// [`path_to_cstring`], reporting a bad path as a spec the caller has to fix.
fn cstring_or_invalid(path: &Path) -> Result<CString, SandboxError> {
    path_to_cstring(path).map_err(|e| {
        SandboxError::InvalidSpec(format!("the path {} is not usable: {e}", path.display()))
    })
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
        let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
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
    if matches!(spec.filesystem, FilesystemPolicy::Isolated { .. }) {
        // A mount namespace of our own, and the user namespace that grants the
        // CAP_SYS_ADMIN inside it that mount and pivot_root require.
        clone_flags |= libc::CLONE_NEWUSER | libc::CLONE_NEWNS;
    }
    if spec.network == NetworkPolicy::Denied {
        // CLONE_NEWNS so the workload can be given a sysfs that belongs to its
        // own network namespace rather than the host one.
        clone_flags |= libc::CLONE_NEWUSER | libc::CLONE_NEWNET | libc::CLONE_NEWNS;
    }
    if spec.isolate_processes {
        // CLONE_NEWNS comes along so the workload can be given its own /proc.
        // Without it the process table of the whole host stays readable through
        // a /proc that belongs to the host's PID namespace.
        clone_flags |=
            libc::CLONE_NEWUSER | libc::CLONE_NEWPID | libc::CLONE_NEWIPC | libc::CLONE_NEWNS;
    }
    let new_pid_ns = clone_flags & libc::CLONE_NEWPID != 0;
    let new_net_ns = clone_flags & libc::CLONE_NEWNET != 0;
    let new_user_ns = clone_flags & libc::CLONE_NEWUSER != 0;

    // SAFETY: getuid and getgid have no preconditions and cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let uid_map = format!("0 {uid} 1\n").into_bytes();
    let gid_map = format!("0 {gid} 1\n").into_bytes();

    let filesystem = match &spec.filesystem {
        FilesystemPolicy::Host => None,
        FilesystemPolicy::Isolated { root, read_only } => Some(plan_filesystem(
            root,
            read_only,
            command.working_dir.as_deref(),
            new_pid_ns,
            new_net_ns,
        )?),
    };

    let confinement = Confinement {
        procs_file,
        cpu_seconds: spec.cpu_time.map(|d| d.as_secs().max(1)),
        max_processes: spec.max_processes,
        no_new_privs: spec.no_new_privileges,
        clone_flags,
        new_user_ns,
        new_pid_ns,
        new_net_ns,
        uid_map,
        gid_map,
        filesystem,
    };

    let mut builder = Command::new(&command.program);
    builder
        .args(&command.args)
        .env_clear()
        .envs(&command.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &command.working_dir {
        // Only when the workload keeps the host's filesystem. With a root of
        // its own the directory is entered after the pivot instead, because
        // `std` applies this one before the `pre_exec` closure runs and it
        // would name a path that is about to stop existing.
        if confinement.filesystem.is_none() {
            builder.current_dir(dir);
        }
    }

    // SAFETY: the closure runs after fork and before exec. It allocates
    // nothing, takes no locks, and calls only async-signal-safe functions.
    unsafe {
        builder.pre_exec(move || confine(&confinement));
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

/// Everything the child needs, prepared where allocating is still allowed.
///
/// One struct rather than a dozen arguments because it is also the list of
/// things that must be built *before* the fork: the child between `fork` and
/// `exec` may not allocate, so a `String` it needs is a bug that only shows up
/// as a deadlock under a multi-threaded parent.
struct Confinement {
    procs_file: Option<CString>,
    cpu_seconds: Option<u64>,
    max_processes: Option<u32>,
    no_new_privs: bool,
    clone_flags: libc::c_int,
    new_user_ns: bool,
    new_pid_ns: bool,
    new_net_ns: bool,
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
    filesystem: Option<FilesystemPlan>,
}

/// Everything the child does between `fork` and `exec`.
///
/// Returns an `io::Error` to abort the spawn, which `std` reports to the
/// parent — so a confinement step that fails means no workload runs, rather
/// than one running unconfined.
fn confine(plan: &Confinement) -> std::io::Result<()> {
    // 1. Join the cgroup while still the original user. After CLONE_NEWUSER
    //    this file is no longer writable by us.
    if let Some(path) = plan.procs_file.as_ref() {
        // "0" means "the process doing the writing", which saves formatting a
        // pid here where formatting would allocate.
        write_file(path, b"0")?;
    }

    // 2. Limits that need no privileges.
    if let Some(seconds) = plan.cpu_seconds {
        set_rlimit(libc::RLIMIT_CPU, seconds)?;
    }
    if let Some(max) = plan.max_processes {
        // Belt and braces alongside pids.max: RLIMIT_NPROC is per-user rather
        // than per-cgroup, so it is the weaker of the two and never the only
        // one relied on.
        set_rlimit(libc::RLIMIT_NPROC, u64::from(max))?;
    }
    if plan.no_new_privs {
        // SAFETY: prctl with this option takes no pointers.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    // 3. One unshare for every namespace: the kernel creates the user
    //    namespace first and grants the capabilities the rest need.
    if plan.clone_flags != 0 {
        // SAFETY: unshare takes only flags.
        if unsafe { libc::unshare(plan.clone_flags) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    // 4. Map ourselves to root inside the new user namespace. Without this the
    //    process runs as the overflow uid and most things fail confusingly.
    if plan.new_user_ns {
        // setgroups must be denied before gid_map may be written.
        let _ = write_path(c"/proc/self/setgroups", b"deny");
        write_path(c"/proc/self/uid_map", plan.uid_map.as_slice())?;
        write_path(c"/proc/self/gid_map", plan.gid_map.as_slice())?;
    }

    // 5. unshare(CLONE_NEWPID) puts the *next* child in the new namespace, not
    //    this process. Forking here is what makes the workload actually PID 1
    //    in it; without this the claim would be false.
    if plan.new_pid_ns {
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

    // 6. Detach from the host's mount propagation, then take the new root if
    //    one was named. After the pivot no host path means anything, which is
    //    why the cgroup file and the id maps above had to come first.
    if plan.clone_flags & libc::CLONE_NEWNS != 0 {
        make_mounts_private()?;
    }
    if let Some(filesystem) = plan.filesystem.as_ref() {
        pivot_into(filesystem)?;
    }

    // 7. Now that this process really is in the new PID namespace — and inside
    //    the new root — give it a /proc and /sys that reflect them. Doing this
    //    before the fork would mount a /proc belonging to the old namespace,
    //    which is the state that made the host's whole process table readable
    //    from inside; doing it before the pivot would put both in the root
    //    that is about to be discarded.
    mount_namespaced_filesystems(plan.new_pid_ns, plan.new_net_ns)?;

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
