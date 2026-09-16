//! What a guest is allowed to talk to.
//!
//! # Why this exists
//!
//! [`Bridge`](crate::bridge::Bridge) carried every frame a guest produced. For
//! a sandbox running code an agent was told to run by a document it read, that
//! is the whole problem: NVIDIA's sandboxing guidance names blocking "outbound
//! network access to unknown destinations" as its first mandatory control,
//! because the direct threats are a reverse shell and exfiltration of whatever
//! the sandbox can see, and neither needs the attacker to be present.
//!
//! `hv2-core` has had a packet filter with connection tracking for a while
//! (`hv2_core::networking::filter`). Nothing in a data path ever called it. A
//! filter that is never asked is not a control.
//!
//! # What this decides on
//!
//! The destination address and port of an outbound IPv4 or IPv6 packet, and
//! its protocol. Deliberately not the payload: a bridge sees frames at line
//! rate on the guest's own kick, and anything that has to parse deeper than a
//! header is a different component with a different budget.
//!
//! # What it does not do
//!
//! **Names.** A rule is an address, not a hostname, so an allowlist written as
//! "our package mirror" has to be resolved to addresses by whoever writes it,
//! and goes stale when they change. Filtering on the name a client *asked* for
//! means terminating TLS or trusting the guest's own DNS, and this does
//! neither.
//!
//! **Inbound.** Replies are allowed by the NAT table, which only has an entry
//! because an outbound packet created one. A policy on the inbound direction
//! would be a second, weaker copy of that.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A transport protocol, as an IP header numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    /// Anything else, by its IP protocol number.
    Other(u8),
}

impl Protocol {
    #[must_use]
    fn from_number(number: u8) -> Self {
        match number {
            1 | 58 => Self::Icmp,
            6 => Self::Tcp,
            17 => Self::Udp,
            other => Self::Other(other),
        }
    }
}

/// Where an outbound packet is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Destination {
    pub address: IpAddr,
    /// `None` for a protocol that has no ports, such as ICMP.
    pub port: Option<u16>,
    pub protocol: Protocol,
}

/// One thing a guest may do.
///
/// Every field that is `None` matches anything, so a rule is as wide as it is
/// written and no wider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The network this rule covers, as an address and a prefix length.
    /// `None` matches any address.
    pub network: Option<(IpAddr, u8)>,
    /// `None` matches any port.
    pub port: Option<u16>,
    /// `None` matches any protocol.
    pub protocol: Option<Protocol>,
}

impl Rule {
    /// Allow anything to one exact address and port.
    #[must_use]
    pub fn host_port(address: IpAddr, port: u16, protocol: Protocol) -> Self {
        let bits = if address.is_ipv4() { 32 } else { 128 };
        Self {
            network: Some((address, bits)),
            port: Some(port),
            protocol: Some(protocol),
        }
    }

    /// Allow a whole network, on any port.
    #[must_use]
    pub fn network(address: IpAddr, prefix: u8) -> Self {
        Self {
            network: Some((address, prefix)),
            port: None,
            protocol: None,
        }
    }

    /// Does this rule cover `destination`?
    #[must_use]
    pub fn covers(&self, destination: &Destination) -> bool {
        if let Some(protocol) = self.protocol {
            if protocol != destination.protocol {
                return false;
            }
        }
        if let Some(port) = self.port {
            // A protocol with no ports cannot match a rule that names one:
            // "port 443" says nothing about an ICMP packet, and treating it as
            // a match would let ping through a rule meant for HTTPS.
            if destination.port != Some(port) {
                return false;
            }
        }
        match self.network {
            None => true,
            Some((network, prefix)) => within(destination.address, network, prefix),
        }
    }
}

/// Is `address` inside `network/prefix`?
///
/// Compares the leading `prefix` bits. A prefix longer than the address has
/// bits is treated as the full length rather than rejected, so a rule written
/// as `10.0.0.1/64` matches only that host instead of silently matching
/// everything.
#[must_use]
fn within(address: IpAddr, network: IpAddr, prefix: u8) -> bool {
    fn compare(address: &[u8], network: &[u8], prefix: u8) -> bool {
        let bits = usize::from(prefix).min(address.len() * 8);
        let whole = bits / 8;
        if address[..whole] != network[..whole] {
            return false;
        }
        let leftover = bits % 8;
        if leftover == 0 {
            return true;
        }
        let mask = 0xffu8 << (8 - leftover);
        address[whole] & mask == network[whole] & mask
    }

    match (address, network) {
        (IpAddr::V4(a), IpAddr::V4(n)) => compare(&a.octets(), &n.octets(), prefix),
        (IpAddr::V6(a), IpAddr::V6(n)) => compare(&a.octets(), &n.octets(), prefix),
        // A v4 rule says nothing about a v6 destination or the reverse.
        // Matching across families is how an allowlist for one address family
        // silently becomes no allowlist at all for the other.
        _ => false,
    }
}

