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
    decode, encode, OpResult, Operation, Request, Response, GUEST_AGENT_PORT, MAX_FRAME_BYTES,
    PROTOCOL_VERSION,
};
use parking_lot::Mutex;
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
}

/// A [`GuestChannel`] over one vsock connection.
pub struct VsockChannel {
    device: Arc<Mutex<VsockDevice>>,
    id: VsockConnectionId,
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

        loop {
            let id = device.lock().connect_ephemeral(GUEST_AGENT_PORT)?;

            match Self::settle(&device, id, deadline) {
                Settled::Established => return Ok(Self { device, id }),
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
        id: VsockConnectionId,
        deadline: Instant,
    ) -> Settled {
        loop {
            match device.lock().state(id) {
                Some(VsockConnectionState::Established) => return Settled::Established,
                Some(VsockConnectionState::Connecting) => {}
                _ => return Settled::Refused,
            }
            if Instant::now() >= deadline {
                return Settled::Deadline;
            }
            std::thread::sleep(POLL_INTERVAL);
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
                std::thread::sleep(POLL_INTERVAL);
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
            std::thread::sleep(POLL_INTERVAL);
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
