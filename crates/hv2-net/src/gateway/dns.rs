//! Just enough DNS to answer a guest's stub resolver.
//!
//! One question per query, `A` and `AAAA` only, answered from the host's
//! resolver. Every other question type is answered `NOTIMP`, and a query the
//! policy will not resolve is answered `REFUSED` -- see
//! [`super::Gateway`] for why resolving is itself a policy decision.

use std::net::IpAddr;

/// `A`.
pub const TYPE_A: u16 = 1;
/// `AAAA`.
pub const TYPE_AAAA: u16 = 28;

/// Response codes used here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rcode {
    NoError = 0,
    ServFail = 2,
    NxDomain = 3,
    NotImp = 4,
    Refused = 5,
}

/// A parsed question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub id: u16,
    /// The flags word, whose RD bit is echoed back.
    pub flags: u16,
    pub name: String,
    pub qtype: u16,
    /// The question section exactly as sent, so the answer repeats it
    /// byte-for-byte and a stub resolver matching on it is not surprised.
    question: Vec<u8>,
}

/// Parse a query, or `None` for anything that is not a single-question query.
#[must_use]
pub fn parse_query(packet: &[u8]) -> Option<Query> {
    if packet.len() < 12 {
        return None;
    }
    let id = u16::from_be_bytes([packet[0], packet[1]]);
    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    let qdcount = u16::from_be_bytes([packet[4], packet[5]]);
    // QR must be 0 (a query) and OPCODE 0 (standard).
    if flags & 0x8000 != 0 || (flags >> 11) & 0xf != 0 || qdcount != 1 {
        return None;
    }
    let mut at = 12;
    let mut labels = Vec::new();
    loop {
        let len = usize::from(*packet.get(at)?);
        at += 1;
        if len == 0 {
            break;
        }
        // No compression pointers in a question from a stub resolver, and
        // following one is a loop a hostile packet gets to write.
        if len > 63 {
            return None;
        }
        let label = packet.get(at..at + len)?;
        labels.push(std::str::from_utf8(label).ok()?.to_ascii_lowercase());
        at += len;
    }
    let qtype = u16::from_be_bytes([*packet.get(at)?, *packet.get(at + 1)?]);
    at += 4; // qtype + qclass
    if packet.len() < at {
        return None;
    }
    let name = labels.join(".");
    if name.len() > 253 {
        return None;
    }
    Some(Query {
        id,
        flags,
        name,
        qtype,
        question: packet[12..at].to_vec(),
    })
}

/// Build the answer to `query`.
#[must_use]
pub fn answer(query: &Query, rcode: Rcode, addresses: &[IpAddr], ttl: u32) -> Vec<u8> {
    let records: Vec<&IpAddr> = addresses
        .iter()
        .filter(|a| match query.qtype {
            TYPE_A => a.is_ipv4(),
            TYPE_AAAA => a.is_ipv6(),
            _ => false,
        })
        .collect();
    let records = if rcode == Rcode::NoError {
        records
    } else {
        Vec::new()
    };

    let mut out = Vec::with_capacity(12 + query.question.len() + records.len() * 28);
    out.extend_from_slice(&query.id.to_be_bytes());
    // QR, the query's RD, RA, and the code.
    let flags = 0x8000 | (query.flags & 0x0100) | 0x0080 | rcode as u16;
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(records.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&query.question);
    for address in records {
        out.extend_from_slice(&[0xc0, 0x0c]); // the name in the question
        let (kind, data): (u16, Vec<u8>) = match address {
            IpAddr::V4(v4) => (TYPE_A, v4.octets().to_vec()),
            IpAddr::V6(v6) => (TYPE_AAAA, v6.octets().to_vec()),
        };
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // IN
        out.extend_from_slice(&ttl.to_be_bytes());
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(&data);
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
        let mut q = Vec::new();
        q.extend_from_slice(&id.to_be_bytes());
        q.extend_from_slice(&0x0100u16.to_be_bytes());
        q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    #[test]
    fn round_trips_a_question_and_its_answer() {
        let q = parse_query(&query(0x1234, "Example.COM", TYPE_A)).unwrap();
        assert_eq!(q.name, "example.com");
        let out = answer(
            &q,
            Rcode::NoError,
            &[
                "93.184.216.34".parse().unwrap(),
                "2606:2800::1".parse().unwrap(),
            ],
            30,
        );
        assert_eq!(&out[0..2], &[0x12, 0x34]);
        assert_eq!(u16::from_be_bytes([out[6], out[7]]), 1, "only the A record");
        assert_eq!(&out[out.len() - 4..], &[93, 184, 216, 34]);
        assert_eq!(out[3] & 0x0f, 0);
    }

    #[test]
    fn refusals_carry_no_answers() {
        let q = parse_query(&query(1, "exfil.attacker.example", TYPE_A)).unwrap();
        let out = answer(&q, Rcode::Refused, &["1.2.3.4".parse().unwrap()], 30);
        assert_eq!(out[3] & 0x0f, 5);
        assert_eq!(u16::from_be_bytes([out[6], out[7]]), 0);
    }

    #[test]
    fn refuses_to_parse_responses_and_compression() {
        let mut response = query(1, "a.com", TYPE_A);
        response[2] |= 0x80;
        assert!(parse_query(&response).is_none());
        let mut pointer = query(1, "a.com", TYPE_A);
        pointer[12] = 0xc0;
        assert!(parse_query(&pointer).is_none());
        assert!(parse_query(&[0; 5]).is_none());
    }
}
