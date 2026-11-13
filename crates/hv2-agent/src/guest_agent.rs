//! Running a command inside the guest, for real.
//!
//! # The gap this closes
//!
//! [`ScriptEngine`](crate::ScriptEngine) evaluates a Rhai script on the host
//! against four read-only scalars describing a VM. It was described in four
//! places as running inside the guest, and the documented example was a shell
//! command it cannot parse. Nothing was wrong with the engine; what was wrong
//! was the claim, because there was no way to reach into a guest at all.
//!
//! This module is that way in. It speaks the [`hv2_guest_agent`] protocol over
//! a vsock connection to `hv2-guest-agentd` running in the guest.
//!
//! # What has to be true for this to work
//!
//! Four things, none of which this module can arrange on its own:
//!
//! 1. The VM has a vsock device — [`VM::attach_vsock`](hv2_core::VM::attach_vsock).
//! 2. The guest kernel was told where to find it — see
//!    [`VM::vsock_kernel_args`](hv2_core::VM::vsock_kernel_args).
//! 3. The guest is running, so its driver is servicing the queues.
//! 4. `hv2-guest-agentd` is running inside it.
//!
//! Each failure is reported as itself rather than as a generic timeout, since
//! "no device attached" and "the agent never answered" send an operator to
//! entirely different places.

use crate::{AgentError, Result};
use hv2_core::devices::virtio_vsock::{VsockConnectionId, VsockConnectionState, VsockDevice};
use hv2_guest_agent::{
    decode, encode, OpResult, Operation, PtySize, Request, Response, GUEST_AGENT_PORT,
    MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the client looks for progress while waiting on the guest.
///
/// The device only moves bytes when the guest driver kicks a queue, so there is
/// nothing to await on — this is a poll, and the interval trades latency
/// against spinning.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// What an [`AgentError::Script`] begins with when the guest agent does not
/// know the operation sent: see [`is_unsupported_op`].
pub const UNSUPPORTED_OP_PREFIX: &str = "this guest agent does not support";

/// Whether `error` says the guest's agent predates the operation sent (the
/// filesystem operations: `list_dir`, `stat`, `make_dir`, `remove`,
/// `set_attr`), so a caller can fall back to running a shell for it.
///
/// An old agent cannot say so itself: it fails to parse a request it does
/// not know and drops the connection. `unsupported_if_dropped` turns that
/// into this; the caller must confirm the agent is otherwise alive.
#[must_use]
pub fn is_unsupported_op(error: &AgentError) -> bool {
    matches!(error, AgentError::Script(message) if message.starts_with(UNSUPPORTED_OP_PREFIX))
}

/// How much request one batch carries at most, in bytes of JSON: half the
/// frame limit, so framing never pushes a batch over it.
pub const BATCH_REQUEST_BYTES: usize = MAX_FRAME_BYTES / 2;

/// Recognise "the connection closed with no answer" as an agent that does not
/// know operation `name`; any other error passes through.
fn unsupported_if_dropped(name: &str, error: AgentError) -> AgentError {
    match &error {
        AgentError::Script(message)
            if message.contains("closed before it answered")
                || message.contains("closed mid-request") =>
        {
            AgentError::Script(format!(
                "{UNSUPPORTED_OP_PREFIX} {name} (it closed the connection on the request)"
            ))
        }
        _ => error,
    }
}

/// A byte channel to a program inside the guest.
///
/// The client is written against this rather than against the vsock device so
/// its framing, correlation and timeout handling can be tested without a
/// booted guest. [`VsockChannel`] is the real implementation.
pub trait GuestChannel: Send {
    /// Send what fits, returning how much was accepted. A partial write is
    /// normal: the guest grants credit and the rest waits.
    fn send(&mut self, data: &[u8]) -> Result<usize>;

    /// Take whatever has arrived, which may be nothing.
    fn recv(&mut self) -> Result<Vec<u8>>;

    /// Whether the channel is still usable.
    fn open(&self) -> bool;

    /// Wait for something to happen on the channel, at most `timeout`.
    ///
    /// The default sleeps, which is right for a channel with no way to say.
    fn wait(&mut self, timeout: Duration) {
        std::thread::sleep(timeout);
    }

    /// This channel as a plain byte stream, if it is a vsock connection: for
    /// a connection the guest has turned over to something else.
    fn into_stream(self: Box<Self>) -> Option<VsockStream> {
        None
    }
}

/// A [`GuestChannel`] over one vsock connection.
pub struct VsockChannel {
    device: Arc<Mutex<VsockDevice>>,
    id: VsockConnectionId,
    /// The device's signal that the guest sent something, and the count last
    /// seen, so a signal that arrives between a look and a wait is not lost.
    progress: Arc<hv2_core::devices::virtio_vsock::Progress>,
    seen: u64,
    /// Handed on to a [`VsockStream`], which closes it instead.
    detached: bool,
}

impl VsockChannel {
    /// Open a connection to the guest agent and wait for it to be accepted.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Timeout`] when the guest does not answer inside
    /// `timeout` — which is what happens when the guest is not running, has no
    /// vsock driver, or has no agent listening. None of those are
    /// distinguishable from out here, and the message says so rather than
    /// guessing.
    pub fn connect(device: Arc<Mutex<VsockDevice>>, timeout: Duration) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        let mut refusals = 0u32;
        let progress = device.lock().progress();

        loop {
            let id = device.lock().connect_ephemeral(GUEST_AGENT_PORT)?;

            match Self::settle(&device, &progress, id, deadline) {
                Settled::Established => {
                    let seen = progress.current();
                    return Ok(Self {
                        device,
                        id,
                        progress,
                        seen,
                        detached: false,
                    });
                }
                Settled::Refused => {
                    // A refusal is not necessarily "nothing is listening". An
                    // agent that serves one caller at a time is still inside
                    // the previous connection for a moment after the host has
                    // finished with it, and a request arriving then is reset by
                    // the guest's kernel because the accept queue is full. That
                    // is a busy service rather than an absent one, and the
                    // timeout the caller gave is the right budget to spend on
                    // it -- failing on the first try turns a sequential second
                    // call into an error about the guest having gone away.
                    device.lock().forget(id);
                    refusals += 1;
                }
                Settled::Deadline => {
                    device.lock().forget(id);
                    return Err(AgentError::Timeout(format!(
                        "no answer from a guest agent on vsock port {GUEST_AGENT_PORT} within \
                         {timeout:?} ({refusals} refused). The guest may not be running, may \
                         have no vsock driver, or may not be running hv2-guest-agentd"
                    )));
                }
            }

            if Instant::now() >= deadline {
                return Err(AgentError::Timeout(format!(
                    "a guest agent on vsock port {GUEST_AGENT_PORT} refused {refusals} \
                     connection(s) in {timeout:?} without accepting one. The agent may be \
                     absent, still starting or busy; refusals alone do not distinguish these cases"
                )));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Wait for one connection attempt to resolve.
    fn settle(
        device: &Arc<Mutex<VsockDevice>>,
        progress: &hv2_core::devices::virtio_vsock::Progress,
        id: VsockConnectionId,
        deadline: Instant,
    ) -> Settled {
        loop {
            // Read before looking, so a signal between the look and the wait
            // makes the wait return at once rather than time out.
            let seen = progress.current();
            match device.lock().state(id) {
                Some(VsockConnectionState::Established) => return Settled::Established,
                Some(VsockConnectionState::Connecting) => {}
                _ => return Settled::Refused,
            }
            if Instant::now() >= deadline {
                return Settled::Deadline;
            }
            progress.wait_past(seen, POLL_INTERVAL);
        }
    }

    /// The connection this channel uses.
    pub fn connection_id(&self) -> VsockConnectionId {
        self.id
    }
}

/// How one connection attempt ended.
enum Settled {
    /// The guest accepted.
    Established,
    /// The guest answered, but not with an acceptance.
    Refused,
    /// The caller's deadline passed while it was still connecting.
    Deadline,
}

impl Drop for VsockChannel {
    fn drop(&mut self) {
        if self.detached {
            return;
        }
        // Tell the guest, so its agent stops waiting on a peer that is gone,
        // and then release the port pair. The shutdown packet is queued by
        // `close` before this forgets the connection, so the guest still hears
        // about it -- and without the forget the port stays taken for the life
        // of the VM, which makes the second call fail for the first call's
        // sake.
        let mut device = self.device.lock();
        let _ = device.close(self.id);
        device.forget(self.id);
    }
}

impl GuestChannel for VsockChannel {
    fn send(&mut self, data: &[u8]) -> Result<usize> {
        Ok(self.device.lock().send(self.id, data)?)
    }

    fn recv(&mut self) -> Result<Vec<u8>> {
        Ok(self.device.lock().recv(self.id)?)
    }

    fn open(&self) -> bool {
        matches!(
            self.device.lock().state(self.id),
            Some(VsockConnectionState::Established)
        )
    }

    fn wait(&mut self, timeout: Duration) {
        self.seen = self.progress.wait_past(self.seen, timeout);
    }

    fn into_stream(mut self: Box<Self>) -> Option<VsockStream> {
        let progress = self.device.lock().connection_progress(self.id)?;
        self.detached = true;
        Some(VsockStream {
            inner: Arc::new(StreamInner {
                device: Arc::clone(&self.device),
                id: self.id,
                progress,
            }),
        })
    }
}

/// A vsock connection as a byte stream, for two threads at once: one
/// reading, one writing. Closed when the last clone goes.
#[derive(Clone)]
pub struct VsockStream {
    inner: Arc<StreamInner>,
}

struct StreamInner {
    device: Arc<Mutex<VsockDevice>>,
    id: VsockConnectionId,
    progress: Arc<hv2_core::devices::virtio_vsock::Progress>,
}

impl Drop for StreamInner {
    fn drop(&mut self) {
        let mut device = self.device.lock();
        let _ = device.close(self.id);
        device.forget(self.id);
    }
}

impl VsockStream {
    /// What arrived, waiting for something; empty once the guest has
    /// closed and everything it sent has been read.
    ///
    /// # Errors
    ///
    /// The connection is gone from the device.
    pub fn read(&self) -> Result<Vec<u8>> {
        loop {
            let seen = self.inner.progress.current();
            let (data, open) = {
                let mut device = self.inner.device.lock();
                let data = device.recv(self.inner.id)?;
                let open = device.receive_open(self.inner.id);
                (data, open)
            };
            if !data.is_empty() || !open {
                return Ok(data);
            }
            self.inner.progress.wait_past(seen, Duration::from_secs(1));
        }
    }

    /// Send all of `data`, waiting for credit as the guest grants it.
    ///
    /// # Errors
    ///
    /// The guest closed the connection first.
    pub fn write_all(&self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let seen = self.inner.progress.current();
            let sent = {
                let mut device = self.inner.device.lock();
                if !matches!(
                    device.state(self.inner.id),
                    Some(VsockConnectionState::Established)
                ) {
                    return Err(AgentError::Script("the guest closed the connection".into()));
                }
                device.send(self.inner.id, data)?
            };
            data = &data[sent..];
            if sent == 0 {
                self.inner
                    .progress
                    .wait_past(seen, Duration::from_millis(100));
            }
        }
        Ok(())
    }

    /// Whether the connection still carries anything.
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(
            self.inner.device.lock().state(self.inner.id),
            Some(VsockConnectionState::Established)
        )
    }

    /// Close both directions now, whoever else holds a clone.
    pub fn close(&self) {
        let _ = self.inner.device.lock().close(self.inner.id);
    }

    /// Send EOF without discarding the guest's remaining response.
    pub fn shutdown_write(&self) -> Result<()> {
        self.inner.device.lock().shutdown_write(self.inner.id)?;
        Ok(())
    }
}

/// What a started program has printed since it was last polled.
///
/// Separate from [`GuestExec`] because the two answer different questions.
/// `GuestExec` is a finished program's whole output; this is a running one's
/// latest, and `running` says which kind of answer it is.
#[derive(Debug, Clone)]
pub struct GuestOutput {
    /// Printed since the previous poll, not since the program began.
    ///
    /// For a program with a terminal this is everything it wrote, stderr
    /// included, because a terminal has one stream.
    pub stdout: String,
    pub stderr: String,
    /// Whether this program has a terminal, so a caller can tell an empty
    /// `stderr` that means "merged" from one that means "wrote nothing".
    pub pty: bool,
    /// While true, `exit_code` and `signal` mean nothing.
    pub running: bool,
    /// `None` when a signal ended the program, which is not exiting 0.
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    /// Bytes written on each stream since the previous poll that the agent
    /// dropped, before this poll's text: it holds a bounded amount between
    /// polls. Zero from an agent too old to say.
    pub stdout_dropped: u64,
    pub stderr_dropped: u64,
}

/// What a command did inside the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestExec {
    /// Exit status, or `None` when a signal ended the program.
    ///
    /// Kept separate from `signal` because a program killed by SIGKILL did not
    /// exit 0, and collapsing the two reports a crash as a success.
    pub exit_code: Option<i32>,
    /// Signal that ended the program, if one did.
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Whether output was cut short at the agent's per-stream ceiling.
    pub truncated: bool,
    /// Whether the agent killed the program for running past its timeout.
    pub timed_out: bool,
}

