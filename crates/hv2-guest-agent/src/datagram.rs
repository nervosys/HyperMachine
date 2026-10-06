//! Bounded datagram framing for stream transports.
//!
//! Each message is a big-endian u16 length followed by exactly that many
//! bytes. Empty datagrams are messages; EOF before a new prefix ends a stream.
use std::io::{self, Read, Write};

/// Maximum UDP payload for an IPv4 packet without IP options.
pub const MAX_PAYLOAD: usize = 65_507;

/// Read one complete message. Partial prefixes and payloads are errors.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0; 2];
    loop {
        match reader.read(&mut prefix[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    reader.read_exact(&mut prefix[1..])?;
    let size = u16::from_be_bytes(prefix) as usize;
    if size > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "datagram exceeds IPv4 UDP payload limit",
        ));
    }
    let mut payload = vec![0; size];
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// Write a single message, refusing oversize input before emitting any bytes.
pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "datagram exceeds IPv4 UDP payload limit",
        ));
    }
    writer.write_all(&(payload.len() as u16).to_be_bytes())?;
    writer.write_all(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn concatenated_empty_binary_and_maximum_frames_preserve_boundaries() {
        let messages = [Vec::new(), vec![0, 255, 13, 10], vec![42; MAX_PAYLOAD]];
        let mut wire = Vec::new();
        for message in &messages {
            write_frame(&mut wire, message).unwrap();
        }
        let mut reader = Cursor::new(wire);
        for message in messages {
            assert_eq!(read_frame(&mut reader).unwrap(), Some(message));
        }
        assert_eq!(read_frame(&mut reader).unwrap(), None);
    }

    #[test]
    fn truncated_prefix_and_payload_are_not_clean_eof() {
        for bytes in [vec![0], vec![0, 2, 42]] {
            assert_eq!(
                read_frame(&mut Cursor::new(bytes)).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[test]
    fn oversize_frames_are_refused_without_reading_or_writing_payload() {
        let mut reader = Cursor::new(vec![255, 255, 42]);
        assert_eq!(
            read_frame(&mut reader).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(reader.position(), 2);
        let mut wire = vec![7];
        assert_eq!(
            write_frame(&mut wire, &vec![0; MAX_PAYLOAD + 1])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(wire, vec![7]);
    }
}
