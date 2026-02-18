//! Network Filtering and Firewall
//!
//! This module provides packet filtering, connection tracking, and NAT
//! capabilities for virtual networks.

use std::collections::HashMap;
use std::net::IpAddr;
#[cfg(test)]
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// IP protocol numbers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpProtocol {
    Icmp,
    Tcp,
    Udp,
    Icmpv6,
    Other(u8),
}

impl IpProtocol {
    /// Get protocol number
    pub fn number(&self) -> u8 {
        match self {
            Self::Icmp => 1,
            Self::Tcp => 6,
            Self::Udp => 17,
            Self::Icmpv6 => 58,
            Self::Other(n) => *n,
        }
    }
}

impl From<u8> for IpProtocol {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Icmp,
            6 => Self::Tcp,
            17 => Self::Udp,
            58 => Self::Icmpv6,
            n => Self::Other(n),
        }
    }
}

impl From<IpProtocol> for u8 {
    fn from(value: IpProtocol) -> Self {
        match value {
            IpProtocol::Icmp => 1,
            IpProtocol::Tcp => 6,
            IpProtocol::Udp => 17,
            IpProtocol::Icmpv6 => 58,
            IpProtocol::Other(n) => n,
        }
    }
}

/// Connection state for stateful filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// New connection
    New,
    /// Established connection
    Established,
    /// Related connection (e.g., FTP data)
    Related,
    /// Invalid connection
    Invalid,
}

/// TCP connection state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TcpState {
    /// SYN sent
    SynSent,
    /// SYN received
    SynRecv,
    /// Established
    Established,
    /// FIN wait 1
    FinWait1,
    /// FIN wait 2
    FinWait2,
    /// Close wait
    CloseWait,
    /// Last ACK
    LastAck,
    /// Time wait
    TimeWait,
    /// Closed
    #[default]
    Closed,
}

/// Connection tuple (5-tuple)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnTuple {
    /// Source IP
    pub src_ip: IpAddr,
    /// Destination IP
    pub dst_ip: IpAddr,
    /// Source port
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// Protocol
    pub protocol: IpProtocol,
}

impl ConnTuple {
    /// Create new connection tuple
    pub fn new(
        src_ip: IpAddr,
        dst_ip: IpAddr,
        src_port: u16,
        dst_port: u16,
        protocol: IpProtocol,
    ) -> Self {
        Self {
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            protocol,
        }
    }

    /// Get reverse tuple
    pub fn reverse(&self) -> Self {
        Self {
            src_ip: self.dst_ip,
            dst_ip: self.src_ip,
            src_port: self.dst_port,
            dst_port: self.src_port,
            protocol: self.protocol,
        }
    }
}

/// Connection tracking entry
#[derive(Debug, Clone)]
pub struct ConnTrackEntry {
    /// Original direction tuple
    pub original: ConnTuple,
    /// Reply direction tuple
    pub reply: ConnTuple,
    /// Connection state
    pub state: ConnState,
    /// TCP state (if TCP)
    pub tcp_state: Option<TcpState>,
    /// Creation time
    pub created: Instant,
    /// Last seen time
    pub last_seen: Instant,
    /// Timeout
    pub timeout: Duration,
    /// Packet count (original direction)
    pub orig_packets: u64,
    /// Byte count (original direction)
    pub orig_bytes: u64,
    /// Packet count (reply direction)
    pub reply_packets: u64,
    /// Byte count (reply direction)
    pub reply_bytes: u64,
    /// NAT info (if any)
    pub nat: Option<NatInfo>,
    /// Mark
    pub mark: u32,
}

impl ConnTrackEntry {
    /// Create new entry
    pub fn new(tuple: ConnTuple, timeout: Duration) -> Self {
        Self {
            original: tuple,
            reply: tuple.reverse(),
            state: ConnState::New,
            tcp_state: if tuple.protocol == IpProtocol::Tcp {
                Some(TcpState::SynSent)
            } else {
                None
            },
            created: Instant::now(),
            last_seen: Instant::now(),
            timeout,
            orig_packets: 1,
            orig_bytes: 0,
            reply_packets: 0,
            reply_bytes: 0,
            nat: None,
            mark: 0,
        }
    }

    /// Check if entry has expired
    pub fn is_expired(&self) -> bool {
        self.last_seen.elapsed() > self.timeout
    }

