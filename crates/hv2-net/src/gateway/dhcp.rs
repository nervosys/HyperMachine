//! A DHCP server for the one guest behind a gateway.
//!
//! A guest booted with this project's kernel is told its address on the
//! kernel command line and never asks. A stock operating system asks: it
//! broadcasts a DHCPDISCOVER and waits. This answers it, with the address,
//! router and resolver the gateway already has for that guest, so there is
//! nothing to allocate and nothing to remember: every guest has its own
//! gateway, and every gateway has one address to give.
//!
//! It is a function from a frame to a frame, ahead of the IP stack. A client
//! with no address yet sends from `0.0.0.0` to the broadcast address, which a
//! stack bound to the gateway's address has no socket for; and the reply goes
//! to a host that cannot yet be reached by IP. Building the two frames by
//! hand is less than teaching the stack either.
//!
//! What it answers: DISCOVER with OFFER and REQUEST with ACK, for any client
//! on the link, since there is one. Everything else -- RELEASE, DECLINE,
//! INFORM, a relayed request -- is left to the stack, which drops it.

use std::net::Ipv4Addr;

use super::GatewayConfig;

const ETHERNET_LEN: usize = 14;
const UDP_LEN: usize = 8;
/// A BOOTP message up to its options.
const BOOTP_LEN: usize = 236;
const MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];
/// The least a BOOTP message may be; shorter replies are padded.
const MIN_MESSAGE: usize = 300;

const SERVER_PORT: u16 = 67;
const CLIENT_PORT: u16 = 68;

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;

/// How long a lease lasts, in seconds. A day: the address never changes, so
/// a renewal only confirms it.
const LEASE_SECS: u32 = 86_400;

/// The reply to `frame`, if it is a DHCPDISCOVER or DHCPREQUEST.
#[must_use]
pub fn reply(frame: &[u8], config: &GatewayConfig) -> Option<Vec<u8>> {
    // Ethernet, carrying IPv4.
    if frame.len() < ETHERNET_LEN || frame[12..14] != [0x08, 0x00] {
        return None;
    }
    let ip = &frame[ETHERNET_LEN..];
    if ip.len() < 20 || ip[0] >> 4 != 4 || ip[9] != 17 {
        return None;
    }
    // A later fragment has no UDP header to read.
    if u16::from_be_bytes([ip[6], ip[7]]) & 0x1fff != 0 {
        return None;
    }
    let header = usize::from(ip[0] & 0x0f) * 4;
    let udp = ip.get(header..)?;
    if udp.len() < UDP_LEN
        || u16::from_be_bytes([udp[0], udp[1]]) != CLIENT_PORT
        || u16::from_be_bytes([udp[2], udp[3]]) != SERVER_PORT
    {
        return None;
    }
    let request = &udp[UDP_LEN..];
    // A request, over Ethernet, not relayed, with DHCP's options.
    if request.len() < BOOTP_LEN + MAGIC_COOKIE.len()
        || request[0] != 1
        || request[1] != 1
        || request[2] != 6
        || request[24..28] != [0, 0, 0, 0]
        || request[BOOTP_LEN..BOOTP_LEN + 4] != MAGIC_COOKIE
    {
        return None;
    }
    let kind = match message_type(&request[BOOTP_LEN + 4..])? {
        DISCOVER => OFFER,
        REQUEST => ACK,
        _ => return None,
    };

    let mut message = vec![0u8; BOOTP_LEN];
    message[0] = 2; // a reply
    message[1] = 1;
    message[2] = 6;
    message[4..8].copy_from_slice(&request[4..8]); // the client's transaction
    message[10..12].copy_from_slice(&request[10..12]); // and its flags
    message[16..20].copy_from_slice(&config.guest.octets()); // your address
    message[20..24].copy_from_slice(&config.gateway.octets()); // the server
    message[28..44].copy_from_slice(&request[28..44]); // the client's hardware
    message.extend_from_slice(&MAGIC_COOKIE);
    let mask = u32::MAX
        .checked_shl(32 - u32::from(config.prefix))
        .unwrap_or(0);
    let mut option = |code: u8, value: &[u8]| {
        message.push(code);
        message.push(value.len() as u8);
        message.extend_from_slice(value);
    };
    option(53, &[kind]);
    option(54, &config.gateway.octets());
    option(51, &LEASE_SECS.to_be_bytes());
    option(1, &mask.to_be_bytes());
    option(3, &config.gateway.octets());
    option(6, &config.dns.octets());
    message.push(255);
    if message.len() < MIN_MESSAGE {
        message.resize(MIN_MESSAGE, 0);
    }

    // To everyone on the link: the client has no address to be sent to yet,
    // and a broadcast is what every client accepts.
    let total = 20 + UDP_LEN + message.len();
    let mut out = Vec::with_capacity(ETHERNET_LEN + total);
    out.extend_from_slice(&[0xff; 6]);
    out.extend_from_slice(&config.mac);
    out.extend_from_slice(&[0x08, 0x00]);
    let mut header = [0u8; 20];
    header[0] = 0x45;
    header[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    header[8] = 64; // time to live
    header[9] = 17; // UDP
    header[12..16].copy_from_slice(&config.gateway.octets());
    header[16..20].copy_from_slice(&Ipv4Addr::BROADCAST.octets());
    let sum = checksum(&header);
    header[10..12].copy_from_slice(&sum.to_be_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&SERVER_PORT.to_be_bytes());
    out.extend_from_slice(&CLIENT_PORT.to_be_bytes());
    out.extend_from_slice(&((UDP_LEN + message.len()) as u16).to_be_bytes());
    // No UDP checksum, which IPv4 allows and DHCP clients accept.
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&message);
    Some(out)
}

