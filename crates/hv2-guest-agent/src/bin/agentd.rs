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
        decode, encode, truncate_utf8, OpResult, Operation, Request, Response, GUEST_AGENT_PORT,
        MAX_FRAME_BYTES, MAX_OUTPUT_BYTES, PROTOCOL_VERSION,
    };
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    /// Accept connections from anywhere; only the host can reach us anyway.
    const VMADDR_CID_ANY: u32 = u32::MAX;

    /// Version reported in a pong, so one guest image can be told from another.
    const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

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
            // Say so on accept. Silence here is ambiguous in exactly the way
            // that costs the most time: a host that gets no answer cannot tell
            // an agent that never accepted from one that accepted and is
            // waiting on a request that never arrived, and those are failures
            // in different halves of the transport.
            eprintln!("hv2-guest-agentd: accepted a connection");

            // One connection at a time: the protocol is request/response and
            // the host opens one channel. Serving them in sequence keeps the
            // agent to a single thread of control, which is what a guest with
            // no scheduler pressure to speak of wants.
            if let Err(e) = serve(fd) {
                eprintln!("hv2-guest-agentd: connection ended: {e}");
            }
            unsafe { libc::close(fd) };
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
            eprintln!("hv2-guest-agentd: read {read} bytes");
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
                let response = handle(request);
                let bytes = encode(&response).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                })?;
                write_all_fd(fd, &bytes)?;
            }
        }
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
        };

        Response {
            id,
            version: PROTOCOL_VERSION,
            result,
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
        if let Some(dir) = cwd {
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
