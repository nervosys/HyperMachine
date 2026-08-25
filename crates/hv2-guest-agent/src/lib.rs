//! The protocol between the host and an agent running inside a guest.
//!
//! # Why this crate exists separately
//!
//! Both ends of this conversation have to agree on the bytes, and the two ends
//! are built for different machines: the host half is linked into
//! [`hv2-agent`](../hv2_agent/index.html) on whatever the host is, and the
//! guest half is a Linux binary that ships inside the VM image. Defining the
//! frames in one crate that both depend on is what keeps a change to the
//! request type from silently meaning two different things.
//!
//! The library half builds anywhere. Only the binary needs `AF_VSOCK`, and it
//! is Linux-only.
//!
//! # Framing
//!
//! A four-byte little-endian length followed by that many bytes of JSON. A
//! stream socket gives no message boundaries, so the length has to be on the
//! wire; JSON because a request is small, rare, and worth being able to read
//! in a packet dump.
//!
//! ```text
//! len u32 | { "id": 1, "op": {...} }
//! ```
//!
//! # What this protocol deliberately does not do
//!
//! There is no streaming: a command runs to completion and its output comes
//! back in one response. That is the right shape for "run this and tell me
//! what happened" and the wrong one for an interactive shell, and pretending
//! otherwise is how `execute_script` came to be described as something it was
//! not. A follow-on protocol version can add streaming; this one says plainly
//! that it has none.

use serde::{Deserialize, Serialize};

/// Port the guest agent listens on.
///
/// Above 1023, so the agent does not need to be root to bind it.
pub const GUEST_AGENT_PORT: u32 = 1024;

/// Largest frame either side will send or accept, in bytes.
///
/// Output from a command is guest-controlled, so the ceiling exists on both
/// ends: the agent truncates what it sends, and the host refuses what it did
/// not expect. A guest that says "my reply is 4 GiB long" must not be able to
/// make the host allocate it.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Bytes of captured output the agent returns per stream before truncating.
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Protocol version, sent in every request and checked by the agent.
///
/// A guest image outlives the host that built it. Without this, an old agent
/// meeting a new host fails by misreading a field rather than by saying so.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from the host to the guest agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Request {
    /// Correlates a response with its request.
    pub id: u64,
    /// Protocol version the host is speaking.
    pub version: u32,
    /// What to do.
    pub op: Operation,
}

/// What the host is asking for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    /// Confirm the agent is alive and report what it is.
    Ping,
    /// Run a program and return what it printed.
    ///
    /// `program` is executed directly, not through a shell: there is no shell
    /// parsing here, so `ls > out` redirects nothing. A caller that wants a
    /// shell asks for one by name, and does so knowingly.
    Exec {
        program: String,
        #[serde(default)]
        args: Vec<String>,
        /// Working directory, or the agent's own if absent.
        #[serde(default)]
        cwd: Option<String>,
        /// Bytes to write to the program's standard input.
        #[serde(default)]
        stdin: Option<String>,
        /// How long the agent will wait before killing the program.
        timeout_ms: u64,
    },
}

/// The guest agent's answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Response {
    /// The `id` of the request this answers.
    pub id: u64,
    /// Protocol version the agent is speaking.
    pub version: u32,
    /// The outcome.
    pub result: OpResult,
}

/// What happened.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpResult {
    /// Answer to [`Operation::Ping`].
    Pong {
        /// Agent build identifier, for telling one guest image from another.
        agent_version: String,
    },
    /// A program ran. Note that this is success at the protocol level: the
    /// program itself may have failed, which `exit_code` reports.
    Exited {
        /// Exit status, or `None` when a signal ended the program — which is
        /// distinct from exiting 0 and must not be flattened into one.
        exit_code: Option<i32>,
        /// Signal that killed the program, if one did.
        signal: Option<i32>,
        stdout: String,
        stderr: String,
        /// Whether either stream hit [`MAX_OUTPUT_BYTES`] and was cut short.
        truncated: bool,
        /// Whether the agent killed the program for exceeding its timeout.
        timed_out: bool,
    },
    /// The request could not be carried out at all.
    Failed { message: String },
}

