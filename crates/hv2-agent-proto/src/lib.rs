//! What an agent and its host say to each other.
//!
//! A stream of frames, each a twelve-byte header and a payload. `no_std`, no
//! allocation, and shared by both sides so the format cannot drift — the guest
//! encodes with the same code the host decodes with, which is the arrangement
//! the Multiboot module list did not have and paid for.
//!
//! # Why there is a length at all
//!
//! Because a vsock connection is a stream and not a sequence of messages. Two
//! replies sent in quick succession arrive as one run of bytes, and a reader
//! that assumes otherwise reads the second request as the tail of the first.
//! That is not hypothetical: an earlier version of the agent examples used
//! prefixes and no framing, and a refused message appeared to have been
//! delivered because its text was still sitting in the buffer when the next,
//! permitted message was read.
//!
//! # Why there is an id
//!
//! Because an agent with tools has more than one thing outstanding. A tool call
//! and a message from another agent can be in flight at once, and their replies
//! can arrive in either order; without an id the only way to match a reply to
//! its request is to allow one at a time, which is a protocol decision
//! masquerading as an omission.
//!
//! # What is deliberately absent
//!
//! No versioning, no negotiation, no compression, no fragmentation. This is the
//! channel between a sandbox and the hypervisor that created it, so both ends
//! ship together and there is no version skew to negotiate. Adding those would
//! be adding surface to the one interface a minimal sandbox exposes.

#![no_std]

/// Bytes in a frame header.
pub const HEADER_LEN: usize = 12;

/// What a frame is for.
///
/// The discriminants are fixed. A guest and a host that disagree about them
/// disagree about everything, so they are written here once and never derived
/// from ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    /// Host to agent: do this.
    Task = 1,
    /// Agent to host: invoke a tool. The payload is the tool name and its
    /// arguments.
    ToolCall = 2,
    /// Host to agent: what the tool returned.
    ToolResult = 3,
    /// Agent to host: send this to another agent. The payload names the
    /// recipient, then the message.
    Send = 4,
    /// Host to agent: a message from another agent, admitted by the graph.
    Deliver = 5,
    /// Either way: this could not be done, and the payload says why.
    ///
    /// A refusal is an `Error` and not an absence, on the side that asked. The
    /// *recipient* of a refused message still sees nothing at all, which is the
    /// property the swarm is for; telling the sender is a courtesy to the
    /// sender and not a weakening of that.
    Error = 6,
}

impl Kind {
    /// The kind with this discriminant, or `None` if it is not one.
    ///
    /// Unknown kinds are refused rather than ignored. A guest that receives a
    /// frame it does not understand has been sent something by a host it does
    /// not agree with, and continuing on that basis is worse than stopping.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Task),
            2 => Some(Self::ToolCall),
            3 => Some(Self::ToolResult),
            4 => Some(Self::Send),
            5 => Some(Self::Deliver),
            6 => Some(Self::Error),
            _ => None,
        }
    }
}

/// A frame's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Correlates a reply with its request. Echoed unchanged in the reply.
    pub id: u32,
    /// What this frame is for.
    pub kind: Kind,
    /// Payload bytes following this header.
    pub len: u32,
}

impl Header {
    /// A header for `kind` with `len` payload bytes.
    pub fn new(id: u32, kind: Kind, len: u32) -> Self {
        Self { id, kind, len }
    }

    /// Write this header into `out`.
    ///
    /// Little-endian throughout, because both ends are x86 and a byte order
    /// nobody has to convert is a byte order nobody gets wrong.
    pub fn encode(&self, out: &mut [u8; HEADER_LEN]) {
        out[0..4].copy_from_slice(&self.id.to_le_bytes());
        out[4..6].copy_from_slice(&(self.kind as u16).to_le_bytes());
        // Two bytes reserved. Written as zero and not read, so that a later
        // field has somewhere to go without moving `len`.
        out[6..8].copy_from_slice(&0u16.to_le_bytes());
        out[8..12].copy_from_slice(&self.len.to_le_bytes());
    }

    /// Read a header from the start of `bytes`.
    ///
    /// Returns `None` if there are not enough bytes, or if the kind is one this
    /// build does not know.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < HEADER_LEN {
            return None;
        }
        let id = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let kind = Kind::from_u16(u16::from_le_bytes([bytes[4], bytes[5]]))?;
        let len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        Some(Self { id, kind, len })
    }
}

/// One complete frame found at the start of `bytes`.
///
/// Returns the header and the payload, or `None` if the whole frame has not
/// arrived yet — which is the ordinary case on a stream and not an error.
pub fn parse(bytes: &[u8]) -> Option<(Header, &[u8])> {
    let header = Header::decode(bytes)?;
    let end = HEADER_LEN.checked_add(header.len as usize)?;
    if bytes.len() < end {
        return None;
    }
    Some((header, &bytes[HEADER_LEN..end]))
}

/// How many bytes a frame with `payload_len` bytes occupies.
pub fn frame_len(payload_len: usize) -> usize {
    HEADER_LEN + payload_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_survives_a_round_trip() {
        let header = Header::new(7, Kind::ToolCall, 42);
        let mut bytes = [0u8; HEADER_LEN];
        header.encode(&mut bytes);
        assert_eq!(Header::decode(&bytes), Some(header));
    }

    #[test]
    fn a_short_buffer_is_not_a_header() {
        let mut bytes = [0u8; HEADER_LEN];
        Header::new(1, Kind::Task, 0).encode(&mut bytes);
        for n in 0..HEADER_LEN {
            assert_eq!(Header::decode(&bytes[..n]), None, "{n} bytes is not enough");
        }
    }

    #[test]
    fn an_unknown_kind_is_refused_rather_than_guessed() {
        let mut bytes = [0u8; HEADER_LEN];
        Header::new(1, Kind::Task, 0).encode(&mut bytes);
        bytes[4] = 99;
        assert_eq!(
            Header::decode(&bytes),
            None,
            "a kind this build does not know should not decode as one it does"
        );
    }

    /// The bug this format exists to prevent: two frames in one buffer must
    /// read as two frames, not as one long one.
    #[test]
    fn two_frames_in_one_buffer_are_two_frames() {
        let mut buffer = [0u8; 2 * (HEADER_LEN + 5)];

        Header::new(1, Kind::Send, 5).encode((&mut buffer[..HEADER_LEN]).try_into().unwrap());
        buffer[HEADER_LEN..HEADER_LEN + 5].copy_from_slice(b"first");

        let second = HEADER_LEN + 5;
        Header::new(2, Kind::Send, 5).encode(
            (&mut buffer[second..second + HEADER_LEN])
                .try_into()
                .unwrap(),
        );
        buffer[second + HEADER_LEN..second + HEADER_LEN + 5].copy_from_slice(b"secnd");

        let (one, payload) = parse(&buffer).expect("the first frame");
        assert_eq!(one.id, 1);
        assert_eq!(payload, b"first");

        let (two, payload) = parse(&buffer[frame_len(5)..]).expect("the second frame");
        assert_eq!(two.id, 2);
        assert_eq!(payload, b"secnd");
    }

    #[test]
    fn half_a_frame_is_not_yet_a_frame() {
        let mut buffer = [0u8; HEADER_LEN + 8];
        Header::new(1, Kind::Task, 8).encode((&mut buffer[..HEADER_LEN]).try_into().unwrap());
        for n in 0..buffer.len() {
            assert!(
                parse(&buffer[..n]).is_none(),
                "{n} of {} bytes should not parse as a whole frame",
                buffer.len()
            );
        }
        assert!(parse(&buffer).is_some(), "all of it should");
    }
}
