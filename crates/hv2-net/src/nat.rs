//! IPv4 NAT (source-address/port translation) for guest egress, and static
//! port-forwarding for guest ingress.
//!
//! ## Scope
//!
//! This module is pure translation logic, in the same spirit as
//! [`vswitch`](crate::vswitch): it holds no socket, no TAP handle, and does
//! no I/O. It rewrites IPv4/TCP/UDP packet bytes (recomputing checksums) and
//! tracks the guest↔external port mappings needed to route replies back.
//! Reading frames off a TAP device and writing translated frames to the
//! host's real network stack (or vice versa) is the caller's job — see
//! [`tap`](crate::tap) — the same division [`vswitch`](crate::vswitch)
//! draws between L2 forwarding decisions and the sockets that carry them.
//!
//! ## Why this exists
//!
//! [`vswitch::VirtualSwitch`] operates at L2 (Ethernet/MAC) and has no
//! concept of an external network at all — it only bridges ports that are
//! already attached to the same switch. A guest that needs to reach an
//! arbitrary host-routable TCP endpoint (a host-bound service, or the
//! wider network beyond the host) needs its outbound packets' source
//! address rewritten to the host's, with the mapping remembered so replies
//! route back to the right guest — that is NAT/masquerade, not switching.
//!
//! ## Architecture
//!
//! ```text
//! guest (10.0.0.2:51000) --egress--> [ NatTable::translate_outbound ]
//!                                            |
//!                                            v
//!                                   host (external_ip:X) ---> internet/host services
//!
//! guest (10.0.0.2:51000) <--ingress-- [ NatTable::translate_inbound ]
//!                                            ^
//!                                            |
//!                          reply to (external_ip:X) <--- internet/host services
//! ```
//!
//! Static [`PortForward`] rules are consulted first on ingress (for
//! exposing a fixed guest port under a fixed host port), then the dynamic
//! NAT table (for guest-initiated connections).
//!
//! ## Limitations of this first version
//!
//! - IPv4 only, no IP options (packets with `IHL != 5` are rejected rather
//!   than mishandled).
//! - TCP and UDP only (no ICMP — so a guest cannot `ping` out through this
//!   NAT yet).
//! - No fragmentation support — a fragmented datagram is rejected rather
//!   than reassembled or silently corrupted.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

/// Transport-layer protocol this NAT understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    fn from_ip_proto(byte: u8) -> Option<Self> {
        match byte {
            6 => Some(Protocol::Tcp),
            17 => Some(Protocol::Udp),
            _ => None,
        }
    }
}

/// A guest-side endpoint: the thing a NAT mapping translates on behalf of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InternalEndpoint {
    pub protocol: Protocol,
    pub guest_ip: Ipv4Addr,
    pub guest_port: u16,
}

/// A static rule exposing one guest port under a fixed host port, so an
/// external peer can reach a service the guest listens on (the mirror
/// image of the dynamic NAT table, which handles guest-initiated
/// connections).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortForward {
    pub protocol: Protocol,
    pub host_port: u16,
    pub guest_ip: Ipv4Addr,
    pub guest_port: u16,
}

#[derive(Debug, Clone)]
struct NatEntry {
    internal: InternalEndpoint,
    last_seen: Instant,
}

/// Configuration for a [`NatTable`].
#[derive(Debug, Clone)]
pub struct NatConfig {
    /// The address outbound packets are rewritten to use as their source —
    /// typically the host's address on whatever interface reaches the
    /// destination.
    pub external_ip: Ipv4Addr,
    /// Range of external ports available for dynamic allocation, inclusive
    /// and *in order*: `.0` must not exceed `.1`. [`NatTable::new`] checks it,
    /// because the allocator's span arithmetic underflows on a reversed range
    /// and the panic it produces points at the allocator rather than at the
    /// caller who wrote the range backwards. A single-port range is fine and
    /// is what the exhaustion test uses.
    ///
    /// Kept small in tests; production use wants the ephemeral range
    /// (49152..=65535) or similar.
    ///
    /// Ports inside this range may also carry a [`PortForward`]: the allocator
    /// skips any port a forward has claimed, so the two never hand out the same
    /// one. See [`NatTable::add_port_forward`].
    pub port_range: (u16, u16),
    /// A mapping with no traffic in either direction for this long is
    /// evicted by [`NatTable::age_entries`].
    pub idle_timeout: Duration,
}

