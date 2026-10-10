//! One way out of an empty network namespace: a port on the host's loopback.
//!
//! # What this is for
//!
//! [`NetworkPolicy::Proxy`](crate::NetworkPolicy::Proxy) gives a workload one
//! address, `127.0.0.1` at a port where the caller's proxy listens, and
//! nothing else. The network namespace that isolates a workload has a
//! loopback of its own, so the host's is not there to connect to. This puts
//! the port there.
//!
//! # How
//!
//! A socket belongs to the namespace it was made in, and stays usable by
//! whoever holds it. So:
//!
//! 1. Inside the new namespace, before the workload exists, a listener is
//!    bound to `127.0.0.1` at the port, and a small process, the doorman, is
//!    left holding it.
//! 2. The doorman accepts each connection the workload makes and sends the
//!    accepted socket, the descriptor itself, over a socket pair to this
//!    process, which is still on the host's network.
//! 3. This process connects to the real port on the host's loopback and
//!    copies bytes between the two.
//!
//! The doorman reads nothing the workload sends. Everything it holds leads to
//! the caller's proxy and nowhere else, so a workload that took it over would
//! have gained the connection it already had.
//!
//! # When it ends
//!
//! The doorman is not the workload's child and is not in its process group,
//! so the workload neither waits on it nor is waited on for it. Where the
//! workload has a PID namespace the doorman is outside that too, and cannot
//! be signalled from inside; where it has none, the workload can kill the
//! doorman as it can any process of its user's, and loses its own network by
//! doing so. The doorman leaves when this process shuts the socket pair
//! down, which [`Relay`] does when it is dropped, and when this process
//! dies.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Connections relayed at once. The workload decides how many it opens, and
/// each costs this process two threads; past this they are closed unanswered.
const MAX_CONNECTIONS: usize = 256;

/// How long a proxy may stay silent, once the workload's side of a
/// connection has closed, before the connection is given up.
const LINGER: Duration = Duration::from_secs(30);

/// How long the caller's proxy has to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

/// This process's half: receives the workload's connections and carries them
/// to the port on the host.
pub(super) struct Relay {
    host: OwnedFd,
    receiver: Option<std::thread::JoinHandle<()>>,
}

/// The half that goes into the workload's namespace. Built before the fork,
/// because nothing after it may allocate.
pub(super) struct Doorway {
    namespace: OwnedFd,
    port: u16,
}

/// A relay to `port` on the host's loopback, and the half to start inside
/// the namespace.
pub(super) fn open(port: u16) -> std::io::Result<(Relay, Doorway)> {
    let mut pair = [0 as RawFd; 2];
    // SAFETY: `pair` has room for the two descriptors socketpair writes.
    // Both are close-on-exec: the workload must hold neither.
    if unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            pair.as_mut_ptr(),
        )
    } != 0
    {
        return Err(last_error());
    }
    // SAFETY: both were just returned by socketpair and are owned here alone.
    let (host, namespace) =
        unsafe { (OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])) };
    let receiving = host.try_clone()?;
    let receiver = std::thread::Builder::new()
        .name("hv2-sandbox-relay".to_string())
        .spawn(move || receive(&receiving, port))?;
    Ok((
        Relay {
            host,
            receiver: Some(receiver),
        },
        Doorway { namespace, port },
    ))
}

impl Drop for Relay {
    fn drop(&mut self) {
        // Both directions: the doorman sees its end hang up and leaves, and
        // the receiver's read returns. The doorman is not waited for. It is
        // not this process's child, and a workload able to stop it must not
        // thereby be able to keep its caller from returning.
        // SAFETY: shutdown on a socket this struct owns.
        unsafe { libc::shutdown(self.host.as_raw_fd(), libc::SHUT_RDWR) };
        if let Some(receiver) = self.receiver.take() {
            let _ = receiver.join();
        }
    }
}