/// Errors from encoding or decoding a frame.
#[derive(Debug)]
pub enum FrameError {
    /// The frame is longer than [`MAX_FRAME_BYTES`].
    TooLarge(usize),
    /// Fewer bytes are present than the length prefix promises.
    Incomplete,
    /// The body is not the JSON this protocol expects.
    Malformed(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(len) => {
                write!(
                    f,
                    "frame of {len} bytes exceeds the {MAX_FRAME_BYTES}-byte limit"
                )
            }
            Self::Incomplete => write!(f, "frame is incomplete"),
            Self::Malformed(e) => write!(f, "frame is not valid protocol JSON: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Encode a value as a length-prefixed frame.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(value).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(body.len()));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Try to decode one frame from the front of `buf`.
///
/// Returns the value and how many bytes it consumed, or `Ok(None)` when the
/// frame has not fully arrived — the normal case on a stream socket, and the
/// reason this takes a buffer rather than a reader.
pub fn decode<T: for<'de> Deserialize<'de>>(buf: &[u8]) -> Result<Option<(T, usize)>, FrameError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_le_bytes(buf[0..4].try_into().expect("4 bytes")) as usize;
    if len > MAX_FRAME_BYTES {
        // Refuse before allocating: the length is written by the other end.
        return Err(FrameError::TooLarge(len));
    }
    if buf.len() < 4 + len {
        return Ok(None);
    }
    let value = serde_json::from_slice(&buf[4..4 + len])
        .map_err(|e| FrameError::Malformed(e.to_string()))?;
    Ok(Some((value, 4 + len)))
}

/// Cut `s` to at most `limit` bytes, on a character boundary.
///
/// Returns the text and whether anything was removed. Truncating a `String` by
/// byte index panics mid-character, and command output is arbitrary bytes.
pub fn truncate_utf8(s: &str, limit: usize) -> (String, bool) {
    if s.len() <= limit {
        return (s.to_string(), false);
    }
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec_request() -> Request {
        Request {
            id: 7,
            version: PROTOCOL_VERSION,
            op: Operation::Exec {
                program: "uname".to_string(),
                args: vec!["-r".to_string()],
                cwd: None,
                stdin: None,
                timeout_ms: 5_000,
            },
        }
    }

    #[test]
    fn a_frame_survives_a_round_trip() {
        let request = exec_request();
        let bytes = encode(&request).expect("encode");
        let (decoded, used) = decode::<Request>(&bytes).expect("decode").expect("a frame");
        assert_eq!(decoded, request);
        assert_eq!(used, bytes.len());
    }

    #[test]
    fn a_partial_frame_is_not_an_error() {
        let bytes = encode(&exec_request()).expect("encode");

        // A stream socket hands over whatever has arrived. Treating a short
        // read as a failure would drop every request split across two packets.
        for cut in [0, 1, 3, 4, bytes.len() - 1] {
            assert!(
                decode::<Request>(&bytes[..cut]).expect("decode").is_none(),
                "{cut} bytes should read as incomplete, not as an error"
            );
        }
    }

    #[test]
    fn two_frames_in_one_buffer_are_read_one_at_a_time() {
        let mut buf = encode(&exec_request()).expect("encode");
        let first_len = buf.len();
        buf.extend_from_slice(&encode(&exec_request()).expect("encode"));

        let (_, used) = decode::<Request>(&buf).expect("decode").expect("a frame");
        assert_eq!(used, first_len);
        assert!(decode::<Request>(&buf[used..]).expect("decode").is_some());
    }

    #[test]
    fn a_length_beyond_the_limit_is_refused_before_anything_is_allocated() {
        // The length prefix is written by the other end. Believing it is how a
        // peer gets to choose how much memory this process uses.
        let mut buf = (u32::MAX).to_le_bytes().to_vec();
        buf.extend_from_slice(b"{}");
        assert!(matches!(
            decode::<Request>(&buf),
            Err(FrameError::TooLarge(_))
        ));
    }

    #[test]
    fn a_body_that_is_not_this_protocol_is_reported_as_such() {
        let body = b"{\"nope\":true}";
        let mut buf = (body.len() as u32).to_le_bytes().to_vec();
        buf.extend_from_slice(body);
        assert!(matches!(
            decode::<Request>(&buf),
            Err(FrameError::Malformed(_))
        ));
    }

    #[test]
    fn an_exit_code_and_a_signal_stay_distinguishable() {
        // A program killed by SIGKILL did not exit 0, and a response that
        // flattened the two would report a crash as a success.
        let killed = OpResult::Exited {
            exit_code: None,
            signal: Some(9),
            stdout: String::new(),
            stderr: String::new(),
            truncated: false,
            timed_out: true,
        };
        let json = serde_json::to_string(&killed).expect("encode");
        let back: OpResult = serde_json::from_str(&json).expect("decode");
        assert_eq!(back, killed);
    }

    #[test]
    fn truncation_cuts_on_a_character_boundary() {
        // Command output is arbitrary bytes; slicing a String by byte index
        // panics in the middle of a multi-byte character.
        let s = "aé😀";
        for limit in 0..s.len() + 2 {
            let (cut, truncated) = truncate_utf8(s, limit);
            assert!(cut.len() <= limit.min(s.len()));
            assert_eq!(truncated, limit < s.len());
            assert!(s.starts_with(&cut));
        }
    }
}