impl Default for NatConfig {
    fn default() -> Self {
        Self {
            external_ip: Ipv4Addr::new(10, 0, 2, 2),
            port_range: (49152, 65535),
            idle_timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NatStats {
    pub outbound_translated: u64,
    pub inbound_translated: u64,
    pub inbound_no_mapping: u64,
    pub mappings_created: u64,
    pub mappings_aged: u64,
    pub rejected_unsupported: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatError {
    /// Not an IPv4 packet, carried IP options, was fragmented, or wasn't
    /// TCP/UDP — anything this first version doesn't attempt to handle.
    Unsupported,
    /// The frame was shorter than its own header fields claim.
    Truncated,
    /// No dynamic mapping (and no matching [`PortForward`]) exists for an
    /// inbound packet's destination — the caller should drop the frame.
    NoMapping,
    /// The port range configured on the [`NatTable`] is exhausted.
    PortsExhausted,
}

/// Tracks guest↔external port mappings and translates IPv4/TCP/UDP packets
/// between them.
pub struct NatTable {
    config: NatConfig,
    by_internal: HashMap<InternalEndpoint, u16>,
    by_external: HashMap<(Protocol, u16), NatEntry>,
    forwards: Vec<PortForward>,
    next_port: u16,
    stats: NatStats,
}

impl NatTable {
    /// # Panics
    ///
    /// If `config.port_range` is reversed. `NatConfig`'s fields are public and
    /// its range is a bare tuple, so writing it backwards is a plausible
    /// mistake and one nothing else would catch: the allocator would subtract
    /// the larger from the smaller. The config cannot be changed after this
    /// point — `config()` hands out a shared reference — so checking here is
    /// enough.
    pub fn new(config: NatConfig) -> Self {
        assert!(
            config.port_range.0 <= config.port_range.1,
            "NatConfig::port_range is (lo, hi) and must have lo <= hi; got {:?}",
            config.port_range
        );
        let next_port = config.port_range.0;
        Self {
            config,
            by_internal: HashMap::new(),
            by_external: HashMap::new(),
            forwards: Vec::new(),
            next_port,
            stats: NatStats::default(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(NatConfig::default())
    }

    pub fn config(&self) -> &NatConfig {
        &self.config
    }

    pub fn stats(&self) -> &NatStats {
        &self.stats
    }

    /// Add a static host-port → guest endpoint forwarding rule.
    ///
    /// The host port may fall inside [`NatConfig::port_range`]. It is not
    /// rejected, because with the default range of 49152–65535 forwarding a
    /// high port is a reasonable thing to want, and because this takes a port
    /// number that may have come from a command line — a guard here would have
    /// to either panic on user input or change this signature. Instead the
    /// dynamic allocator skips forwarded ports, which is checked once per new
    /// flow rather than once per packet and holds however the two are ordered.
    ///
    /// Without that, an inbound reply to a *guest-initiated* connection whose
    /// external port happened to match a rule would be delivered to the rule's
    /// target instead, because [`Self::translate_inbound`] consults the
    /// forwards first. Silent misdelivery rather than an error.
    pub fn add_port_forward(&mut self, rule: PortForward) {
        self.forwards.push(rule);
    }

    pub fn remove_port_forward(&mut self, protocol: Protocol, host_port: u16) {
        self.forwards
            .retain(|r| !(r.protocol == protocol && r.host_port == host_port));
    }

    /// Find or allocate the external port for a guest endpoint's outbound
    /// traffic.
    fn external_port_for(&mut self, internal: InternalEndpoint) -> Result<u16, NatError> {
        if let Some(&port) = self.by_internal.get(&internal) {
            return Ok(port);
        }

        let (lo, hi) = self.config.port_range;
        let span = (hi - lo) as u32 + 1;
        for offset in 0..span {
            let candidate = lo + (((self.next_port - lo) as u32 + offset) % span) as u16;
            // Not already mapped, and not claimed by a static rule. The scan of
            // `forwards` is a handful of entries and only runs when a flow is
            // new -- an existing one returned from `by_internal` above.
            let forwarded = self
                .forwards
                .iter()
                .any(|r| r.protocol == internal.protocol && r.host_port == candidate);
            if !forwarded
                && !self
                    .by_external
                    .contains_key(&(internal.protocol, candidate))
            {
                self.next_port = if candidate == hi { lo } else { candidate + 1 };
                self.by_internal.insert(internal, candidate);
                self.by_external.insert(
                    (internal.protocol, candidate),
                    NatEntry {
                        internal,
                        last_seen: Instant::now(),
                    },
                );
                self.stats.mappings_created += 1;
                return Ok(candidate);
            }
        }
        Err(NatError::PortsExhausted)
    }

    /// Rewrite an outbound IPv4 packet's source address/port to the
    /// external mapping (allocating one if this is a new guest endpoint),
    /// and recompute the IPv4/TCP-or-UDP checksums.
    pub fn translate_outbound(&mut self, frame: &mut [u8]) -> Result<(), NatError> {
        let hdr = Ipv4HeaderView::parse(frame)?;
        let protocol = Protocol::from_ip_proto(hdr.protocol).ok_or_else(|| {
            self.stats.rejected_unsupported += 1;
            NatError::Unsupported
        })?;
        let guest_ip = hdr.src;
        let ihl = hdr.ihl as usize;
        let l4_off = ihl;
        let guest_port = read_u16(frame, l4_off)?;

        let internal = InternalEndpoint {
            protocol,
            guest_ip,
            guest_port,
        };
        let external_port = self.external_port_for(internal)?;

        write_ipv4_addr(frame, 12, self.config.external_ip);
        write_u16(frame, l4_off, external_port)?;
        recompute_checksums(frame, protocol, ihl)?;

        if let Some(entry) = self.by_external.get_mut(&(protocol, external_port)) {
            entry.last_seen = Instant::now();
        }
        self.stats.outbound_translated += 1;
        Ok(())
    }

    /// Rewrite an inbound IPv4 packet's destination address/port back to
    /// the guest endpoint it belongs to (consulting static
    /// [`PortForward`] rules first, then the dynamic NAT table), and
    /// recompute checksums. Returns [`NatError::NoMapping`] if this
    /// packet doesn't correspond to any known guest endpoint — the caller
    /// should drop it rather than deliver it to a guest it was never
    /// meant for.
    pub fn translate_inbound(&mut self, frame: &mut [u8]) -> Result<(), NatError> {
        let hdr = Ipv4HeaderView::parse(frame)?;
        let protocol = Protocol::from_ip_proto(hdr.protocol).ok_or_else(|| {
            self.stats.rejected_unsupported += 1;
            NatError::Unsupported
        })?;
        let ihl = hdr.ihl as usize;
        let l4_off = ihl;
        let dst_port = read_u16(frame, l4_off + 2)?;

        let target = if let Some(fwd) = self
            .forwards
            .iter()
            .find(|r| r.protocol == protocol && r.host_port == dst_port)
        {
            (fwd.guest_ip, fwd.guest_port)
        } else if let Some(entry) = self.by_external.get_mut(&(protocol, dst_port)) {
            entry.last_seen = Instant::now();
            (entry.internal.guest_ip, entry.internal.guest_port)
        } else {
            self.stats.inbound_no_mapping += 1;
            return Err(NatError::NoMapping);
        };

        write_ipv4_addr(frame, 16, target.0);
        write_u16(frame, l4_off + 2, target.1)?;
        recompute_checksums(frame, protocol, ihl)?;

        self.stats.inbound_translated += 1;
        Ok(())
    }

    /// Evict mappings idle for longer than [`NatConfig::idle_timeout`].
    /// Returns the number of mappings removed.
    pub fn age_entries(&mut self) -> usize {
        let timeout = self.config.idle_timeout;
        let now = Instant::now();
        let before = self.by_external.len();

        self.by_external
            .retain(|_, entry| now.duration_since(entry.last_seen) < timeout);
        self.by_internal
            .retain(|internal, port| self.by_external.contains_key(&(internal.protocol, *port)));

        let aged = before - self.by_external.len();
        self.stats.mappings_aged += aged as u64;
        aged
    }

    pub fn mapping_count(&self) -> usize {
        self.by_external.len()
    }
}

struct Ipv4HeaderView {
    ihl: u8,
    protocol: u8,
    src: Ipv4Addr,
}

impl Ipv4HeaderView {
    fn parse(frame: &[u8]) -> Result<Self, NatError> {
        if frame.len() < 20 {
            return Err(NatError::Truncated);
        }
        let version = frame[0] >> 4;
        let ihl = (frame[0] & 0x0F) * 4;
        if version != 4 || ihl != 20 {
            // Not IPv4, or carries options — out of scope for this version.
            return Err(NatError::Unsupported);
        }
        let flags_frag = u16::from_be_bytes([frame[6], frame[7]]);
        let more_fragments = flags_frag & 0x2000 != 0;
        let frag_offset = flags_frag & 0x1FFF;
        if more_fragments || frag_offset != 0 {
            return Err(NatError::Unsupported);
        }
        if frame.len() < ihl as usize + 4 {
            return Err(NatError::Truncated);
        }
        let protocol = frame[9];
        let src = Ipv4Addr::new(frame[12], frame[13], frame[14], frame[15]);
        Ok(Self { ihl, protocol, src })
    }
}

fn read_u16(frame: &[u8], offset: usize) -> Result<u16, NatError> {
    frame
        .get(offset..offset + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or(NatError::Truncated)
}

fn write_u16(frame: &mut [u8], offset: usize, value: u16) -> Result<(), NatError> {
    let slot = frame
        .get_mut(offset..offset + 2)
        .ok_or(NatError::Truncated)?;
    slot.copy_from_slice(&value.to_be_bytes());
    Ok(())
}

fn write_ipv4_addr(frame: &mut [u8], offset: usize, addr: Ipv4Addr) {
    frame[offset..offset + 4].copy_from_slice(&addr.octets());
}

/// RFC 1071 internet checksum: ones'-complement sum of 16-bit words,
/// folded to 16 bits, then complemented.
#[allow(clippy::chunks_exact_to_as_chunks)] // `as_chunks` is nightly-only; this stays on stable.
fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for chunk in &mut chunks {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn recompute_checksums(frame: &mut [u8], protocol: Protocol, ihl: usize) -> Result<(), NatError> {
    // IPv4 header checksum: zero the field, checksum the header, write it back.
    frame[10] = 0;
    frame[11] = 0;
    let ip_csum = internet_checksum(&frame[..ihl]);
    frame[10..12].copy_from_slice(&ip_csum.to_be_bytes());

    let total_len = read_u16(frame, 2)? as usize;
    if frame.len() < total_len || total_len < ihl {
        return Err(NatError::Truncated);
    }
    let l4_len = total_len - ihl;

    match protocol {
        Protocol::Udp => {
            if l4_len < 8 {
                return Err(NatError::Truncated);
            }
            // A zero UDP checksum means "not used" (RFC 768) — leave it alone.
            let existing = read_u16(frame, ihl + 6)?;
            if existing == 0 {
                return Ok(());
            }
            frame[ihl + 6] = 0;
            frame[ihl + 7] = 0;
            let csum = transport_checksum(frame, ihl, l4_len, 17);
            frame[ihl + 6..ihl + 8].copy_from_slice(&csum.to_be_bytes());
        }
        Protocol::Tcp => {
            if l4_len < 20 {
                return Err(NatError::Truncated);
            }
            frame[ihl + 16] = 0;
            frame[ihl + 17] = 0;
            let csum = transport_checksum(frame, ihl, l4_len, 6);
            frame[ihl + 16..ihl + 18].copy_from_slice(&csum.to_be_bytes());
        }
    }
    Ok(())
}

/// TCP/UDP checksum: the internet checksum of a pseudo-header (src/dst IP,
/// zero byte, protocol, segment length) followed by the segment itself
/// (with its own checksum field already zeroed by the caller).
fn transport_checksum(frame: &[u8], ihl: usize, l4_len: usize, proto: u8) -> u16 {
    let mut pseudo = Vec::with_capacity(12 + l4_len);
    pseudo.extend_from_slice(&frame[12..16]); // src ip
    pseudo.extend_from_slice(&frame[16..20]); // dst ip
    pseudo.push(0);
    pseudo.push(proto);
    pseudo.extend_from_slice(&(l4_len as u16).to_be_bytes());
    pseudo.extend_from_slice(&frame[ihl..ihl + l4_len]);
    internet_checksum(&pseudo)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal IPv4 + UDP packet with correct checksums, no
    /// options, no payload beyond what's given.
    fn build_udp_packet(
        src: Ipv4Addr,
        src_port: u16,
        dst: Ipv4Addr,
        dst_port: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let udp_len = 8 + payload.len();
        let total_len = 20 + udp_len;
        let mut frame = vec![0u8; total_len];

        frame[0] = 0x45; // version 4, IHL 5 (20 bytes)
        frame[1] = 0; // DSCP/ECN
        frame[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
        frame[4..6].copy_from_slice(&0u16.to_be_bytes()); // identification
        frame[6..8].copy_from_slice(&0u16.to_be_bytes()); // flags/frag offset
        frame[8] = 64; // TTL
        frame[9] = 17; // UDP
        write_ipv4_addr(&mut frame, 12, src);
        write_ipv4_addr(&mut frame, 16, dst);

        frame[20..22].copy_from_slice(&src_port.to_be_bytes());
        frame[22..24].copy_from_slice(&dst_port.to_be_bytes());
        frame[24..26].copy_from_slice(&(udp_len as u16).to_be_bytes());
        frame[28..].copy_from_slice(payload);

        recompute_checksums(&mut frame, Protocol::Udp, 20).unwrap();
        frame
    }

    fn build_tcp_packet(
        src: Ipv4Addr,
        src_port: u16,
        dst: Ipv4Addr,
        dst_port: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let tcp_len = 20 + payload.len();
        let total_len = 20 + tcp_len;
        let mut frame = vec![0u8; total_len];

        frame[0] = 0x45;
        frame[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
        frame[8] = 64;
        frame[9] = 6; // TCP
        write_ipv4_addr(&mut frame, 12, src);
        write_ipv4_addr(&mut frame, 16, dst);

        frame[20..22].copy_from_slice(&src_port.to_be_bytes());
        frame[22..24].copy_from_slice(&dst_port.to_be_bytes());
        frame[32] = 0x50; // data offset 5 (20 bytes), reserved bits 0
        frame[33] = 0x18; // PSH|ACK, arbitrary
        frame[40..].copy_from_slice(payload);

        recompute_checksums(&mut frame, Protocol::Tcp, 20).unwrap();
        frame
    }

    fn ipv4_of(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
        Ipv4Addr::new(a, b, c, d)
    }

    #[test]
    fn outbound_rewrites_source_and_allocates_port() {
        let mut nat = NatTable::with_defaults();
        let mut pkt = build_udp_packet(
            ipv4_of(10, 0, 0, 2),
            51000,
            ipv4_of(93, 184, 216, 34),
            80,
            b"hello",
        );

        nat.translate_outbound(&mut pkt).unwrap();

        let hdr = Ipv4HeaderView::parse(&pkt).unwrap();
        assert_eq!(hdr.src, nat.config().external_ip);
        let ext_port = read_u16(&pkt, 20).unwrap();
        assert!(nat.config().port_range.0 <= ext_port && ext_port <= nat.config().port_range.1);
        assert_eq!(nat.mapping_count(), 1);
        assert_eq!(nat.stats().mappings_created, 1);
    }

    #[test]
    fn outbound_then_inbound_round_trips_to_the_same_guest_endpoint() {
        let mut nat = NatTable::with_defaults();
        let guest = ipv4_of(10, 0, 0, 2);
        let peer = ipv4_of(93, 184, 216, 34);

        let mut out = build_udp_packet(guest, 51000, peer, 80, b"ping");
        nat.translate_outbound(&mut out).unwrap();
        let ext_port = read_u16(&out, 20).unwrap();

        // Peer replies to (external_ip, ext_port).
        let mut reply = build_udp_packet(peer, 80, nat.config().external_ip, ext_port, b"pong");
        nat.translate_inbound(&mut reply).unwrap();

        let hdr = Ipv4HeaderView::parse(&reply).unwrap();
        assert_eq!(hdr.src, peer);
        let dst_ip = Ipv4Addr::new(reply[16], reply[17], reply[18], reply[19]);
        assert_eq!(dst_ip, guest);
        let dst_port = read_u16(&reply, 22).unwrap();
        assert_eq!(dst_port, 51000);
    }

    #[test]
    fn inbound_with_no_mapping_is_rejected() {
        let mut nat = NatTable::with_defaults();
        let mut pkt = build_udp_packet(
            ipv4_of(93, 184, 216, 34),
            80,
            nat.config().external_ip,
            60000,
            b"unsolicited",
        );
        let err = nat.translate_inbound(&mut pkt).unwrap_err();
        assert_eq!(err, NatError::NoMapping);
        assert_eq!(nat.stats().inbound_no_mapping, 1);
    }

    #[test]
    fn same_guest_endpoint_reuses_its_mapping() {
        let mut nat = NatTable::with_defaults();
        let guest = ipv4_of(10, 0, 0, 2);
        let peer = ipv4_of(1, 1, 1, 1);

        let mut a = build_udp_packet(guest, 51000, peer, 53, b"a");
        nat.translate_outbound(&mut a).unwrap();
        let port_a = read_u16(&a, 20).unwrap();

        let mut b = build_udp_packet(guest, 51000, peer, 53, b"b");
        nat.translate_outbound(&mut b).unwrap();
        let port_b = read_u16(&b, 20).unwrap();

        assert_eq!(port_a, port_b);
        assert_eq!(nat.mapping_count(), 1);
        assert_eq!(nat.stats().mappings_created, 1);
    }

    #[test]
    fn different_guest_endpoints_get_different_ports() {
        let mut nat = NatTable::with_defaults();
        let peer = ipv4_of(1, 1, 1, 1);

        let mut a = build_udp_packet(ipv4_of(10, 0, 0, 2), 51000, peer, 53, b"a");
        nat.translate_outbound(&mut a).unwrap();
        let port_a = read_u16(&a, 20).unwrap();

        let mut b = build_udp_packet(ipv4_of(10, 0, 0, 3), 51000, peer, 53, b"b");
        nat.translate_outbound(&mut b).unwrap();
        let port_b = read_u16(&b, 20).unwrap();

        assert_ne!(port_a, port_b);
        assert_eq!(nat.mapping_count(), 2);
    }

    #[test]
    fn port_forward_routes_unsolicited_inbound_to_fixed_guest_endpoint() {
        let mut nat = NatTable::with_defaults();
        let guest = ipv4_of(10, 0, 0, 2);
        nat.add_port_forward(PortForward {
            protocol: Protocol::Tcp,
            host_port: 8080,
            guest_ip: guest,
            guest_port: 80,
        });

        let mut pkt = build_tcp_packet(
            ipv4_of(203, 0, 113, 9),
            54321,
            nat.config().external_ip,
            8080,
            b"GET / HTTP/1.0\r\n\r\n",
        );
        nat.translate_inbound(&mut pkt).unwrap();

        let dst_ip = Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]);
        assert_eq!(dst_ip, guest);
        let dst_port = read_u16(&pkt, 22).unwrap();
        assert_eq!(dst_port, 80);
    }

    #[test]
    fn tcp_round_trip_preserves_checksum_validity() {
        let mut nat = NatTable::with_defaults();
        let guest = ipv4_of(10, 0, 0, 2);
        let peer = ipv4_of(8, 8, 8, 8);

        let mut out = build_tcp_packet(guest, 51000, peer, 443, b"clienthello");
        nat.translate_outbound(&mut out).unwrap();

        // Recomputing checksums on an already-consistent packet must not
        // change them — if it did, our checksum math would be wrong.
        let ip_csum_before = read_u16(&out, 10).unwrap();
        let tcp_csum_before = read_u16(&out, 20 + 16).unwrap();
        let mut resigned = out.clone();
        recompute_checksums(&mut resigned, Protocol::Tcp, 20).unwrap();
        assert_eq!(read_u16(&resigned, 10).unwrap(), ip_csum_before);
        assert_eq!(read_u16(&resigned, 20 + 16).unwrap(), tcp_csum_before);
    }

    #[test]
    fn zero_udp_checksum_is_left_unset() {
        let mut nat = NatTable::with_defaults();
        let mut pkt = build_udp_packet(ipv4_of(10, 0, 0, 2), 51000, ipv4_of(1, 1, 1, 1), 53, b"x");
        pkt[20 + 6] = 0;
        pkt[20 + 7] = 0; // force UDP checksum to 0 ("not used") after the builder set it
        nat.translate_outbound(&mut pkt).unwrap();
        assert_eq!(read_u16(&pkt, 20 + 6).unwrap(), 0);
    }

    #[test]
    fn non_ipv4_frame_is_rejected() {
        let mut nat = NatTable::with_defaults();
        let mut junk = vec![0x60u8; 40]; // version 6
        assert_eq!(
            nat.translate_outbound(&mut junk).unwrap_err(),
            NatError::Unsupported
        );
    }

    #[test]
    fn ipv4_with_options_is_rejected_not_mishandled() {
        let mut nat = NatTable::with_defaults();
        let mut pkt = build_udp_packet(ipv4_of(10, 0, 0, 2), 51000, ipv4_of(1, 1, 1, 1), 53, b"x");
        pkt[0] = 0x46; // IHL 6 (24 bytes) -> options present
        assert_eq!(
            nat.translate_outbound(&mut pkt).unwrap_err(),
            NatError::Unsupported
        );
    }

    #[test]
    fn fragmented_packet_is_rejected() {
        let mut nat = NatTable::with_defaults();
        let mut pkt = build_udp_packet(ipv4_of(10, 0, 0, 2), 51000, ipv4_of(1, 1, 1, 1), 53, b"x");
        pkt[6] = 0x20; // more-fragments flag set
        assert_eq!(
            nat.translate_outbound(&mut pkt).unwrap_err(),
            NatError::Unsupported
        );
    }

    #[test]
    fn icmp_is_rejected_as_unsupported() {
        let mut nat = NatTable::with_defaults();
        let mut pkt = build_udp_packet(ipv4_of(10, 0, 0, 2), 51000, ipv4_of(1, 1, 1, 1), 53, b"x");
        pkt[9] = 1; // ICMP
        assert_eq!(
            nat.translate_outbound(&mut pkt).unwrap_err(),
            NatError::Unsupported
        );
        assert_eq!(nat.stats().rejected_unsupported, 1);
    }

    #[test]
    fn truncated_frame_is_rejected() {
        let mut nat = NatTable::with_defaults();
        let mut short = vec![0x45u8, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0];
        assert_eq!(
            nat.translate_outbound(&mut short).unwrap_err(),
            NatError::Truncated
        );
    }

    #[test]
    fn idle_mappings_are_aged_out() {
        let mut nat = NatTable::new(NatConfig {
            idle_timeout: Duration::from_millis(1),
            ..NatConfig::default()
        });
        let mut pkt = build_udp_packet(ipv4_of(10, 0, 0, 2), 51000, ipv4_of(1, 1, 1, 1), 53, b"x");
        nat.translate_outbound(&mut pkt).unwrap();
        assert_eq!(nat.mapping_count(), 1);

        std::thread::sleep(Duration::from_millis(5));
        let aged = nat.age_entries();
        assert_eq!(aged, 1);
        assert_eq!(nat.mapping_count(), 0);
        assert_eq!(nat.stats().mappings_aged, 1);
    }

    #[test]
    fn port_range_exhaustion_is_reported_not_panicked() {
        let mut nat = NatTable::new(NatConfig {
            port_range: (40000, 40000), // exactly one port available
            ..NatConfig::default()
        });
        let peer = ipv4_of(1, 1, 1, 1);

        let mut a = build_udp_packet(ipv4_of(10, 0, 0, 2), 51000, peer, 53, b"a");
        nat.translate_outbound(&mut a).unwrap();

        let mut b = build_udp_packet(ipv4_of(10, 0, 0, 3), 51000, peer, 53, b"b");
        assert_eq!(
            nat.translate_outbound(&mut b).unwrap_err(),
            NatError::PortsExhausted
        );
    }

    /// Reported, and reproduced before being believed: it used to reach the
    /// allocator and subtract the larger port from the smaller.
    #[test]
    #[should_panic(expected = "must have lo <= hi")]
    fn reversed_port_range_is_refused_at_construction() {
        let _ = NatTable::new(NatConfig {
            port_range: (40100, 40000), // backwards
            ..NatConfig::default()
        });
    }

    /// A single-port range is not reversed, and has always worked. Here so the
    /// check above cannot be tightened into rejecting it.
    #[test]
    fn a_single_port_range_is_allowed() {
        let nat = NatTable::new(NatConfig {
            port_range: (40000, 40000),
            ..NatConfig::default()
        });
        assert_eq!(nat.config().port_range, (40000, 40000));
    }

    /// Also reported. Not a panic: a reply going to the wrong guest port.
    #[test]
    fn dynamic_allocation_does_not_collide_with_a_static_forward() {
        let mut nat = NatTable::new(NatConfig {
            port_range: (8080, 8080),
            ..NatConfig::default()
        });
        let guest = ipv4_of(10, 0, 0, 2);
        nat.add_port_forward(PortForward {
            protocol: Protocol::Tcp,
            host_port: 8080,
            guest_ip: ipv4_of(10, 0, 0, 9),
            guest_port: 80,
        });

        // The guest opens an outbound connection. The only port in the range
        // is the one the forward owns, so there is nothing to hand out and the
        // allocator has to say so rather than hand out a port twice.
        let mut out = build_tcp_packet(guest, 51000, ipv4_of(8, 8, 8, 8), 443, b"hello");
        assert_eq!(
            nat.translate_outbound(&mut out).unwrap_err(),
            NatError::PortsExhausted
        );

        // With one port beside it, that is the one it takes.
        nat.remove_port_forward(Protocol::Tcp, 8080);
        let mut wider = NatTable::new(NatConfig {
            port_range: (8080, 8081),
            ..NatConfig::default()
        });
        wider.add_port_forward(PortForward {
            protocol: Protocol::Tcp,
            host_port: 8080,
            guest_ip: ipv4_of(10, 0, 0, 9),
            guest_port: 80,
        });
        let mut nat = wider;
        let mut out = build_tcp_packet(guest, 51000, ipv4_of(8, 8, 8, 8), 443, b"hello");
        nat.translate_outbound(&mut out).unwrap();
        let external = read_u16(&out, 20).unwrap();
        assert_eq!(external, 8081, "the allocator handed out a forwarded port");

        // And the reply comes back to the guest that opened the connection,
        // not to the forward's target.
        let mut back = build_tcp_packet(
            ipv4_of(8, 8, 8, 8),
            443,
            nat.config().external_ip,
            external,
            b"world",
        );
        nat.translate_inbound(&mut back).unwrap();
        let dst_ip = Ipv4Addr::new(back[16], back[17], back[18], back[19]);
        let dst_port = read_u16(&back, 22).unwrap();
        assert_eq!(
            (dst_ip, dst_port),
            (guest, 51000),
            "the reply was delivered to the port-forward's target instead"
        );
    }

    #[test]
    fn remove_port_forward_stops_matching() {
        let mut nat = NatTable::with_defaults();
        let guest = ipv4_of(10, 0, 0, 2);
        let rule = PortForward {
            protocol: Protocol::Tcp,
            host_port: 8080,
            guest_ip: guest,
            guest_port: 80,
        };
        nat.add_port_forward(rule);
        nat.remove_port_forward(Protocol::Tcp, 8080);

        let mut pkt = build_tcp_packet(
            ipv4_of(203, 0, 113, 9),
            54321,
            nat.config().external_ip,
            8080,
            b"x",
        );
        assert_eq!(
            nat.translate_inbound(&mut pkt).unwrap_err(),
            NatError::NoMapping
        );
    }
}