/// Take each connection the doorman sends and carry it to `port`, until the
/// socket pair is shut down.
fn receive(host: &OwnedFd, port: u16) {
    let active = Arc::new(AtomicUsize::new(0));
    loop {
        let mut byte = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: 1,
        };
        // Room for one descriptor, aligned as a control message must be.
        let mut control = [0u64; 4];
        // SAFETY: zeroed is a valid msghdr; the fields set below point at
        // buffers that outlive the call.
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&control) as _;
        // SAFETY: as above. A descriptor received is close-on-exec from the
        // moment it exists here.
        let read = unsafe { libc::recvmsg(host.as_raw_fd(), &mut message, libc::MSG_CMSG_CLOEXEC) };
        if read < 0 && last_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if read <= 0 {
            return;
        }
        // SAFETY: the kernel filled `control` up to msg_controllen; the
        // CMSG_* macros walk exactly that.
        let received = unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            if header.is_null()
                || (*header).cmsg_level != libc::SOL_SOCKET
                || (*header).cmsg_type != libc::SCM_RIGHTS
            {
                continue;
            }
            let descriptor = std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<RawFd>());
            OwnedFd::from_raw_fd(descriptor)
        };
        let workload = TcpStream::from(received);
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let counted = Arc::clone(&active);
        let carried = std::thread::Builder::new()
            .name("hv2-sandbox-relay-conn".to_string())
            .spawn(move || {
                carry(workload, port);
                counted.fetch_sub(1, Ordering::SeqCst);
            });
        if carried.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Connect to `port` on the host's loopback and copy both ways until each
/// side has finished. A proxy that is not there closes the workload's
/// connection, which is what a refused one looks like from inside.
fn carry(workload: TcpStream, port: u16) {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(proxy) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) else {
        return;
    };
    let (Ok(mut from_proxy), Ok(mut to_workload)) = (proxy.try_clone(), workload.try_clone())
    else {
        return;
    };
    let back = std::thread::Builder::new()
        .name("hv2-sandbox-relay-back".to_string())
        .spawn(move || {
            copy(&mut from_proxy, &mut to_workload);
            let _ = to_workload.shutdown(Shutdown::Write);
        });
    let (mut from_workload, mut to_proxy) = (workload, proxy);
    copy(&mut from_workload, &mut to_proxy);
    let _ = to_proxy.shutdown(Shutdown::Write);
    // The workload has finished sending, or is gone. What the proxy still
    // has to say is carried, but a proxy that then says nothing and does not
    // close would keep these two threads for as long as it liked, long after
    // the run. The timeout is on the socket, so the thread reading it sees
    // it.
    let _ = to_proxy.set_read_timeout(Some(LINGER));
    match back {
        Ok(back) => {
            let _ = back.join();
        }
        // Nothing carries the answers, so there is nothing to wait for.
        Err(_) => {
            let _ = from_workload.shutdown(Shutdown::Both);
            let _ = to_proxy.shutdown(Shutdown::Both);
        }
    }
}