    /// Update for packet in original direction
    pub fn update_original(&mut self, bytes: u64) {
        self.orig_packets += 1;
        self.orig_bytes += bytes;
        self.last_seen = Instant::now();
    }

    /// Update for packet in reply direction
    pub fn update_reply(&mut self, bytes: u64) {
        self.reply_packets += 1;
        self.reply_bytes += bytes;
        self.last_seen = Instant::now();

        if self.state == ConnState::New {
            self.state = ConnState::Established;
        }
    }
}

/// NAT type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    /// Source NAT (masquerade)
    Snat,
    /// Destination NAT (port forwarding)
    Dnat,
    /// Both source and destination
    FullNat,
}

/// NAT information
#[derive(Debug, Clone)]
pub struct NatInfo {
    /// NAT type
    pub nat_type: NatType,
    /// Translated source IP
    pub src_ip: Option<IpAddr>,
    /// Translated source port
    pub src_port: Option<u16>,
    /// Translated destination IP
    pub dst_ip: Option<IpAddr>,
    /// Translated destination port
    pub dst_port: Option<u16>,
}

/// Connection tracker
#[derive(Debug)]
pub struct ConnTracker {
    /// Connections by original tuple
    connections: RwLock<HashMap<ConnTuple, Arc<RwLock<ConnTrackEntry>>>>,
    /// Maximum entries
    max_entries: usize,
    /// Default TCP timeout
    tcp_timeout: Duration,
    /// Default UDP timeout
    udp_timeout: Duration,
    /// Default ICMP timeout
    icmp_timeout: Duration,
    /// Statistics
    stats: ConnTrackStats,
}

/// Connection tracker statistics
#[derive(Debug, Default)]
pub struct ConnTrackStats {
    /// Total entries created
    pub entries_created: AtomicU64,
    /// Total entries destroyed
    pub entries_destroyed: AtomicU64,
    /// Lookups performed
    pub lookups: AtomicU64,
    /// Lookup hits
    pub hits: AtomicU64,
    /// Entries dropped (table full)
    pub drops: AtomicU64,
}

impl ConnTracker {
    /// Create new connection tracker
    pub fn new(max_entries: usize) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            max_entries,
            tcp_timeout: Duration::from_secs(432000), // 5 days
            udp_timeout: Duration::from_secs(180),    // 3 minutes
            icmp_timeout: Duration::from_secs(30),    // 30 seconds
            stats: ConnTrackStats::default(),
        }
    }

    /// Get timeout for protocol
    fn timeout_for(&self, protocol: IpProtocol) -> Duration {
        match protocol {
            IpProtocol::Tcp => self.tcp_timeout,
            IpProtocol::Udp => self.udp_timeout,
            IpProtocol::Icmp | IpProtocol::Icmpv6 => self.icmp_timeout,
            IpProtocol::Other(_) => self.udp_timeout,
        }
    }

    /// Lookup connection
    pub fn lookup(&self, tuple: &ConnTuple) -> Option<Arc<RwLock<ConnTrackEntry>>> {
        self.stats.lookups.fetch_add(1, Ordering::Relaxed);

        let conns = self.connections.read().unwrap();

        // Try original direction
        if let Some(entry) = conns.get(tuple) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            return Some(Arc::clone(entry));
        }

        // Try reply direction
        let reverse = tuple.reverse();
        if let Some(entry) = conns.get(&reverse) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            return Some(Arc::clone(entry));
        }

        None
    }

    /// Create or update connection
    pub fn track(
        &self,
        tuple: ConnTuple,
        bytes: u64,
        is_reply: bool,
    ) -> Arc<RwLock<ConnTrackEntry>> {
        // Check if exists
        if let Some(entry) = self.lookup(&tuple) {
            {
                let mut e = entry.write().unwrap();
                if is_reply {
                    e.update_reply(bytes);
                } else {
                    e.update_original(bytes);
                }
            }
            return entry;
        }

        // Create new entry
        let mut conns = self.connections.write().unwrap();

        // Check capacity
        if conns.len() >= self.max_entries {
            self.stats.drops.fetch_add(1, Ordering::Relaxed);
            // Return a temporary entry
            return Arc::new(RwLock::new(ConnTrackEntry::new(
                tuple,
                self.timeout_for(tuple.protocol),
            )));
        }

        let timeout = self.timeout_for(tuple.protocol);
        let entry = Arc::new(RwLock::new(ConnTrackEntry::new(tuple, timeout)));
        conns.insert(tuple, Arc::clone(&entry));
        self.stats.entries_created.fetch_add(1, Ordering::Relaxed);

        entry
    }

    /// Remove expired entries
    pub fn gc(&self) -> usize {
        let mut conns = self.connections.write().unwrap();
        let before = conns.len();

        conns.retain(|_, entry| {
            let e = entry.read().unwrap();
            !e.is_expired()
        });

        let removed = before - conns.len();
        self.stats
            .entries_destroyed
            .fetch_add(removed as u64, Ordering::Relaxed);
        removed
    }

    /// Get connection count
    pub fn len(&self) -> usize {
        self.connections.read().unwrap().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.connections.read().unwrap().is_empty()
    }

    /// Get statistics
    pub fn stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.stats.entries_created.load(Ordering::Relaxed),
            self.stats.entries_destroyed.load(Ordering::Relaxed),
            self.stats.lookups.load(Ordering::Relaxed),
            self.stats.hits.load(Ordering::Relaxed),
            self.stats.drops.load(Ordering::Relaxed),
        )
    }
}

