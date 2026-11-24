//! [`Control::SyscallShield`]: system calls trapped to the host, so rules that
//! refuse them can be added to and taken from a workload that is running.
//!
//! A seccomp filter cannot be changed once it is installed, and only the
//! process itself can install one. So the filter is installed before `exec`,
//! and what it does is fixed and small: the calls named when the sandbox
//! started return `SECCOMP_RET_USER_NOTIF`, and every other call is allowed
//! by the kernel without leaving it. A trapped call waits while a supervisor
//! thread in this process asks the caller's [`SyscallGate`] what to do, and
//! answers: run it (`SECCOMP_USER_NOTIF_FLAG_CONTINUE`), fail it with an
//! errno, or kill the process. The gate is the caller's, and it can change its
//! answers at any time; that is what makes a shield live.
//!
//! What is tested is the call's number and its six arguments as registers.
//! Memory an argument points to is never read: the workload could change it
//! between the read and the call, so an answer based on it could be passed
//! and then not hold. A rule about a path is not something this enforces.
//!
//! The filter also kills a process that makes a call under another ABI than
//! the native one (32-bit or x32 on x86-64), whose numbers would otherwise
//! slip past the list. A sandbox with syscall shields runs native code only.
//!
//! The listener is passed to the parent over a socket pair made before the
//! fork; the supervisor thread is started before the spawn, because `exec`
//! itself may be a trapped call and `spawn` does not return until it is done.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::{GateVerdict, SyscallGate, TrappedCall};

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7;

/// The calls a shield may name: rare enough to trap without slowing a
/// workload down, and each one a way in that advisories have named. Calls a
/// workload makes all the time (`read`, `openat`, `mmap`) are not here: every
/// one would wait on the supervisor.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) const TRAPPABLE: &[(&str, libc::c_long)] = &[
    ("add_key", libc::SYS_add_key),
    ("bind", libc::SYS_bind),
    ("bpf", libc::SYS_bpf),
    ("capset", libc::SYS_capset),
    ("chroot", libc::SYS_chroot),
    ("clock_settime", libc::SYS_clock_settime),
    ("connect", libc::SYS_connect),
    ("delete_module", libc::SYS_delete_module),
    ("execve", libc::SYS_execve),
    ("execveat", libc::SYS_execveat),
    ("fanotify_init", libc::SYS_fanotify_init),
    ("finit_module", libc::SYS_finit_module),
    ("fsconfig", libc::SYS_fsconfig),
    ("fsmount", libc::SYS_fsmount),
    ("fsopen", libc::SYS_fsopen),
    ("init_module", libc::SYS_init_module),
    ("io_uring_enter", libc::SYS_io_uring_enter),
    ("io_uring_register", libc::SYS_io_uring_register),
    ("io_uring_setup", libc::SYS_io_uring_setup),
    ("kexec_load", libc::SYS_kexec_load),
    ("keyctl", libc::SYS_keyctl),
    ("listen", libc::SYS_listen),
    ("memfd_create", libc::SYS_memfd_create),
    ("mount", libc::SYS_mount),
    ("move_mount", libc::SYS_move_mount),
    ("name_to_handle_at", libc::SYS_name_to_handle_at),
    ("open_by_handle_at", libc::SYS_open_by_handle_at),
    ("open_tree", libc::SYS_open_tree),
    ("perf_event_open", libc::SYS_perf_event_open),
    ("personality", libc::SYS_personality),
    ("pidfd_getfd", libc::SYS_pidfd_getfd),
    ("pidfd_open", libc::SYS_pidfd_open),
    ("pivot_root", libc::SYS_pivot_root),
    ("process_vm_readv", libc::SYS_process_vm_readv),
    ("process_vm_writev", libc::SYS_process_vm_writev),
    ("ptrace", libc::SYS_ptrace),
    ("quotactl", libc::SYS_quotactl),
    ("reboot", libc::SYS_reboot),
    ("request_key", libc::SYS_request_key),
    ("setdomainname", libc::SYS_setdomainname),
    ("setgid", libc::SYS_setgid),
    ("sethostname", libc::SYS_sethostname),
    ("setns", libc::SYS_setns),
    ("setresgid", libc::SYS_setresgid),
    ("setresuid", libc::SYS_setresuid),
    ("setuid", libc::SYS_setuid),
    ("socket", libc::SYS_socket),
    ("socketpair", libc::SYS_socketpair),
    ("splice", libc::SYS_splice),
    ("swapon", libc::SYS_swapon),
    ("syslog", libc::SYS_syslog),
    ("tee", libc::SYS_tee),
    ("umount2", libc::SYS_umount2),
    ("unshare", libc::SYS_unshare),
    ("userfaultfd", libc::SYS_userfaultfd),
    ("vmsplice", libc::SYS_vmsplice),
];