/// What a guest may send, and what happens to the rest.
///
/// Default-deny is the only default offered. [`EgressPolicy::allow_all`] exists
/// and has to be said out loud, because "the bridge had no policy" and "the
/// operator chose to allow everything" should not look the same in a
/// configuration or a code review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressPolicy {
    rules: Vec<Rule>,
    /// Set by [`EgressPolicy::allow_all`] only.
    unrestricted: bool,
}

impl Default for EgressPolicy {
    /// Deny everything.
    ///
    /// A policy nobody configured is the case where nobody decided, and the
    /// safe reading of "nobody decided" for a sandbox running untrusted code
    /// is no network.
    fn default() -> Self {
        Self::deny_all()
    }
}

impl EgressPolicy {
    /// Nothing leaves.
    #[must_use]
    pub fn deny_all() -> Self {
        Self {
            rules: Vec::new(),
            unrestricted: false,
        }
    }

    /// Everything leaves, and the caller has said so deliberately.
    ///
    /// For a guest on a private switch with its host, or a lab bridge where
    /// the point is to watch traffic move. Not for a sandbox running code from
    /// somewhere else.
    #[must_use]
    pub fn allow_all() -> Self {
        Self {
            rules: Vec::new(),
            unrestricted: true,
        }
    }

    /// Only what these rules cover.
    #[must_use]
    pub fn allow(rules: Vec<Rule>) -> Self {
        Self {
            rules,
            unrestricted: false,
        }
    }