impl Default for ConnTracker {
    fn default() -> Self {
        Self::new(65536)
    }
}

/// Filter action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAction {
    /// Accept packet
    Accept,
    /// Drop packet silently
    Drop,
    /// Reject with ICMP error
    Reject,
    /// Jump to another chain
    Jump(u32),
    /// Return from chain
    Return,
    /// Log and continue
    Log,
    /// Apply SNAT
    Snat,
    /// Apply DNAT
    Dnat,
    /// Apply masquerade
    Masquerade,
}

/// IP address match
#[derive(Debug, Clone, Default)]
pub enum IpMatch {
    /// Any address
    #[default]
    Any,
    /// Exact address
    Exact(IpAddr),
    /// Network (CIDR)
    Network { addr: IpAddr, prefix_len: u8 },
    /// Range
    Range { start: IpAddr, end: IpAddr },
    /// Negated match
    Not(Box<IpMatch>),
}

impl IpMatch {
    /// Check if IP matches
    pub fn matches(&self, ip: IpAddr) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(addr) => ip == *addr,
            Self::Network { addr, prefix_len } => match (ip, addr) {
                (IpAddr::V4(ip), IpAddr::V4(net)) => {
                    let mask = if *prefix_len >= 32 {
                        !0u32
                    } else {
                        !0u32 << (32 - prefix_len)
                    };
                    (u32::from(ip) & mask) == (u32::from(*net) & mask)
                }
                (IpAddr::V6(ip), IpAddr::V6(net)) => {
                    let ip_bits = u128::from(ip);
                    let net_bits = u128::from(*net);
                    let mask = if *prefix_len >= 128 {
                        !0u128
                    } else {
                        !0u128 << (128 - prefix_len)
                    };
                    (ip_bits & mask) == (net_bits & mask)
                }
                _ => false,
            },
            Self::Range { start, end } => ip >= *start && ip <= *end,
            Self::Not(inner) => !inner.matches(ip),
        }
    }
}

/// Port match
#[derive(Debug, Clone, Default)]
pub enum PortMatch {
    /// Any port
    #[default]
    Any,
    /// Exact port
    Exact(u16),
    /// Port range
    Range { start: u16, end: u16 },
    /// Multiple ports
    Set(Vec<u16>),
    /// Negated match
    Not(Box<PortMatch>),
}

impl PortMatch {
    /// Check if port matches
    pub fn matches(&self, port: u16) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(p) => port == *p,
            Self::Range { start, end } => port >= *start && port <= *end,
            Self::Set(ports) => ports.contains(&port),
            Self::Not(inner) => !inner.matches(port),
        }
    }
}

/// Protocol match
#[derive(Debug, Clone, Default)]
pub enum ProtocolMatch {
    /// Any protocol
    #[default]
    Any,
    /// Specific protocol
    Exact(IpProtocol),
    /// Multiple protocols
    Set(Vec<IpProtocol>),
}

impl ProtocolMatch {
    /// Check if protocol matches
    pub fn matches(&self, proto: IpProtocol) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(p) => proto == *p,
            Self::Set(protos) => protos.contains(&proto),
        }
    }
}

