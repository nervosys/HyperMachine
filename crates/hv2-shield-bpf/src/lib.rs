//! A small BPF LSM engine: kernel operations a shield can refuse, live.
//!
//! A shield rule says "refuse this kernel operation for the processes in this
//! cgroup" (or "only count it"). The Linux Security Module hooks are where
//! the kernel asks permission before it does one of those operations, and a
//! BPF program attached to a hook (`BPF_PROG_TYPE_LSM`) answers for it: 0
//! lets it go on, `-errno` refuses it. That is an enforcement point that
//! seccomp does not have: the question is asked after the kernel has resolved
//! what is being asked for, so a rule cannot be dodged by a different system
//! call, and it covers operations that are not calls at all (a module
//! request, a kernel file read).
//!
//! This crate loads the programs and manages the rules. It needs no compiler
//! for BPF: the one program (see `insn::lsm_program`) is assembled by hand, one
//! copy per hook, the hook's index in [`HOOKS`] an immediate in it. Every
//! hook it can name returns `int` and none needs its arguments read, so the
//! program never touches its context.
//!
//! What a program does when its hook fires:
//!
//! 1. A process whose tgid was passed to [`HookEngine::exempt`] is never
//!    judged.
//! 2. The rule for the current cgroup and this hook is looked up, then the
//!    rule for [`Scope::Everywhere`]. With neither, the operation goes on.
//! 3. An enforcing rule counts a refusal and returns `-errno`; a monitoring
//!    rule counts a match and lets the operation go on.
//!
//! [`Scope::Cgroup`] is an exact match on the cgroup the process is in when
//! the hook fires. It does not cover descendant cgroups: a process that moves
//! itself to (or is started in) a child cgroup is out of scope, so a caller
//! that wants a subtree must name every cgroup in it, and keep that list
//! current as cgroups are made.
//!
//! The kernel must have `bpf` in its active LSM list (the `lsm=` boot
//! option), BTF for itself in `/sys/kernel/btf/vmlinux`, and the caller needs
//! `CAP_BPF` and `CAP_SYS_ADMIN` (or `CAP_PERFMON` and `CAP_MAC_ADMIN`
//! depending on the kernel; root has them). [`probe`] says whether a host can
//! do it, by doing it. Dropping a [`HookEngine`] closes every link and so
//! detaches everything; nothing is pinned, and nothing outlives the process.

/// LSM hooks a shield may name: all return int, none needs its arguments read.
pub const HOOKS: &[&str] = &[
    "bpf",
    "perf_event_open",
    "ptrace_access_check",
    "ptrace_traceme",
    "socket_create",
    "socket_connect",
    "socket_bind",
    "socket_listen",
    "socket_accept",
    "sb_mount",
    "move_mount",
    "sb_pivotroot",
    "kernel_module_request",
    "kernel_load_data",
    "kernel_read_file",
    "userns_create",
    "uring_override_creds",
    "uring_sqpoll",
    "uring_cmd",
    "key_alloc",
    "settime",
    "syslog",
    "quotactl",
];

/// The names in [`HOOKS`].
pub fn hooks() -> impl Iterator<Item = &'static str> {
    HOOKS.iter().copied()
}

/// Which processes a rule judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    /// Processes in exactly this cgroup (v2), by id (see [`cgroup_id`]).
    /// Descendant cgroups are not included. `Cgroup(0)` is not valid.
    Cgroup(u64),
    /// Every process on the host (except the exempt ones). A rule of this
    /// scope reaches whatever else is running; use it for hooks that nothing
    /// legitimate on the host calls.
    Everywhere,
}

/// One rule: what to do at `hook` for processes in `scope`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRule {
    /// A name in [`HOOKS`].
    pub hook: String,
    /// Whom it judges.
    pub scope: Scope,
    /// The error the refused operation fails with, 1..=4095.
    pub errno: i32,
    /// Refuse (true), or only count (false: monitor mode).
    pub enforce: bool,
}

/// What a `(hook, scope)` has done so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCount {
    /// The hook's name.
    pub hook: String,
    /// The scope the rule has.
    pub scope: Scope,
    /// Operations a monitoring rule matched (and let go on).
    pub monitored: u64,
    /// Operations an enforcing rule refused.
    pub refused: u64,
}

mod btf;
mod insn;
mod rules;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod engine;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use engine::{available_hooks, probe, HookEngine};

#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
mod unsupported;
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub use unsupported::{available_hooks, probe, HookEngine};

/// A cgroup v2 directory's id, what `bpf_get_current_cgroup_id` returns for
/// a process in it: the directory's inode number.
#[cfg(unix)]
pub fn cgroup_id(path: &std::path::Path) -> std::io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(path)?.ino())
}

/// A cgroup v2 directory's id. Only Linux has cgroups.
#[cfg(not(unix))]
pub fn cgroup_id(_path: &std::path::Path) -> std::io::Result<u64> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "cgroups need Linux",
    ))
}
