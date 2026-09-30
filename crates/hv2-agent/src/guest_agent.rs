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
                     connection(s) in {timeout:?} without accepting one. It is listening but \
                     never free, most likely still serving a caller that has not finished"
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
        self.detached = true;
        Some(VsockStream {
            inner: Arc::new(StreamInner {
                device: Arc::clone(&self.device),
                id: self.id,
                progress: Arc::clone(&self.progress),
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
                let open = matches!(
                    device.state(self.inner.id),
                    Some(VsockConnectionState::Established)
                );
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
            } => Ok(GuestOutput {
                stdout,
                stderr,
                pty,
                running,
                exit_code,
                signal,
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
    pub fn forward(mut self, port: u16, timeout: Duration) -> Result<(VsockStream, Vec<u8>)> {
        match self.request(Operation::Forward { port }, timeout)? {
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