fn copy(from: &mut TcpStream, to: &mut TcpStream) {
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => {
                if to.write_all(&buffer[..read]).is_err() {
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

impl Doorway {
    /// Bring loopback up, listen on the port, and leave the doorman holding
    /// the listener.
    ///
    /// Called in the child between `fork` and `exec`, inside the new network
    /// namespace and while it still holds the capabilities the user
    /// namespace gave it. It allocates nothing and takes no locks.
    pub(super) fn start(&self) -> std::io::Result<()> {
        bring_loopback_up()?;
        let listener = listen_on_loopback(self.port)?;
        // Two forks, so the doorman is nobody's child here: the workload
        // must not find a child it did not start, and a workload that calls
        // `wait` must not wait for this.
        // SAFETY: this process is single-threaded; the children make only
        // system calls.
        let first = unsafe { libc::fork() };
        if first < 0 {
            let error = last_error();
            unsafe { libc::close(listener) };
            return Err(error);
        }
        if first == 0 {
            // SAFETY: as above.
            let second = unsafe { libc::fork() };
            if second == 0 {
                doorman(listener, self.namespace.as_raw_fd());
            }
            // SAFETY: _exit is async-signal-safe and does not return.
            unsafe { libc::_exit(i32::from(second < 0)) };
        }
        // SAFETY: closing this process's copy; the doorman has its own.
        unsafe { libc::close(listener) };
        let mut status = 0;
        loop {
            // SAFETY: `first` is this process's child.
            let waited = unsafe { libc::waitpid(first, &mut status, 0) };
            if waited == first {
                break;
            }
            if waited < 0 && last_error().kind() != std::io::ErrorKind::Interrupted {
                return Err(last_error());
            }
        }
        if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
            Ok(())
        } else {
            Err(std::io::Error::other(
                "the process that relays the workload's connections could not be started",
            ))
        }
    }
}

/// Set `IFF_UP` on `lo`. A new network namespace has loopback, down.
fn bring_loopback_up() -> std::io::Result<()> {
    // SAFETY: socket takes no pointers.
    let socket = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if socket < 0 {
        return Err(last_error());
    }
    // SAFETY: zeroed is a valid ifreq, and "lo" with its terminator fits.
    let mut request: libc::ifreq = unsafe { std::mem::zeroed() };
    request.ifr_name[0] = b'l' as libc::c_char;
    request.ifr_name[1] = b'o' as libc::c_char;
    // SAFETY: both requests read and write the ifreq passed, which outlives
    // them; the flags member is the one these two requests use.
    let result = unsafe {
        if libc::ioctl(socket, libc::SIOCGIFFLAGS as _, &mut request) != 0 {
            Err(last_error())
        } else {
            request.ifr_ifru.ifru_flags |= libc::IFF_UP as libc::c_short;
            if libc::ioctl(socket, libc::SIOCSIFFLAGS as _, &request) != 0 {
                Err(last_error())
            } else {
                Ok(())
            }
        }
    };
    // SAFETY: closing a descriptor opened above.
    unsafe { libc::close(socket) };
    result
}

/// A listening socket on `127.0.0.1` at `port` in this namespace.
fn listen_on_loopback(port: u16) -> std::io::Result<RawFd> {
    // SAFETY: socket takes no pointers. Non-blocking, so a connection that
    // went away between the doorman's poll and its accept does not hold it.
    let listener = unsafe {
        libc::socket(
            libc::AF_INET,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if listener < 0 {
        return Err(last_error());
    }
    // SAFETY: zeroed is a valid sockaddr_in.
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    address.sin_family = libc::AF_INET as libc::sa_family_t;
    address.sin_port = port.to_be();
    address.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
    // SAFETY: `address` is a whole sockaddr_in and its size is passed.
    let bound = unsafe {
        libc::bind(
            listener,
            std::ptr::addr_of!(address).cast(),
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        ) == 0
            && libc::listen(listener, 128) == 0
    };
    if !bound {
        let error = last_error();
        // SAFETY: closing a descriptor opened above.
        unsafe { libc::close(listener) };
        return Err(error);
    }
    Ok(listener)
}

/// Accept on `listener` and send each connection to the host over `host`,
/// until `host` hangs up. Does not return.
fn doorman(listener: RawFd, host: RawFd) -> ! {
    // This process was forked from one holding everything its parent had
    // open, and it never calls exec, so close-on-exec closes none of it. A
    // pipe kept open here is one whose reader never sees the end.
    close_all_but(listener, host);
    // The capabilities were for the namespace's setup. Nothing here needs
    // one, and the result is not checked because nothing here would use one.
    let _ = super::drop_capabilities();
    loop {
        let mut waiting = [
            libc::pollfd {
                fd: listener,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: host,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `waiting` holds the two entries its length says.
        let ready = unsafe { libc::poll(waiting.as_mut_ptr(), 2, -1) };
        if ready < 0 {
            if last_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            // SAFETY: _exit is async-signal-safe and does not return.
            unsafe { libc::_exit(1) };
        }
        // The host never writes: anything here is its end going away.
        if waiting[1].revents != 0 {
            // SAFETY: as above.
            unsafe { libc::_exit(0) };
        }
        if waiting[0].revents & libc::POLLIN == 0 {
            if waiting[0].revents != 0 {
                // SAFETY: as above.
                unsafe { libc::_exit(1) };
            }
            continue;
        }
        // SAFETY: accept4 with no address to fill.
        let connection = unsafe {
            libc::accept4(
                listener,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                libc::SOCK_CLOEXEC,
            )
        };
        if connection < 0 {
            continue;
        }
        let sent = send_descriptor(host, connection);
        // SAFETY: closing this process's copy; the host has its own.
        unsafe { libc::close(connection) };
        if !sent {
            // SAFETY: as above.
            unsafe { libc::_exit(0) };
        }
    }
}

/// Send `descriptor` over `channel`. False when the other end is gone.
fn send_descriptor(channel: RawFd, descriptor: RawFd) -> bool {
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    let mut control = [0u64; 4];
    // SAFETY: zeroed is a valid msghdr. CMSG_SPACE for one descriptor is 24
    // bytes on 64-bit Linux and `control` is 32, aligned for a header; the
    // CMSG_* macros write within the length set here.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen =
            libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as libc::c_uint) as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as libc::c_uint) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(header).cast::<RawFd>(), descriptor);
        loop {
            if libc::sendmsg(channel, &message, libc::MSG_NOSIGNAL) >= 0 {
                return true;
            }
            if last_error().kind() != std::io::ErrorKind::Interrupted {
                return false;
            }
        }
    }
}

/// Close every descriptor but these two.
fn close_all_but(one: RawFd, other: RawFd) {
    let (low, high) = (one.min(other), one.max(other));
    let ranges = [(0, low - 1), (low + 1, high - 1), (high + 1, RawFd::MAX)];
    for (first, last) in ranges {
        if first > last {
            continue;
        }
        // SAFETY: close_range takes three integers. Linux 5.9 has it; before
        // that it fails and each descriptor is closed by number instead.
        let closed = unsafe {
            libc::syscall(
                libc::SYS_close_range,
                first as libc::c_uint,
                last as libc::c_uint,
                0 as libc::c_uint,
            )
        };
        if closed == 0 {
            continue;
        }
        // SAFETY: zeroed is a valid rlimit, and getrlimit fills it.
        let open_limit = unsafe {
            let mut limit: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 {
                limit.rlim_cur.min(1 << 20) as RawFd
            } else {
                4096
            }
        };
        for descriptor in first..=last.min(open_limit) {
            // SAFETY: closing a number that may not be open is an error
            // return and nothing else.
            unsafe { libc::close(descriptor) };
        }
    }
}

/// Whether a namespace here can be given the listener: its own loopback
/// brought up and a socket bound on it.
pub(super) fn probe() -> std::io::Result<()> {
    // SAFETY: the child makes only system calls and _exit.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(last_error()),
        0 => {
            // SAFETY: unshare takes only flags; _exit does not return.
            unsafe {
                if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) != 0 {
                    libc::_exit(2);
                }
            }
            if bring_loopback_up().is_err() {
                // SAFETY: as above.
                unsafe { libc::_exit(3) };
            }
            // Any port: what is asked is whether one can be bound at all.
            let bound = listen_on_loopback(0).is_ok();
            // SAFETY: as above.
            unsafe { libc::_exit(if bound { 0 } else { 4 }) };
        }
        _ => {
            let mut status = 0;
            // SAFETY: `pid` is this process's child.
            unsafe { libc::waitpid(pid, &mut status, 0) };
            let step = if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else {
                1
            };
            match step {
                0 => Ok(()),
                2 => Err(std::io::Error::other(
                    "a network namespace could not be created",
                )),
                3 => Err(std::io::Error::other(
                    "loopback could not be brought up in a new network namespace",
                )),
                4 => Err(std::io::Error::other(
                    "nothing could listen on loopback in a new network namespace",
                )),
                _ => Err(std::io::Error::other(
                    "the rehearsal of a relayed port did not finish",
                )),
            }
        }
    }
}