/// The names of the calls a shield may trap on this host.
pub fn trappable() -> impl Iterator<Item = &'static str> {
    TRAPPABLE.iter().map(|(name, _)| *name)
}

fn number(name: &str) -> Option<libc::c_long> {
    TRAPPABLE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, number)| *number)
}

fn name_of(number: i32) -> Option<&'static str> {
    TRAPPABLE
        .iter()
        .find(|(_, n)| *n == libc::c_long::from(number))
        .map(|(name, _)| *name)
}

// Classic BPF, as in <linux/filter.h>.
const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
#[cfg(target_arch = "x86_64")]
const BPF_JGE: u16 = 0x30;
const BPF_K: u16 = 0x00;
const BPF_RET: u16 = 0x06;
const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
/// `offsetof(struct seccomp_data, nr)` and `arch`.
const OFFSET_NR: u32 = 0;
const OFFSET_ARCH: u32 = 4;
/// x32 system calls are numbered from here.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

fn statement(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

/// The filter program, built before the fork: the child must not allocate.
pub struct Filter {
    program: Vec<libc::sock_filter>,
}

impl Filter {
    /// A filter trapping the calls `names`.
    ///
    /// # Errors
    ///
    /// If a name is not one [`TRAPPABLE`] lists, or none is given.
    pub fn new(names: &[String]) -> Result<Self, String> {
        if names.is_empty() {
            return Err("a syscall shield traps at least one call".into());
        }
        let mut numbers = Vec::with_capacity(names.len());
        for name in names {
            let number = number(name).ok_or_else(|| {
                format!("{name:?} is not a call a syscall shield can trap on this host")
            })?;
            if !numbers.contains(&number) {
                numbers.push(number);
            }
        }
        // Every jump below is at most numbers.len() long, and a jump is a u8.
        let count = u8::try_from(numbers.len())
            .map_err(|_| "a syscall shield traps at most 255 calls".to_string())?;
        let mut program = vec![
            statement(BPF_LD | BPF_W | BPF_ABS, OFFSET_ARCH),
            jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH, 1, 0),
            statement(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
            statement(BPF_LD | BPF_W | BPF_ABS, OFFSET_NR),
        ];
        #[cfg(target_arch = "x86_64")]
        {
            program.push(jump(BPF_JMP | BPF_JGE | BPF_K, X32_SYSCALL_BIT, 0, 1));
            program.push(statement(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));
        }
        // Each test jumps over the tests after it and the ALLOW, to NOTIF.
        for (index, number) in numbers.iter().enumerate() {
            let index = index as u8;
            program.push(jump(
                BPF_JMP | BPF_JEQ | BPF_K,
                *number as u32,
                count - index,
                0,
            ));
        }
        program.push(statement(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));
        program.push(statement(BPF_RET | BPF_K, libc::SECCOMP_RET_USER_NOTIF));
        Ok(Self { program })
    }
}

/// In the child, between fork and exec: install the filter and pass its
/// listener to the parent over `socket`. Allocates nothing.
///
/// `PR_SET_NO_NEW_PRIVS` must already be set, or the caller must hold
/// `CAP_SYS_ADMIN` (in its user namespace); the filter is refused otherwise.
/// A caller that sets NNP makes every later `exec` of a setuid binary ignore
/// the set-id bit, which a root process that will `sudo` cannot accept.
pub fn install(filter: &Filter, socket: RawFd) -> io::Result<()> {
    let program = libc::sock_fprog {
        len: filter.program.len() as libc::c_ushort,
        filter: filter.program.as_ptr().cast_mut(),
    };
    // SAFETY: `program` points to a filter that lives as long as this call.
    let listener = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            libc::SECCOMP_FILTER_FLAG_NEW_LISTENER,
            &raw const program,
        )
    };
    if listener < 0 {
        return Err(io::Error::last_os_error());
    }
    let listener = listener as RawFd;
    let sent = send_fd(socket, listener);
    // SAFETY: the listener is ours; the parent has its own copy if it was sent.
    unsafe { libc::close(listener) };
    sent
}