/// Connection state match
#[derive(Debug, Clone, Default)]
pub struct StateMatch {
    /// Match new connections
    pub new: bool,
    /// Match established connections
    pub established: bool,
    /// Match related connections
    pub related: bool,
    /// Match invalid connections
    pub invalid: bool,
}

impl StateMatch {
    /// Match any state
    pub fn any() -> Self {
        Self {
            new: true,
            established: true,
            related: true,
            invalid: true,
        }
    }

    /// Match established and related
    pub fn established_related() -> Self {
        Self {
            new: false,
            established: true,
            related: true,
            invalid: false,
        }
    }

    /// Check if state matches
    pub fn matches(&self, state: ConnState) -> bool {
        match state {
            ConnState::New => self.new,
            ConnState::Established => self.established,
            ConnState::Related => self.related,
            ConnState::Invalid => self.invalid,
        }
    }
}

/// Filter rule
#[derive(Debug)]
pub struct FilterRule {
    /// Rule ID
    pub id: u32,
    /// Rule name/comment
    pub comment: Option<String>,
    /// Source IP match
    pub src_ip: IpMatch,
    /// Destination IP match
    pub dst_ip: IpMatch,
    /// Protocol match
    pub protocol: ProtocolMatch,
    /// Source port match
    pub src_port: PortMatch,
    /// Destination port match
    pub dst_port: PortMatch,
    /// Connection state match
    pub state: Option<StateMatch>,
    /// Input interface
    pub in_interface: Option<String>,
    /// Output interface
    pub out_interface: Option<String>,
    /// Action
    pub action: FilterAction,
    /// NAT target (for SNAT/DNAT)
    pub nat_target: Option<NatTarget>,
    /// Packet count
    pub packets: AtomicU64,
    /// Byte count
    pub bytes: AtomicU64,
}

impl Clone for FilterRule {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            comment: self.comment.clone(),
            src_ip: self.src_ip.clone(),
            dst_ip: self.dst_ip.clone(),
            protocol: self.protocol.clone(),
            src_port: self.src_port.clone(),
            dst_port: self.dst_port.clone(),
            state: self.state.clone(),
            in_interface: self.in_interface.clone(),
            out_interface: self.out_interface.clone(),
            action: self.action,
            nat_target: self.nat_target.clone(),
            packets: AtomicU64::new(self.packets.load(Ordering::Relaxed)),
            bytes: AtomicU64::new(self.bytes.load(Ordering::Relaxed)),
        }
    }
}

impl FilterRule {
    /// Create new rule
    pub fn new(id: u32, action: FilterAction) -> Self {
        Self {
            id,
            comment: None,
            src_ip: IpMatch::Any,
            dst_ip: IpMatch::Any,
            protocol: ProtocolMatch::Any,
            src_port: PortMatch::Any,
            dst_port: PortMatch::Any,
            state: None,
            in_interface: None,
            out_interface: None,
            action,
            nat_target: None,
            packets: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        }
    }

    /// Check if packet matches rule
    pub fn matches(&self, packet: &PacketInfo) -> bool {
        // Check source IP
        if !self.src_ip.matches(packet.src_ip) {
            return false;
        }

        // Check destination IP
        if !self.dst_ip.matches(packet.dst_ip) {
            return false;
        }

        // Check protocol
        if !self.protocol.matches(packet.protocol) {
            return false;
        }

        // Check source port
        if !self.src_port.matches(packet.src_port) {
            return false;
        }

        // Check destination port
        if !self.dst_port.matches(packet.dst_port) {
            return false;
        }

        // Check state
        if let Some(ref state_match) = self.state {
            if !state_match.matches(packet.conn_state) {
                return false;
            }
        }

        // Check interfaces
        if let Some(ref iface) = self.in_interface {
            if packet.in_interface.as_ref() != Some(iface) {
                return false;
            }
        }

        if let Some(ref iface) = self.out_interface {
            if packet.out_interface.as_ref() != Some(iface) {
                return false;
            }
        }

        true
    }