/// Whether `frame` is an ARP request for `address`.
///
/// A DHCP client probes the address it was offered before it uses it
/// (RFC 5227): it asks who has it, and takes any answer as someone else
/// holding it.
#[must_use]
pub fn asks_for(frame: &[u8], address: Ipv4Addr) -> bool {
    // Ethernet carrying ARP for IPv4 over Ethernet, a request, for `address`.
    frame.len() >= ETHERNET_LEN + 28
        && frame[12..14] == [0x08, 0x06]
        && frame[ETHERNET_LEN..ETHERNET_LEN + 6] == [0, 1, 0x08, 0x00, 6, 4]
        && frame[ETHERNET_LEN + 6..ETHERNET_LEN + 8] == [0, 1]
        && frame[ETHERNET_LEN + 24..ETHERNET_LEN + 28] == address.octets()
}

/// The DHCP message type in `options`, if they hold one.
fn message_type(options: &[u8]) -> Option<u8> {
    let mut at = 0;
    while at < options.len() {
        match options[at] {
            0 => at += 1,
            255 => return None,
            code => {
                let len = usize::from(*options.get(at + 1)?);
                let value = options.get(at + 2..at + 2 + len)?;
                if code == 53 {
                    return value.first().copied();
                }
                at += 2 + len;
            }
        }
    }
    None
}