/// `sendmsg` with one descriptor. Stack buffers only.
fn send_fd(socket: RawFd, fd: RawFd) -> io::Result<()> {
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // Room for one cmsghdr and one int, aligned for cmsghdr.
    let mut control = [0u64; 4];
    // SAFETY: zeroed msghdr is a valid empty message; fields are set below.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &raw mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    // SAFETY: CMSG_SPACE only computes a size.
    message.msg_controllen = unsafe { libc::CMSG_SPACE(size_of::<RawFd>() as u32) } as _;
    // SAFETY: the control buffer is large enough for one header and an int,
    // as CMSG_SPACE says, and is aligned for cmsghdr.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&raw const message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(size_of::<RawFd>() as u32) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(header).cast::<RawFd>(), fd);
    }
    loop {
        // SAFETY: `message` describes the buffers above.
        let sent = unsafe { libc::sendmsg(socket, &raw const message, libc::MSG_NOSIGNAL) };
        if sent >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// `recvmsg` of one descriptor, waiting until `stop` is set.
fn receive_fd(socket: &OwnedFd, stop: &AtomicBool) -> Option<OwnedFd> {
    while !stop.load(Ordering::Acquire) {
        if !readable(socket.as_raw_fd(), 100)? {
            continue;
        }
        let mut byte = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: 1,
        };
        let mut control = [0u64; 4];
        // SAFETY: as in send_fd.
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &raw mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&control) as _;
        // SAFETY: `message` describes the buffers above.
        let got =
            unsafe { libc::recvmsg(socket.as_raw_fd(), &raw mut message, libc::MSG_CMSG_CLOEXEC) };
        if got <= 0 {
            // The child closed its end without sending: it never got as far.
            return None;
        }
        // SAFETY: the kernel filled `control` and set msg_controllen.
        unsafe {
            let header = libc::CMSG_FIRSTHDR(&raw const message);
            if header.is_null()
                || (*header).cmsg_level != libc::SOL_SOCKET
                || (*header).cmsg_type != libc::SCM_RIGHTS
            {
                return None;
            }
            let fd = std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<RawFd>());
            return Some(OwnedFd::from_raw_fd(fd));
        }
    }
    None
}

/// Whether `fd` is readable within `millis`, or `None` once it never will be.
fn readable(fd: RawFd, millis: i32) -> Option<bool> {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd, ours.
    let ready = unsafe { libc::poll(&raw mut poll, 1, millis) };
    if ready < 0 {
        let error = io::Error::last_os_error();
        return (error.kind() == io::ErrorKind::Interrupted).then_some(false);
    }
    if ready == 0 {
        return Some(false);
    }
    if poll.revents & libc::POLLIN != 0 {
        return Some(true);
    }
    // POLLHUP or POLLERR, and nothing to read: nothing ever will be.
    None
}

/// The thread that answers trapped calls for one run.
pub struct Supervisor {
    stop: Arc<AtomicBool>,
    /// Set by `detach`: the thread outlives this handle and ends when the
    /// kernel says no process holds the filter any more.
    detached: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Supervisor {
    /// Start answering for whatever child sends its listener down `socket`.
    pub fn start(socket: OwnedFd, gate: Arc<dyn SyscallGate>) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let detached = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("syscall-shield".into())
            .spawn({
                let stop = stop.clone();
                let detached = detached.clone();
                move || {
                    if let Some(listener) = receive_fd(&socket, &stop) {
                        drop(socket);
                        serve(&listener, gate.as_ref(), &stop, &detached);
                    }
                }
            })?;
        Ok(Self {
            stop,
            detached,
            thread: Some(thread),
        })
    }