    /// Add one more thing a guest may reach.
    #[must_use]
    pub fn and(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Would this policy let anything at all through?
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    /// May this frame leave?
    ///
    /// A frame whose destination cannot be read is refused. That covers ARP
    /// and every other non-IP ethertype as well as a truncated header, and it
    /// is the conservative reading: a policy that cannot tell where something
    /// is going has not established that it is allowed to go there.
    ///
    /// The cost is real and worth naming: ARP is how a guest finds its
    /// gateway's MAC, so a routed bridge whose guest must ARP needs a link
    /// that answers ARP itself, or a policy that allows it. This deliberately
    /// does not carve out an exception, because "allow the one protocol that
    /// is not IP" is the kind of exception that quietly grows.
    #[must_use]
    pub fn allows_frame(&self, frame: &[u8]) -> bool {
        if self.unrestricted {
            return true;
        }
        match destination_of(frame) {
            Some(destination) => self.allows(&destination),
            None => false,
        }
    }

    /// May a packet to `destination` leave?
    #[must_use]
    pub fn allows(&self, destination: &Destination) -> bool {
        self.unrestricted || self.rules.iter().any(|rule| rule.covers(destination))
    }
}

/// Read where an ethernet frame is going, or `None` if it cannot be told.
///
/// Handles IPv4 and IPv6 over ethernet, including a single VLAN tag. Returns
/// `None` for anything else -- ARP, a truncated header, an IPv4 header whose
/// own length field does not fit, or an IPv6 packet whose next header is an
/// extension this does not walk.
#[must_use]
pub fn destination_of(frame: &[u8]) -> Option<Destination> {
    const ETHERNET_HEADER: usize = 14;
    const VLAN_TAG: usize = 4;

    let ethertype = u16::from_be_bytes([*frame.get(12)?, *frame.get(13)?]);
    // 802.1Q: the real ethertype is four bytes further along.
    let (ethertype, payload_at) = if ethertype == 0x8100 {
        (
            u16::from_be_bytes([*frame.get(16)?, *frame.get(17)?]),
            ETHERNET_HEADER + VLAN_TAG,
        )
    } else {
        (ethertype, ETHERNET_HEADER)
    };

    let packet = frame.get(payload_at..)?;
    match ethertype {
        0x0800 => ipv4_destination(packet),
        0x86DD => ipv6_destination(packet),
        _ => None,
    }
}

fn ipv4_destination(packet: &[u8]) -> Option<Destination> {
    let version_and_length = *packet.first()?;
    if version_and_length >> 4 != 4 {
        return None;
    }
    // The low nibble counts 32-bit words, and a header is never shorter than
    // five of them. A smaller value is a malformed packet, not a short header.
    let header_len = usize::from(version_and_length & 0x0f) * 4;
    if header_len < 20 || packet.len() < header_len {
        return None;
    }

    let protocol = Protocol::from_number(*packet.get(9)?);
    let address = Ipv4Addr::new(
        *packet.get(16)?,
        *packet.get(17)?,
        *packet.get(18)?,
        *packet.get(19)?,
    );

    // A fragment after the first carries no transport header, so its port
    // cannot be read. It is left as `None`, which means a rule naming a port
    // will not match it -- the conservative answer, since the alternative is
    // to guess a port from a packet that does not contain one.
    let fragment_offset = u16::from_be_bytes([*packet.get(6)?, *packet.get(7)?]) & 0x1fff;
    let port = if fragment_offset == 0 {
        destination_port(protocol, packet.get(header_len..)?)
    } else {
        None
    };

    Some(Destination {
        address: IpAddr::V4(address),
        port,
        protocol,
    })
}

fn ipv6_destination(packet: &[u8]) -> Option<Destination> {
    const IPV6_HEADER: usize = 40;
    if packet.len() < IPV6_HEADER || packet.first()? >> 4 != 6 {
        return None;
    }
    let protocol = Protocol::from_number(*packet.get(6)?);
    let mut address = [0u8; 16];
    address.copy_from_slice(packet.get(24..40)?);

    // Extension headers are not walked: the next header is taken as the
    // transport, and if it is not one, the port reads as `None`. Walking them
    // is a chain a packet controls the length of, which is a budget question
    // this does not have an answer for yet.
    let port = destination_port(protocol, packet.get(IPV6_HEADER..)?);

    Some(Destination {
        address: IpAddr::V6(Ipv6Addr::from(address)),
        port,
        protocol,
    })
}

/// The destination port, for a protocol that has one.
fn destination_port(protocol: Protocol, transport: &[u8]) -> Option<u16> {
    match protocol {
        Protocol::Tcp | Protocol::Udp => {
            Some(u16::from_be_bytes([*transport.get(2)?, *transport.get(3)?]))
        }
        Protocol::Icmp | Protocol::Other(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ethernet frame carrying an IPv4 packet to `address:port`.
    fn frame_to(address: Ipv4Addr, port: u16, protocol: u8) -> Vec<u8> {
        let mut frame = vec![0u8; 14];
        frame[12] = 0x08;
        frame[13] = 0x00;

        let mut packet = vec![0u8; 20];
        packet[0] = 0x45;
        packet[9] = protocol;
        packet[12..16].copy_from_slice(&Ipv4Addr::new(10, 0, 2, 15).octets());
        packet[16..20].copy_from_slice(&address.octets());

        let mut transport = vec![0u8; 4];
        transport[2..4].copy_from_slice(&port.to_be_bytes());

        frame.extend_from_slice(&packet);
        frame.extend_from_slice(&transport);
        frame
    }

    fn https_to(address: Ipv4Addr) -> Vec<u8> {
        frame_to(address, 443, 6)
    }

    #[test]
    fn nothing_leaves_a_default_policy() {
        // The case that matters most: a bridge whose policy nobody configured
        // must not be a bridge with no policy.
        let policy = EgressPolicy::default();
        assert!(!policy.allows_frame(&https_to(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!policy.is_unrestricted());
    }

    #[test]
    fn allow_all_is_a_thing_someone_said_out_loud() {
        let policy = EgressPolicy::allow_all();
        assert!(policy.allows_frame(&https_to(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(policy.is_unrestricted());
    }

    #[test]
    fn an_allowlist_covers_what_it_names_and_nothing_else() {
        let policy = EgressPolicy::allow(vec![Rule::host_port(
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            443,
            Protocol::Tcp,
        )]);
        assert!(policy.allows_frame(&https_to(Ipv4Addr::new(1, 1, 1, 1))));
        // A neighbour of the allowed address is not allowed.
        assert!(!policy.allows_frame(&https_to(Ipv4Addr::new(1, 1, 1, 2))));
        // The allowed address on another port is not allowed: a rule naming a
        // port is a rule about that port.
        assert!(!policy.allows_frame(&frame_to(Ipv4Addr::new(1, 1, 1, 1), 22, 6)));
        // Nor on another protocol.
        assert!(!policy.allows_frame(&frame_to(Ipv4Addr::new(1, 1, 1, 1), 443, 17)));
    }

    #[test]
    fn a_network_rule_covers_its_network_and_stops_at_the_boundary() {
        let policy = EgressPolicy::allow(vec![Rule::network(
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
            8,
        )]);
        assert!(policy.allows_frame(&https_to(Ipv4Addr::new(10, 255, 255, 254))));
        assert!(!policy.allows_frame(&https_to(Ipv4Addr::new(11, 0, 0, 1))));
    }

    #[test]
    fn a_prefix_that_is_not_a_whole_number_of_bytes_still_stops_where_it_says() {
        // /12 is 10.16.0.0 through 10.31.255.255. The boundary is inside a
        // byte, which is where an implementation comparing whole octets is
        // wrong and passes every other test anyway.
        let policy = EgressPolicy::allow(vec![Rule::network(
            IpAddr::V4(Ipv4Addr::new(10, 16, 0, 0)),
            12,
        )]);
        assert!(policy.allows_frame(&https_to(Ipv4Addr::new(10, 16, 0, 1))));
        assert!(policy.allows_frame(&https_to(Ipv4Addr::new(10, 31, 255, 255))));
        assert!(!policy.allows_frame(&https_to(Ipv4Addr::new(10, 32, 0, 0))));
        assert!(!policy.allows_frame(&https_to(Ipv4Addr::new(10, 15, 255, 255))));
    }

    #[test]
    fn a_frame_whose_destination_cannot_be_read_does_not_leave() {
        let permissive = EgressPolicy::allow(vec![Rule {
            network: None,
            port: None,
            protocol: None,
        }]);
        // ARP: not IP, so there is no destination to check against a rule that
        // matches "any IP destination".
        let mut arp = vec![0u8; 42];
        arp[12] = 0x08;
        arp[13] = 0x06;
        assert!(!permissive.allows_frame(&arp));

        // A frame too short to hold an ethertype.
        assert!(!permissive.allows_frame(&[0u8; 8]));

        // An IPv4 header claiming to be shorter than one can be.
        let mut truncated = https_to(Ipv4Addr::new(1, 1, 1, 1));
        truncated[14] = 0x43;
        assert!(!permissive.allows_frame(&truncated));
    }

    #[test]
    fn a_v4_rule_does_not_cover_a_v6_destination() {
        // Otherwise an allowlist written for one address family reads as no
        // allowlist at all for the other, which is worse than having none:
        // it looks like a control.
        let policy = EgressPolicy::allow(vec![Rule::network(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            0,
        )]);
        assert!(policy.allows(&Destination {
            address: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            port: Some(443),
            protocol: Protocol::Tcp,
        }));
        assert!(!policy.allows(&Destination {
            address: IpAddr::V6(Ipv6Addr::LOCALHOST),
            port: Some(443),
            protocol: Protocol::Tcp,
        }));
    }

    #[test]
    fn a_vlan_tagged_frame_is_read_past_its_tag() {
        // The tag shifts the IP header by four bytes. Reading the tag's own
        // bytes as an IP header gives a destination that is not the packet's,
        // which is an allowlist checking the wrong address.
        let plain = https_to(Ipv4Addr::new(1, 1, 1, 1));
        let mut tagged = plain[..12].to_vec();
        tagged.extend_from_slice(&[0x81, 0x00, 0x00, 0x64]);
        tagged.extend_from_slice(&plain[12..]);

        let destination = destination_of(&tagged).expect("a tagged frame still has a destination");
        assert_eq!(
            destination.address,
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            "the address must come from the packet, not the tag"
        );
        assert_eq!(destination.port, Some(443));
    }

    #[test]
    fn a_later_fragment_has_no_port_and_so_matches_no_port_rule() {
        // It carries no transport header. Guessing a port from a packet that
        // does not contain one is how a fragmented flow walks through a rule
        // that names a port.
        let mut frame = https_to(Ipv4Addr::new(1, 1, 1, 1));
        frame[14 + 6] = 0x00;
        frame[14 + 7] = 0x02; // fragment offset 2, so not the first
        let destination = destination_of(&frame).expect("still an IPv4 packet");
        assert_eq!(destination.port, None);

        let policy = EgressPolicy::allow(vec![Rule::host_port(
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            443,
            Protocol::Tcp,
        )]);
        assert!(!policy.allows(&destination));

        // A rule for the host without a port still covers it, which is the
        // honest outcome: that rule did not ask about ports.
        let by_host = EgressPolicy::allow(vec![Rule::network(
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            32,
        )]);
        assert!(by_host.allows(&destination));
    }

    #[test]
    fn icmp_is_not_let_through_by_a_rule_about_a_port() {
        // ICMP has no ports. A rule saying "443/tcp" says nothing about ping,
        // and a match here would be a policy that allows what it never named.
        let policy = EgressPolicy::allow(vec![Rule::host_port(
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            443,
            Protocol::Tcp,
        )]);
        assert!(!policy.allows_frame(&frame_to(Ipv4Addr::new(1, 1, 1, 1), 0, 1)));
    }
}