/// The Internet checksum of an IPv4 header whose checksum field is zero.
fn checksum(header: &[u8]) -> u16 {
    let mut sum: u32 = header
        .chunks(2)
        .map(|pair| u32::from(u16::from_be_bytes([pair[0], pair[1]])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: [u8; 6] = [0x52, 0x54, 0x00, 0x00, 0x00, 0x02];

    /// A client's broadcast, as a guest with no address sends it.
    fn request(kind: u8) -> Vec<u8> {
        let mut message = vec![0u8; BOOTP_LEN];
        message[0] = 1;
        message[1] = 1;
        message[2] = 6;
        message[4..8].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        message[10] = 0x80; // please broadcast the reply
        message[28..34].copy_from_slice(&CLIENT);
        message.extend_from_slice(&MAGIC_COOKIE);
        // A hostname first, so the type is not the first option.
        message.extend_from_slice(&[12, 3, b'v', b'm', b'1', 53, 1, kind, 55, 2, 1, 3, 255]);
        let total = 20 + UDP_LEN + message.len();
        let mut frame = Vec::new();
        frame.extend_from_slice(&[0xff; 6]);
        frame.extend_from_slice(&CLIENT);
        frame.extend_from_slice(&[0x08, 0x00]);
        let mut header = [0u8; 20];
        header[0] = 0x45;
        header[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        header[9] = 17;
        header[16..20].copy_from_slice(&[255; 4]);
        frame.extend_from_slice(&header);
        frame.extend_from_slice(&CLIENT_PORT.to_be_bytes());
        frame.extend_from_slice(&SERVER_PORT.to_be_bytes());
        frame.extend_from_slice(&((UDP_LEN + message.len()) as u16).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&message);
        frame
    }

    /// The option `code` in a reply frame.
    fn option(frame: &[u8], code: u8) -> Option<Vec<u8>> {
        let options = &frame[ETHERNET_LEN + 20 + UDP_LEN + BOOTP_LEN + 4..];
        let mut at = 0;
        while at < options.len() && options[at] != 255 {
            if options[at] == 0 {
                at += 1;
                continue;
            }
            let len = usize::from(options[at + 1]);
            if options[at] == code {
                return Some(options[at + 2..at + 2 + len].to_vec());
            }
            at += 2 + len;
        }
        None
    }

    #[test]
    fn a_discover_is_offered_the_guests_address_and_a_request_acknowledged() {
        let config = GatewayConfig::default();
        for (asked, answered) in [(DISCOVER, OFFER), (REQUEST, ACK)] {
            let frame = reply(&request(asked), &config).expect("a reply");
            // To everyone, from the gateway.
            assert_eq!(&frame[..6], &[0xff; 6]);
            assert_eq!(&frame[6..12], &config.mac);
            let ip = &frame[ETHERNET_LEN..ETHERNET_LEN + 20];
            assert_eq!(&ip[12..16], &config.gateway.octets());
            assert_eq!(&ip[16..20], &[255; 4]);
            // A header whose checksum is right sums to all ones.
            let mut whole: u32 = ip
                .chunks(2)
                .map(|pair| u32::from(u16::from_be_bytes([pair[0], pair[1]])))
                .sum();
            while whole > 0xffff {
                whole = (whole & 0xffff) + (whole >> 16);
            }
            assert_eq!(whole, 0xffff);
            assert_eq!(
                usize::from(u16::from_be_bytes([ip[2], ip[3]])),
                frame.len() - ETHERNET_LEN
            );
            let udp = &frame[ETHERNET_LEN + 20..];
            assert_eq!(u16::from_be_bytes([udp[0], udp[1]]), SERVER_PORT);
            assert_eq!(u16::from_be_bytes([udp[2], udp[3]]), CLIENT_PORT);
            let message = &udp[UDP_LEN..];
            assert!(message.len() >= MIN_MESSAGE);
            assert_eq!(message[0], 2);
            assert_eq!(
                &message[4..8],
                &[0xde, 0xad, 0xbe, 0xef],
                "the client's transaction"
            );
            assert_eq!(message[10], 0x80, "and its flags");
            assert_eq!(&message[16..20], &config.guest.octets());
            assert_eq!(&message[28..34], &CLIENT);
            assert_eq!(option(&frame, 53), Some(vec![answered]));
            assert_eq!(option(&frame, 54), Some(config.gateway.octets().to_vec()));
            assert_eq!(option(&frame, 3), Some(config.gateway.octets().to_vec()));
            assert_eq!(option(&frame, 6), Some(config.dns.octets().to_vec()));
            assert_eq!(option(&frame, 1), Some(vec![255, 255, 255, 0]));
            assert_eq!(option(&frame, 51), Some(LEASE_SECS.to_be_bytes().to_vec()));
        }
    }

    /// An ARP request for the guest's own address is the one the gateway
    /// must not answer; for any other address, and any other ARP, it is not.
    #[test]
    fn only_a_request_for_the_guests_own_address_is_a_probe() {
        let config = GatewayConfig::default();
        let arp = |operation: u8, target: Ipv4Addr| {
            let mut frame = Vec::new();
            frame.extend_from_slice(&[0xff; 6]);
            frame.extend_from_slice(&CLIENT);
            frame.extend_from_slice(&[0x08, 0x06]);
            frame.extend_from_slice(&[0, 1, 0x08, 0x00, 6, 4, 0, operation]);
            frame.extend_from_slice(&CLIENT);
            frame.extend_from_slice(&[0, 0, 0, 0]); // a probe has no sender address
            frame.extend_from_slice(&[0; 6]);
            frame.extend_from_slice(&target.octets());
            frame
        };
        assert!(asks_for(&arp(1, config.guest), config.guest));
        assert!(!asks_for(&arp(1, config.gateway), config.guest));
        assert!(!asks_for(&arp(2, config.guest), config.guest), "a reply");
        assert!(!asks_for(&arp(1, config.guest)[..40], config.guest));
        assert!(!asks_for(&request(DISCOVER), config.guest));
    }

    #[test]
    fn what_is_not_a_discover_or_a_request_is_left_alone() {
        let config = GatewayConfig::default();
        // A release, and a type this does not know.
        assert!(reply(&request(7), &config).is_none());
        assert!(reply(&request(99), &config).is_none());
        // Another port: DNS, say.
        let mut dns = request(DISCOVER);
        dns[ETHERNET_LEN + 20 + 2..ETHERNET_LEN + 20 + 4].copy_from_slice(&53u16.to_be_bytes());
        assert!(reply(&dns, &config).is_none());
        // A reply, not a request; a relayed request; no DHCP cookie.
        for (at, value) in [(0usize, 2u8), (24, 10), (BOOTP_LEN, 0)] {
            let mut frame = request(DISCOVER);
            frame[ETHERNET_LEN + 20 + UDP_LEN + at] = value;
            assert!(reply(&frame, &config).is_none(), "byte {at}");
        }
        // ARP, a truncated frame, and nothing.
        let mut arp = request(DISCOVER);
        arp[12..14].copy_from_slice(&[0x08, 0x06]);
        assert!(reply(&arp, &config).is_none());
        assert!(reply(&request(DISCOVER)[..120], &config).is_none());
        assert!(reply(&[], &config).is_none());
        // Options that run off the end are no message type.
        let mut cut = request(DISCOVER);
        let end = cut.len();
        cut.truncate(end - 9);
        assert!(reply(&cut, &config).is_none());
    }
}