    /// Let the supervisor run on its own: it answers until every process the
    /// filter applies to (the child and whatever it started) has exited, then
    /// ends. For a caller that cannot say when that is -- a child may leave a
    /// daemon behind -- and must not stop answering while one is alive: a
    /// trapped call with no listener fails with ENOSYS.
    pub fn detach(mut self) {
        self.detached.store(true, Ordering::Release);
        // Dropping the JoinHandle detaches the thread.
        self.thread.take();
    }

    /// Stop, once the workload is gone.
    pub fn finish(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if self.detached.load(Ordering::Acquire) {
            return;
        }
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(listener: &OwnedFd, gate: &dyn SyscallGate, stop: &AtomicBool, detached: &AtomicBool) {
    let fd = listener.as_raw_fd();
    loop {
        // A detached supervisor has no stop flag to look at: wait for a
        // notification or for the last filtered process to exit (POLLHUP),
        // rather than waking ten times a second for as long as it lives.
        let wait = if detached.load(Ordering::Acquire) {
            -1
        } else {
            100
        };
        match readable(fd, wait) {
            // Every process the filter applied to has exited.
            None => return,
            Some(false) => {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                continue;
            }
            Some(true) => {}
        }
        // SAFETY: the kernel requires a zeroed struct for RECV.
        let mut request: libc::seccomp_notif = unsafe { std::mem::zeroed() };
        // SAFETY: `request` is the struct this ioctl fills.
        if unsafe { libc::ioctl(fd, libc::SECCOMP_IOCTL_NOTIF_RECV, &raw mut request) } != 0 {
            // ENOENT: the caller died while waiting. Anything else is retried
            // at the next poll, which reports a listener that is gone.
            continue;
        }
        let verdict = match name_of(request.data.nr) {
            Some(syscall) => gate.decide(&TrappedCall {
                syscall,
                args: request.data.args,
                pid: request.pid,
            }),
            // The filter traps only listed numbers; an unknown one is refused.
            None => GateVerdict::Deny {
                errno: libc::ENOSYS,
            },
        };
        let mut response = libc::seccomp_notif_resp {
            id: request.id,
            val: 0,
            error: 0,
            flags: 0,
        };
        match verdict {
            GateVerdict::Allow => {
                response.flags = libc::SECCOMP_USER_NOTIF_FLAG_CONTINUE as u32;
            }
            GateVerdict::Deny { errno } => {
                response.error = -errno.clamp(1, 4095);
            }
            GateVerdict::Kill => {
                // The process the kernel named, while its call waits: it is
                // still there, since a call cannot return before the answer.
                // SAFETY: kill takes no pointers.
                unsafe { libc::kill(request.pid as libc::pid_t, libc::SIGKILL) };
                response.error = -libc::EPERM;
            }
        }
        // SAFETY: `response` is the struct this ioctl reads. ENOENT means the
        // caller is gone, which is no error here.
        unsafe { libc::ioctl(fd, libc::SECCOMP_IOCTL_NOTIF_SEND, &raw mut response) };
    }
}

/// A socket pair for the listener: the parent's end, and the child's.
pub fn channel() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: two ints for the kernel to fill.
    if unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both are new and ours.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Whether this host can trap calls to a supervisor and let them continue:
/// a listener (Linux 5.0) and `SECCOMP_USER_NOTIF_FLAG_CONTINUE` (5.5).
pub fn probe() -> Result<(), String> {
    probe_with(true)
}

/// As [`probe`], but says whether the probe child sets
/// `PR_SET_NO_NEW_PRIVS` first. With `false` it checks what a root caller
/// that must not set it can do: install the filter on `CAP_SYS_ADMIN` alone.
pub fn probe_with(no_new_privs: bool) -> Result<(), String> {
    let release = kernel_release().ok_or("the kernel's release could not be read")?;
    if release < (5, 5) {
        return Err(format!(
            "Linux {}.{} has no SECCOMP_USER_NOTIF_FLAG_CONTINUE (5.5)",
            release.0, release.1
        ));
    }
    let filter = Filter::new(&["syslog".to_string()])?;
    let (ours, theirs) = channel().map_err(|e| format!("socketpair: {e}"))?;
    // SAFETY: the child only makes syscalls and exits.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(format!("fork: {}", io::Error::last_os_error())),
        0 => {
            // SAFETY: prctl takes no pointers here.
            let ok = (!no_new_privs
                || unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } == 0)
                && install(&filter, theirs.as_raw_fd()).is_ok();
            // SAFETY: leave the child without running the parent's destructors.
            unsafe { libc::_exit(if ok { 0 } else { 1 }) }
        }
        _ => {
            drop(theirs);
            let mut status = 0;
            // SAFETY: `pid` is our child.
            unsafe { libc::waitpid(pid, &raw mut status, 0) };
            drop(ours);
            if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
                Ok(())
            } else {
                Err("installing a seccomp filter with a listener was refused".into())
            }
        }
    }
}

