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
        decode, encode, truncate_utf8, OpResult, Operation, PtySize, Request, Response,
        TemplateDefaults, GUEST_AGENT_PORT, MAX_FRAME_BYTES, MAX_OUTPUT_BYTES, PROTOCOL_VERSION,
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

    pub fn run() -> std::io::Result<()> {
        let listener = bind()?;
        eprintln!("hv2-guest-agentd {AGENT_VERSION} listening on vsock port {GUEST_AGENT_PORT}");

        loop {
            let fd = unsafe { libc::accept(listener, std::ptr::null_mut(), std::ptr::null_mut()) };
            if fd < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
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

        if unsafe { libc::listen(fd, 4) } < 0 {
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
            } => exec(
                &program,
                &args,
                cwd.as_deref(),
                stdin.as_deref(),
                timeout_ms,
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
        stdout: Arc<Mutex<Vec<u8>>>,
        stderr: Arc<Mutex<Vec<u8>>>,
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

    /// Drain a pipe into a buffer until it closes.
    fn drain<R: Read + Send + 'static>(mut source: R, into: Arc<Mutex<Vec<u8>>>) {
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
                        // newest are the ones a caller is waiting on.
                        if buf.len() + n > MAX_OUTPUT_BYTES {
                            let over = buf.len() + n - MAX_OUTPUT_BYTES;
                            let drop_to = over.min(buf.len());
                            buf.drain(..drop_to);
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                }
            }
        });
    }

    /// Take everything buffered so far, leaving the buffer empty.
    fn take(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        let mut guard = match buf.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let bytes = std::mem::take(&mut *guard);
        String::from_utf8_lossy(&bytes).into_owned()
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
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));

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
                        drain(reader, Arc::clone(&stdout));
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
                    drain(pipe, Arc::clone(&stdout));
                }
                if let Some(pipe) = child.stderr.take() {
                    drain(pipe, Arc::clone(&stderr));
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

        let stdout = take(&proc.stdout);
        let stderr = take(&proc.stderr);
        let finished = match proc.finished.lock() {
            Ok(slot) => *slot,
            Err(poisoned) => *poisoned.into_inner(),
        };

        let pty = proc.pty.is_some();
        match finished {
            None => OpResult::Output {
                stdout,
                stderr,
                pty,
                running: true,
                exit_code: None,
                signal: None,
            },
            Some((exit_code, signal)) => OpResult::Output {
                stdout,
                stderr,
                pty,
                running: false,
                exit_code,
                signal,
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
    ) -> OpResult {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let defaults = template_defaults();
        command.envs(&defaults.env);
        if let Some(dir) = cwd.or(defaults.cwd.as_deref()) {
            command.current_dir(dir);
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
}