    /// Update counters
    pub fn update_counters(&self, bytes: u64) {
        self.packets.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Get counters
    pub fn counters(&self) -> (u64, u64) {
        (
            self.packets.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }
}

/// NAT target
#[derive(Debug, Clone)]
pub struct NatTarget {
    /// Target IP (or range)
    pub ip: Option<IpAddr>,
    /// Target port (or range)
    pub port: Option<u16>,
    /// Random port selection
    pub random: bool,
    /// Persistent mapping
    pub persistent: bool,
}

/// Packet information for filtering
#[derive(Debug, Clone)]
pub struct PacketInfo {
    /// Source IP
    pub src_ip: IpAddr,
    /// Destination IP
    pub dst_ip: IpAddr,
    /// Source port
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// Protocol
    pub protocol: IpProtocol,
    /// Packet length
    pub length: u32,
    /// Connection state
    pub conn_state: ConnState,
    /// Input interface
    pub in_interface: Option<String>,
    /// Output interface
    pub out_interface: Option<String>,
    /// TCP flags (if TCP)
    pub tcp_flags: Option<u8>,
}

impl PacketInfo {
    /// Create from connection tuple
    pub fn from_tuple(tuple: &ConnTuple, length: u32) -> Self {
        Self {
            src_ip: tuple.src_ip,
            dst_ip: tuple.dst_ip,
            src_port: tuple.src_port,
            dst_port: tuple.dst_port,
            protocol: tuple.protocol,
            length,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        }
    }

    /// Get connection tuple
    pub fn to_tuple(&self) -> ConnTuple {
        ConnTuple {
            src_ip: self.src_ip,
            dst_ip: self.dst_ip,
            src_port: self.src_port,
            dst_port: self.dst_port,
            protocol: self.protocol,
        }
    }
}

/// Filter chain type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChainType {
    /// Input chain (packets to local)
    Input,
    /// Output chain (packets from local)
    Output,
    /// Forward chain (routed packets)
    Forward,
    /// Prerouting chain (before routing decision)
    Prerouting,
    /// Postrouting chain (after routing decision)
    Postrouting,
    /// Custom chain
    Custom(u32),
}

/// Filter chain
#[derive(Debug)]
pub struct FilterChain {
    /// Chain type
    pub chain_type: ChainType,
    /// Chain name
    pub name: String,
    /// Rules
    rules: Vec<FilterRule>,
    /// Default policy
    pub policy: FilterAction,
    /// Next rule ID
    next_rule_id: u32,
}

impl FilterChain {
    /// Create new chain
    pub fn new(chain_type: ChainType, name: &str) -> Self {
        Self {
            chain_type,
            name: name.to_string(),
            rules: Vec::new(),
            policy: FilterAction::Accept,
            next_rule_id: 1,
        }
    }

    /// Add rule
    pub fn add_rule(&mut self, mut rule: FilterRule) -> u32 {
        let id = self.next_rule_id;
        self.next_rule_id += 1;
        rule.id = id;
        self.rules.push(rule);
        id
    }

    /// Insert rule at position
    pub fn insert_rule(&mut self, index: usize, mut rule: FilterRule) -> u32 {
        let id = self.next_rule_id;
        self.next_rule_id += 1;
        rule.id = id;
        if index >= self.rules.len() {
            self.rules.push(rule);
        } else {
            self.rules.insert(index, rule);
        }
        id
    }

    /// Remove rule by ID
    pub fn remove_rule(&mut self, rule_id: u32) -> Option<FilterRule> {
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule_id) {
            Some(self.rules.remove(pos))
        } else {
            None
        }
    }

    /// Get rules
    pub fn rules(&self) -> &[FilterRule] {
        &self.rules
    }

    /// Evaluate packet against chain
    pub fn evaluate(&self, packet: &PacketInfo) -> FilterAction {
        for rule in &self.rules {
            if rule.matches(packet) {
                rule.update_counters(packet.length as u64);
                return rule.action;
            }
        }
        self.policy
    }

    /// Flush all rules
    pub fn flush(&mut self) {
        self.rules.clear();
    }
}

/// Network filter (firewall)
#[derive(Debug)]
pub struct NetworkFilter {
    /// Filter chains
    chains: RwLock<HashMap<ChainType, FilterChain>>,
    /// Connection tracker
    conntrack: Arc<ConnTracker>,
    /// Enabled
    enabled: std::sync::atomic::AtomicBool,
}