impl GuestExec {
    /// Whether the program ran to completion and exited zero.
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }
}

/// A client for the agent inside a guest.
pub struct GuestAgent {
    channel: Box<dyn GuestChannel>,
    next_id: u64,
    /// Bytes received but not yet a whole frame.
    pending: Vec<u8>,
}

impl GuestAgent {
    /// Wrap an already-open channel.
    pub fn new(channel: Box<dyn GuestChannel>) -> Self {
        Self {
            channel,
            next_id: 1,
            pending: Vec::new(),
        }
    }

    /// Connect to the agent in a guest over `device`.
    pub fn over_vsock(device: Arc<Mutex<VsockDevice>>, timeout: Duration) -> Result<Self> {
        Ok(Self::new(Box::new(VsockChannel::connect(device, timeout)?)))
    }

    /// Ask the agent to identify itself.
    ///
    /// The cheapest way to answer "is anything actually listening in there",
    /// and the check worth making before reporting that a VM is ready for work.
    /// What the guest is using: CPU ticks, memory, its root filesystem.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or an agent too old to answer.
    pub fn stats(&mut self, timeout: Duration) -> Result<hv2_guest_agent::GuestStats> {
        match self.request(Operation::Stats, timeout)? {
            OpResult::Stats(stats) => Ok(stats),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a stats request with {other:?}"
            ))),
        }
    }

    pub fn ping(&mut self, timeout: Duration) -> Result<String> {
        match self.request(Operation::Ping, timeout)? {
            OpResult::Pong { agent_version } => Ok(agent_version),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a ping with {other:?}"
            ))),
        }
    }

    /// Run a program in the guest and wait for it to finish.
    ///
    /// `program` is executed directly, not through a shell: `ls > out`
    /// redirects nothing. A caller wanting shell semantics runs a shell, and
    /// does so knowingly — the alternative is an API that quietly means
    /// something different from what it says, which is the defect this whole
    /// module exists to fix.
    pub fn exec(&mut self, program: &str, args: &[String], timeout: Duration) -> Result<GuestExec> {
        self.exec_with(program, args, None, None, timeout)
    }

    /// [`Self::exec`] with a working directory and standard input.
    pub fn exec_with(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        stdin: Option<&str>,
        timeout: Duration,
    ) -> Result<GuestExec> {
        self.exec_as(program, args, cwd, stdin, None, timeout)
    }

    /// [`Self::exec_with`] as `user`, by name or number (root if `None`).
    pub fn exec_as(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        stdin: Option<&str>,
        user: Option<&str>,
        timeout: Duration,
    ) -> Result<GuestExec> {
        // The guest is given a shorter deadline than the host waits, so a
        // program that overruns comes back as a reported timeout with whatever
        // it printed, rather than as host-side silence.
        let guest_timeout = timeout.mul_f32(0.8).max(Duration::from_millis(100));

        let op = Operation::Exec {
            program: program.to_string(),
            args: args.to_vec(),
            cwd: cwd.map(str::to_string),
            stdin: stdin.map(str::to_string),
            timeout_ms: guest_timeout.as_millis() as u64,
            user: user.map(str::to_string),
        };

        match self.request(op, timeout)? {
            OpResult::Exited {
                exit_code,
                signal,
                stdout,
                stderr,
                truncated,
                timed_out,
            } => Ok(GuestExec {
                exit_code,
                signal,
                stdout,
                stderr,
                truncated,
                timed_out,
            }),
            OpResult::Failed { message } => Err(AgentError::Script(format!(
                "the guest agent could not run {program}: {message}"
            ))),
            other => Err(AgentError::Script(format!(
                "the guest answered an exec with {other:?}"
            ))),
        }
    }

    /// Start a program in the guest and leave it running.
    ///
    /// Returns the guest pid, which every other call here takes. Unlike
    /// [`Self::exec`], nothing is collected and nothing is waited for: the
    /// program is still running when this returns, which is the point.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or the guest's reason for not starting
    /// the program.
    pub fn start(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        envs: &BTreeMap<String, String>,
        pty: Option<PtySize>,
        timeout: Duration,
    ) -> Result<u32> {
        self.start_as(program, args, cwd, envs, pty, None, timeout)
    }

    /// [`Self::start`], as `user`: `None` is the template's user, or root.
    ///
    /// # Errors
    ///
    /// As [`Self::start`]; and a user the guest has no account for.
    #[allow(clippy::too_many_arguments)]
    pub fn start_as(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        envs: &BTreeMap<String, String>,
        pty: Option<PtySize>,
        user: Option<&str>,
        timeout: Duration,
    ) -> Result<u32> {
        let op = Operation::Start {
            program: program.to_string(),
            args: args.to_vec(),
            cwd: cwd.map(str::to_string),
            envs: envs.clone(),
            pty,
            user: user.map(str::to_string),
        };
        match self.request(op, timeout)? {
            OpResult::Started { pid } => Ok(pid),
            OpResult::Failed { message } => Err(AgentError::Script(format!(
                "the guest agent could not start {program}: {message}"
            ))),
            other => Err(AgentError::Script(format!(
                "the guest answered a start with {other:?}"
            ))),
        }
    }

    /// Collect what a started program has printed since the last poll.
    ///
    /// Each poll returns only what is new. A caller streaming output calls
    /// this repeatedly until `running` is false.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or the guest not knowing that pid.
    pub fn poll(&mut self, pid: u32, timeout: Duration) -> Result<GuestOutput> {
        match self.request(Operation::Poll { pid }, timeout)? {
            OpResult::Output {
                stdout,
                stderr,
                pty,
                running,
                exit_code,
                signal,
                stdout_dropped,
                stderr_dropped,
            } => Ok(GuestOutput {
                stdout,
                stderr,
                pty,
                running,
                exit_code,
                signal,
                stdout_dropped,
                stderr_dropped,
            }),
            OpResult::Failed { message } => Err(AgentError::Script(format!(
                "the guest agent could not poll pid {pid}: {message}"
            ))),
            other => Err(AgentError::Script(format!(
                "the guest answered a poll with {other:?}"
            ))),
        }
    }

    /// Write to a started program's standard input.
    ///
    /// `close` sends end-of-input afterwards, which a program reading until
    /// EOF needs in order to finish at all.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or the guest not knowing that pid, or
    /// its stdin already being closed.
    pub fn write_stdin(
        &mut self,
        pid: u32,
        data: &str,
        close: bool,
        timeout: Duration,
    ) -> Result<()> {
        let op = Operation::WriteStdin {
            pid,
            data: data.to_string(),
            close,
        };
        match self.request(op, timeout)? {
            OpResult::Acknowledged => Ok(()),
            OpResult::Failed { message } => Err(AgentError::Script(format!(
                "the guest agent could not write to pid {pid}: {message}"
            ))),
            other => Err(AgentError::Script(format!(
                "the guest answered a write with {other:?}"
            ))),
        }
    }

    /// Tell a program's terminal it is a different size.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, the guest not knowing that pid, or the
    /// program having been started with pipes rather than a terminal.
    pub fn resize_pty(&mut self, pid: u32, size: PtySize, timeout: Duration) -> Result<()> {
        match self.request(Operation::ResizePty { pid, size }, timeout)? {
            OpResult::Acknowledged => Ok(()),
            OpResult::Failed { message } => Err(AgentError::Script(format!(
                "the guest agent could not resize the terminal of pid {pid}: {message}"
            ))),
            other => Err(AgentError::Script(format!(
                "the guest answered a resize with {other:?}"
            ))),
        }
    }

    /// Resynchronise a restored guest with host time sampled on this connected
    /// channel, rather than before waiting for a worker or guest connection.
    /// Transport and guest processing still take time; this is not a clock
    /// synchronisation protocol with round-trip compensation.
    ///
    /// # Errors
    ///
    /// Fails if host time cannot be represented as Unix nanoseconds, or if the
    /// guest refuses clock correction or RNG reseeding.
    pub fn restored_now(&mut self, entropy: Vec<u8>, timeout: Duration) -> Result<()> {
        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| {
                AgentError::Script(format!("host clock precedes Unix epoch: {error}"))
            })?;
        let unix_time_ns = u64::try_from(elapsed.as_nanos())
            .map_err(|_| AgentError::Script("host time exceeds Unix nanosecond range".into()))?;
        self.restored(unix_time_ns, entropy, timeout)
    }

    /// Tell a guest restored from a snapshot the time, and give it entropy to
    /// reseed from. See [`Operation::Restored`].
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or the guest refusing either step. An
    /// agent too old to know the operation drops the connection, which
    /// arrives as a transport failure.
    pub fn restored(
        &mut self,
        unix_time_ns: u64,
        entropy: Vec<u8>,
        timeout: Duration,
    ) -> Result<()> {
        match self.request(
            Operation::Restored {
                unix_time_ns,
                entropy,
            },
            timeout,
        )? {
            OpResult::Acknowledged => Ok(()),
            OpResult::Failed { message } => Err(AgentError::Script(format!(
                "the guest agent could not resynchronise after a restore: {message}"
            ))),
            other => Err(AgentError::Script(format!(
                "the guest answered a restore notice with {other:?}"
            ))),
        }
    }

    /// Write `data` to `path` in the guest, replacing it -- in chunks, each
    /// its own request, so a file of any size fits the frame limit.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or the guest's reason for refusing.
    pub fn write_file(&mut self, path: &str, data: &[u8], timeout: Duration) -> Result<()> {
        self.write_file_as(path, data, None, timeout)
    }

    /// [`Self::write_file`], the file and the directories made for it owned
    /// by `owner` (see [`Operation::WriteFile`]).
    ///
    /// # Errors
    ///
    /// As [`Self::write_file`]; and an owner the guest has no account for.
    pub fn write_file_as(
        &mut self,
        path: &str,
        data: &[u8],
        owner: Option<&str>,
        timeout: Duration,
    ) -> Result<()> {
        let mut chunks = data.chunks(hv2_guest_agent::FILE_CHUNK).peekable();
        let mut append = false;
        // An empty file is still one write, which creates it.
        let empty: &[u8] = &[];
        let first = chunks.next().unwrap_or(empty);
        for chunk in std::iter::once(first).chain(chunks) {
            match self.request(
                Operation::WriteFile {
                    path: path.to_string(),
                    data: hv2_guest_agent::b64::encode(chunk),
                    append,
                    owner: owner.map(str::to_string),
                },
                timeout,
            )? {
                OpResult::Acknowledged => {}
                OpResult::Failed { message } => return Err(AgentError::Script(message)),
                other => {
                    return Err(AgentError::Script(format!(
                        "the guest answered a file write with {other:?}"
                    )))
                }
            }
            append = true;
        }
        Ok(())
    }

    /// Have the guest carry this connection to its TCP `port`, and give the
    /// connection up as a byte stream, with what already arrived past the
    /// answer.
    ///
    /// # Errors
    ///
    /// Nothing listens on `port` in the guest, or the transport failed.
    pub fn forward(self, port: u16, timeout: Duration) -> Result<(VsockStream, Vec<u8>)> {
        self.forward_transport(Operation::Forward { port }, timeout)
    }

    /// Have the guest listen on its TCP `port` and carry the next connection
    /// a guest program makes there over this one: answered once it listens,
    /// then [`hv2_guest_agent::REVERSE_ACCEPTED`] when a program connects.
    /// Returns the stream and what already arrived past the answer.
    ///
    /// # Errors
    ///
    /// The guest cannot listen on `port`, or the transport failed.
    pub fn listen(self, port: u16, timeout: Duration) -> Result<(VsockStream, Vec<u8>)> {
        self.forward_transport(Operation::Listen { port }, timeout)
    }

    /// Open a framed IPv4 loopback UDP session. Setup does not prove a listener.
    ///
    /// # Errors
    /// Rejects port zero, failed guest setup, unexpected replies and non-vsock channels.
    pub fn forward_udp(self, port: u16, timeout: Duration) -> Result<(VsockStream, Vec<u8>)> {
        if port == 0 {
            return Err(AgentError::Script("UDP port must be nonzero".into()));
        }
        self.forward_transport(Operation::ForwardUdp { port }, timeout)
    }

    /// Open a framed IPv6 loopback UDP session without IPv4 fallback.
    ///
    /// # Errors
    /// Rejects port zero, unavailable IPv6, failed setup and non-vsock channels.
    pub fn forward_udp_ipv6(self, port: u16, timeout: Duration) -> Result<(VsockStream, Vec<u8>)> {
        if port == 0 {
            return Err(AgentError::Script("UDP port must be nonzero".into()));
        }
        self.forward_transport(Operation::ForwardUdp6 { port }, timeout)
    }

    fn forward_transport(
        mut self,
        operation: Operation,
        timeout: Duration,
    ) -> Result<(VsockStream, Vec<u8>)> {
        match self.request(operation, timeout)? {
            OpResult::Acknowledged => {
                let early = std::mem::take(&mut self.pending);
                let stream = self.channel.into_stream().ok_or_else(|| {
                    AgentError::Script("only a vsock connection can be forwarded".into())
                })?;
                Ok((stream, early))
            }
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a forward with {other:?}"
            ))),
        }
    }

    /// Have the guest mount a volume at `path` over this connection, and
    /// give the connection up to whoever serves it: after the agent's answer
    /// every byte on it is 9P. Returns the channel and what already arrived
    /// past the answer -- the kernel may have sent its `Tversion` by then.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or the guest's reason for refusing.
    pub fn mount_volume(
        mut self,
        path: &str,
        timeout: Duration,
    ) -> Result<(Box<dyn GuestChannel>, Vec<u8>)> {
        let op = Operation::MountVolume {
            path: path.to_string(),
        };
        match self.request(op, timeout)? {
            OpResult::Acknowledged => Ok((self.channel, std::mem::take(&mut self.pending))),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a volume mount with {other:?}"
            ))),
        }
    }

    /// Read all of `path` in the guest, up to `limit` bytes.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, the guest's reason for refusing, or
    /// a file larger than `limit`.
    pub fn read_file(&mut self, path: &str, limit: u64, timeout: Duration) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            match self.request(
                Operation::ReadFile {
                    path: path.to_string(),
                    offset: out.len() as u64,
                    length: hv2_guest_agent::FILE_CHUNK as u64,
                },
                timeout,
            )? {
                OpResult::FileData { data, size } => {
                    if size > limit {
                        return Err(AgentError::Script(format!(
                            "{path} is {size} bytes, over the {limit}-byte limit"
                        )));
                    }
                    let bytes = hv2_guest_agent::b64::decode(&data).ok_or_else(|| {
                        AgentError::Script("the guest sent file data that is not base64".into())
                    })?;
                    let done = bytes.is_empty();
                    out.extend_from_slice(&bytes);
                    if done || out.len() as u64 >= size {
                        return Ok(out);
                    }
                }
                OpResult::Failed { message } => return Err(AgentError::Script(message)),
                other => {
                    return Err(AgentError::Script(format!(
                        "the guest answered a file read with {other:?}"
                    )))
                }
            }
        }
    }

    /// Send one of the filesystem operations, which an agent built before
    /// they existed cannot answer. It does not answer an operation it cannot
    /// parse: it drops the connection. That, from this end, looks the same as
    /// a guest that died mid-request, so it is reported as "unsupported"
    /// ([`is_unsupported_op`]) and the caller that can tell the two apart
    /// ([`AgentVM`](crate::AgentVM) pings on a fresh connection) does.
    fn request_fs_op(
        &mut self,
        name: &'static str,
        op: Operation,
        timeout: Duration,
    ) -> Result<OpResult> {
        self.request(op, timeout)
            .map_err(|e| unsupported_if_dropped(name, e))
    }

    /// List `path` in the guest: its entries (sorted by name, as `lstat`
    /// sees them) and whether the list was cut short. `limit` of 0 is the
    /// agent's own cap, [`hv2_guest_agent::MAX_DIR_ENTRIES`].
    ///
    /// # Errors
    ///
    /// The guest's reason for refusing, a transport failure, or
    /// [`is_unsupported_op`] for an agent from before this existed.
    pub fn list_dir(
        &mut self,
        path: &str,
        limit: u32,
        timeout: Duration,
    ) -> Result<(Vec<hv2_guest_agent::DirEntry>, bool)> {
        let op = Operation::ListDir {
            path: path.to_string(),
            limit,
        };
        match self.request_fs_op("list_dir", op, timeout)? {
            OpResult::DirEntries { entries, truncated } => Ok((entries, truncated)),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a directory listing with {other:?}"
            ))),
        }
    }

    /// `stat` one path in the guest (`lstat` unless `follow`).
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`]; a missing path is the guest's refusal.
    pub fn stat(
        &mut self,
        path: &str,
        follow: bool,
        timeout: Duration,
    ) -> Result<hv2_guest_agent::DirEntry> {
        let op = Operation::Stat {
            path: path.to_string(),
            follow,
        };
        match self.request_fs_op("stat", op, timeout)? {
            OpResult::FileStat(entry) => Ok(entry),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a stat with {other:?}"
            ))),
        }
    }

    /// Make a directory in the guest; see [`Operation::MakeDir`].
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`].
    pub fn make_dir(
        &mut self,
        path: &str,
        parents: bool,
        owner: Option<&str>,
        mode: Option<u32>,
        timeout: Duration,
    ) -> Result<()> {
        let op = Operation::MakeDir {
            path: path.to_string(),
            parents,
            owner: owner.map(str::to_string),
            mode,
        };
        self.acknowledged("make_dir", op, "a directory creation", timeout)
    }

    /// Delete a path in the guest; see [`Operation::Remove`].
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`].
    pub fn remove(
        &mut self,
        path: &str,
        recursive: bool,
        missing_ok: bool,
        timeout: Duration,
    ) -> Result<()> {
        let op = Operation::Remove {
            path: path.to_string(),
            recursive,
            missing_ok,
        };
        self.acknowledged("remove", op, "a removal", timeout)
    }

    /// `chown` and `chmod` in the guest; see [`Operation::SetAttr`].
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`].
    pub fn set_attr(
        &mut self,
        path: &str,
        owner: Option<&str>,
        mode: Option<u32>,
        recursive: bool,
        timeout: Duration,
    ) -> Result<()> {
        let op = Operation::SetAttr {
            path: path.to_string(),
            owner: owner.map(str::to_string),
            mode,
            recursive,
        };
        self.acknowledged("set_attr", op, "an attribute change", timeout)
    }

    /// Read a symlink's target in the guest; see [`Operation::ReadLink`].
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`]; a path that is not a symlink, or whose target is
    /// not UTF-8, is the guest's refusal.
    pub fn read_link(&mut self, path: &str, timeout: Duration) -> Result<String> {
        let op = Operation::ReadLink {
            path: path.to_string(),
        };
        match self.request_fs_op("read_link", op, timeout)? {
            OpResult::LinkTarget { target } => Ok(target),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a link read with {other:?}"
            ))),
        }
    }

    /// Create a symlink in the guest; see [`Operation::MakeSymlink`].
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`]; an existing path is the guest's refusal.
    pub fn make_symlink(
        &mut self,
        target: &str,
        path: &str,
        owner: Option<&str>,
        timeout: Duration,
    ) -> Result<()> {
        let op = Operation::MakeSymlink {
            target: target.to_string(),
            path: path.to_string(),
            owner: owner.map(str::to_string),
        };
        self.acknowledged("make_symlink", op, "a symlink creation", timeout)
    }

    /// Send a named signal to any pid in the guest; see
    /// [`Operation::SignalProcess`]. `Ok(true)` if delivered, `Ok(false)` if
    /// the process no longer exists.
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`]; pid 1, 0 and the agent's own are the guest's refusal.
    pub fn signal_process(
        &mut self,
        pid: u32,
        signal: hv2_guest_agent::SignalName,
        timeout: Duration,
    ) -> Result<bool> {
        let op = Operation::SignalProcess { pid, signal };
        match self.request_fs_op("signal_process", op, timeout)? {
            OpResult::Acknowledged => Ok(true),
            OpResult::NoSuchProcess => Ok(false),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a signal with {other:?}"
            ))),
        }
    }

    /// Mount a block device in the guest; see [`Operation::MountBlock`].
    /// `Ok(true)` once mounted, `Ok(false)` if `expect_sectors` was set and
    /// the device never reported that size.
    ///
    /// # Errors
    ///
    /// As [`Self::list_dir`]; a refused device or target, or the kernel's
    /// reason for failing the mount, is the guest's refusal.
    pub fn mount_block(
        &mut self,
        device: &str,
        target: &str,
        expect_sectors: Option<u64>,
        drop_caches: bool,
        timeout: Duration,
    ) -> Result<bool> {
        let op = Operation::MountBlock {
            device: device.to_string(),
            target: target.to_string(),
            fstype: hv2_guest_agent::MountFsType::Ext4,
            expect_sectors,
            drop_caches,
        };
        match self.request_fs_op("mount_block", op, timeout)? {
            OpResult::Acknowledged => Ok(true),
            OpResult::DeviceSizeNotSeen => Ok(false),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered a block mount with {other:?}"
            ))),
        }
    }

    /// Run filesystem operations in order on this one connection, one round
    /// trip per batch the agent takes rather than one per operation; see
    /// [`Operation::Batch`]. Returns one result per operation, in order:
    /// a failing operation is its own [`OpResult::Failed`] entry and the rest
    /// still ran, so the caller reads them in order.
    ///
    /// The operations are split into batches of at most
    /// [`hv2_guest_agent::MAX_BATCH_OPS`] and about [`BATCH_REQUEST_BYTES`] of
    /// request, and any the agent stopped short of (its reply cap) are sent
    /// again in the next, so the caller need not size anything. A single
    /// `write_file` whose data does not fit one frame is not supported here:
    /// write such a file with [`Self::write_file_as`].
    ///
    /// # Errors
    ///
    /// A transport failure; the guest refusing the whole batch (an operation
    /// that is not a filesystem one, which is refused here first); an
    /// operation too large for a frame; [`is_unsupported_op`] for an agent
    /// from before batching existed.
    pub fn batch(&mut self, ops: Vec<Operation>, timeout: Duration) -> Result<Vec<OpResult>> {
        if let Some(op) = ops.iter().find(|op| !hv2_guest_agent::batchable(op)) {
            return Err(AgentError::Script(format!(
                "refusing a batch: {} is not a filesystem operation",
                hv2_guest_agent::op_name(op)
            )));
        }
        let total = ops.len();
        let mut results = Vec::with_capacity(total);
        let mut ops = ops.into_iter().peekable();
        while ops.peek().is_some() {
            let mut chunk = Vec::new();
            let mut bytes = 0usize;
            while chunk.len() < hv2_guest_agent::MAX_BATCH_OPS {
                let Some(op) = ops.peek() else { break };
                let size = serde_json::to_vec(op).map_or(0, |v| v.len()) + 1;
                // One op always goes, so the frame check reports one too big.
                if !chunk.is_empty() && bytes + size > BATCH_REQUEST_BYTES {
                    break;
                }
                bytes += size;
                chunk.extend(ops.next());
            }
            let sent = chunk.len();
            match self.request_fs_op("batch", Operation::Batch { ops: chunk.clone() }, timeout)? {
                OpResult::Batch { results: got } => {
                    if got.is_empty() || got.len() > sent {
                        return Err(AgentError::Script(format!(
                            "the guest answered a batch of {sent} operations with {} results",
                            got.len()
                        )));
                    }
                    let ran = got.len();
                    results.extend(got);
                    // What the agent stopped short of goes first in the next.
                    if ran < sent {
                        let rest = chunk.split_off(ran);
                        let tail: Vec<Operation> = rest.into_iter().chain(ops).collect();
                        ops = tail.into_iter().peekable();
                    }
                }
                OpResult::Failed { message } => return Err(AgentError::Script(message)),
                other => {
                    return Err(AgentError::Script(format!(
                        "the guest answered a batch with {other:?}"
                    )))
                }
            }
        }
        debug_assert_eq!(results.len(), total);
        Ok(results)
    }

    fn acknowledged(
        &mut self,
        name: &'static str,
        op: Operation,
        what: &str,
        timeout: Duration,
    ) -> Result<()> {
        match self.request_fs_op(name, op, timeout)? {
            OpResult::Acknowledged => Ok(()),
            OpResult::Failed { message } => Err(AgentError::Script(message)),
            other => Err(AgentError::Script(format!(
                "the guest answered {what} with {other:?}"
            ))),
        }
    }

    /// Send a signal to a started program.
    ///
    /// # Errors
    ///
    /// Propagates a transport failure, or the guest not knowing that pid --
    /// which includes any pid this agent did not start, deliberately.
    pub fn signal(&mut self, pid: u32, signal: i32, timeout: Duration) -> Result<()> {
        match self.request(Operation::Signal { pid, signal }, timeout)? {
            OpResult::Acknowledged => Ok(()),
            OpResult::Failed { message } => Err(AgentError::Script(format!(
                "the guest agent could not signal pid {pid}: {message}"
            ))),
            other => Err(AgentError::Script(format!(
                "the guest answered a signal with {other:?}"
            ))),
        }
    }

    /// Send one request and wait for the matching response.
    fn request(&mut self, op: Operation, timeout: Duration) -> Result<OpResult> {
        let id = self.next_id;
        self.next_id += 1;

        let request = Request {
            id,
            version: PROTOCOL_VERSION,
            op,
        };
        let frame = encode(&request)
            .map_err(|e| AgentError::Script(format!("could not encode a guest request: {e}")))?;

        let deadline = Instant::now() + timeout;
        self.write_all(&frame, deadline)?;
        self.read_response(id, deadline)
    }

    /// Write every byte, waiting for credit as the guest grants it.
    fn write_all(&mut self, mut data: &[u8], deadline: Instant) -> Result<()> {
        while !data.is_empty() {
            if !self.channel.open() {
                return Err(AgentError::Script(
                    "the connection to the guest agent closed mid-request".to_string(),
                ));
            }
            let sent = self.channel.send(data)?;
            data = &data[sent..];

            if sent == 0 {
                if Instant::now() >= deadline {
                    return Err(AgentError::Timeout(
                        "the guest agent stopped granting credit before the request was sent"
                            .to_string(),
                    ));
                }
                self.channel.wait(POLL_INTERVAL);
            }
        }
        Ok(())
    }

    /// Read until the response with `id` arrives.
    fn read_response(&mut self, id: u64, deadline: Instant) -> Result<OpResult> {
        loop {
            let chunk = self.channel.recv()?;
            if !chunk.is_empty() {
                if self.pending.len() + chunk.len() > MAX_FRAME_BYTES + 4 {
                    // The guest wrote the length prefix; believing an
                    // unbounded one is how it gets to choose host memory use.
                    return Err(AgentError::Script(
                        "the guest agent sent more than one frame of data".to_string(),
                    ));
                }
                self.pending.extend_from_slice(&chunk);
            }

            while let Some((response, used)) = decode::<Response>(&self.pending)
                .map_err(|e| AgentError::Script(format!("the guest agent sent {e}")))?
            {
                self.pending.drain(..used);
                if response.id != id {
                    // A stale answer to a request that already timed out.
                    // Dropping it is right; treating it as this answer would
                    // report one command's output as another's.
                    tracing::debug!(
                        "guest agent: discarding a response to request {}, waiting for {id}",
                        response.id
                    );
                    continue;
                }
                if response.version != PROTOCOL_VERSION {
                    return Err(AgentError::Script(format!(
                        "the guest agent speaks protocol version {}, this host speaks \
                         {PROTOCOL_VERSION}",
                        response.version
                    )));
                }
                return Ok(response.result);
            }

            if Instant::now() >= deadline {
                return Err(AgentError::Timeout(format!(
                    "the guest agent did not answer request {id} in time"
                )));
            }
            if !self.channel.open() {
                return Err(AgentError::Script(
                    "the connection to the guest agent closed before it answered".to_string(),
                ));
            }
            self.channel.wait(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_guest_agent::MAX_OUTPUT_BYTES;
    use std::collections::VecDeque;

    /// A channel with a scripted guest on the far end.
    ///
    /// It decodes each request and answers with whatever `reply` returns, so
    /// the client's framing, correlation and timeouts are exercised without a
    /// booted guest — the parts of this that a kernel would not tell us more
    /// about anyway.
    struct FakeGuest {
        /// Bytes written by the client that have not formed a frame yet, as a
        /// real peer on a stream socket would hold them.
        inbound: Vec<u8>,
        outbound: VecDeque<u8>,
        reply: Box<dyn FnMut(Request) -> Option<Response> + Send>,
        open: bool,
        /// Bytes the guest will accept per send, to model credit pressure.
        credit: usize,
    }

    impl FakeGuest {
        fn new(reply: impl FnMut(Request) -> Option<Response> + Send + 'static) -> Box<Self> {
            Box::new(Self {
                inbound: Vec::new(),
                outbound: VecDeque::new(),
                reply: Box::new(reply),
                open: true,
                credit: usize::MAX,
            })
        }
    }

    impl GuestChannel for FakeGuest {
        fn send(&mut self, data: &[u8]) -> Result<usize> {
            let take = data.len().min(self.credit);
            if take == 0 {
                return Ok(0);
            }
            self.inbound.extend_from_slice(&data[..take]);

            while let Some((request, used)) =
                decode::<Request>(&self.inbound).map_err(|e| AgentError::Script(e.to_string()))?
            {
                self.inbound.drain(..used);
                if let Some(response) = (self.reply)(request) {
                    self.outbound.extend(encode(&response).expect("encode"));
                }
            }
            Ok(take)
        }

        fn recv(&mut self) -> Result<Vec<u8>> {
            Ok(self.outbound.drain(..).collect())
        }

        fn open(&self) -> bool {
            self.open
        }
    }

    fn exited(id: u64, code: i32, stdout: &str) -> Response {
        Response {
            id,
            version: PROTOCOL_VERSION,
            result: OpResult::Exited {
                exit_code: Some(code),
                signal: None,
                stdout: stdout.to_string(),
                stderr: String::new(),
                truncated: false,
                timed_out: false,
            },
        }
    }

    /// An agent from before the filesystem operations: it cannot parse the
    /// request, so it hangs up without a word.
    struct HangsUpOnUnknown {
        open: bool,
    }

    impl GuestChannel for HangsUpOnUnknown {
        fn send(&mut self, data: &[u8]) -> Result<usize> {
            self.open = false;
            Ok(data.len())
        }

        fn recv(&mut self) -> Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn open(&self) -> bool {
            self.open
        }
    }

    #[test]
    fn an_old_agent_dropping_the_connection_reads_as_an_unsupported_op() {
        let t = Duration::from_secs(1);
        let new = || GuestAgent::new(Box::new(HangsUpOnUnknown { open: true }));
        let errors = [
            new().list_dir("/", 0, t).unwrap_err(),
            new().stat("/", false, t).unwrap_err(),
            new().make_dir("/a", true, None, None, t).unwrap_err(),
            new().remove("/a", true, false, t).unwrap_err(),
            new()
                .set_attr("/a", None, Some(0o755), false, t)
                .unwrap_err(),
            new().read_link("/a", t).unwrap_err(),
            new().make_symlink("x", "/a", None, t).unwrap_err(),
            new()
                .signal_process(5, hv2_guest_agent::SignalName::Stop, t)
                .unwrap_err(),
            new()
                .mount_block("/dev/vda", "/data", None, false, t)
                .unwrap_err(),
        ];
        for error in &errors {
            assert!(is_unsupported_op(error), "{error}");
        }
        assert!(errors[0].to_string().contains("list_dir"));
        assert!(errors[7].to_string().contains("signal_process"));
        // The same hang-up on an operation every agent knows is no such thing.
        let error = new().ping(t).unwrap_err();
        assert!(!is_unsupported_op(&error), "{error}");
        // Nor is a refusal, or a timeout.
        assert!(!is_unsupported_op(&AgentError::Script(
            "no such file".into()
        )));
        assert!(!is_unsupported_op(&AgentError::Timeout(
            "the guest agent did not answer request 1 in time".into()
        )));
    }

    #[test]
    fn a_current_agents_filesystem_answers_come_through() {
        let entry = hv2_guest_agent::DirEntry {
            name: "f".into(),
            kind: hv2_guest_agent::DirEntryKind::File,
            size: 3,
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime_secs: 1,
        };
        let answer = entry.clone();
        let mut agent = GuestAgent::new(FakeGuest::new(move |request| {
            let result = match request.op {
                Operation::ListDir { path, limit } => {
                    assert_eq!((path.as_str(), limit), ("/d", 7));
                    OpResult::DirEntries {
                        entries: vec![answer.clone()],
                        truncated: true,
                    }
                }
                Operation::Stat { follow, .. } => {
                    assert!(follow);
                    OpResult::FileStat(answer.clone())
                }
                Operation::Remove { path, .. } if path == "/" => OpResult::Failed {
                    message: "refusing to remove \"/\"".into(),
                },
                _ => OpResult::Acknowledged,
            };
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result,
            })
        }));
        let t = Duration::from_secs(1);
        assert_eq!(
            agent.list_dir("/d", 7, t).unwrap(),
            (vec![entry.clone()], true)
        );
        assert_eq!(agent.stat("/d/f", true, t).unwrap(), entry);
        agent
            .make_dir("/d/x", true, Some("1000"), Some(0o700), t)
            .unwrap();
        agent.set_attr("/d/x", None, Some(0o755), true, t).unwrap();
        agent.remove("/d/x", true, false, t).unwrap();
        let refused = agent.remove("/", true, false, t).unwrap_err();
        assert!(refused.to_string().contains("refusing"));
        assert!(!is_unsupported_op(&refused));
    }

    #[test]
    fn link_and_signal_answers_come_through() {
        use hv2_guest_agent::SignalName;
        let mut agent = GuestAgent::new(FakeGuest::new(|request| {
            let result = match request.op {
                Operation::ReadLink { path } if path == "/l" => OpResult::LinkTarget {
                    target: "../t".into(),
                },
                Operation::ReadLink { .. } => OpResult::Failed {
                    message: "not a symlink".into(),
                },
                Operation::SignalProcess { pid: 9, .. } => OpResult::NoSuchProcess,
                Operation::SignalProcess { pid: 1, .. } => OpResult::Failed {
                    message: "refusing to signal pid 1".into(),
                },
                _ => OpResult::Acknowledged,
            };
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result,
            })
        }));
        let t = Duration::from_secs(1);
        assert_eq!(agent.read_link("/l", t).unwrap(), "../t");
        assert!(agent
            .read_link("/f", t)
            .unwrap_err()
            .to_string()
            .contains("not a symlink"));
        agent.make_symlink("t", "/x", Some("0"), t).unwrap();
        assert!(agent.signal_process(5, SignalName::Stop, t).unwrap());
        assert!(!agent.signal_process(9, SignalName::Cont, t).unwrap());
        let refused = agent.signal_process(1, SignalName::Stop, t).unwrap_err();
        assert!(refused.to_string().contains("refusing"));
        assert!(!is_unsupported_op(&refused));
    }

    fn stat_op(path: &str) -> Operation {
        Operation::Stat {
            path: path.into(),
            follow: false,
        }
    }

    /// A guest that answers every batch by acknowledging at most `take` of
    /// its operations, and records the size of each batch it was sent.
    fn batching_guest(take: usize, seen: Arc<Mutex<Vec<(usize, usize)>>>) -> Box<FakeGuest> {
        FakeGuest::new(move |request| {
            let Operation::Batch { ops } = request.op else {
                return None;
            };
            let frame = encode(&Request {
                id: 0,
                version: PROTOCOL_VERSION,
                op: Operation::Batch { ops: ops.clone() },
            })
            .unwrap();
            seen.lock().push((ops.len(), frame.len()));
            let results = ops
                .iter()
                .take(take)
                .map(|op| match op {
                    Operation::Stat { path, .. } if path.contains("bad") => OpResult::Failed {
                        message: format!("stat {path}: No such file or directory"),
                    },
                    Operation::Stat { path, .. } => OpResult::LinkTarget {
                        target: path.clone(),
                    },
                    _ => OpResult::Acknowledged,
                })
                .collect();
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Batch { results },
            })
        })
    }

    #[test]
    fn a_batch_returns_results_in_order_and_a_failure_is_its_own_entry() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut agent = GuestAgent::new(batching_guest(usize::MAX, seen.clone()));
        let t = Duration::from_secs(5);
        let results = agent
            .batch(vec![stat_op("/a"), stat_op("/bad"), stat_op("/c")], t)
            .unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0],
            OpResult::LinkTarget {
                target: "/a".into()
            }
        );
        assert!(matches!(&results[1], OpResult::Failed { message } if message.contains("/bad")));
        assert_eq!(
            results[2],
            OpResult::LinkTarget {
                target: "/c".into()
            }
        );
        assert_eq!(seen.lock().len(), 1, "one round trip");
        assert!(agent.batch(vec![], t).unwrap().is_empty());
        assert_eq!(seen.lock().len(), 1, "an empty batch sends nothing");
    }

    #[test]
    fn a_long_batch_is_split_at_the_operation_cap() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut agent = GuestAgent::new(batching_guest(usize::MAX, seen.clone()));
        let ops: Vec<Operation> = (0..1200).map(|i| stat_op(&format!("/f{i}"))).collect();
        let results = agent.batch(ops, Duration::from_secs(5)).unwrap();
        assert_eq!(results.len(), 1200);
        // In order across the split.
        assert_eq!(
            results[1199],
            OpResult::LinkTarget {
                target: "/f1199".into()
            }
        );
        let sizes: Vec<usize> = seen.lock().iter().map(|s| s.0).collect();
        assert_eq!(sizes, [512, 512, 176]);
    }

    #[test]
    fn a_batch_the_agent_stops_short_of_is_continued_without_repeats() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut agent = GuestAgent::new(batching_guest(2, seen.clone()));
        let ops: Vec<Operation> = (0..5).map(|i| stat_op(&format!("/f{i}"))).collect();
        let results = agent.batch(ops, Duration::from_secs(5)).unwrap();
        let targets: Vec<String> = results
            .into_iter()
            .map(|r| match r {
                OpResult::LinkTarget { target } => target,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(targets, ["/f0", "/f1", "/f2", "/f3", "/f4"]);
        let sizes: Vec<usize> = seen.lock().iter().map(|s| s.0).collect();
        assert_eq!(sizes, [5, 3, 1], "each resend holds only what was not run");
    }

    #[test]
    fn a_batch_of_large_writes_is_split_to_fit_the_request_budget() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut agent = GuestAgent::new(batching_guest(usize::MAX, seen.clone()));
        // 40 writes of 300 KiB of data each: about 16 MiB of base64 in all.
        let data = hv2_guest_agent::b64::encode(&vec![1u8; 300 * 1024]);
        let ops: Vec<Operation> = (0..40)
            .map(|i| Operation::WriteFile {
                path: format!("/w{i}"),
                data: data.clone(),
                append: false,
                owner: None,
            })
            .collect();
        let results = agent.batch(ops, Duration::from_secs(10)).unwrap();
        assert_eq!(results.len(), 40);
        assert!(results.iter().all(|r| *r == OpResult::Acknowledged));
        let seen = seen.lock();
        assert!(seen.len() >= 4, "{seen:?}");
        for (_, frame) in seen.iter() {
            assert!(*frame < MAX_FRAME_BYTES, "{frame}");
        }
        assert_eq!(seen.iter().map(|s| s.0).sum::<usize>(), 40);
    }

    #[test]
    fn only_filesystem_operations_are_sent_in_a_batch() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut agent = GuestAgent::new(batching_guest(usize::MAX, seen.clone()));
        let error = agent
            .batch(vec![stat_op("/a"), Operation::Ping], Duration::from_secs(1))
            .unwrap_err();
        assert!(error.to_string().contains("ping"), "{error}");
        assert!(!is_unsupported_op(&error));
        assert!(seen.lock().is_empty(), "nothing was sent");
    }

    #[test]
    fn a_whole_batch_refused_by_the_guest_is_an_error() {
        let mut agent = GuestAgent::new(FakeGuest::new(|request| {
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Failed {
                    message: "refusing a batch: nope".into(),
                },
            })
        }));
        let error = agent
            .batch(vec![stat_op("/a")], Duration::from_secs(1))
            .unwrap_err();
        assert!(error.to_string().contains("nope"));
        assert!(!is_unsupported_op(&error));
        // An agent that answers with nothing run, or more than was sent, is
        // not believed.
        let mut agent = GuestAgent::new(batching_guest(0, Arc::new(Mutex::new(Vec::new()))));
        assert!(agent
            .batch(vec![stat_op("/a")], Duration::from_secs(1))
            .is_err());
    }

    #[test]
    fn an_old_agent_dropping_the_connection_on_a_batch_is_an_unsupported_op() {
        let mut agent = GuestAgent::new(Box::new(HangsUpOnUnknown { open: true }));
        let error = agent
            .batch(vec![stat_op("/a")], Duration::from_secs(1))
            .unwrap_err();
        assert!(is_unsupported_op(&error), "{error}");
        assert!(error.to_string().contains("batch"));
    }

    #[test]
    fn mount_block_answers_come_through() {
        let mut agent = GuestAgent::new(FakeGuest::new(|request| {
            let result = match request.op {
                Operation::MountBlock {
                    expect_sectors: Some(9),
                    ..
                } => OpResult::DeviceSizeNotSeen,
                Operation::MountBlock { ref device, .. } if device == "/dev/vdb" => {
                    OpResult::Failed {
                        message: "mounting /dev/vdb: Invalid argument".into(),
                    }
                }
                _ => OpResult::Acknowledged,
            };
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result,
            })
        }));
        let t = Duration::from_secs(1);
        assert!(agent
            .mount_block("/dev/vda", "/d", Some(8), true, t)
            .unwrap());
        assert!(!agent
            .mount_block("/dev/vda", "/d", Some(9), true, t)
            .unwrap());
        let refused = agent
            .mount_block("/dev/vdb", "/d", None, false, t)
            .unwrap_err();
        assert!(refused.to_string().contains("Invalid argument"));
        assert!(!is_unsupported_op(&refused));
    }

    #[test]
    fn udp_forward_selects_udp_operation_and_propagates_guest_failure() {
        let agent = GuestAgent::new(FakeGuest::new(|request| {
            assert!(matches!(request.op, Operation::ForwardUdp { port: 5353 }));
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Failed {
                    message: "owned UDP setup refusal".into(),
                },
            })
        }));
        let error = agent
            .forward_udp(5353, Duration::from_secs(1))
            .err()
            .unwrap();
        assert!(error.to_string().contains("owned UDP setup refusal"));
        let agent = GuestAgent::new(FakeGuest::new(|_| panic!("port zero contacted guest")));
        assert!(agent
            .forward_udp(0, Duration::from_secs(1))
            .err()
            .unwrap()
            .to_string()
            .contains("nonzero"));
    }

    #[test]
    fn udp_ipv6_forward_selects_ipv6_operation_and_propagates_guest_failure() {
        let agent = GuestAgent::new(FakeGuest::new(|request| {
            assert!(matches!(request.op, Operation::ForwardUdp6 { port: 5353 }));
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Failed {
                    message: "owned UDP setup refusal".into(),
                },
            })
        }));
        let error = agent
            .forward_udp_ipv6(5353, Duration::from_secs(1))
            .err()
            .unwrap();
        assert!(error.to_string().contains("owned UDP setup refusal"));
        let agent = GuestAgent::new(FakeGuest::new(|_| panic!("port zero contacted guest")));
        assert!(agent
            .forward_udp_ipv6(0, Duration::from_secs(1))
            .err()
            .unwrap()
            .to_string()
            .contains("nonzero"));
    }

    #[test]
    fn restore_samples_time_after_channel_creation_and_preserves_entropy() {
        let lower_bound = Arc::new(Mutex::new(0u64));
        let observed_bound = lower_bound.clone();
        let mut agent = GuestAgent::new(FakeGuest::new(move |request| {
            match request.op {
                Operation::Restored {
                    unix_time_ns,
                    entropy,
                } => {
                    let received_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as u64;
                    assert!(unix_time_ns >= *observed_bound.lock());
                    assert!(unix_time_ns <= received_at);
                    assert_eq!(entropy, vec![0x5a; 64]);
                }
                other => panic!("unexpected operation {other:?}"),
            }
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Acknowledged,
            })
        }));
        *lower_bound.lock() = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        agent
            .restored_now(vec![0x5a; 64], Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn fresh_restore_does_not_hide_a_failed_rng_reseed() {
        let mut agent = GuestAgent::new(FakeGuest::new(|request| {
            Some(Response {
                id: request.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Failed {
                    message: "RNDRESEEDCRNG refused".into(),
                },
            })
        }));
        let error = agent
            .restored_now(vec![0x5a; 64], Duration::from_secs(1))
            .unwrap_err();
        assert!(error.to_string().contains("RNDRESEEDCRNG refused"));
    }

    #[test]
    fn a_command_runs_and_its_output_comes_back() {
        let mut agent = GuestAgent::new(FakeGuest::new(|request| match &request.op {
            Operation::Exec { program, args, .. } => {
                assert_eq!(program, "uname");
                assert_eq!(args, &["-r".to_string()]);
                Some(exited(request.id, 0, "6.1.0\n"))
            }
            other => panic!("unexpected operation {other:?}"),
        }));

        let out = agent
            .exec("uname", &["-r".to_string()], Duration::from_secs(1))
            .expect("exec");
        assert_eq!(out.stdout, "6.1.0\n");
        assert!(out.succeeded());
    }

    #[test]
    fn a_failing_command_is_a_result_not_an_error() {
        let mut agent = GuestAgent::new(FakeGuest::new(|r| Some(exited(r.id, 2, ""))));

        // Exiting non-zero is what the program did, not a failure to run it.
        // Turning it into an Err would lose the output that explains it.
        let out = agent
            .exec("false", &[], Duration::from_secs(1))
            .expect("exec should succeed at the protocol level");
        assert_eq!(out.exit_code, Some(2));
        assert!(!out.succeeded());
    }

    #[test]
    fn a_program_killed_by_a_signal_is_not_reported_as_exiting_zero() {
        let mut agent = GuestAgent::new(FakeGuest::new(|r| {
            Some(Response {
                id: r.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Exited {
                    exit_code: None,
                    signal: Some(9),
                    stdout: "partial".to_string(),
                    stderr: String::new(),
                    truncated: false,
                    timed_out: true,
                },
            })
        }));

        let out = agent
            .exec("sleep", &["999".to_string()], Duration::from_secs(1))
            .expect("exec");
        assert_eq!(out.exit_code, None);
        assert_eq!(out.signal, Some(9));
        assert!(out.timed_out);
        assert!(!out.succeeded());
        assert_eq!(
            out.stdout, "partial",
            "what it printed before dying is kept"
        );
    }

    #[test]
    fn an_agent_that_cannot_start_the_program_says_so() {
        let mut agent = GuestAgent::new(FakeGuest::new(|r| {
            Some(Response {
                id: r.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Failed {
                    message: "could not start nope: No such file or directory".to_string(),
                },
            })
        }));

        let err = agent
            .exec("nope", &[], Duration::from_secs(1))
            .expect_err("a program that does not exist is not an exit code");
        assert!(err.to_string().contains("No such file"), "got: {err}");
    }

    #[test]
    fn a_guest_that_never_answers_times_out_rather_than_hanging() {
        let mut agent = GuestAgent::new(FakeGuest::new(|_| None));

        let start = Instant::now();
        let err = agent
            .exec("sleep", &[], Duration::from_millis(150))
            .expect_err("silence must not hang");
        assert!(matches!(err, AgentError::Timeout(_)), "got: {err}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_stale_response_does_not_answer_the_current_request() {
        // The guest replies to request 1 with the id of a request that has
        // already timed out. Accepting it would report one command's output as
        // another's.
        let mut agent = GuestAgent::new(FakeGuest::new(|r| Some(exited(r.id - 1, 0, "stale"))));

        let err = agent
            .exec("echo", &[], Duration::from_millis(150))
            .expect_err("a mismatched id is not this answer");
        assert!(matches!(err, AgentError::Timeout(_)), "got: {err}");
    }

    #[test]
    fn a_version_mismatch_is_named_rather_than_misread() {
        let mut agent = GuestAgent::new(FakeGuest::new(|r| {
            Some(Response {
                id: r.id,
                version: PROTOCOL_VERSION + 1,
                result: OpResult::Pong {
                    agent_version: "9.9.9".to_string(),
                },
            })
        }));

        let err = agent
            .ping(Duration::from_millis(200))
            .expect_err("a newer agent is not silently understood");
        assert!(err.to_string().contains("protocol version"), "got: {err}");
    }

    #[test]
    fn a_request_is_written_across_several_credit_grants() {
        let mut guest = FakeGuest::new(|r| Some(exited(r.id, 0, "ok")));
        // The guest grants eight bytes at a time. A client that assumed one
        // send took the whole frame would truncate every request it made, and
        // credit this small is exactly what a busy guest grants.
        guest.credit = 8;
        let mut agent = GuestAgent::new(guest);

        let out = agent
            .exec("echo", &[], Duration::from_secs(1))
            .expect("a frame written in pieces is still one request");
        assert_eq!(out.stdout, "ok");
    }

    #[test]
    fn a_closed_channel_is_reported_as_closed_not_as_a_timeout() {
        let mut guest = FakeGuest::new(|_| None);
        guest.open = false;
        let mut agent = GuestAgent::new(guest);

        let err = agent
            .ping(Duration::from_millis(200))
            .expect_err("a closed channel is not a slow one");
        assert!(err.to_string().contains("closed"), "got: {err}");
    }

    #[test]
    fn the_guest_deadline_is_shorter_than_the_host_one() {
        // Otherwise the host gives up first and the operator gets silence
        // instead of the timeout report plus whatever the program printed.
        let mut agent = GuestAgent::new(FakeGuest::new(|request| {
            let Operation::Exec { timeout_ms, .. } = &request.op else {
                panic!("expected an exec");
            };
            assert!(
                *timeout_ms < 10_000,
                "the guest was given {timeout_ms}ms of a 10s host budget"
            );
            Some(exited(request.id, 0, ""))
        }));

        agent
            .exec("true", &[], Duration::from_secs(10))
            .expect("exec");
    }

    #[test]
    fn truncated_output_is_flagged_rather_than_passed_off_as_complete() {
        let mut agent = GuestAgent::new(FakeGuest::new(|r| {
            Some(Response {
                id: r.id,
                version: PROTOCOL_VERSION,
                result: OpResult::Exited {
                    exit_code: Some(0),
                    signal: None,
                    stdout: "x".repeat(MAX_OUTPUT_BYTES),
                    stderr: String::new(),
                    truncated: true,
                    timed_out: false,
                },
            })
        }));

        let out = agent
            .exec("yes", &[], Duration::from_secs(1))
            .expect("exec");
        assert!(
            out.truncated,
            "a caller reading this must know the tail is missing"
        );
    }
}
