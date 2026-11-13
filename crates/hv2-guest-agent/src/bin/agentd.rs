//! `hv2-guest-agentd` — the half of the agent that runs inside the guest.
//!
//! It listens on the `AF_VSOCK` port named by `hv2_guest_agent::GUEST_AGENT_PORT`
//! and answers the requests that crate defines. This is the program that makes
//! "run a command in the VM" true: without it, the host has a channel and
//! nothing on the other end of it.
//!
//! # Running it
//!
//! ```text
//! # in the guest, once the vsock driver has loaded
//! modprobe vmw_vsock_virtio_transport   # usually automatic
//! /usr/local/bin/hv2-guest-agentd
//! ```
//!
//! Nothing here daemonises or supervises: a guest image is expected to start
//! this from its init system, which already knows how to restart a service and
//! where to put its logs.
//!
//! # Trust
//!
//! This runs commands the host asks for, with the privileges it was started
//! with, and does no authentication of its own — the channel is the boundary,
//! and only the host can open it. That is the same trust model as a serial
//! console, and it is worth being explicit about: do not start this in a guest
//! whose host you do not trust with the account it runs as.

fn main() {
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = linux::run() {
            eprintln!("hv2-guest-agentd: {e}");
            if linux::is_pid1() {
                // PID 1 exiting panics the kernel, which hides the message
                // above; stay up so the console keeps it.
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
            std::process::exit(1);
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // The binary still builds everywhere so the workspace builds
        // everywhere; it just has nothing to do off Linux, and says so rather
        // than failing to link.
        eprintln!(
            "hv2-guest-agentd runs inside a Linux guest: AF_VSOCK has no equivalent on {}",
            std::env::consts::OS
        );
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use hv2_guest_agent::{
        decode, encode, truncate_utf8, DirEntry, DirEntryKind, OpResult, Operation, PtySize,
        Request, Response, TemplateDefaults, GUEST_AGENT_PORT, MAX_DIR_ENTRIES,
        MAX_DIR_REPLY_BYTES, MAX_FRAME_BYTES, MAX_OUTPUT_BYTES, PROTOCOL_VERSION,
        TEMPLATE_DEFAULTS_PATH, TEMPLATE_USER,
    };
    use std::collections::{BTreeMap, HashMap};
    use std::io::{Read, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    /// Accept connections from anywhere; only the host can reach us anyway.
    const VMADDR_CID_ANY: u32 = u32::MAX;

    /// Version reported in a pong, so one guest image can be told from another.
    const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

    /// Whether to log every connection and read, with `HV2_AGENT_TRACE` set --
    /// which a kernel command line can do, since the kernel hands init the
    /// `KEY=value` words it does not know.
    ///
    /// Off by default because the console is a UART, and every byte written
    /// to one is a trap to the host. Two lines a request came to about 250
    /// VM exits: most of what a restored sandbox's first request cost, and a
    /// tax on every request after it, measured under nested virtualisation
    /// where each exit is dear. Worth paying when debugging a transport, and
    /// not otherwise.
    fn tracing() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("HV2_AGENT_TRACE").is_some())
    }

    /// Best-effort boot milestones on the same kernel clock as init's markers.
    /// Disabled by default and never allowed to prevent agent readiness.
    fn boot_trace(stage: &str) {
        static ON: OnceLock<bool> = OnceLock::new();
        if !*ON.get_or_init(|| std::env::var_os("HV2_BOOT_TRACE").is_some()) {
            return;
        }
        if let Ok(mut log) = std::fs::OpenOptions::new().write(true).open("/dev/kmsg") {
            // kmsg treats each write as a separate record; formatting directly
            // into the file can split the prefix and stage across records.
            let record = format!("<6>HV2_BOOT {stage}\n");
            let _ = log.write_all(record.as_bytes());
        }
    }

    /// One filesystem the agent mounts when it is the guest's init.
    #[derive(Debug, PartialEq, Eq)]
    pub struct MountSpec {
        pub source: &'static str,
        pub target: &'static str,
        pub fstype: &'static str,
    }

    /// What init mounts, in order. `/dev/pts` follows `/dev` because it is a
    /// directory inside it, and the agent's own pty support needs it.
    pub fn pid1_mounts() -> Vec<MountSpec> {
        let m = |source, target, fstype| MountSpec {
            source,
            target,
            fstype,
        };
        vec![
            m("proc", "/proc", "proc"),
            m("sysfs", "/sys", "sysfs"),
            m("devtmpfs", "/dev", "devtmpfs"),
            m("devpts", "/dev/pts", "devpts"),
            m("tmpfs", "/tmp", "tmpfs"),
        ]
    }

    /// Make `ROOT/etc/resolv.conf` a link to `/proc/net/pnp` unless something
    /// is already there. The kernel writes the nameserver it got from `ip=` on
    /// its command line to that file, in resolv.conf's own format. Takes the
    /// root as an argument so a test can use a temporary directory.
    pub fn ensure_resolv_conf(root: &std::path::Path) -> std::io::Result<()> {
        let etc = root.join("etc");
        std::fs::create_dir_all(&etc)?;
        let link = etc.join("resolv.conf");
        // symlink_metadata, so that even a dangling link counts as "there".
        match std::fs::symlink_metadata(&link) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::os::unix::fs::symlink("/proc/net/pnp", &link)
            }
            Err(e) => Err(e),
        }
    }

    fn mount_one(spec: &MountSpec) -> std::io::Result<()> {
        use std::ffi::CString;
        let nul = |s: &str| {
            CString::new(s).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))
        };
        let (source, target, fstype) = (nul(spec.source)?, nul(spec.target)?, nul(spec.fstype)?);
        // SAFETY: three NUL-terminated strings that outlive the call, no
        // flags, and a null data pointer, which mount(2) accepts; the return
        // value is checked.
        let rc = unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                fstype.as_ptr(),
                0,
                std::ptr::null(),
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        // Already mounted (the kernel can mount devtmpfs itself): not a failure.
        if err.raw_os_error() == Some(libc::EBUSY) {
            return Ok(());
        }
        Err(err)
    }

    /// If the kernel gave init no standard streams (no /dev/console in the
    /// initramfs when it started), open the console now that /dev exists, so
    /// that the log lines below and the children's stderr have somewhere to go.
    fn ensure_stdio() {
        // SAFETY: `fcntl` with F_GETFD on a small integer descriptor; -1 means
        // it is not open, which is what is being asked.
        if unsafe { libc::fcntl(2, libc::F_GETFD) } >= 0 {
            return;
        }
        // SAFETY: a NUL-terminated literal path; the result is checked.
        let fd = unsafe { libc::open(c"/dev/console".as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
        if fd < 0 {
            return;
        }
        for target in 0..3 {
            // SAFETY: `fd` is open, checked above; dup2 onto 0, 1 and 2.
            unsafe { libc::dup2(fd, target) };
        }
        if fd > 2 {
            // SAFETY: `fd` is the descriptor opened above and not used again.
            unsafe { libc::close(fd) };
        }
    }

    /// What an init does before it serves, when this process is PID 1. Each
    /// step's failure is reported and does not stop the others.
    ///
    /// Orphans are not reaped here: a `waitpid(-1)` loop would take exit
    /// statuses that the agent's own child handling is waiting for, and a
    /// guest that lives for one command can carry a few zombies.
    fn become_init() {
        for spec in pid1_mounts() {
            if let Err(e) = std::fs::create_dir_all(spec.target) {
                eprintln!("hv2-guest-agentd: init: mkdir {}: {e}", spec.target);
            }
            if let Err(e) = mount_one(&spec) {
                eprintln!(
                    "hv2-guest-agentd: init: mount {} on {}: {e}",
                    spec.fstype, spec.target
                );
            }
            if spec.target == "/dev" {
                ensure_stdio();
            }
        }
        if let Err(e) = ensure_resolv_conf(std::path::Path::new("/")) {
            eprintln!("hv2-guest-agentd: init: /etc/resolv.conf: {e}");
        }
        // Single-threaded here, before the first thread is started.
        std::env::set_var("PATH", "/bin");
        std::env::set_var("HOME", "/root");
        eprintln!("hv2-guest-agentd: running as PID 1 (init)");
    }

    /// Whether this process is the guest's init.
    pub fn is_pid1() -> bool {
        std::process::id() == 1
    }

    #[cfg(test)]
    mod init_tests {
        use super::*;

        #[test]
        fn init_mounts_proc_sys_dev_and_a_tmpfs_tmp_in_order() {
            let list: Vec<(&str, &str)> =
                pid1_mounts().iter().map(|m| (m.fstype, m.target)).collect();
            assert_eq!(
                list,
                [
                    ("proc", "/proc"),
                    ("sysfs", "/sys"),
                    ("devtmpfs", "/dev"),
                    ("devpts", "/dev/pts"),
                    ("tmpfs", "/tmp"),
                ]
            );
        }

        fn scratch(name: &str) -> std::path::PathBuf {
            let dir =
                std::env::temp_dir().join(format!("hv2-agentd-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        #[test]
        fn resolv_conf_becomes_a_link_to_the_kernels_pnp_file() {
            let root = scratch("resolv-new");
            // No /etc yet: it is created too.
            ensure_resolv_conf(&root).unwrap();
            let link = root.join("etc/resolv.conf");
            assert_eq!(
                std::fs::read_link(&link).unwrap(),
                std::path::Path::new("/proc/net/pnp")
            );
            // Running it again changes nothing.
            ensure_resolv_conf(&root).unwrap();
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn an_existing_resolv_conf_is_left_alone() {
            let root = scratch("resolv-existing");
            std::fs::create_dir_all(root.join("etc")).unwrap();
            std::fs::write(root.join("etc/resolv.conf"), "nameserver 192.0.2.1\n").unwrap();
            ensure_resolv_conf(&root).unwrap();
            assert_eq!(
                std::fs::read_to_string(root.join("etc/resolv.conf")).unwrap(),
                "nameserver 192.0.2.1\n"
            );
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn the_test_process_is_not_init() {
            assert!(!is_pid1());
        }
    }

    pub fn run() -> std::io::Result<()> {
        if is_pid1() {
            become_init();
        }
        let listener = bind()?;
        boot_trace("agent_listening");
        eprintln!("hv2-guest-agentd {AGENT_VERSION} listening on vsock port {GUEST_AGENT_PORT}");

        let mut first_accept = true;
        loop {
            let fd = unsafe { libc::accept(listener, std::ptr::null_mut(), std::ptr::null_mut()) };
            if fd < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            if first_accept {
                boot_trace("agent_first_accept");
                first_accept = false;
            }
            // Say so on accept, when tracing. Silence here is ambiguous in
            // exactly the way that costs the most time: a host that gets no
            // answer cannot tell an agent that never accepted from one that
            // accepted and is waiting on a request that never arrived, and
            // those are failures in different halves of the transport.
            if tracing() {
                eprintln!("hv2-guest-agentd: accepted a connection");
            }

            // A thread per connection. This was one connection at a time,
            // which a snapshot turned into a hang: the guest in every restored
            // copy still held the connection the snapshot was taken during,
            // blocked in `read` on a host-side peer that no longer existed --
            // the host's connection table is not part of a snapshot -- so
            // the agent never returned to `accept`, and every sandbox from
            // that template (or that paused sandbox) was unreachable. With a
            // thread each, a dead connection costs one parked thread and
            // nothing else. It also stops one long command from holding the
            // agent against every other request.
            std::thread::spawn(move || {
                if let Err(e) = serve(fd) {
                    eprintln!("hv2-guest-agentd: connection ended: {e}");
                }
                unsafe { libc::close(fd) };
            });
        }
    }

    /// Create and bind the listening socket.
    fn bind() -> std::io::Result<libc::c_int> {
        let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        let mut addr: libc::sockaddr_vm = unsafe { std::mem::zeroed() };
        addr.svm_family = libc::AF_VSOCK as libc::sa_family_t;
        addr.svm_port = GUEST_AGENT_PORT;
        addr.svm_cid = VMADDR_CID_ANY;

        let rc = unsafe {
            libc::bind(
                fd,
                &addr as *const libc::sockaddr_vm as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(err);
        }

        // Host requests and forwarded streams can arrive as a burst. Keep a
        // bounded pending queue so they can wait while the accept loop creates
        // each connection's worker, instead of overflowing a four-slot queue.
        if unsafe { libc::listen(fd, 128) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(err);
        }

        Ok(fd)
    }

    /// Read requests from one connection until it closes.
    ///
    /// Raw `read`/`write` rather than a socket wrapper: the fd belongs to the
    /// caller, and every std wrapper that could hold it would also close it.
    fn serve(fd: libc::c_int) -> std::io::Result<()> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];

        loop {
            let read = read_fd(fd, &mut chunk)?;
            // Same reason as the accept log: from the host, a read that never
            // returns and a read that returns something unparseable are the
            // same silence.
            if tracing() {
                eprintln!("hv2-guest-agentd: read {read} bytes");
            }
            if read == 0 {
                return Ok(());
            }
            if buf.len() + read > MAX_FRAME_BYTES + 4 {
                // The host would have to be malfunctioning to send this. Drop
                // the connection rather than grow without limit.
                eprintln!("hv2-guest-agentd: oversized request, closing connection");
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..read]);

            while let Some((request, used)) = decode::<Request>(&buf)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?
            {
                buf.drain(..used);
                // A forward takes the connection too: answered, then spliced.
                if let Operation::Forward { port } = request.op {
                    let target = std::net::TcpStream::connect(("127.0.0.1", port))
                        .or_else(|_| std::net::TcpStream::connect(("::1", port)));
                    let response = Response {
                        id: request.id,
                        version: PROTOCOL_VERSION,
                        result: match &target {
                            Ok(_) => OpResult::Acknowledged,
                            Err(e) => OpResult::Failed {
                                message: format!("nothing is listening on port {port}: {e}"),
                            },
                        },
                    };
                    let bytes = encode(&response).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                    })?;
                    write_all_fd(fd, &bytes)?;
                    let Ok(tcp) = target else { return Ok(()) };
                    splice(fd, tcp, &buf);
                    return Ok(());
                }
                // A listen takes it too: answered, then held until a guest
                // program connects, then spliced.
                if let Operation::Listen { port } = request.op {
                    let held = listening::hold(port);
                    let response = Response {
                        id: request.id,
                        version: PROTOCOL_VERSION,
                        result: match &held {
                            Ok(_) => OpResult::Acknowledged,
                            Err(e) => OpResult::Failed {
                                message: format!("listening on port {port}: {e}"),
                            },
                        },
                    };
                    let bytes = encode(&response).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                    })?;
                    write_all_fd(fd, &bytes)?;
                    let Ok(held) = held else { return Ok(()) };
                    let accepted = held.accept_or_hangup(fd);
                    drop(held);
                    if let Some(tcp) = accepted {
                        write_all_fd(fd, &[hv2_guest_agent::REVERSE_ACCEPTED])?;
                        splice(fd, tcp, &buf);
                    }
                    return Ok(());
                }
                if let Operation::ForwardUdp { port } | Operation::ForwardUdp6 { port } = request.op
                {
                    let host = if matches!(request.op, Operation::ForwardUdp6 { .. }) {
                        "::1"
                    } else {
                        "127.0.0.1"
                    };
                    let target = if port == 0 {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "UDP port must be nonzero",
                        ))
                    } else {
                        std::net::UdpSocket::bind((host, 0)).and_then(|socket| {
                            socket.connect((host, port))?;
                            Ok(socket)
                        })
                    };
                    let response = Response {
                        id: request.id,
                        version: PROTOCOL_VERSION,
                        result: match &target {
                            Ok(_) => OpResult::Acknowledged,
                            Err(error) => OpResult::Failed {
                                message: format!("UDP socket setup failed: {error}"),
                            },
                        },
                    };
                    write_all_fd(fd, &encode(&response).map_err(std::io::Error::other)?)?;
                    if let Ok(socket) = target {
                        relay_udp(fd, socket, &buf)?;
                    }
                    return Ok(());
                }
                // A volume mount takes the connection itself: answered here,
                // then handed to the kernel, and this loop is done with it.
                if let Operation::MountVolume { path } = &request.op {
                    let ready = prepare_mount_point(path);
                    let response = Response {
                        id: request.id,
                        version: PROTOCOL_VERSION,
                        result: match &ready {
                            Ok(()) => OpResult::Acknowledged,
                            Err(message) => OpResult::Failed {
                                message: message.clone(),
                            },
                        },
                    };
                    let bytes = encode(&response).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                    })?;
                    write_all_fd(fd, &bytes)?;
                    if ready.is_ok() {
                        if let Err(e) = mount_volume(fd, path) {
                            eprintln!("hv2-guest-agentd: mounting a volume at {path}: {e}");
                        }
                        // Ours no more: the kernel holds its own reference,
                        // and the caller's close drops only this one.
                        return Ok(());
                    }
                    continue;
                }
                let response = handle(request);
                let bytes = encode(&response).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                })?;
                write_all_fd(fd, &bytes)?;
            }
        }
    }

    /// See [`Operation::Stats`]. Whatever cannot be read reads as zero: a
    /// partial answer is still a sample.
    fn stats() -> hv2_guest_agent::GuestStats {
        let mut s = hv2_guest_agent::GuestStats::default();
        if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
            for line in stat.lines() {
                let mut fields = line.split_whitespace();
                match fields.next() {
                    Some("cpu") => {
                        let ticks: Vec<u64> = fields.filter_map(|f| f.parse().ok()).collect();
                        // user nice system idle iowait irq softirq steal
                        let idle =
                            ticks.get(3).copied().unwrap_or(0) + ticks.get(4).copied().unwrap_or(0);
                        let total: u64 = ticks.iter().take(8).sum();
                        s.cpu_total_ticks = total;
                        s.cpu_busy_ticks = total.saturating_sub(idle);
                    }
                    Some(name) if name.starts_with("cpu") => s.cpus += 1,
                    _ => {}
                }
            }
        }
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                let mut fields = line.split_whitespace();
                let (Some(key), Some(kb)) = (
                    fields.next(),
                    fields.next().and_then(|v| v.parse::<u64>().ok()),
                ) else {
                    continue;
                };
                match key {
                    "MemTotal:" => s.mem_total = kb * 1024,
                    "MemAvailable:" => s.mem_available = kb * 1024,
                    "Cached:" => s.mem_cached = kb * 1024,
                    _ => {}
                }
            }
        }
        let mut fs: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c"/".as_ptr(), &mut fs) } == 0 {
            let block = fs.f_frsize as u64;
            s.disk_total = fs.f_blocks as u64 * block;
            s.disk_used = (fs.f_blocks as u64).saturating_sub(fs.f_bfree as u64) * block;
        }
        s
    }

    #[cfg(test)]
    #[test]
    fn udp_relay_preserves_empty_binary_and_early_frames_and_stops_on_eof() {
        use hv2_guest_agent::datagram::{read_frame, write_frame};
        use std::os::fd::AsRawFd;
        let peer = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        socket.connect(peer.local_addr().unwrap()).unwrap();
        let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut early = Vec::new();
        write_frame(&mut early, &[0, 255]).unwrap();
        let relay = std::thread::spawn(move || relay_udp(server.as_raw_fd(), socket, &early));
        for payload in [vec![0, 255], Vec::new(), vec![13, 10, 0, 255]] {
            if payload != [0, 255] {
                write_frame(&mut client, &payload).unwrap();
            }
            let mut bytes = [0; 16];
            let (size, sender) = peer.recv_from(&mut bytes).unwrap();
            assert_eq!(&bytes[..size], payload);
            peer.send_to(&payload, sender).unwrap();
            assert_eq!(read_frame(&mut client).unwrap(), Some(payload));
        }
        client.shutdown(std::net::Shutdown::Both).unwrap();
        relay.join().unwrap().unwrap();
    }

    fn relay_udp(
        fd: libc::c_int,
        socket: std::net::UdpSocket,
        early: &[u8],
    ) -> std::io::Result<()> {
        use hv2_guest_agent::datagram::{read_frame, write_frame, MAX_PAYLOAD};
        use std::os::fd::FromRawFd;
        use std::sync::atomic::{AtomicBool, Ordering};
        let duplicate = unsafe { libc::dup(fd) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut transport = unsafe { std::os::unix::net::UnixStream::from_raw_fd(duplicate) };
        transport.set_read_timeout(Some(Duration::from_secs(30)))?;
        transport.set_write_timeout(Some(Duration::from_secs(5)))?;
        socket.set_read_timeout(Some(Duration::from_millis(100)))?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
        let from_host = transport.try_clone()?;
        let shutdown = transport.try_clone()?;
        let sender = socket.try_clone()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_reader = stopped.clone();
        let early = early.to_vec();
        let upstream = std::thread::spawn(move || {
            let mut reader = std::io::Cursor::new(early).chain(from_host);
            let result = (|| -> std::io::Result<()> {
                while let Some(payload) = read_frame(&mut reader)? {
                    if sender.send(&payload)? != payload.len() {
                        return Err(std::io::ErrorKind::WriteZero.into());
                    }
                }
                Ok(())
            })();
            stop_reader.store(true, Ordering::Release);
            let _ = shutdown.shutdown(std::net::Shutdown::Both);
            result
        });
        let mut payload = vec![0; MAX_PAYLOAD + 1];
        let result = (|| -> std::io::Result<()> {
            while !stopped.load(Ordering::Acquire) {
                match socket.recv(&mut payload) {
                    Ok(size) => write_frame(&mut transport, &payload[..size])?,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        })();
        let _ = transport.shutdown(std::net::Shutdown::Both);
        let reader_result = upstream
            .join()
            .map_err(|_| std::io::Error::other("UDP relay reader failed"))?;
        result.and(reader_result)
    }

    /// Guest loopback ports listened on for the host: a program in the guest
    /// connects, and the connection is carried out to the host's caller.
    mod listening {
        use std::collections::HashMap;
        use std::net::{TcpListener, TcpStream};
        use std::os::fd::AsRawFd;
        use std::sync::{Arc, Mutex, OnceLock};
        use std::time::Duration;

        /// How long a port stays listened on after the last connection
        /// waiting on it has gone: the host's next one is on its way.
        const GRACE: Duration = Duration::from_secs(5);

        struct Entry {
            listener: Arc<TcpListener>,
            /// Connections from the host waiting on it now.
            holders: usize,
            /// Changes with every hold, so a grace that ended is not one a
            /// later hold already outlived.
            generation: u64,
        }

        fn table() -> std::sync::MutexGuard<'static, HashMap<u16, Entry>> {
            static TABLE: OnceLock<Mutex<HashMap<u16, Entry>>> = OnceLock::new();
            TABLE
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        /// One host connection's share of a listened port.
        pub(super) struct Held {
            port: u16,
            listener: Arc<TcpListener>,
        }

        /// Listen on `port`, or share the listener already there.
        pub(super) fn hold(port: u16) -> std::io::Result<Held> {
            if port == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "the port must be nonzero",
                ));
            }
            let mut table = table();
            if let Some(entry) = table.get_mut(&port) {
                entry.holders += 1;
                entry.generation += 1;
                return Ok(Held {
                    port,
                    listener: Arc::clone(&entry.listener),
                });
            }
            let listener = TcpListener::bind(("127.0.0.1", port))?;
            listener.set_nonblocking(true)?;
            let listener = Arc::new(listener);
            table.insert(
                port,
                Entry {
                    listener: Arc::clone(&listener),
                    holders: 1,
                    generation: 0,
                },
            );
            Ok(Held { port, listener })
        }

        impl Held {
            /// A guest program's connection, or `None` once the host has hung
            /// up -- it sends nothing before the answer, so anything it does
            /// send means it is gone.
            pub(super) fn accept_or_hangup(&self, host: libc::c_int) -> Option<TcpStream> {
                loop {
                    let mut fds = [
                        libc::pollfd {
                            fd: self.listener.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        },
                        libc::pollfd {
                            fd: host,
                            events: libc::POLLIN,
                            revents: 0,
                        },
                    ];
                    // SAFETY: two valid pollfds, for the length given.
                    let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                    if ready < 0 {
                        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        return None;
                    }
                    if fds[1].revents != 0 {
                        return None;
                    }
                    if fds[0].revents != 0 {
                        // Another host connection waiting on the same port
                        // may have taken it.
                        match self.listener.accept() {
                            Ok((tcp, _)) => {
                                let _ = tcp.set_nonblocking(false);
                                return Some(tcp);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                            Err(_) => return None,
                        }
                    }
                }
            }
        }

        impl Drop for Held {
            fn drop(&mut self) {
                let mut entries = table();
                let Some(entry) = entries.get_mut(&self.port) else {
                    return;
                };
                entry.holders -= 1;
                if entry.holders > 0 {
                    return;
                }
                let (port, generation) = (self.port, entry.generation);
                std::thread::spawn(move || {
                    std::thread::sleep(GRACE);
                    let mut entries = table();
                    if entries
                        .get(&port)
                        .is_some_and(|e| e.holders == 0 && e.generation == generation)
                    {
                        entries.remove(&port);
                    }
                });
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn a_port_is_shared_and_closed_after_its_last_holder() {
                let port = {
                    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
                    probe.local_addr().unwrap().port()
                };
                let first = hold(port).unwrap();
                let second = hold(port).unwrap();
                assert!(Arc::ptr_eq(&first.listener, &second.listener));
                // A guest program connects; one holder takes it.
                let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let (host, _keep) = std::os::unix::net::UnixStream::pair().unwrap();
                let taken = first.accept_or_hangup(host.as_raw_fd()).unwrap();
                assert_eq!(taken.peer_addr().unwrap(), client.local_addr().unwrap());
                drop((first, second));
                assert!(table().contains_key(&port), "closed before its grace");
                std::thread::sleep(GRACE + Duration::from_millis(500));
                assert!(!table().contains_key(&port));
                assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
            }

            #[test]
            fn the_host_hanging_up_ends_the_wait() {
                let held = hold({
                    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
                    probe.local_addr().unwrap().port()
                })
                .unwrap();
                let (host, other) = std::os::unix::net::UnixStream::pair().unwrap();
                drop(other);
                assert!(held.accept_or_hangup(host.as_raw_fd()).is_none());
            }
        }
    }

    /// Copy bytes both ways between the host's connection `fd` and `tcp`
    /// until either side closes; `early` is what the host sent past the
    /// forward request, which belongs to the stream.
    fn splice(fd: libc::c_int, tcp: std::net::TcpStream, early: &[u8]) {
        use std::io::Write as _;
        // The caller closes `fd` when this returns: the copies use their
        // own duplicates, so each closes its half cleanly.
        let dup = |fd: libc::c_int| {
            let d = unsafe { libc::dup(fd) };
            (d >= 0).then(|| unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(d) })
        };
        let (Some(mut from_host), Some(mut to_host)) = (dup(fd), dup(fd)) else {
            return;
        };
        let _ = tcp.set_nodelay(true);
        let Ok(mut tcp_reader) = tcp.try_clone() else {
            return;
        };
        let mut tcp_writer = tcp;
        if !early.is_empty() && tcp_writer.write_all(early).is_err() {
            return;
        }
        let upstream = std::thread::spawn(move || {
            if std::io::copy(&mut from_host, &mut tcp_writer).is_err() {
                let _ = tcp_writer.shutdown(std::net::Shutdown::Both);
                unsafe { libc::shutdown(fd, libc::SHUT_RDWR) };
            } else {
                let _ = tcp_writer.shutdown(std::net::Shutdown::Write);
            }
        });
        if std::io::copy(&mut tcp_reader, &mut to_host).is_err() {
            let _ = tcp_reader.shutdown(std::net::Shutdown::Both);
            unsafe { libc::shutdown(fd, libc::SHUT_RDWR) };
        } else {
            // EOF only closes this direction; the program may still read.
            unsafe { libc::shutdown(fd, libc::SHUT_WR) };
        }
        let _ = upstream.join();
        unsafe { libc::shutdown(fd, libc::SHUT_RDWR) };
    }

    #[cfg(test)]
    #[test]
    fn forwarded_server_eof_still_allows_the_client_request() {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (mut client, relay) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let tcp = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let worker = std::thread::spawn(move || splice(relay.as_raw_fd(), tcp, &[]));
        server.write_all(b"ready").unwrap();
        server.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert_eq!(response, b"ready");
        client.write_all(b"request after EOF").unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut request = Vec::new();
        server.read_to_end(&mut request).unwrap();
        assert_eq!(request, b"request after EOF");
        worker.join().unwrap();
    }

    /// `path`, a directory with nothing mounted on it: made if missing, and
    /// a mount left there -- one whose host end did not survive a snapshot's
    /// restore -- detached, so the new one is what is seen.
    fn prepare_mount_point(path: &str) -> Result<(), String> {
        if !path.starts_with('/') {
            return Err(format!("{path}: a volume mounts at an absolute path"));
        }
        let target = std::ffi::CString::new(path).map_err(|e| e.to_string())?;
        // Detached first: a dead mount there -- restored from a snapshot,
        // its server gone -- cannot even be looked at, so making the
        // directory would fail on it. EINVAL: nothing mounted, as usual.
        unsafe { libc::umount2(target.as_ptr(), libc::MNT_DETACH) };
        std::fs::create_dir_all(path).map_err(|e| format!("{path}: {e}"))?;
        Ok(())
    }

    /// Mount 9P over connection `fd` at `path`: the kernel reads and writes
    /// the socket from here on, and this call returns once it has attached.
    fn mount_volume(fd: libc::c_int, path: &str) -> std::io::Result<()> {
        let source = c"hv2-volume";
        let fstype = c"9p";
        let target = std::ffi::CString::new(path)?;
        // cache=none: other sandboxes, and the volume API, change the files
        // too, and each must see the others' writes. access=user: each guest
        // user attaches as itself, so the host records who made what.
        let options = std::ffi::CString::new(format!(
            "trans=fd,rfdno={fd},wfdno={fd},version=9p2000.L,msize=524288,cache=none,access=user"
        ))?;
        let rc = unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                fstype.as_ptr(),
                0,
                options.as_ptr().cast(),
            )
        };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn read_fd(fd: libc::c_int, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            return Ok(n as usize);
        }
    }

    fn write_all_fd(fd: libc::c_int, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            // A short write is normal on a socket; the rest goes next time.
            data = &data[n as usize..];
        }
        Ok(())
    }

    /// Carry out one request.
    fn handle(request: Request) -> Response {
        let id = request.id;

        if request.version != PROTOCOL_VERSION {
            // A guest image outlives the host that built it. Saying so beats
            // misreading a field that moved.
            return Response {
                id,
                version: PROTOCOL_VERSION,
                result: OpResult::Failed {
                    message: format!(
                        "host speaks protocol version {}, this agent speaks {PROTOCOL_VERSION}",
                        request.version
                    ),
                },
            };
        }

        let result = match request.op {
            Operation::Ping => OpResult::Pong {
                agent_version: AGENT_VERSION.to_string(),
            },
            Operation::Exec {
                program,
                args,
                cwd,
                stdin,
                timeout_ms,
                user,
            } => exec(
                &program,
                &args,
                cwd.as_deref(),
                stdin.as_deref(),
                timeout_ms,
                user.as_deref(),
            ),
            Operation::Start {
                program,
                args,
                cwd,
                envs,
                pty,
                user,
            } => start(&program, &args, cwd.as_deref(), &envs, pty, user.as_deref()),
            Operation::Poll { pid } => poll(pid),
            Operation::WriteStdin { pid, data, close } => write_stdin(pid, &data, close),
            Operation::Signal {
                pid,
                signal: number,
            } => signal(pid, number),
            Operation::ResizePty { pid, size } => resize_pty(pid, size),
            Operation::Restored {
                unix_time_ns,
                entropy,
            } => restored(unix_time_ns, &entropy),
            Operation::WriteFile {
                path,
                data,
                append,
                owner,
            } => write_file(&path, &data, append, owner.as_deref()),
            Operation::ReadFile {
                path,
                offset,
                length,
            } => read_file(&path, offset, length),
            Operation::Stats => OpResult::Stats(stats()),
            Operation::ListDir { path, limit } => list_dir(&path, limit),
            Operation::Stat { path, follow } => stat_path(&path, follow),
            Operation::MakeDir {
                path,
                parents,
                owner,
                mode,
            } => make_dir(&path, parents, owner.as_deref(), mode),
            Operation::Remove {
                path,
                recursive,
                missing_ok,
            } => remove(&path, recursive, missing_ok),
            Operation::SetAttr {
                path,
                owner,
                mode,
                recursive,
            } => set_attr(&path, owner.as_deref(), mode, recursive),
            Operation::ReadLink { path } => read_link(&path),
            Operation::MakeSymlink {
                target,
                path,
                owner,
            } => make_symlink(&target, &path, owner.as_deref()),
            Operation::SignalProcess { pid, signal } => signal_process(pid, signal),
            Operation::MountBlock {
                device,
                target,
                fstype,
                expect_sectors,
                drop_caches,
            } => mount_block(&device, &target, fstype, expect_sectors, drop_caches),
            // Served in `serve`, which owns the connection it takes.
            Operation::Forward { .. }
            | Operation::Listen { .. }
            | Operation::ForwardUdp { .. }
            | Operation::ForwardUdp6 { .. } => OpResult::Failed {
                message: "a forward must be the connection's own request".into(),
            },
            // Served in `serve`, which owns the connection it takes.
            Operation::MountVolume { .. } => OpResult::Failed {
                message: "a volume mount must be the connection's own request".into(),
            },
        };

        Response {
            id,
            version: PROTOCOL_VERSION,
            result,
        }
    }

    /// See [`Operation::WriteFile`].
    fn write_file(path: &str, data: &str, append: bool, owner: Option<&str>) -> OpResult {
        let failed = |what: &str, e: &dyn std::fmt::Display| OpResult::Failed {
            message: format!("{what} {path}: {e}"),
        };
        let Some(bytes) = hv2_guest_agent::b64::decode(data) else {
            return failed("decoding the data for", &"not base64");
        };
        let owner = match owner {
            Some(TEMPLATE_USER) => template_defaults().user,
            other => other.map(str::to_string),
        };
        let account = match owner.as_deref().map(Account::of).transpose() {
            Ok(account) => account.filter(|a| a.uid != 0),
            Err(e) => return failed("owning", &e),
        };
        // The directories this write makes, outermost first: the owner's
        // too, or a file they own sits somewhere they cannot write.
        let mut made = Vec::new();
        let mut missing = std::path::Path::new(path).parent();
        while let Some(dir) = missing.filter(|d| !d.as_os_str().is_empty() && !d.exists()) {
            made.push(dir.to_path_buf());
            missing = dir.parent();
        }
        if let Some(parent) = std::path::Path::new(path).parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return failed("creating the directory of", &e);
            }
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path);
        if let Err(e) = file.and_then(|mut f| f.write_all(&bytes)) {
            return failed("writing", &e);
        }
        if let Some(account) = account {
            for made in made.iter().rev().map(std::path::PathBuf::as_path) {
                let _ = std::os::unix::fs::chown(made, Some(account.uid), Some(account.gid));
            }
            if let Err(e) = std::os::unix::fs::chown(path, Some(account.uid), Some(account.gid)) {
                return failed("giving the owner", &e);
            }
        }
        OpResult::Acknowledged
    }

    /// A user of this guest, from `/etc/passwd` and `/etc/group`.
    struct Account {
        name: String,
        uid: u32,
        gid: u32,
        /// Every group listing the user, for `setgroups`.
        groups: Vec<libc::gid_t>,
        home: String,
    }

    impl Account {
        /// `user` by name or number. Root needs no `/etc/passwd`: an image
        /// with none still runs as root.
        fn of(user: &str) -> Result<Self, String> {
            if user == "root" || user == "0" {
                return Ok(Self {
                    name: "root".into(),
                    uid: 0,
                    gid: 0,
                    groups: vec![0],
                    home: "/root".into(),
                });
            }
            let passwd = std::fs::read_to_string("/etc/passwd")
                .map_err(|e| format!("reading /etc/passwd for user {user}: {e}"))?;
            let fields = passwd
                .lines()
                .map(|line| line.split(':').collect::<Vec<_>>())
                .find(|f| f.len() >= 6 && (f[0] == user || f[2] == user))
                .ok_or_else(|| format!("no user {user} in this sandbox"))?;
            let number = |s: &str| s.parse::<u32>().map_err(|e| format!("user {user}: {e}"));
            let (name, uid, gid) = (
                fields[0].to_string(),
                number(fields[2])?,
                number(fields[3])?,
            );
            let mut groups = vec![gid];
            if let Ok(group) = std::fs::read_to_string("/etc/group") {
                for f in group.lines().map(|l| l.split(':').collect::<Vec<_>>()) {
                    if f.len() >= 4 && f[3].split(',').any(|m| m == name) {
                        if let Ok(g) = f[2].parse::<u32>() {
                            if !groups.contains(&g) {
                                groups.push(g);
                            }
                        }
                    }
                }
            }
            Ok(Self {
                name,
                uid,
                gid,
                groups,
                home: fields[5].to_string(),
            })
        }
    }

    /// See [`Operation::ReadFile`].
    fn read_file(path: &str, offset: u64, length: u64) -> OpResult {
        use std::io::{Seek, SeekFrom};
        let failed = |e: std::io::Error| OpResult::Failed {
            message: format!("reading {path}: {e}"),
        };
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) => return failed(e),
        };
        let size = match file.metadata() {
            Ok(meta) if meta.is_dir() => {
                return OpResult::Failed {
                    message: format!("reading {path}: it is a directory"),
                }
            }
            Ok(meta) => meta.len(),
            Err(e) => return failed(e),
        };
        let want = length.min(hv2_guest_agent::FILE_CHUNK as u64) as usize;
        let mut buf = vec![0u8; want];
        let mut got = 0;
        if let Err(e) = file.seek(SeekFrom::Start(offset)) {
            return failed(e);
        }
        while got < want {
            match file.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return failed(e),
            }
        }
        OpResult::FileData {
            data: hv2_guest_agent::b64::encode(&buf[..got]),
            size,
        }
    }

    /// One entry as `meta` (from `lstat` unless the caller followed) sees it.
    fn dir_entry(name: String, meta: &std::fs::Metadata) -> DirEntry {
        use std::os::unix::fs::MetadataExt;
        let file_type = meta.file_type();
        let kind = if file_type.is_symlink() {
            DirEntryKind::Symlink
        } else if file_type.is_dir() {
            DirEntryKind::Dir
        } else if file_type.is_file() {
            DirEntryKind::File
        } else {
            DirEntryKind::Other
        };
        DirEntry {
            name,
            kind,
            size: meta.size(),
            mode: meta.mode() & 0o7777,
            uid: meta.uid(),
            gid: meta.gid(),
            mtime_secs: meta.mtime(),
        }
    }

    fn fs_failed(what: &str, path: &str, e: &dyn std::fmt::Display) -> OpResult {
        OpResult::Failed {
            message: format!("{what} {path}: {e}"),
        }
    }

    /// An owner as [`Operation::WriteFile`] names one, but an explicit root
    /// is kept: `chown root` is a request, where a write's root is "leave it".
    fn resolve_owner(owner: Option<&str>) -> Result<Option<Account>, String> {
        let owner = match owner {
            Some(TEMPLATE_USER) => template_defaults().user,
            other => other.map(str::to_string),
        };
        owner.as_deref().map(Account::of).transpose()
    }

    /// See [`Operation::ListDir`].
    fn list_dir(path: &str, limit: u32) -> OpResult {
        let cap = match limit {
            0 => MAX_DIR_ENTRIES,
            n => (n as usize).min(MAX_DIR_ENTRIES),
        };
        let reader = match std::fs::read_dir(path) {
            Ok(reader) => reader,
            Err(e) => return fs_failed("listing", path, &e),
        };
        let mut entries = Vec::new();
        let mut weight = 0usize;
        let mut truncated = false;
        for item in reader {
            let item = match item {
                Ok(item) => item,
                Err(e) => return fs_failed("listing", path, &e),
            };
            let name = item.file_name().to_string_lossy().into_owned();
            let meta = match std::fs::symlink_metadata(item.path()) {
                Ok(meta) => meta,
                // Gone between the directory read and the stat.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return fs_failed("listing", path, &e),
            };
            // Fixed keys and numbers, plus the name at JSON's worst escaping.
            let cost = 200 + name.len() * 6;
            if entries.len() >= cap || weight + cost > MAX_DIR_REPLY_BYTES {
                truncated = true;
                break;
            }
            weight += cost;
            entries.push(dir_entry(name, &meta));
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        OpResult::DirEntries { entries, truncated }
    }

    /// See [`Operation::Stat`].
    fn stat_path(path: &str, follow: bool) -> OpResult {
        let meta = if follow {
            std::fs::metadata(path)
        } else {
            std::fs::symlink_metadata(path)
        };
        match meta {
            Ok(meta) => OpResult::FileStat(dir_entry(path.to_string(), &meta)),
            Err(e) => fs_failed("stat", path, &e),
        }
    }

    /// See [`Operation::MakeDir`].
    fn make_dir(path: &str, parents: bool, owner: Option<&str>, mode: Option<u32>) -> OpResult {
        use std::os::unix::fs::PermissionsExt;
        if path.is_empty() {
            return fs_failed("making", "an empty path", &"nothing to make");
        }
        if mode.is_some_and(|m| m > 0o7777) {
            return fs_failed("making", path, &"mode is more than the 0o7777 bits");
        }
        let account = match resolve_owner(owner) {
            Ok(account) => account,
            Err(e) => return fs_failed("owning", path, &e),
        };
        // What this call makes, outermost first: only those are given the
        // owner and mode, never a directory that was already there.
        let mut made = Vec::new();
        if parents {
            let mut missing = Some(std::path::Path::new(path));
            while let Some(dir) = missing
                .filter(|d| !d.as_os_str().is_empty() && std::fs::symlink_metadata(d).is_err())
            {
                made.push(dir.to_path_buf());
                missing = dir.parent();
            }
            made.reverse();
            if let Err(e) = std::fs::create_dir_all(path) {
                return fs_failed("making", path, &e);
            }
        } else {
            if let Err(e) = std::fs::create_dir(path) {
                return fs_failed("making", path, &e);
            }
            made.push(std::path::PathBuf::from(path));
        }
        for dir in &made {
            let shown = dir.display().to_string();
            if let Some(account) = &account {
                if let Err(e) = std::os::unix::fs::chown(dir, Some(account.uid), Some(account.gid))
                {
                    return fs_failed("giving the owner", &shown, &e);
                }
            }
            if let Some(mode) = mode {
                if let Err(e) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
                {
                    return fs_failed("setting the mode of", &shown, &e);
                }
            }
        }
        OpResult::Acknowledged
    }

    /// See [`Operation::Remove`].
    fn remove(path: &str, recursive: bool, missing_ok: bool) -> OpResult {
        let trimmed = path.trim_end_matches('/');
        let last = std::path::Path::new(path).components().next_back();
        if trimmed.is_empty()
            || trimmed == "."
            || trimmed.ends_with("/.")
            || !matches!(last, Some(std::path::Component::Normal(_)))
        {
            return fs_failed(
                "refusing to remove",
                &format!("{path:?}"),
                &"not inside the tree",
            );
        }
        let meta = match std::fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && missing_ok => {
                return OpResult::Acknowledged
            }
            Err(e) => return fs_failed("removing", path, &e),
        };
        // `lstat`, so a link to a directory is a link: removed, not entered.
        // The standard library's recursive removal does not follow links it
        // meets inside the tree either.
        let done = if meta.is_dir() {
            if recursive {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_dir(path)
            }
        } else {
            std::fs::remove_file(path)
        };
        match done {
            Ok(()) => OpResult::Acknowledged,
            Err(e) => fs_failed("removing", path, &e),
        }
    }

    /// See [`Operation::SetAttr`].
    fn set_attr(path: &str, owner: Option<&str>, mode: Option<u32>, recursive: bool) -> OpResult {
        use std::os::unix::fs::PermissionsExt;
        if owner.is_none() && mode.is_none() {
            return fs_failed("changing", path, &"neither an owner nor a mode was given");
        }
        if mode.is_some_and(|m| m > 0o7777) {
            return fs_failed("changing", path, &"mode is more than the 0o7777 bits");
        }
        let account = match resolve_owner(owner) {
            Ok(account) => account,
            Err(e) => return fs_failed("owning", path, &e),
        };
        let mut stack = vec![std::path::PathBuf::from(path)];
        while let Some(entry) = stack.pop() {
            let shown = entry.display().to_string();
            let meta = match std::fs::symlink_metadata(&entry) {
                Ok(meta) => meta,
                Err(e) => return fs_failed("changing", &shown, &e),
            };
            // Read before changing: a mode with no read bit would hide it.
            if recursive && meta.is_dir() {
                match std::fs::read_dir(&entry) {
                    Ok(reader) => {
                        for child in reader {
                            match child {
                                Ok(child) => stack.push(child.path()),
                                Err(e) => return fs_failed("listing", &shown, &e),
                            }
                        }
                    }
                    Err(e) => return fs_failed("listing", &shown, &e),
                }
            }
            // Owner first: changing it clears setuid and setgid.
            if let Some(account) = &account {
                if let Err(e) =
                    std::os::unix::fs::lchown(&entry, Some(account.uid), Some(account.gid))
                {
                    return fs_failed("giving the owner", &shown, &e);
                }
            }
            // A link has no mode, and chmod would change its target's.
            if let Some(mode) = mode.filter(|_| !meta.file_type().is_symlink()) {
                if let Err(e) =
                    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(mode))
                {
                    return fs_failed("setting the mode of", &shown, &e);
                }
            }
        }
        OpResult::Acknowledged
    }

    /// See [`Operation::ReadLink`].
    fn read_link(path: &str) -> OpResult {
        match std::fs::read_link(path) {
            Ok(target) => match target.into_os_string().into_string() {
                Ok(target) => OpResult::LinkTarget { target },
                Err(_) => fs_failed("reading the link", path, &"its target is not valid UTF-8"),
            },
            // EINVAL is "not a symlink"; the OS's own words say so.
            Err(e) => fs_failed("reading the link", path, &e),
        }
    }

    /// See [`Operation::MakeSymlink`].
    fn make_symlink(target: &str, path: &str, owner: Option<&str>) -> OpResult {
        if path.is_empty() {
            return fs_failed("linking", "an empty path", &"nothing to make");
        }
        if target.is_empty() || target.contains('\0') || path.contains('\0') {
            return fs_failed("linking", path, &"empty or NUL-bearing target");
        }
        let account = match resolve_owner(owner) {
            Ok(account) => account,
            Err(e) => return fs_failed("owning", path, &e),
        };
        // `symlink(2)` never follows and fails with EEXIST on anything at all
        // there, a dangling link included.
        if let Err(e) = std::os::unix::fs::symlink(target, path) {
            return fs_failed("linking", path, &e);
        }
        if let Some(account) = account {
            if let Err(e) = std::os::unix::fs::lchown(path, Some(account.uid), Some(account.gid)) {
                // Do not leave a root-owned link where the caller was told none exists.
                let _ = std::fs::remove_file(path);
                return fs_failed("giving the owner", path, &e);
            }
        }
        OpResult::Acknowledged
    }

    /// See [`Operation::SignalProcess`].
    fn signal_process(pid: u32, signal: hv2_guest_agent::SignalName) -> OpResult {
        use hv2_guest_agent::SignalName;
        if pid <= 1 || pid > i32::MAX as u32 {
            return OpResult::Failed {
                message: format!("refusing to signal pid {pid}"),
            };
        }
        if pid == std::process::id() {
            return OpResult::Failed {
                message: "refusing to signal the agent itself".into(),
            };
        }
        let number = match signal {
            SignalName::Stop => libc::SIGSTOP,
            SignalName::Cont => libc::SIGCONT,
            SignalName::Term => libc::SIGTERM,
            SignalName::Hup => libc::SIGHUP,
            SignalName::Int => libc::SIGINT,
        };
        // SAFETY: `kill` with a pid checked above to be neither 0/1 nor this
        // agent, and a signal from a closed set.
        if unsafe { libc::kill(pid as libc::pid_t, number) } == 0 {
            return OpResult::Acknowledged;
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            OpResult::NoSuchProcess
        } else {
            OpResult::Failed {
                message: format!("signalling pid {pid}: {error}"),
            }
        }
    }

    /// See [`Operation::MountBlock`].
    fn mount_block(
        device: &str,
        target: &str,
        fstype: hv2_guest_agent::MountFsType,
        expect_sectors: Option<u64>,
        drop_caches: bool,
    ) -> OpResult {
        let name = match hv2_guest_agent::validate_mount_block(device, target) {
            Ok(name) => name,
            Err(message) => return OpResult::Failed { message },
        };
        if let Some(sectors) = expect_sectors {
            let size_file = format!("/sys/class/block/{name}/size");
            let mut seen = false;
            // 200 x 10 ms: how long the shell script this replaces waited.
            for _ in 0..=200 {
                let size = std::fs::read_to_string(&size_file)
                    .ok()
                    .and_then(|text| text.trim().parse::<u64>().ok());
                if size == Some(sectors) {
                    seen = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if !seen {
                return OpResult::DeviceSizeNotSeen;
            }
        }
        if drop_caches {
            // SAFETY: `sync` takes no arguments and touches no memory of ours.
            unsafe { libc::sync() };
            if let Err(e) = std::fs::write("/proc/sys/vm/drop_caches", b"1") {
                return fs_failed("dropping caches before mounting", device, &e);
            }
        }
        if let Err(e) = std::fs::create_dir_all(target) {
            return fs_failed("creating the mount point", target, &e);
        }
        let (Ok(source), Ok(mount_point), Ok(fs)) = (
            std::ffi::CString::new(device),
            std::ffi::CString::new(target),
            std::ffi::CString::new(fstype.as_str()),
        ) else {
            return fs_failed("mounting", device, &"a NUL in an argument");
        };
        // SAFETY: three valid NUL-terminated strings that outlive the call; no
        // flags and no filesystem-specific data.
        let rc = unsafe {
            libc::mount(
                source.as_ptr(),
                mount_point.as_ptr(),
                fs.as_ptr(),
                0,
                std::ptr::null(),
            )
        };
        if rc == 0 {
            OpResult::Acknowledged
        } else {
            let error = std::io::Error::last_os_error();
            OpResult::Failed {
                message: format!(
                    "mounting {device} at {target} as {}: {error}",
                    fstype.as_str()
                ),
            }
        }
    }

    /// See [`Operation::Restored`]: clock first, then a forced reseed.
    fn restored(unix_time_ns: u64, entropy: &[u8]) -> OpResult {
        let now = libc::timespec {
            tv_sec: (unix_time_ns / 1_000_000_000) as libc::time_t,
            tv_nsec: (unix_time_ns % 1_000_000_000) as libc::c_long,
        };
        if unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &now) } != 0 {
            return OpResult::Failed {
                message: format!("setting the clock: {}", std::io::Error::last_os_error()),
            };
        }
        if entropy.is_empty() || entropy.len() > 512 {
            return OpResult::Failed {
                message: format!("{} bytes of entropy; want 1-512", entropy.len()),
            };
        }

        // `struct rand_pool_info`: entropy bits credited, buffer length, buffer.
        let mut info = Vec::with_capacity(8 + entropy.len());
        info.extend_from_slice(&((entropy.len() * 8) as i32).to_ne_bytes());
        info.extend_from_slice(&(entropy.len() as i32).to_ne_bytes());
        info.extend_from_slice(entropy);

        // _IOW('R', 0x03, int[2]) and _IO('R', 0x07).
        const RNDADDENTROPY: libc::c_ulong = 0x4008_5203;
        const RNDRESEEDCRNG: libc::c_ulong = 0x5207;
        let path = b"/dev/urandom\0";
        let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_WRONLY | libc::O_CLOEXEC) };
        if fd < 0 {
            return OpResult::Failed {
                message: format!("opening /dev/urandom: {}", std::io::Error::last_os_error()),
            };
        }
        let added = unsafe { libc::ioctl(fd, RNDADDENTROPY as _, info.as_ptr()) };
        let added_err = std::io::Error::last_os_error();
        let reseeded = unsafe { libc::ioctl(fd, RNDRESEEDCRNG as _, 0) };
        let reseed_err = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        if added != 0 {
            return OpResult::Failed {
                message: format!("RNDADDENTROPY: {added_err}"),
            };
        }
        if reseeded != 0 {
            return OpResult::Failed {
                message: format!("RNDRESEEDCRNG: {reseed_err}"),
            };
        }
        OpResult::Acknowledged
    }

    /// How a program ended: its exit code, and the signal that killed it.
    ///
    /// Both are optional and neither implies the other -- a program killed by
    /// SIGKILL has no exit code of its own, and flattening the two would
    /// report a kill as a clean exit.
    type Ended = Option<(Option<i32>, Option<i32>)>;

    /// A program started by [`Operation::Start`] and still owned by this agent.
    ///
    /// The pipes are drained by their own threads rather than read on demand.
    /// A pipe has a fixed kernel buffer, and a program that fills it blocks
    /// forever writing -- so "read it when the host asks" would hang exactly
    /// the chatty programs streaming exists for.
    struct Proc {
        /// The pty master, for a program started with one. Both ends of the
        /// conversation: what the program writes is read here, and what is
        /// written here arrives as the program's input. A pty has no separate
        /// stdin, which is why this is not the field below.
        pty: Option<std::fs::File>,
        stdin: Option<std::process::ChildStdin>,
        stdout: Arc<Mutex<Pending>>,
        stderr: Arc<Mutex<Pending>>,
        /// `Some` once the program has finished, with its exit code and the
        /// signal that ended it, kept apart because exiting 0 and being killed
        /// are not the same outcome.
        finished: Arc<Mutex<Ended>>,
    }

    /// Every program this agent started and has not yet been asked to forget.
    ///
    /// A process-wide table because `handle` is called per request and the
    /// whole point is that a process outlives the request that started it.
    fn procs() -> &'static Mutex<HashMap<u32, Proc>> {
        static PROCS: OnceLock<Mutex<HashMap<u32, Proc>>> = OnceLock::new();
        PROCS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// What a started program printed on one stream and no poll has taken
    /// yet, and how many bytes were dropped to keep it bounded since the last
    /// poll took any.
    #[derive(Default)]
    struct Pending {
        bytes: Vec<u8>,
        dropped: u64,
    }

    /// Drain a pipe into a buffer until it closes.
    fn drain<R: Read + Send + 'static>(
        mut source: R,
        into: Arc<Mutex<Pending>>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match source.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        let mut buf = match into.lock() {
                            Ok(buf) => buf,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        // Bounded like `exec`'s output: a program that prints
                        // forever must not grow this agent until the guest is
                        // out of memory. Oldest bytes go first, because the
                        // newest are the ones a caller is waiting on -- and
                        // are counted, so the caller knows.
                        if buf.bytes.len() + n > MAX_OUTPUT_BYTES {
                            let over = buf.bytes.len() + n - MAX_OUTPUT_BYTES;
                            let drop_to = past_partial(&buf.bytes, over.min(buf.bytes.len()));
                            buf.bytes.drain(..drop_to);
                            buf.dropped += drop_to as u64;
                        }
                        buf.bytes.extend_from_slice(&chunk[..n]);
                    }
                }
            }
        })
    }

    /// `at`, or the first byte after it that does not continue a UTF-8
    /// character: where to cut so no character is cut in two.
    fn past_partial(bytes: &[u8], mut at: usize) -> usize {
        while at < bytes.len() && (bytes[at] & 0xC0) == 0x80 {
            at += 1;
        }
        at
    }

    /// Take what is buffered and how many bytes were dropped since the last
    /// take. A character the program has only begun to write stays for the
    /// next take, unless `last`, when the program has ended and nothing will
    /// finish it.
    fn take(buf: &Arc<Mutex<Pending>>, last: bool) -> (String, u64) {
        let mut guard = match buf.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let dropped = std::mem::take(&mut guard.dropped);
        let whole = match std::str::from_utf8(&guard.bytes) {
            Err(e) if !last && e.error_len().is_none() => e.valid_up_to(),
            _ => guard.bytes.len(),
        };
        let bytes: Vec<u8> = guard.bytes.drain(..whole).collect();
        (String::from_utf8_lossy(&bytes).into_owned(), dropped)
    }

    /// Open a pseudo-terminal, returning (master, slave).
    ///
    /// Built from `posix_openpt` and friends rather than `openpty`, which
    /// lives in libutil: this agent is linked static-pie against glibc for a
    /// guest with no shared libraries, and depending on one more library to
    /// get one convenience function is a way to not link at all.
    ///
    /// # Safety
    ///
    /// Every call is a libc call with checked arguments; each return value is
    /// tested before the next call uses it.
    fn open_pty(size: PtySize) -> std::io::Result<(std::fs::File, std::fs::File)> {
        use std::os::fd::FromRawFd;

        // SAFETY: no arguments but flags; -1 on failure, which is checked.
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        if master < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `master` is a valid fd, checked above. Wrapped now so that
        // every early return below closes it rather than leaking it.
        let master = unsafe { std::fs::File::from_raw_fd(master) };
        let master_fd = std::os::fd::AsRawFd::as_raw_fd(&master);

        // SAFETY: valid fd. `grantpt`/`unlockpt` are what make the slave
        // openable; without them the open below fails with EIO.
        if unsafe { libc::grantpt(master_fd) } < 0 || unsafe { libc::unlockpt(master_fd) } < 0 {
            return Err(std::io::Error::last_os_error());
        }

        let mut name = [0 as libc::c_char; 128];
        // SAFETY: valid fd, and a buffer whose real length is passed.
        let named = unsafe { libc::ptsname_r(master_fd, name.as_mut_ptr(), name.len()) };
        if named != 0 {
            return Err(std::io::Error::from_raw_os_error(named));
        }
        // SAFETY: `ptsname_r` returning 0 means `name` holds a NUL-terminated
        // path within the buffer.
        let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(std::ffi::OsStr::from_bytes(path.to_bytes()))?;

        set_pty_size(master_fd, size)?;
        Ok((master, slave))
    }

    /// Tell a pty how big it is.
    fn set_pty_size(master_fd: libc::c_int, size: PtySize) -> std::io::Result<()> {
        let winsize = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: a valid fd and a correctly typed `winsize` for TIOCSWINSZ.
        if unsafe { libc::ioctl(master_fd, libc::TIOCSWINSZ, &raw const winsize) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Start a program and keep it.
    /// This template's defaults, read fresh: a template build writes them
    /// while this agent runs, and every sandbox restored from it keeps this
    /// same agent. None, if the template was not built by steps.
    fn template_defaults() -> TemplateDefaults {
        std::fs::read(TEMPLATE_DEFAULTS_PATH)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Give a program the signals a program expects to start with.
    ///
    /// This agent may have been started with some of them ignored: a shell
    /// without job control ignores SIGINT and SIGQUIT in whatever it starts
    /// with `&`, which is how an init script starts a daemon and how the
    /// sandbox guest's starts this one. What is ignored stays ignored across
    /// `exec`, and a shell cannot trap a signal that was ignored when it
    /// started. So an interrupt sent to a program started here was accepted
    /// and did nothing, and a Ctrl-C at a terminal did nothing to a program
    /// with no handler of its own.
    fn default_signals(command: &mut Command) {
        // SAFETY: runs in the child between fork and exec, and calls only
        // `signal`, which is async-signal-safe.
        unsafe {
            command.pre_exec(|| {
                for signal in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP] {
                    libc::signal(signal, libc::SIG_DFL);
                }
                Ok(())
            });
        }
    }

    fn start(
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        envs: &BTreeMap<String, String>,
        pty: Option<PtySize>,
        user: Option<&str>,
    ) -> OpResult {
        let mut command = Command::new(program);
        command.args(args);
        default_signals(&mut command);
        // Added to the agent's own environment rather than replacing it:
        // `env_clear` would leave the program without a `PATH`, and the first
        // thing most of them do is look something up in it. The template's
        // defaults go under the request's own.
        let defaults = template_defaults();
        let account = match user
            .or(defaults.user.as_deref())
            .map(Account::of)
            .transpose()
        {
            Ok(account) => account.filter(|a| a.uid != 0),
            Err(message) => return OpResult::Failed { message },
        };
        if let Some(account) = &account {
            command
                .env("HOME", &account.home)
                .env("USER", &account.name)
                .env("LOGNAME", &account.name);
        }
        command.envs(&defaults.env);
        command.envs(envs);
        let home = account.as_ref().map(|a| a.home.as_str());
        if let Some(dir) = cwd
            .or(defaults.cwd.as_deref())
            .or(home.filter(|h| std::path::Path::new(h).is_dir()))
        {
            command.current_dir(dir);
        }
        if let Some(account) = account {
            let (uid, gid, groups) = (account.uid, account.gid, account.groups);
            // SAFETY: runs in the child between fork and exec, and calls only
            // async-signal-safe functions. Groups before the gid, the gid
            // before the uid: once the uid is dropped, neither may change.
            unsafe {
                command.pre_exec(move || {
                    if libc::setgroups(groups.len(), groups.as_ptr()) < 0
                        || libc::setgid(gid) < 0
                        || libc::setuid(uid) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        // Opened before the fork so a failure is reported as a failure to
        // start, rather than leaving a child running against a terminal
        // nothing holds.
        let pty_pair = match pty {
            Some(size) => match open_pty(size) {
                Ok(pair) => Some(pair),
                Err(e) => {
                    return OpResult::Failed {
                        message: format!("could not open a terminal for {program}: {e}"),
                    }
                }
            },
            None => None,
        };

        if let Some((_, slave)) = pty_pair.as_ref() {
            use std::os::fd::AsRawFd;
            let slave_fd = slave.as_raw_fd();
            command
                .stdin(duplicate(slave_fd))
                .stdout(duplicate(slave_fd))
                .stderr(duplicate(slave_fd));
            // SAFETY: runs in the child between fork and exec, so it may call
            // only async-signal-safe functions; `setsid` and `ioctl` are.
            //
            // Both are needed and neither is optional: `setsid` makes the
            // child a session leader, which is a precondition for having a
            // controlling terminal at all, and `TIOCSCTTY` then makes this pty
            // that terminal. Without them the program has a terminal on its
            // file descriptors but no *controlling* one, so Ctrl-C sends no
            // signal and a shell reports "no job control".
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        } else {
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                return OpResult::Failed {
                    message: format!("could not start {program}: {e}"),
                }
            }
        };

        let pid = child.id();
        let stdout = Arc::new(Mutex::new(Pending::default()));
        let stderr = Arc::new(Mutex::new(Pending::default()));
        let mut readers = Vec::new();

        // The parent's copy of the slave is dropped here. It has to be: the
        // master reads end-of-file only when *every* slave handle is closed,
        // and holding one would make a finished program look like it was still
        // producing nothing forever.
        let (pty_master, pty_stdin) = match pty_pair {
            Some((master, slave)) => {
                drop(slave);
                match master.try_clone() {
                    Ok(reader) => {
                        // stderr stays empty for a pty: a terminal has one
                        // stream, and inventing a split would mean guessing.
                        readers.push(drain(reader, Arc::clone(&stdout)));
                        (Some(master), None)
                    }
                    Err(e) => {
                        return OpResult::Failed {
                            message: format!("could not read the terminal for {program}: {e}"),
                        }
                    }
                }
            }
            None => {
                if let Some(pipe) = child.stdout.take() {
                    readers.push(drain(pipe, Arc::clone(&stdout)));
                }
                if let Some(pipe) = child.stderr.take() {
                    readers.push(drain(pipe, Arc::clone(&stderr)));
                }
                // Taken before `child` moves into the waiter below, or there
                // would be no way to write to the program after starting it --
                // which is most of the point of starting it this way.
                (None, child.stdin.take())
            }
        };
        let stdin = pty_stdin;

        // Reaped on its own thread. Without this the process becomes a zombie
        // the moment it exits, and `Poll` would report it running forever.
        let finished = Arc::new(Mutex::new(None));
        let done = Arc::clone(&finished);
        std::thread::spawn(move || {
            let status = child.wait();
            // Publish exit only after both streams have reached EOF. Otherwise
            // a fast host poll can discard bytes still held by a reader.
            for reader in readers {
                let _ = reader.join();
            }
            let mut slot = match done.lock() {
                Ok(slot) => slot,
                Err(poisoned) => poisoned.into_inner(),
            };
            *slot = Some(match status {
                Ok(status) => (status.code(), status.signal()),
                Err(_) => (None, None),
            });
        });

        let mut table = match procs().lock() {
            Ok(table) => table,
            Err(poisoned) => poisoned.into_inner(),
        };
        table.insert(
            pid,
            Proc {
                pty: pty_master,
                stdin,
                stdout,
                stderr,
                finished,
            },
        );

        OpResult::Started { pid }
    }

    /// Collect what a started program has printed since the last poll.
    fn poll(pid: u32) -> OpResult {
        let table = match procs().lock() {
            Ok(table) => table,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(proc) = table.get(&pid) else {
            return OpResult::Failed {
                message: format!("no started process with pid {pid}"),
            };
        };

        let finished = match proc.finished.lock() {
            Ok(slot) => *slot,
            Err(poisoned) => *poisoned.into_inner(),
        };
        // Observe completion before draining: completion guarantees all final
        // bytes are buffered, and a later completion waits for another poll.
        let (stdout, stdout_dropped) = take(&proc.stdout, finished.is_some());
        let (stderr, stderr_dropped) = take(&proc.stderr, finished.is_some());

        let pty = proc.pty.is_some();
        match finished {
            None => OpResult::Output {
                stdout,
                stderr,
                pty,
                running: true,
                exit_code: None,
                signal: None,
                stdout_dropped,
                stderr_dropped,
            },
            Some((exit_code, signal)) => OpResult::Output {
                stdout,
                stderr,
                pty,
                running: false,
                exit_code,
                signal,
                stdout_dropped,
                stderr_dropped,
            },
        }
    }

    /// Write to a started program's standard input.
    fn write_stdin(pid: u32, data: &str, close: bool) -> OpResult {
        let mut table = match procs().lock() {
            Ok(table) => table,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(proc) = table.get_mut(&pid) else {
            return OpResult::Failed {
                message: format!("no started process with pid {pid}"),
            };
        };
        // A pty takes input through the master, not a separate stdin.
        if let Some(master) = proc.pty.as_mut() {
            if let Err(e) = master
                .write_all(data.as_bytes())
                .and_then(|()| master.flush())
            {
                return OpResult::Failed {
                    message: format!("writing to the terminal of pid {pid}: {e}"),
                };
            }
            if close {
                // 0x04 is Ctrl-D, which is how end-of-input is expressed on a
                // terminal. Closing the master instead would tear the terminal
                // down under the program rather than telling it the input has
                // ended -- the proto says as much: "Only works for non-PTY
                // processes. For PTY, send Ctrl+D (0x04) instead."
                if let Err(e) = master.write_all(&[0x04]).and_then(|()| master.flush()) {
                    return OpResult::Failed {
                        message: format!("ending input for pid {pid}: {e}"),
                    };
                }
            }
            return OpResult::Acknowledged;
        }

        let Some(pipe) = proc.stdin.as_mut() else {
            return OpResult::Failed {
                message: format!("stdin for pid {pid} is already closed"),
            };
        };
        if let Err(e) = pipe.write_all(data.as_bytes()).and_then(|()| pipe.flush()) {
            return OpResult::Failed {
                message: format!("writing to pid {pid}: {e}"),
            };
        }
        if close {
            // Dropping the pipe is what sends EOF, and a program waiting on
            // end-of-input never finishes without it.
            proc.stdin = None;
        }
        OpResult::Acknowledged
    }

    /// Tell a program's terminal it is a different size.
    fn resize_pty(pid: u32, size: PtySize) -> OpResult {
        let table = match procs().lock() {
            Ok(table) => table,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(proc) = table.get(&pid) else {
            return OpResult::Failed {
                message: format!("no started process with pid {pid}"),
            };
        };
        let Some(master) = proc.pty.as_ref() else {
            return OpResult::Failed {
                message: format!("pid {pid} has no terminal to resize; it was started with pipes"),
            };
        };
        match set_pty_size(std::os::fd::AsRawFd::as_raw_fd(master), size) {
            Ok(()) => OpResult::Acknowledged,
            Err(e) => OpResult::Failed {
                message: format!("resizing the terminal of pid {pid}: {e}"),
            },
        }
    }

    /// Duplicate a file descriptor into an owned `Stdio`.
    ///
    /// Each of the child's three descriptors needs its own, because `Stdio`
    /// takes ownership and the same pty slave has to serve all three.
    fn duplicate(fd: libc::c_int) -> Stdio {
        use std::os::fd::FromRawFd;
        // SAFETY: `fd` is open for as long as the caller holds the slave file,
        // which outlives this call. A failed `dup` gives -1, which
        // `Stdio::from_raw_fd` would treat as a real descriptor, so that case
        // becomes a closed stream instead -- the child then fails to start,
        // which is reported, rather than reading from an arbitrary fd.
        let copy = unsafe { libc::dup(fd) };
        if copy < 0 {
            return Stdio::null();
        }
        // SAFETY: `copy` is a fresh descriptor this call owns.
        unsafe { Stdio::from_raw_fd(copy) }
    }

    /// Send a signal to a started program.
    fn signal(pid: u32, signal: i32) -> OpResult {
        {
            let table = match procs().lock() {
                Ok(table) => table,
                Err(poisoned) => poisoned.into_inner(),
            };
            if !table.contains_key(&pid) {
                // Only signal what this agent started. A pid the host names
                // from somewhere else could be any process in the guest,
                // including this agent.
                return OpResult::Failed {
                    message: format!("no started process with pid {pid}"),
                };
            }
        }

        // SAFETY: `kill` with a pid this agent started and a signal number
        // from the host. Checked above that the pid is one of ours.
        let sent = unsafe { libc::kill(pid as libc::pid_t, signal) };
        if sent == 0 {
            OpResult::Acknowledged
        } else {
            OpResult::Failed {
                message: format!("signalling pid {pid}: {}", std::io::Error::last_os_error()),
            }
        }
    }

    /// Run a program, bounded by `timeout_ms`.
    fn exec(
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        stdin: Option<&str>,
        timeout_ms: u64,
        user: Option<&str>,
    ) -> OpResult {
        let mut command = Command::new(program);
        default_signals(&mut command);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let defaults = template_defaults();
        // Only the user the caller names: the template's is for programs a
        // person starts, and the host's own commands need to be root's.
        let account = match user.map(Account::of).transpose() {
            Ok(account) => account.filter(|a| a.uid != 0),
            Err(message) => return OpResult::Failed { message },
        };
        if let Some(account) = &account {
            command
                .env("HOME", &account.home)
                .env("USER", &account.name)
                .env("LOGNAME", &account.name);
        }
        command.envs(&defaults.env);
        let home = account.as_ref().map(|a| a.home.as_str());
        if let Some(dir) = cwd
            .or(defaults.cwd.as_deref())
            .or(home.filter(|h| std::path::Path::new(h).is_dir()))
        {
            command.current_dir(dir);
        }
        if let Some(account) = account {
            let (uid, gid, groups) = (account.uid, account.gid, account.groups);
            // SAFETY: as in `start`: only async-signal-safe calls between
            // fork and exec, groups before the gid, the gid before the uid.
            unsafe {
                command.pre_exec(move || {
                    if libc::setgroups(groups.len(), groups.as_ptr()) < 0
                        || libc::setgid(gid) < 0
                        || libc::setuid(uid) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                return OpResult::Failed {
                    message: format!("could not start {program}: {e}"),
                }
            }
        };

        if let Some(input) = stdin {
            if let Some(mut pipe) = child.stdin.take() {
                let _ = pipe.write_all(input.as_bytes());
            }
        } else {
            // Close stdin, or a program that reads it waits for a write that
            // is never coming and then hits the timeout for the wrong reason.
            drop(child.stdin.take());
        }

        let pid = child.id() as libc::pid_t;

        // wait_with_output drains both pipes, which is what keeps a chatty
        // program from filling a pipe buffer and blocking forever. It has no
        // timeout, so it waits on another thread and this one enforces one.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });

        let (output, timed_out) = match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(Ok(output)) => (output, false),
            Ok(Err(e)) => {
                return OpResult::Failed {
                    message: format!("waiting for {program} failed: {e}"),
                }
            }
            Err(_) => {
                // Kill it and take whatever it managed to print. Reporting a
                // timeout with no output at all would lose the half-finished
                // work that usually explains the hang.
                unsafe { libc::kill(pid, libc::SIGKILL) };
                match rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(Ok(output)) => (output, true),
                    _ => {
                        return OpResult::Exited {
                            exit_code: None,
                            signal: Some(libc::SIGKILL),
                            stdout: String::new(),
                            stderr: String::new(),
                            truncated: false,
                            timed_out: true,
                        }
                    }
                }
            }
        };

        let (stdout, out_cut) =
            truncate_utf8(&String::from_utf8_lossy(&output.stdout), MAX_OUTPUT_BYTES);
        let (stderr, err_cut) =
            truncate_utf8(&String::from_utf8_lossy(&output.stderr), MAX_OUTPUT_BYTES);

        OpResult::Exited {
            exit_code: output.status.code(),
            signal: output.status.signal(),
            stdout,
            stderr,
            truncated: out_cut || err_cut,
            timed_out,
        }
    }

    #[cfg(test)]
    mod filesystem_op_tests {
        use super::*;
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::path::{Path, PathBuf};

        fn scratch(name: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("hv2-agentd-fs-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn s(p: &Path) -> &str {
            p.to_str().unwrap()
        }

        fn failure(result: OpResult) -> String {
            match result {
                OpResult::Failed { message } => message,
                other => panic!("expected a failure, got {other:?}"),
            }
        }

        fn mode(p: &Path) -> u32 {
            std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
        }

        #[test]
        fn list_dir_reports_each_kind_without_following_links() {
            let root = scratch("list");
            std::fs::write(root.join("f"), b"hello").unwrap();
            std::fs::create_dir(root.join("d")).unwrap();
            symlink("/etc", root.join("l")).unwrap();
            let OpResult::DirEntries { entries, truncated } = list_dir(s(&root), 0) else {
                panic!("not a listing")
            };
            assert!(!truncated);
            let seen: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.kind)).collect();
            assert_eq!(
                seen,
                [
                    ("d", DirEntryKind::Dir),
                    ("f", DirEntryKind::File),
                    ("l", DirEntryKind::Symlink)
                ]
            );
            assert_eq!(entries[1].size, 5);
            assert_eq!(entries[1].mode & 0o600, 0o600);
            assert!(failure(list_dir(s(&root.join("f")), 0)).contains("listing"));
            assert!(failure(list_dir(s(&root.join("nope")), 0)).contains("No such file"));
        }

        #[test]
        fn list_dir_truncates_at_the_limit_and_the_cap() {
            let root = scratch("trunc");
            for i in 0..5 {
                std::fs::write(root.join(format!("f{i}")), b"").unwrap();
            }
            let OpResult::DirEntries { entries, truncated } = list_dir(s(&root), 3) else {
                panic!("not a listing")
            };
            assert_eq!((entries.len(), truncated), (3, true));
            let OpResult::DirEntries { entries, truncated } = list_dir(s(&root), 5) else {
                panic!("not a listing")
            };
            assert_eq!((entries.len(), truncated), (5, false));
            // The cap is a constant no limit can raise.
            let big = scratch("trunc-big");
            for i in 0..(MAX_DIR_ENTRIES + 5) {
                std::fs::write(big.join(format!("{i:05}")), b"").unwrap();
            }
            let OpResult::DirEntries { entries, truncated } = list_dir(s(&big), u32::MAX) else {
                panic!("not a listing")
            };
            assert_eq!((entries.len(), truncated), (MAX_DIR_ENTRIES, true));
            let answer = Response {
                id: 1,
                version: PROTOCOL_VERSION,
                result: OpResult::DirEntries { entries, truncated },
            };
            assert!(encode(&answer).unwrap().len() < MAX_FRAME_BYTES);
        }

        #[test]
        fn stat_follows_a_link_only_when_asked() {
            let root = scratch("stat");
            std::fs::create_dir(root.join("d")).unwrap();
            symlink(root.join("d"), root.join("l")).unwrap();
            let OpResult::FileStat(link) = stat_path(s(&root.join("l")), false) else {
                panic!("no stat")
            };
            assert_eq!(link.kind, DirEntryKind::Symlink);
            assert_eq!(link.name, s(&root.join("l")));
            let OpResult::FileStat(target) = stat_path(s(&root.join("l")), true) else {
                panic!("no stat")
            };
            assert_eq!(target.kind, DirEntryKind::Dir);
            assert!(failure(stat_path(s(&root.join("x")), false)).contains("No such file"));
            // A dangling link is a link to lstat and an error to stat.
            symlink(root.join("gone"), root.join("dangling")).unwrap();
            assert!(matches!(
                stat_path(s(&root.join("dangling")), false),
                OpResult::FileStat(_)
            ));
            assert!(matches!(
                stat_path(s(&root.join("dangling")), true),
                OpResult::Failed { .. }
            ));
        }

        #[test]
        fn make_dir_needs_parents_to_make_them() {
            let root = scratch("mkdir");
            let deep = root.join("a/b/c");
            assert!(failure(make_dir(s(&deep), false, None, None)).contains("No such file"));
            assert_eq!(
                make_dir(s(&deep), true, None, Some(0o750)),
                OpResult::Acknowledged
            );
            for dir in ["a", "a/b", "a/b/c"] {
                assert_eq!(mode(&root.join(dir)), 0o750, "{dir}");
            }
            // Existing: fine with parents, an error without, and untouched.
            assert_eq!(
                make_dir(s(&deep), true, None, Some(0o700)),
                OpResult::Acknowledged
            );
            assert_eq!(mode(&deep), 0o750);
            assert!(failure(make_dir(s(&deep), false, None, None)).contains("exists"));
            std::fs::write(root.join("file"), b"").unwrap();
            assert!(matches!(
                make_dir(s(&root.join("file")), true, None, None),
                OpResult::Failed { .. }
            ));
            assert!(matches!(
                make_dir(s(&root.join("m")), false, None, Some(0o10000)),
                OpResult::Failed { .. }
            ));
        }

        #[test]
        fn remove_recursive_does_not_follow_a_link_out_of_the_tree() {
            let root = scratch("rm");
            let outside = root.join("outside");
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(outside.join("keep"), b"x").unwrap();
            let tree = root.join("tree");
            std::fs::create_dir_all(tree.join("sub")).unwrap();
            std::fs::write(tree.join("sub/f"), b"x").unwrap();
            symlink(&outside, tree.join("sub/link")).unwrap();
            assert!(matches!(
                remove(s(&tree), false, false),
                OpResult::Failed { .. }
            ));
            assert_eq!(remove(s(&tree), true, false), OpResult::Acknowledged);
            assert!(!tree.exists());
            assert!(outside.join("keep").exists());
            // A link at the top is removed itself.
            symlink(&outside, root.join("top")).unwrap();
            assert_eq!(
                remove(s(&root.join("top")), true, false),
                OpResult::Acknowledged
            );
            assert!(std::fs::symlink_metadata(root.join("top")).is_err());
            assert!(outside.join("keep").exists());
        }

        #[test]
        fn remove_refuses_the_root_and_reports_a_missing_path() {
            for path in ["", "/", "//", "/.", "/..", ".", "..", "/tmp/..", "/etc/."] {
                let message = failure(remove(path, true, true));
                assert!(message.contains("refusing"), "{path:?}: {message}");
            }
            let root = scratch("rm-missing");
            let gone = s(&root.join("gone")).to_string();
            assert!(failure(remove(&gone, false, false)).contains("No such file"));
            assert_eq!(remove(&gone, false, true), OpResult::Acknowledged);
            std::fs::write(root.join("f"), b"").unwrap();
            assert_eq!(
                remove(s(&root.join("f")), false, false),
                OpResult::Acknowledged
            );
        }

        #[test]
        fn set_attr_changes_modes_through_a_tree_but_not_through_links() {
            let root = scratch("attr");
            let outside = root.join("outside");
            std::fs::write(&outside, b"x").unwrap();
            std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
            let tree = root.join("tree");
            std::fs::create_dir_all(tree.join("sub")).unwrap();
            std::fs::write(tree.join("sub/f"), b"x").unwrap();
            symlink(&outside, tree.join("sub/link")).unwrap();
            assert_eq!(
                set_attr(s(&tree), None, Some(0o750), true),
                OpResult::Acknowledged
            );
            assert_eq!(mode(&tree), 0o750);
            assert_eq!(mode(&tree.join("sub")), 0o750);
            assert_eq!(mode(&tree.join("sub/f")), 0o750);
            assert_eq!(mode(&outside), 0o600);
            // Not recursive: only the named path.
            assert_eq!(
                set_attr(s(&tree), None, Some(0o700), false),
                OpResult::Acknowledged
            );
            assert_eq!((mode(&tree), mode(&tree.join("sub"))), (0o700, 0o750));
            assert!(failure(set_attr(s(&tree), None, None, false)).contains("neither"));
            assert!(
                failure(set_attr(s(&root.join("x")), None, Some(0o700), false))
                    .contains("No such file")
            );
        }

        #[test]
        fn set_attr_and_make_dir_give_an_owner() {
            use std::os::unix::fs::MetadataExt;
            // Giving files away takes root; elsewhere there is nothing to check.
            if unsafe { libc::geteuid() } != 0 {
                return;
            }
            let root = scratch("owner");
            let dir = root.join("a/b");
            assert_eq!(
                make_dir(s(&dir), true, Some("0"), None),
                OpResult::Acknowledged
            );
            std::fs::write(root.join("f"), b"").unwrap();
            std::os::unix::fs::chown(root.join("f"), Some(12345), Some(12345)).unwrap();
            assert_eq!(
                set_attr(s(&root.join("f")), Some("root"), None, false),
                OpResult::Acknowledged
            );
            let meta = std::fs::metadata(root.join("f")).unwrap();
            assert_eq!((meta.uid(), meta.gid()), (0, 0));
            assert!(
                failure(set_attr(s(&root), Some("no-such-user-xyz"), None, false))
                    .contains("no user")
            );
        }

        #[test]
        fn mount_block_refuses_what_it_must_before_touching_anything() {
            use hv2_guest_agent::MountFsType::Ext4;
            let root = scratch("mountblock");
            let target = format!("{}/mnt", s(&root));
            for (device, target) in [
                ("/dev/sda", target.as_str()),
                ("/dev/vda/../sda", target.as_str()),
                ("/dev/vda", "relative"),
                ("/dev/vda", "/a/../b"),
                ("/dev/vda", "/"),
            ] {
                let message = failure(mount_block(device, target, Ext4, None, false));
                assert!(message.contains("refusing"), "{message}");
            }
            assert!(
                !root.join("mnt").exists(),
                "a refusal must not create the target"
            );
        }

        #[test]
        fn mount_block_gives_up_when_the_device_never_has_the_size() {
            use hv2_guest_agent::MountFsType::Ext4;
            // No disk of this many sectors exists on any machine running this.
            let result = mount_block("/dev/vdz", "/mnt/never", Ext4, Some(u64::MAX), true);
            assert_eq!(result, OpResult::DeviceSizeNotSeen);
            assert!(!Path::new("/mnt/never").exists());
        }

        #[test]
        fn mount_block_reports_the_os_error_when_there_is_no_such_device() {
            use hv2_guest_agent::MountFsType::Ext4;
            // Needs root to get past mkdir/mount permission; without it the
            // failure is a permission error, which is also reported, not hidden.
            let root = scratch("mountblock-nodev");
            let target = format!("{}/mnt", s(&root));
            let message = failure(mount_block("/dev/vdz9", &target, Ext4, None, false));
            assert!(message.contains("mounting /dev/vdz9"), "{message}");
        }

        #[test]
        fn read_link_returns_the_target_verbatim_and_refuses_non_links() {
            let root = scratch("readlink");
            symlink("../x/./y", root.join("l")).unwrap();
            symlink("/nowhere", root.join("dangling")).unwrap();
            std::fs::write(root.join("f"), b"").unwrap();
            assert_eq!(
                read_link(s(&root.join("l"))),
                OpResult::LinkTarget {
                    target: "../x/./y".into()
                }
            );
            assert_eq!(
                read_link(s(&root.join("dangling"))),
                OpResult::LinkTarget {
                    target: "/nowhere".into()
                }
            );
            assert!(failure(read_link(s(&root.join("f")))).contains("reading the link"));
            assert!(failure(read_link(s(&root.join("nope")))).contains("No such file"));
            // Not UTF-8: refused, not converted into some other path.
            let odd = std::ffi::OsStr::from_bytes(b"bad-\xff-target");
            symlink(odd, root.join("odd")).unwrap();
            assert!(failure(read_link(s(&root.join("odd")))).contains("UTF-8"));
        }

        #[test]
        fn make_symlink_does_not_follow_and_does_not_replace() {
            let root = scratch("mksymlink");
            std::fs::create_dir(root.join("real")).unwrap();
            assert_eq!(
                make_symlink("real", s(&root.join("l")), None),
                OpResult::Acknowledged
            );
            assert_eq!(
                std::fs::read_link(root.join("l")).unwrap(),
                Path::new("real")
            );
            // An existing path, a symlink (even dangling) and a missing parent all fail.
            assert!(failure(make_symlink("x", s(&root.join("l")), None)).contains("exists"));
            symlink("/nowhere", root.join("dangling")).unwrap();
            assert!(failure(make_symlink("x", s(&root.join("dangling")), None)).contains("exists"));
            assert!(failure(make_symlink("x", s(&root.join("a/b")), None)).contains("No such"));
            // The link at `l` is not followed to put a new one inside `real`.
            assert!(failure(make_symlink("x", s(&root.join("l")), None)).contains("exists"));
            assert!(!root.join("real/l").exists());
            assert!(failure(make_symlink("", s(&root.join("e")), None)).contains("empty"));
        }

        #[test]
        fn make_symlink_gives_the_link_an_owner_without_touching_its_target() {
            use std::os::unix::fs::MetadataExt;
            if unsafe { libc::geteuid() } != 0 {
                return;
            }
            let root = scratch("mksymlink-owner");
            std::fs::write(root.join("target"), b"").unwrap();
            std::os::unix::fs::chown(root.join("target"), Some(12345), Some(12345)).unwrap();
            assert_eq!(
                make_symlink("target", s(&root.join("l")), Some("0")),
                OpResult::Acknowledged
            );
            let link = std::fs::symlink_metadata(root.join("l")).unwrap();
            assert_eq!((link.uid(), link.gid()), (0, 0));
            let target = std::fs::metadata(root.join("target")).unwrap();
            assert_eq!(target.uid(), 12345);
            assert!(failure(make_symlink(
                "t",
                s(&root.join("m")),
                Some("no-such-user-xyz")
            ))
            .contains("no user"));
            assert!(!root.join("m").exists());
        }

        fn stat_state(pid: u32) -> char {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
            // The state follows the last ")" (the command name may contain one).
            let after = &stat[stat.rfind(')').unwrap() + 2..];
            after.chars().next().unwrap()
        }

        fn wait_for_state(pid: u32, want: &[char]) -> char {
            for _ in 0..200 {
                let state = stat_state(pid);
                if want.contains(&state) {
                    return state;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            stat_state(pid)
        }

        #[test]
        fn signal_process_stops_and_continues_a_process_it_did_not_start() {
            use hv2_guest_agent::SignalName;
            let mut child = std::process::Command::new("sleep")
                .arg("60")
                .spawn()
                .unwrap();
            let pid = child.id();
            assert_eq!(
                signal_process(pid, SignalName::Stop),
                OpResult::Acknowledged
            );
            assert_eq!(wait_for_state(pid, &['T']), 'T');
            assert_eq!(
                signal_process(pid, SignalName::Cont),
                OpResult::Acknowledged
            );
            assert_ne!(wait_for_state(pid, &['S', 'R']), 'T');
            assert_eq!(
                signal_process(pid, SignalName::Term),
                OpResult::Acknowledged
            );
            child.wait().unwrap();
            // Reaped: the pid is gone, which is reported distinctly.
            assert_eq!(
                signal_process(pid, SignalName::Cont),
                OpResult::NoSuchProcess
            );
        }

        #[test]
        fn signal_process_refuses_init_zero_and_the_agent_itself() {
            use hv2_guest_agent::SignalName;
            for pid in [0, 1, std::process::id(), u32::MAX] {
                let message = failure(signal_process(pid, SignalName::Stop));
                assert!(message.contains("refusing"), "{pid}: {message}");
            }
        }
    }

    #[cfg(test)]
    mod process_output_tests {
        use super::*;

        #[test]
        fn a_character_begun_is_kept_for_the_next_take() {
            let buf = Arc::new(Mutex::new(Pending::default()));
            // "é" is two bytes; the program has written only the first.
            buf.lock().unwrap().bytes.extend_from_slice(b"caf\xc3");
            assert_eq!(take(&buf, false), ("caf".to_string(), 0));
            buf.lock().unwrap().bytes.push(0xa9);
            assert_eq!(take(&buf, false), ("\u{e9}".to_string(), 0));
            // Ended with half a character: what there is, as it is.
            buf.lock().unwrap().bytes.push(0xc3);
            assert_eq!(take(&buf, true), ("\u{fffd}".to_string(), 0));
        }

        #[test]
        fn a_cut_is_between_characters() {
            let text = "a\u{e9}b".as_bytes();
            // Byte 2 continues the character that begins at 1.
            assert_eq!(past_partial(text, 2), 3);
            assert_eq!(past_partial(text, 1), 1);
            assert_eq!(past_partial(text, text.len()), text.len());
        }

        /// A program that prints more between two polls than the agent
        /// holds: what is dropped is counted, and with what is taken makes
        /// up every byte it wrote.
        #[test]
        fn what_a_program_outprints_is_counted() {
            const WROTE: usize = 3 * MAX_OUTPUT_BYTES + 12_345;
            let OpResult::Started { pid } = start(
                "/bin/sh",
                &[
                    "-c".into(),
                    format!("head -c {WROTE} /dev/zero | tr '\\0' x"),
                ],
                None,
                &BTreeMap::new(),
                None,
                None,
            ) else {
                panic!("could not start test process")
            };
            // Not polled until it has finished, so the agent must drop.
            std::thread::sleep(std::time::Duration::from_millis(500));
            let (mut taken, mut dropped) = (0usize, 0u64);
            loop {
                let OpResult::Output {
                    stdout,
                    stdout_dropped,
                    running,
                    ..
                } = poll(pid)
                else {
                    panic!("could not poll test process")
                };
                taken += stdout.len();
                dropped += stdout_dropped;
                if !running {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            assert!(
                dropped > 0,
                "nothing dropped: the test did not outprint the buffer"
            );
            assert_eq!(taken as u64 + dropped, WROTE as u64);
        }

        #[test]
        fn rapid_polls_preserve_final_stdout_and_stderr() {
            for _ in 0..100 {
                let OpResult::Started { pid } = start(
                    "/bin/sh",
                    &["-c".into(), "printf final-out; printf final-err >&2".into()],
                    None,
                    &BTreeMap::new(),
                    None,
                    None,
                ) else {
                    panic!("could not start test process")
                };
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut out = String::new();
                let mut err = String::new();
                loop {
                    let OpResult::Output {
                        stdout,
                        stderr,
                        running,
                        exit_code,
                        ..
                    } = poll(pid)
                    else {
                        panic!("could not poll test process")
                    };
                    out.push_str(&stdout);
                    err.push_str(&stderr);
                    if !running {
                        assert_eq!(exit_code, Some(0));
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "process did not finish"
                    );
                    std::thread::yield_now();
                }
                procs().lock().unwrap().remove(&pid);
                assert_eq!(out, "final-out");
                assert_eq!(err, "final-err");
            }
        }

        /// Poll until the program has printed `until`, or ended; what it
        /// printed, and how it ended if it did.
        fn follow(pid: u32, until: Option<&str>) -> (String, Option<(Option<i32>, Option<i32>)>) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut out = String::new();
            loop {
                let OpResult::Output {
                    stdout,
                    running,
                    exit_code,
                    signal,
                    ..
                } = poll(pid)
                else {
                    panic!("could not poll test process")
                };
                out.push_str(&stdout);
                if !running {
                    return (out, Some((exit_code, signal)));
                }
                if until.is_some_and(|text| out.contains(text)) {
                    return (out, None);
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "process neither printed nor finished: {out:?}"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        #[test]
        fn an_interrupt_reaches_a_program_though_this_agent_was_started_ignoring_it() {
            // As an init script's `agentd &` leaves this process.
            // SAFETY: `signal` with a valid signal and disposition.
            let before = unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
            let begin = |script: &str| {
                let OpResult::Started { pid } = start(
                    "/bin/sh",
                    &["-c".into(), script.into()],
                    None,
                    &BTreeMap::new(),
                    None,
                    None,
                ) else {
                    panic!("could not start test process")
                };
                assert_eq!(follow(pid, Some("ready")).1, None, "it ended early");
                pid
            };

            // A shell that traps it: its handler runs, and it ends as it chose.
            let trapping =
                begin("trap 'echo caught; exit 42' INT; echo ready; while :; do sleep 0.05; done");
            // A program with no handler: ended by the signal.
            let plain = begin("echo ready; exec sleep 30");
            for pid in [trapping, plain] {
                assert_eq!(signal(pid, libc::SIGINT), OpResult::Acknowledged);
            }
            let (out, ended) = follow(trapping, None);
            assert_eq!(ended, Some((Some(42), None)), "{out:?}");
            assert!(out.contains("caught"), "{out:?}");
            assert_eq!(follow(plain, None).1, Some((None, Some(libc::SIGINT))));

            // And one run to its end sees the same.
            let OpResult::Exited { stdout, .. } = exec(
                "/bin/sh",
                &[
                    "-c".into(),
                    "trap 'echo caught' INT; kill -INT $$; echo after".into(),
                ],
                None,
                None,
                5_000,
                None,
            ) else {
                panic!("could not run test process")
            };
            assert_eq!(stdout, "caught\nafter\n");

            for pid in [trapping, plain] {
                procs().lock().unwrap().remove(&pid);
            }
            // SAFETY: restores what was there.
            unsafe { libc::signal(libc::SIGINT, before) };
        }
    }
}