fn kernel_release() -> Option<(u32, u32)> {
    // SAFETY: zeroed utsname is valid for uname to fill.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    if unsafe { libc::uname(&raw mut name) } != 0 {
        return None;
    }
    let release: Vec<u8> = name
        .release
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    let release = String::from_utf8_lossy(&release);
    let mut parts = release.split(|c: char| !c.is_ascii_digit());
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessSandbox;
    use crate::{Control, Sandbox, SandboxCommand, SandboxSpec, SyscallShield};
    use std::sync::Mutex;

    const CHILD: &str = "HV2_SYSCALL_SHIELD_CHILD";
    const PERSONALITY_QUERY: u64 = 0xffff_ffff;

    /// Not a test: the workload the tests below run, as this same binary.
    #[test]
    fn shield_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        for _ in 0..6 {
            // SAFETY: personality(0xffffffff) only reads the persona.
            let result = unsafe { libc::syscall(libc::SYS_personality, PERSONALITY_QUERY) };
            let errno = io::Error::last_os_error().raw_os_error().unwrap_or(0);
            println!(
                "personality {} {}",
                result >= 0,
                if result >= 0 { 0 } else { errno }
            );
        }
    }

    fn run(gate: Arc<dyn SyscallGate>) -> Option<crate::SandboxOutput> {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::SyscallShield) {
            eprintln!(
                "skipping: {:?}",
                sandbox.controls().reason(Control::SyscallShield)
            );
            return None;
        }
        let mut command =
            SandboxCommand::new(std::env::current_exe().unwrap().display().to_string()).args([
                "--exact",
                "process::linux::syscall_shield::tests::shield_child",
                "--nocapture",
                "--test-threads=1",
                "-q",
            ]);
        command.env.insert(CHILD.to_string(), "1".to_string());
        let spec = SandboxSpec {
            syscall_shield: Some(SyscallShield {
                syscalls: vec!["personality".into()],
                gate,
            }),
            ..SandboxSpec::unconfined()
        };
        Some(sandbox.run(&command, &spec).unwrap())
    }

    fn lines(output: &crate::SandboxOutput) -> Vec<String> {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|l| l.starts_with("personality "))
            .map(str::to_string)
            .collect()
    }

    /// Refuses the first `refuse` calls whose first argument is the query,
    /// then lets them run: an answer that changes while the workload runs.
    struct Changing {
        seen: Mutex<Vec<TrappedCallRecord>>,
        refuse: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TrappedCallRecord {
        syscall: String,
        arg0: u64,
    }

    impl SyscallGate for Changing {
        fn decide(&self, call: &TrappedCall<'_>) -> GateVerdict {
            let mut seen = self.seen.lock().unwrap();
            seen.push(TrappedCallRecord {
                syscall: call.syscall.to_string(),
                arg0: call.args[0],
            });
            let queries = seen
                .iter()
                .filter(|c| c.arg0 & 0xffff_ffff == PERSONALITY_QUERY)
                .count();
            if call.args[0] & 0xffff_ffff == PERSONALITY_QUERY && queries <= self.refuse {
                GateVerdict::Deny { errno: libc::EPERM }
            } else {
                GateVerdict::Allow
            }
        }
    }

    #[test]
    fn a_trapped_call_is_refused_or_let_run_as_the_gate_answers_at_the_time() {
        let gate = Arc::new(Changing {
            seen: Mutex::new(Vec::new()),
            refuse: 3,
        });
        let Some(output) = run(gate.clone()) else {
            return;
        };
        assert_eq!(
            lines(&output),
            [
                "personality false 1",
                "personality false 1",
                "personality false 1",
                "personality true 0",
                "personality true 0",
                "personality true 0",
            ],
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let seen = gate.seen.lock().unwrap();
        assert!(seen.iter().all(|c| c.syscall == "personality"));
        assert!(seen.len() >= 6);
    }

    struct Killer;

    impl SyscallGate for Killer {
        fn decide(&self, call: &TrappedCall<'_>) -> GateVerdict {
            if call.args[0] & 0xffff_ffff == PERSONALITY_QUERY {
                GateVerdict::Kill
            } else {
                GateVerdict::Allow
            }
        }
    }

    #[test]
    fn a_kill_verdict_kills_the_caller_before_its_call_runs() {
        let Some(output) = run(Arc::new(Killer)) else {
            return;
        };
        assert!(
            lines(&output).is_empty(),
            "nothing after the first call ran"
        );
        assert_eq!(output.signal, Some(libc::SIGKILL));
    }

    #[test]
    fn only_listed_calls_are_trapped_and_the_program_is_what_it_says() {
        assert!(Filter::new(&[]).is_err());
        assert!(Filter::new(&["read".to_string()]).is_err(), "not trappable");
        let names: Vec<String> = ["ptrace", "bpf", "ptrace"].map(String::from).to_vec();
        let filter = Filter::new(&names).unwrap();
        let program = &filter.program;
        // Two numbers, not three: the repeat is dropped.
        let tests = program
            .iter()
            .filter(|i| i.code == BPF_JMP | BPF_JEQ | BPF_K && i.k != AUDIT_ARCH)
            .count();
        assert_eq!(tests, 2);
        let n = program.len();
        assert_eq!(program[n - 2].k, SECCOMP_RET_ALLOW);
        assert_eq!(program[n - 1].k, libc::SECCOMP_RET_USER_NOTIF);
        // Each test lands on the NOTIF return.
        for (at, instruction) in program.iter().enumerate() {
            if instruction.code == BPF_JMP | BPF_JEQ | BPF_K && instruction.k != AUDIT_ARCH {
                assert_eq!(at + 1 + usize::from(instruction.jt), n - 1);
            }
        }
        assert!(trappable().any(|n| n == "ptrace"));
        assert!(!trappable().any(|n| n == "read"));
    }

    /// A detached supervisor is not stopped by its handle going away: the
    /// handle is gone before the child even starts, and the child's trapped
    /// calls are still answered (a stopped one would leave them to fail with
    /// ENOSYS). The way the guest agent uses the engine: its own `Command`,
    /// its own `pre_exec`.
    #[test]
    fn a_detached_supervisor_answers_after_its_handle_is_gone() {
        use std::os::unix::process::CommandExt;
        if let Err(why) = probe() {
            eprintln!("skipping: {why}");
            return;
        }
        let gate = Arc::new(Changing {
            seen: Mutex::new(Vec::new()),
            refuse: 6,
        });
        let filter = Filter::new(&["personality".to_string()]).unwrap();
        let (ours, theirs) = channel().unwrap();
        Supervisor::start(ours, gate.clone()).unwrap().detach();
        let fd = theirs.as_raw_fd();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "process::linux::syscall_shield::tests::shield_child",
                "--nocapture",
                "--test-threads=1",
                "-q",
            ])
            .env(CHILD, "1");
        // SAFETY: prctl and `install` only, between fork and exec; the filter
        // was built before the fork.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                install(&filter, fd)
            });
        }
        let output = command.output().unwrap();
        drop(theirs);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout
            .lines()
            .filter(|l| l.starts_with("personality "))
            .collect();
        assert_eq!(
            lines,
            ["personality false 1"; 6],
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(gate.seen.lock().unwrap().len() >= 6);
    }
}