impl NetworkFilter {
    /// Create new filter
    pub fn new() -> Self {
        let mut chains = HashMap::new();

        // Create built-in chains
        chains.insert(
            ChainType::Input,
            FilterChain::new(ChainType::Input, "INPUT"),
        );
        chains.insert(
            ChainType::Output,
            FilterChain::new(ChainType::Output, "OUTPUT"),
        );
        chains.insert(
            ChainType::Forward,
            FilterChain::new(ChainType::Forward, "FORWARD"),
        );
        chains.insert(
            ChainType::Prerouting,
            FilterChain::new(ChainType::Prerouting, "PREROUTING"),
        );
        chains.insert(
            ChainType::Postrouting,
            FilterChain::new(ChainType::Postrouting, "POSTROUTING"),
        );

        Self {
            chains: RwLock::new(chains),
            conntrack: Arc::new(ConnTracker::default()),
            enabled: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Enable filtering
    pub fn enable(&self) {
        self.enabled
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Disable filtering
    pub fn disable(&self) {
        self.enabled
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Get connection tracker
    pub fn conntrack(&self) -> &Arc<ConnTracker> {
        &self.conntrack
    }

    /// Add rule to chain
    pub fn add_rule(&self, chain: ChainType, rule: FilterRule) -> Option<u32> {
        self.chains
            .write()
            .unwrap()
            .get_mut(&chain)
            .map(|c| c.add_rule(rule))
    }

    /// Remove rule from chain
    pub fn remove_rule(&self, chain: ChainType, rule_id: u32) -> Option<FilterRule> {
        self.chains
            .write()
            .unwrap()
            .get_mut(&chain)
            .and_then(|c| c.remove_rule(rule_id))
    }

    /// Set chain policy
    pub fn set_policy(&self, chain: ChainType, policy: FilterAction) -> bool {
        if let Some(c) = self.chains.write().unwrap().get_mut(&chain) {
            c.policy = policy;
            true
        } else {
            false
        }
    }

    /// Flush chain
    pub fn flush_chain(&self, chain: ChainType) -> bool {
        if let Some(c) = self.chains.write().unwrap().get_mut(&chain) {
            c.flush();
            true
        } else {
            false
        }
    }

    /// Filter packet
    pub fn filter(&self, chain: ChainType, packet: &mut PacketInfo) -> FilterAction {
        if !self.is_enabled() {
            return FilterAction::Accept;
        }

        // Track connection
        let tuple = packet.to_tuple();
        if let Some(entry) = self.conntrack.lookup(&tuple) {
            let e = entry.read().unwrap();
            packet.conn_state = e.state;
        } else {
            self.conntrack.track(tuple, packet.length as u64, false);
        }

        // Evaluate chain
        let chains = self.chains.read().unwrap();
        if let Some(chain) = chains.get(&chain) {
            chain.evaluate(packet)
        } else {
            FilterAction::Accept
        }
    }

    /// Get chain rules
    pub fn get_rules(&self, chain: ChainType) -> Vec<FilterRule> {
        self.chains
            .read()
            .unwrap()
            .get(&chain)
            .map(|c| c.rules().to_vec())
            .unwrap_or_default()
    }
}

impl Default for NetworkFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_protocol_conversion() {
        assert_eq!(IpProtocol::from(6), IpProtocol::Tcp);
        assert_eq!(IpProtocol::from(17), IpProtocol::Udp);
        assert_eq!(u8::from(IpProtocol::Tcp), 6);
    }

    #[test]
    fn test_conn_tuple_reverse() {
        let tuple = ConnTuple::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            12345,
            80,
            IpProtocol::Tcp,
        );

        let reverse = tuple.reverse();
        assert_eq!(reverse.src_ip, tuple.dst_ip);
        assert_eq!(reverse.dst_ip, tuple.src_ip);
        assert_eq!(reverse.src_port, tuple.dst_port);
        assert_eq!(reverse.dst_port, tuple.src_port);
    }

    #[test]
    fn test_conn_track_entry() {
        let tuple = ConnTuple::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            12345,
            80,
            IpProtocol::Tcp,
        );

        let mut entry = ConnTrackEntry::new(tuple, Duration::from_secs(60));
        assert_eq!(entry.state, ConnState::New);

        entry.update_reply(100);
        assert_eq!(entry.state, ConnState::Established);
        assert_eq!(entry.reply_packets, 1);
    }

    #[test]
    fn test_conn_tracker() {
        let tracker = ConnTracker::new(100);

        let tuple = ConnTuple::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            12345,
            80,
            IpProtocol::Tcp,
        );

        let entry = tracker.track(tuple, 100, false);
        assert_eq!(tracker.len(), 1);

        let found = tracker.lookup(&tuple);
        assert!(found.is_some());
    }

    #[test]
    fn test_ip_match_exact() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let match_exact = IpMatch::Exact(ip);

        assert!(match_exact.matches(ip));
        assert!(!match_exact.matches(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2))));
    }

    #[test]
    fn test_ip_match_network() {
        let network = IpMatch::Network {
            addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 0)),
            prefix_len: 24,
        };

        assert!(network.matches(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(network.matches(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 254))));
        assert!(!network.matches(IpAddr::V4(Ipv4Addr::new(192, 168, 2, 1))));
    }

    #[test]
    fn test_port_match() {
        let exact = PortMatch::Exact(80);
        assert!(exact.matches(80));
        assert!(!exact.matches(443));

        let range = PortMatch::Range {
            start: 1024,
            end: 65535,
        };
        assert!(range.matches(8080));
        assert!(!range.matches(80));
    }

    #[test]
    fn test_state_match() {
        let est_rel = StateMatch::established_related();
        assert!(est_rel.matches(ConnState::Established));
        assert!(est_rel.matches(ConnState::Related));
        assert!(!est_rel.matches(ConnState::New));
    }

    #[test]
    fn test_filter_rule_match() {
        let rule = FilterRule::new(1, FilterAction::Accept);

        let packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 12345,
            dst_port: 80,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        assert!(rule.matches(&packet));
    }

    #[test]
    fn test_filter_rule_specific_match() {
        let mut rule = FilterRule::new(1, FilterAction::Drop);
        rule.dst_port = PortMatch::Exact(22);
        rule.protocol = ProtocolMatch::Exact(IpProtocol::Tcp);

        let ssh_packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 12345,
            dst_port: 22,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        let http_packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 12345,
            dst_port: 80,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        assert!(rule.matches(&ssh_packet));
        assert!(!rule.matches(&http_packet));
    }

    #[test]
    fn test_filter_chain() {
        let mut chain = FilterChain::new(ChainType::Input, "INPUT");
        chain.policy = FilterAction::Drop;

        let mut allow_established = FilterRule::new(0, FilterAction::Accept);
        allow_established.state = Some(StateMatch::established_related());
        chain.add_rule(allow_established);

        let established_packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 80,
            dst_port: 12345,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::Established,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        let new_packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 12345,
            dst_port: 22,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        assert_eq!(chain.evaluate(&established_packet), FilterAction::Accept);
        assert_eq!(chain.evaluate(&new_packet), FilterAction::Drop);
    }

    #[test]
    fn test_network_filter() {
        let filter = NetworkFilter::new();

        // Add rule to drop SSH
        let mut rule = FilterRule::new(0, FilterAction::Drop);
        rule.dst_port = PortMatch::Exact(22);
        rule.protocol = ProtocolMatch::Exact(IpProtocol::Tcp);
        filter.add_rule(ChainType::Input, rule);

        let mut packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 12345,
            dst_port: 22,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        assert_eq!(
            filter.filter(ChainType::Input, &mut packet),
            FilterAction::Drop
        );
    }

    #[test]
    fn test_network_filter_disabled() {
        let filter = NetworkFilter::new();
        filter.disable();

        let mut packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 12345,
            dst_port: 22,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        // Should accept everything when disabled
        assert_eq!(
            filter.filter(ChainType::Input, &mut packet),
            FilterAction::Accept
        );
    }

    #[test]
    fn test_chain_policy() {
        let filter = NetworkFilter::new();
        filter.set_policy(ChainType::Forward, FilterAction::Drop);

        let mut packet = PacketInfo {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            src_port: 12345,
            dst_port: 80,
            protocol: IpProtocol::Tcp,
            length: 100,
            conn_state: ConnState::New,
            in_interface: None,
            out_interface: None,
            tcp_flags: None,
        };

        assert_eq!(
            filter.filter(ChainType::Forward, &mut packet),
            FilterAction::Drop
        );
    }

    #[test]
    fn test_ip_match_negation() {
        let not_local = IpMatch::Not(Box::new(IpMatch::Network {
            addr: IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0)),
            prefix_len: 16,
        }));

        assert!(!not_local.matches(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(not_local.matches(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    }

    #[test]
    fn test_filter_rule_counters() {
        let rule = FilterRule::new(1, FilterAction::Accept);
        rule.update_counters(100);
        rule.update_counters(200);

        let (packets, bytes) = rule.counters();
        assert_eq!(packets, 2);
        assert_eq!(bytes, 300);
    }
}
