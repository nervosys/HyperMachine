//! Virtual Switch Implementation
//!
//! This module provides a software-based virtual switch with support for
//! MAC learning, VLAN tagging, spanning tree protocol, and port mirroring.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// MAC address (6 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Broadcast MAC address
    pub const BROADCAST: Self = Self([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);

    /// Zero MAC address
    pub const ZERO: Self = Self([0, 0, 0, 0, 0, 0]);

    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// Check if broadcast
    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }

    /// Check if multicast
    pub fn is_multicast(&self) -> bool {
        self.0[0] & 0x01 != 0
    }

    /// Check if locally administered
    pub fn is_local(&self) -> bool {
        self.0[0] & 0x02 != 0
    }

    /// Generate random locally administered MAC
    pub fn random_local() -> Self {
        let mut bytes = [0u8; 6];
        // Use simple pseudo-random based on time
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = ((seed >> (i * 8)) & 0xFF) as u8;
        }

        // Set locally administered bit, clear multicast bit
        bytes[0] = (bytes[0] & 0xFE) | 0x02;
        Self(bytes)
    }
}

impl std::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

/// VLAN ID (12 bits, 0-4095)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VlanId(pub u16);

impl VlanId {
    /// Default VLAN (untagged)
    pub const DEFAULT: Self = Self(1);

    /// Reserved VLAN
    pub const RESERVED: Self = Self(4095);

    /// Create new VLAN ID
    pub fn new(id: u16) -> Option<Self> {
        if id <= 4094 {
            Some(Self(id))
        } else {
            None
        }
    }

    /// Get raw ID
    pub fn id(&self) -> u16 {
        self.0
    }

    /// Check if valid (non-reserved)
    pub fn is_valid(&self) -> bool {
        self.0 > 0 && self.0 < 4095
    }
}

/// VLAN tagging mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VlanMode {
    /// Access port - single VLAN, untagged
    Access(VlanId),
    /// Trunk port - multiple VLANs, tagged
    Trunk {
        native_vlan: VlanId,
        allowed_vlans: VlanSet,
    },
    /// Hybrid port - some tagged, some untagged
    Hybrid {
        untagged_vlan: VlanId,
        tagged_vlans: VlanSet,
    },
}

impl Default for VlanMode {
    fn default() -> Self {
        Self::Access(VlanId::DEFAULT)
    }
}

/// Set of VLANs (bitmap)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VlanSet {
    /// Bitmap for VLANs 0-63
    low: u64,
    /// Bitmap for VLANs 64-127
    mid_low: u64,
    /// Bitmap for VLANs 128-191
    mid_high: u64,
    /// Bitmap for VLANs 192-255
    high: u64,
    // For simplicity, we track only first 256 VLANs in bitmap
    // Full range would need 512 bytes
}

impl VlanSet {
    /// Empty set
    pub fn empty() -> Self {
        Self {
            low: 0,
            mid_low: 0,
            mid_high: 0,
            high: 0,
        }
    }

    /// All VLANs (1-4094)
    pub fn all() -> Self {
        Self {
            low: !0,
            mid_low: !0,
            mid_high: !0,
            high: !0,
        }
    }

    /// Add VLAN to set
    pub fn add(&mut self, vlan: VlanId) {
        let id = vlan.0 as usize;
        if id < 64 {
            self.low |= 1 << id;
        } else if id < 128 {
            self.mid_low |= 1 << (id - 64);
        } else if id < 192 {
            self.mid_high |= 1 << (id - 128);
        } else if id < 256 {
            self.high |= 1 << (id - 192);
        }
    }

    /// Remove VLAN from set
    pub fn remove(&mut self, vlan: VlanId) {
        let id = vlan.0 as usize;
        if id < 64 {
            self.low &= !(1 << id);
        } else if id < 128 {
            self.mid_low &= !(1 << (id - 64));
        } else if id < 192 {
            self.mid_high &= !(1 << (id - 128));
        } else if id < 256 {
            self.high &= !(1 << (id - 192));
        }
    }

    /// Check if VLAN is in set
    pub fn contains(&self, vlan: VlanId) -> bool {
        let id = vlan.0 as usize;
        if id < 64 {
            self.low & (1 << id) != 0
        } else if id < 128 {
            self.mid_low & (1 << (id - 64)) != 0
        } else if id < 192 {
            self.mid_high & (1 << (id - 128)) != 0
        } else if id < 256 {
            self.high & (1 << (id - 192)) != 0
        } else {
            // VLANs 256-4094 are always considered in "all" set
            true
        }
    }
}

impl Default for VlanSet {
    fn default() -> Self {
        Self::all()
    }
}

/// Port state (STP)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PortState {
    /// Port is disabled
    Disabled,
    /// Port is blocking (STP)
    Blocking,
    /// Port is listening (STP)
    Listening,
    /// Port is learning (STP)
    Learning,
    /// Port is forwarding
    #[default]
    Forwarding,
}

/// Port type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortType {
    /// Internal port (connected to VM)
    Internal,
    /// External port (connected to physical NIC)
    External,
    /// Patch port (connected to another switch)
    Patch,
    /// Mirror port (receives copies of traffic)
    Mirror,
}

/// Port statistics
#[derive(Debug, Clone, Default)]
pub struct PortStats {
    /// Packets received
    pub rx_packets: u64,
    /// Packets transmitted
    pub tx_packets: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Receive errors
    pub rx_errors: u64,
    /// Transmit errors
    pub tx_errors: u64,
    /// Dropped packets (RX)
    pub rx_dropped: u64,
    /// Dropped packets (TX)
    pub tx_dropped: u64,
    /// Multicast packets received
    pub rx_multicast: u64,
    /// Broadcast packets received
    pub rx_broadcast: u64,
}

impl PortStats {
    /// Add received packet
    pub fn add_rx(&mut self, bytes: u64, is_multicast: bool, is_broadcast: bool) {
        self.rx_packets += 1;
        self.rx_bytes += bytes;
        if is_multicast {
            self.rx_multicast += 1;
        }
        if is_broadcast {
            self.rx_broadcast += 1;
        }
    }

    /// Add transmitted packet
    pub fn add_tx(&mut self, bytes: u64) {
        self.tx_packets += 1;
        self.tx_bytes += bytes;
    }
}

/// Port ID
pub type PortId = u32;

/// Virtual switch port
#[derive(Debug, Clone)]
pub struct Port {
    /// Port ID
    pub id: PortId,
    /// Port name
    pub name: String,
    /// Port type
    pub port_type: PortType,
    /// Port state
    pub state: PortState,
    /// VLAN mode
    pub vlan_mode: VlanMode,
    /// MAC address of the port
    pub mac: MacAddress,
    /// Port is administratively enabled
    pub admin_enabled: bool,
    /// Link is up
    pub link_up: bool,
    /// Speed in Mbps
    pub speed: u32,
    /// Full duplex
    pub full_duplex: bool,
    /// MTU
    pub mtu: u16,
    /// Statistics
    pub stats: PortStats,
    /// Mirror destination port (if any)
    pub mirror_to: Option<PortId>,
}

impl Port {
    /// Create new port
    pub fn new(id: PortId, name: &str, port_type: PortType) -> Self {
        Self {
            id,
            name: name.to_string(),
            port_type,
            state: PortState::Forwarding,
            vlan_mode: VlanMode::default(),
            mac: MacAddress::random_local(),
            admin_enabled: true,
            link_up: true,
            speed: 10000, // 10 Gbps default
            full_duplex: true,
            mtu: 1500,
            stats: PortStats::default(),
            mirror_to: None,
        }
    }

    /// Check if port can forward
    pub fn can_forward(&self) -> bool {
        self.admin_enabled && self.link_up && self.state == PortState::Forwarding
    }

    /// Check if port can learn
    pub fn can_learn(&self) -> bool {
        self.admin_enabled
            && self.link_up
            && (self.state == PortState::Learning || self.state == PortState::Forwarding)
    }

    /// Check if VLAN is allowed on this port
    pub fn allows_vlan(&self, vlan: VlanId) -> bool {
        match &self.vlan_mode {
            VlanMode::Access(access_vlan) => *access_vlan == vlan,
            VlanMode::Trunk { allowed_vlans, .. } => allowed_vlans.contains(vlan),
            VlanMode::Hybrid {
                untagged_vlan,
                tagged_vlans,
            } => *untagged_vlan == vlan || tagged_vlans.contains(vlan),
        }
    }

    /// Get effective VLAN for untagged frame
    pub fn untagged_vlan(&self) -> VlanId {
        match &self.vlan_mode {
            VlanMode::Access(vlan) => *vlan,
            VlanMode::Trunk { native_vlan, .. } => *native_vlan,
            VlanMode::Hybrid { untagged_vlan, .. } => *untagged_vlan,
        }
    }

    /// Check if frame should be tagged on egress
    pub fn should_tag(&self, vlan: VlanId) -> bool {
        match &self.vlan_mode {
            VlanMode::Access(_) => false,
            VlanMode::Trunk { native_vlan, .. } => vlan != *native_vlan,
            VlanMode::Hybrid {
                untagged_vlan,
                tagged_vlans,
            } => vlan != *untagged_vlan && tagged_vlans.contains(vlan),
        }
    }
}

/// MAC table entry
#[derive(Debug, Clone)]
pub struct MacEntry {
    /// MAC address
    pub mac: MacAddress,
    /// Port ID
    pub port: PortId,
    /// VLAN ID
    pub vlan: VlanId,
    /// Entry is static (won't age)
    pub is_static: bool,
    /// Last seen timestamp
    pub last_seen: Instant,
}

impl MacEntry {
    /// Create new dynamic entry
    pub fn new(mac: MacAddress, port: PortId, vlan: VlanId) -> Self {
        Self {
            mac,
            port,
            vlan,
            is_static: false,
            last_seen: Instant::now(),
        }
    }

    /// Create static entry
    pub fn new_static(mac: MacAddress, port: PortId, vlan: VlanId) -> Self {
        Self {
            mac,
            port,
            vlan,
            is_static: true,
            last_seen: Instant::now(),
        }
    }

    /// Check if entry has expired
    pub fn is_expired(&self, aging_time: Duration) -> bool {
        !self.is_static && self.last_seen.elapsed() > aging_time
    }

    /// Update last seen time
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }
}

/// MAC learning table
#[derive(Debug)]
pub struct MacTable {
    /// Entries keyed by (MAC, VLAN)
    entries: HashMap<(MacAddress, VlanId), MacEntry>,
    /// Maximum entries
    max_entries: usize,
    /// Aging time
    aging_time: Duration,
}

impl MacTable {
    /// Create new MAC table
    pub fn new(max_entries: usize, aging_time: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
            aging_time,
        }
    }

    /// Learn MAC address
    pub fn learn(&mut self, mac: MacAddress, port: PortId, vlan: VlanId) -> bool {
        // Don't learn broadcast/multicast
        if mac.is_broadcast() || mac.is_multicast() {
            return false;
        }

        let key = (mac, vlan);

        if let Some(entry) = self.entries.get_mut(&key) {
            // Update existing entry
            entry.port = port;
            entry.touch();
            true
        } else if self.entries.len() < self.max_entries {
            // Add new entry
            self.entries.insert(key, MacEntry::new(mac, port, vlan));
            true
        } else {
            // Table full
            false
        }
    }

    /// Add static MAC entry
    pub fn add_static(&mut self, mac: MacAddress, port: PortId, vlan: VlanId) -> bool {
        let key = (mac, vlan);
        if self.entries.len() < self.max_entries || self.entries.contains_key(&key) {
            self.entries
                .insert(key, MacEntry::new_static(mac, port, vlan));
            true
        } else {
            false
        }
    }

    /// Remove MAC entry
    pub fn remove(&mut self, mac: MacAddress, vlan: VlanId) -> Option<MacEntry> {
        self.entries.remove(&(mac, vlan))
    }

    /// Lookup MAC address
    pub fn lookup(&self, mac: MacAddress, vlan: VlanId) -> Option<&MacEntry> {
        self.entries.get(&(mac, vlan))
    }

    /// Age out old entries
    pub fn age(&mut self) -> usize {
        let aging_time = self.aging_time;
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| !entry.is_expired(aging_time));
        before - self.entries.len()
    }

    /// Flush all dynamic entries
    pub fn flush_dynamic(&mut self) {
        self.entries.retain(|_, entry| entry.is_static);
    }

    /// Flush entries for port
    pub fn flush_port(&mut self, port: PortId) {
        self.entries.retain(|_, entry| entry.port != port);
    }

    /// Get entry count
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get all entries
    pub fn entries(&self) -> impl Iterator<Item = &MacEntry> {
        self.entries.values()
    }
}

impl Default for MacTable {
    fn default() -> Self {
        Self::new(8192, Duration::from_secs(300))
    }
}

/// Spanning Tree Protocol state
#[derive(Debug, Clone)]
pub struct StpState {
    /// Bridge ID (priority + MAC)
    pub bridge_id: u64,
    /// Root bridge ID
    pub root_id: u64,
    /// Root path cost
    pub root_path_cost: u32,
    /// Root port
    pub root_port: Option<PortId>,
    /// Is this bridge the root?
    pub is_root: bool,
    /// Hello time (seconds)
    pub hello_time: u16,
    /// Max age (seconds)
    pub max_age: u16,
    /// Forward delay (seconds)
    pub forward_delay: u16,
}

impl Default for StpState {
    fn default() -> Self {
        Self {
            bridge_id: 0x8000_0000_0000_0000, // Priority 32768
            root_id: 0x8000_0000_0000_0000,
            root_path_cost: 0,
            root_port: None,
            is_root: true,
            hello_time: 2,
            max_age: 20,
            forward_delay: 15,
        }
    }
}

/// Ethernet frame (simplified)
#[derive(Debug, Clone)]
pub struct EthernetFrame {
    /// Destination MAC
    pub dst_mac: MacAddress,
    /// Source MAC
    pub src_mac: MacAddress,
    /// VLAN tag (if present)
    pub vlan_tag: Option<VlanId>,
    /// EtherType
    pub ethertype: u16,
    /// Payload
    pub payload: Vec<u8>,
}

impl EthernetFrame {
    /// Minimum frame size (without VLAN)
    pub const MIN_SIZE: usize = 64;

    /// Maximum frame size (without VLAN)
    pub const MAX_SIZE: usize = 1518;

    /// VLAN EtherType
    pub const ETHERTYPE_VLAN: u16 = 0x8100;

    /// IPv4 EtherType
    pub const ETHERTYPE_IPV4: u16 = 0x0800;

    /// IPv6 EtherType
    pub const ETHERTYPE_IPV6: u16 = 0x86DD;

    /// ARP EtherType
    pub const ETHERTYPE_ARP: u16 = 0x0806;

    /// Parse from raw bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 14 {
            return None;
        }

        let dst_mac =
            MacAddress::from_bytes([data[0], data[1], data[2], data[3], data[4], data[5]]);
        let src_mac =
            MacAddress::from_bytes([data[6], data[7], data[8], data[9], data[10], data[11]]);

        let ethertype = u16::from_be_bytes([data[12], data[13]]);

        if ethertype == Self::ETHERTYPE_VLAN && data.len() >= 18 {
            // VLAN tagged frame
            let vlan_tag = VlanId::new(u16::from_be_bytes([data[14], data[15]]) & 0x0FFF);
            let inner_ethertype = u16::from_be_bytes([data[16], data[17]]);
            let payload = data[18..].to_vec();

            Some(Self {
                dst_mac,
                src_mac,
                vlan_tag,
                ethertype: inner_ethertype,
                payload,
            })
        } else {
            // Untagged frame
            let payload = data[14..].to_vec();

            Some(Self {
                dst_mac,
                src_mac,
                vlan_tag: None,
                ethertype,
                payload,
            })
        }
    }

    /// Serialize to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(self.payload.len() + 18);

        result.extend_from_slice(&self.dst_mac.0);
        result.extend_from_slice(&self.src_mac.0);

        if let Some(vlan) = self.vlan_tag {
            result.extend_from_slice(&Self::ETHERTYPE_VLAN.to_be_bytes());
            result.extend_from_slice(&vlan.0.to_be_bytes());
        }

        result.extend_from_slice(&self.ethertype.to_be_bytes());
        result.extend_from_slice(&self.payload);

        result
    }

    /// Get total frame size
    pub fn size(&self) -> usize {
        14 + if self.vlan_tag.is_some() { 4 } else { 0 } + self.payload.len()
    }

    /// Check if broadcast
    pub fn is_broadcast(&self) -> bool {
        self.dst_mac.is_broadcast()
    }

    /// Check if multicast
    pub fn is_multicast(&self) -> bool {
        self.dst_mac.is_multicast()
    }
}

/// Switch statistics
#[derive(Debug, Clone, Default)]
pub struct SwitchStats {
    /// Total frames received
    pub rx_frames: u64,
    /// Total frames transmitted
    pub tx_frames: u64,
    /// Frames forwarded (unicast)
    pub forwarded_unicast: u64,
    /// Frames flooded (unknown unicast)
    pub flooded_unknown: u64,
    /// Frames flooded (broadcast)
    pub flooded_broadcast: u64,
    /// Frames flooded (multicast)
    pub flooded_multicast: u64,
    /// Frames dropped (VLAN)
    pub dropped_vlan: u64,
    /// Frames dropped (STP)
    pub dropped_stp: u64,
    /// Frames dropped (port down)
    pub dropped_port_down: u64,
    /// MAC table lookups
    pub mac_lookups: u64,
    /// MAC table hits
    pub mac_hits: u64,
    /// MAC addresses learned
    pub mac_learned: u64,
}

/// Virtual switch
#[derive(Debug)]
pub struct VirtualSwitch {
    /// Switch name
    name: String,
    /// Ports
    ports: RwLock<HashMap<PortId, Port>>,
    /// Next port ID
    next_port_id: AtomicU64,
    /// MAC address table
    mac_table: RwLock<MacTable>,
    /// STP state
    stp: RwLock<StpState>,
    /// STP enabled
    stp_enabled: AtomicBool,
    /// Statistics
    stats: RwLock<SwitchStats>,
    /// Mirror source ports
    mirror_sources: RwLock<HashSet<PortId>>,
    /// Mirror destination port
    mirror_dest: RwLock<Option<PortId>>,
}

impl VirtualSwitch {
    /// Create new virtual switch
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ports: RwLock::new(HashMap::new()),
            next_port_id: AtomicU64::new(1),
            mac_table: RwLock::new(MacTable::default()),
            stp: RwLock::new(StpState::default()),
            stp_enabled: AtomicBool::new(false),
            stats: RwLock::new(SwitchStats::default()),
            mirror_sources: RwLock::new(HashSet::new()),
            mirror_dest: RwLock::new(None),
        }
    }

    /// Get switch name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Add port
    pub fn add_port(&self, name: &str, port_type: PortType) -> PortId {
        let id = self.next_port_id.fetch_add(1, Ordering::SeqCst) as PortId;
        let port = Port::new(id, name, port_type);
        self.ports.write().unwrap_or_else(|e| e.into_inner()).insert(id, port);
        id
    }

    /// Remove port
    pub fn remove_port(&self, port_id: PortId) -> Option<Port> {
        let port = self.ports.write().unwrap_or_else(|e| e.into_inner()).remove(&port_id);
        if port.is_some() {
            self.mac_table.write().unwrap_or_else(|e| e.into_inner()).flush_port(port_id);
        }
        port
    }

    /// Get port
    pub fn get_port(&self, port_id: PortId) -> Option<Port> {
        self.ports.read().unwrap_or_else(|e| e.into_inner()).get(&port_id).cloned()
    }

    /// List all ports
    pub fn list_ports(&self) -> Vec<Port> {
        self.ports.read().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
    }

    /// Set port state
    pub fn set_port_state(&self, port_id: PortId, state: PortState) -> bool {
        if let Some(port) = self.ports.write().unwrap_or_else(|e| e.into_inner()).get_mut(&port_id) {
            port.state = state;
            true
        } else {
            false
        }
    }

    /// Set port VLAN mode
    pub fn set_port_vlan(&self, port_id: PortId, mode: VlanMode) -> bool {
        if let Some(port) = self.ports.write().unwrap_or_else(|e| e.into_inner()).get_mut(&port_id) {
            port.vlan_mode = mode;
            true
        } else {
            false
        }
    }

    /// Enable/disable port
    pub fn set_port_enabled(&self, port_id: PortId, enabled: bool) -> bool {
        if let Some(port) = self.ports.write().unwrap_or_else(|e| e.into_inner()).get_mut(&port_id) {
            port.admin_enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Set port link state
    pub fn set_port_link(&self, port_id: PortId, up: bool) -> bool {
        if let Some(port) = self.ports.write().unwrap_or_else(|e| e.into_inner()).get_mut(&port_id) {
            port.link_up = up;
            true
        } else {
            false
        }
    }

    /// Process incoming frame
    pub fn process_frame(
        &self,
        ingress_port: PortId,
        frame: &EthernetFrame,
    ) -> Vec<(PortId, EthernetFrame)> {
        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.rx_frames += 1;

        let ports = self.ports.read().unwrap_or_else(|e| e.into_inner());

        // Get ingress port
        let in_port = match ports.get(&ingress_port) {
            Some(p) => p,
            None => return Vec::new(),
        };

        // Check if port can receive
        if !in_port.can_forward() {
            stats.dropped_port_down += 1;
            return Vec::new();
        }

        // Determine VLAN
        let vlan = frame.vlan_tag.unwrap_or_else(|| in_port.untagged_vlan());

        // Check VLAN is allowed
        if !in_port.allows_vlan(vlan) {
            stats.dropped_vlan += 1;
            return Vec::new();
        }

        // Update port stats
        drop(ports);
        if let Some(port) = self.ports.write().unwrap_or_else(|e| e.into_inner()).get_mut(&ingress_port) {
            port.stats.add_rx(
                frame.size() as u64,
                frame.is_multicast(),
                frame.is_broadcast(),
            );
        }

        // MAC learning
        if !frame.src_mac.is_multicast() && !frame.src_mac.is_broadcast() {
            let ports = self.ports.read().unwrap_or_else(|e| e.into_inner());
            if let Some(port) = ports.get(&ingress_port) {
                if port.can_learn() {
                    drop(ports);
                    if self
                        .mac_table
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .learn(frame.src_mac, ingress_port, vlan)
                    {
                        stats.mac_learned += 1;
                    }
                }
            }
        }

        // Lookup destination
        stats.mac_lookups += 1;
        let egress_ports = if frame.is_broadcast() || frame.is_multicast() {
            // Flood to all ports in VLAN except ingress
            if frame.is_broadcast() {
                stats.flooded_broadcast += 1;
            } else {
                stats.flooded_multicast += 1;
            }
            self.get_flood_ports(ingress_port, vlan)
        } else {
            // Unicast lookup
            let mac_table = self.mac_table.read().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = mac_table.lookup(frame.dst_mac, vlan) {
                stats.mac_hits += 1;
                stats.forwarded_unicast += 1;
                if entry.port != ingress_port {
                    vec![entry.port]
                } else {
                    Vec::new()
                }
            } else {
                // Unknown unicast - flood
                stats.flooded_unknown += 1;
                drop(mac_table);
                self.get_flood_ports(ingress_port, vlan)
            }
        };

        // Build output frames
        let mut output = Vec::new();
        let ports = self.ports.read().unwrap_or_else(|e| e.into_inner());

        for port_id in egress_ports {
            if let Some(port) = ports.get(&port_id) {
                if !port.can_forward() {
                    continue;
                }

                // Create output frame with proper VLAN tagging
                let mut out_frame = frame.clone();
                if port.should_tag(vlan) {
                    out_frame.vlan_tag = Some(vlan);
                } else {
                    out_frame.vlan_tag = None;
                }

                output.push((port_id, out_frame));
                stats.tx_frames += 1;
            }
        }

        // Handle mirroring
        drop(ports);
        if let Some(mirror_port) = self.get_mirror_frame(ingress_port, frame) {
            output.push(mirror_port);
        }

        output
    }

    /// Get flood ports for VLAN
    fn get_flood_ports(&self, ingress_port: PortId, vlan: VlanId) -> Vec<PortId> {
        let ports = self.ports.read().unwrap_or_else(|e| e.into_inner());
        ports
            .values()
            .filter(|p| {
                p.id != ingress_port
                    && p.can_forward()
                    && p.allows_vlan(vlan)
                    && p.port_type != PortType::Mirror
            })
            .map(|p| p.id)
            .collect()
    }

    /// Get mirror frame if configured
    fn get_mirror_frame(
        &self,
        ingress_port: PortId,
        frame: &EthernetFrame,
    ) -> Option<(PortId, EthernetFrame)> {
        let sources = self.mirror_sources.read().unwrap_or_else(|e| e.into_inner());
        let dest = self.mirror_dest.read().unwrap_or_else(|e| e.into_inner());

        if sources.contains(&ingress_port) {
            if let Some(mirror_port) = *dest {
                return Some((mirror_port, frame.clone()));
            }
        }
        None
    }

    /// Configure port mirroring
    pub fn set_mirror(&self, sources: Vec<PortId>, destination: PortId) {
        *self.mirror_sources.write().unwrap_or_else(|e| e.into_inner()) = sources.into_iter().collect();
        *self.mirror_dest.write().unwrap_or_else(|e| e.into_inner()) = Some(destination);
    }

    /// Disable port mirroring
    pub fn disable_mirror(&self) {
        self.mirror_sources.write().unwrap_or_else(|e| e.into_inner()).clear();
        *self.mirror_dest.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Enable STP
    pub fn enable_stp(&self) {
        self.stp_enabled.store(true, Ordering::Release);
    }

    /// Disable STP
    pub fn disable_stp(&self) {
        self.stp_enabled.store(false, Ordering::Release);
        // Set all ports to forwarding
        for port in self.ports.write().unwrap_or_else(|e| e.into_inner()).values_mut() {
            port.state = PortState::Forwarding;
        }
    }

    /// Check if STP is enabled
    pub fn is_stp_enabled(&self) -> bool {
        self.stp_enabled.load(Ordering::Acquire)
    }

    /// Get STP state
    pub fn stp_state(&self) -> StpState {
        self.stp.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Age MAC table
    pub fn age_mac_table(&self) -> usize {
        self.mac_table.write().unwrap_or_else(|e| e.into_inner()).age()
    }

    /// Flush MAC table
    pub fn flush_mac_table(&self) {
        self.mac_table.write().unwrap_or_else(|e| e.into_inner()).flush_dynamic();
    }

    /// Add static MAC entry
    pub fn add_static_mac(&self, mac: MacAddress, port: PortId, vlan: VlanId) -> bool {
        self.mac_table.write().unwrap_or_else(|e| e.into_inner()).add_static(mac, port, vlan)
    }

    /// Get MAC table entries
    pub fn mac_entries(&self) -> Vec<MacEntry> {
        self.mac_table.read().unwrap_or_else(|e| e.into_inner()).entries().cloned().collect()
    }

    /// Get statistics
    pub fn stats(&self) -> SwitchStats {
        self.stats.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        *self.stats.write().unwrap_or_else(|e| e.into_inner()) = SwitchStats::default();
        for port in self.ports.write().unwrap_or_else(|e| e.into_inner()).values_mut() {
            port.stats = PortStats::default();
        }
    }
}

impl Default for VirtualSwitch {
    fn default() -> Self {
        Self::new("vswitch0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_address_broadcast() {
        assert!(MacAddress::BROADCAST.is_broadcast());
        assert!(!MacAddress::ZERO.is_broadcast());
    }

    #[test]
    fn test_mac_address_multicast() {
        let multicast = MacAddress([0x01, 0x00, 0x5e, 0x00, 0x00, 0x01]);
        assert!(multicast.is_multicast());
        assert!(!MacAddress::ZERO.is_multicast());
    }

    #[test]
    fn test_mac_address_local() {
        let local = MacAddress::random_local();
        assert!(local.is_local());
    }

    #[test]
    fn test_mac_address_display() {
        let mac = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(mac.to_string(), "00:11:22:33:44:55");
    }

    #[test]
    fn test_vlan_id() {
        assert!(VlanId::new(1).is_some());
        assert!(VlanId::new(4094).is_some());
        assert!(VlanId::new(4095).is_none());
        assert!(VlanId::DEFAULT.is_valid());
    }

    #[test]
    fn test_vlan_set() {
        let mut set = VlanSet::empty();
        assert!(!set.contains(VlanId(10)));

        set.add(VlanId(10));
        assert!(set.contains(VlanId(10)));

        set.remove(VlanId(10));
        assert!(!set.contains(VlanId(10)));
    }

    #[test]
    fn test_vlan_set_all() {
        let set = VlanSet::all();
        assert!(set.contains(VlanId(1)));
        assert!(set.contains(VlanId(100)));
        assert!(set.contains(VlanId(4000)));
    }

    #[test]
    fn test_port_creation() {
        let port = Port::new(1, "eth0", PortType::Internal);
        assert_eq!(port.id, 1);
        assert_eq!(port.name, "eth0");
        assert!(port.can_forward());
    }

    #[test]
    fn test_port_vlan_access() {
        let mut port = Port::new(1, "eth0", PortType::Internal);
        port.vlan_mode = VlanMode::Access(VlanId(10));

        assert!(port.allows_vlan(VlanId(10)));
        assert!(!port.allows_vlan(VlanId(20)));
        assert!(!port.should_tag(VlanId(10)));
    }

    #[test]
    fn test_port_vlan_trunk() {
        let mut port = Port::new(1, "eth0", PortType::External);
        let mut allowed = VlanSet::empty();
        allowed.add(VlanId(10));
        allowed.add(VlanId(20));

        port.vlan_mode = VlanMode::Trunk {
            native_vlan: VlanId(1),
            allowed_vlans: allowed,
        };

        assert!(port.allows_vlan(VlanId(10)));
        assert!(port.allows_vlan(VlanId(20)));
        assert!(!port.should_tag(VlanId(1))); // Native VLAN
        assert!(port.should_tag(VlanId(10)));
    }

    #[test]
    fn test_mac_table_learn() {
        let mut table = MacTable::new(100, Duration::from_secs(300));
        let mac = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);

        assert!(table.learn(mac, 1, VlanId(10)));
        assert_eq!(table.len(), 1);

        let entry = table.lookup(mac, VlanId(10)).unwrap();
        assert_eq!(entry.port, 1);
    }

    #[test]
    fn test_mac_table_no_learn_broadcast() {
        let mut table = MacTable::new(100, Duration::from_secs(300));
        assert!(!table.learn(MacAddress::BROADCAST, 1, VlanId(10)));
        assert!(table.is_empty());
    }

    #[test]
    fn test_mac_table_static() {
        let mut table = MacTable::new(100, Duration::from_secs(300));
        let mac = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);

        table.add_static(mac, 1, VlanId(10));
        let entry = table.lookup(mac, VlanId(10)).unwrap();
        assert!(entry.is_static);
    }

    #[test]
    fn test_mac_table_flush_port() {
        let mut table = MacTable::new(100, Duration::from_secs(300));
        let mac1 = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let mac2 = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x66]);

        table.learn(mac1, 1, VlanId(10));
        table.learn(mac2, 2, VlanId(10));

        table.flush_port(1);
        assert!(table.lookup(mac1, VlanId(10)).is_none());
        assert!(table.lookup(mac2, VlanId(10)).is_some());
    }

    #[test]
    fn test_ethernet_frame_parse() {
        let data = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // Dst MAC (broadcast)
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // Src MAC
            0x08, 0x00, // EtherType (IPv4)
            0x45, 0x00, // Payload start
        ];

        let frame = EthernetFrame::parse(&data).unwrap();
        assert!(frame.is_broadcast());
        assert_eq!(frame.ethertype, EthernetFrame::ETHERTYPE_IPV4);
        assert!(frame.vlan_tag.is_none());
    }

    #[test]
    fn test_ethernet_frame_parse_vlan() {
        let data = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // Dst MAC
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // Src MAC
            0x81, 0x00, // VLAN EtherType
            0x00, 0x0A, // VLAN ID 10
            0x08, 0x00, // Inner EtherType
            0x45, 0x00, // Payload
        ];

        let frame = EthernetFrame::parse(&data).unwrap();
        assert_eq!(frame.vlan_tag, Some(VlanId(10)));
        assert_eq!(frame.ethertype, EthernetFrame::ETHERTYPE_IPV4);
    }

    #[test]
    fn test_ethernet_frame_serialize() {
        let frame = EthernetFrame {
            dst_mac: MacAddress::BROADCAST,
            src_mac: MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_IPV4,
            payload: vec![0x45, 0x00],
        };

        let data = frame.serialize();
        assert_eq!(data.len(), 16);
        assert_eq!(&data[0..6], &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_virtual_switch_creation() {
        let switch = VirtualSwitch::new("test-switch");
        assert_eq!(switch.name(), "test-switch");
    }

    #[test]
    fn test_virtual_switch_add_port() {
        let switch = VirtualSwitch::new("test");
        let port_id = switch.add_port("eth0", PortType::Internal);
        assert!(port_id > 0);

        let port = switch.get_port(port_id).unwrap();
        assert_eq!(port.name, "eth0");
    }

    #[test]
    fn test_virtual_switch_remove_port() {
        let switch = VirtualSwitch::new("test");
        let port_id = switch.add_port("eth0", PortType::Internal);

        let port = switch.remove_port(port_id);
        assert!(port.is_some());
        assert!(switch.get_port(port_id).is_none());
    }

    #[test]
    fn test_virtual_switch_process_broadcast() {
        let switch = VirtualSwitch::new("test");
        let port1 = switch.add_port("eth0", PortType::Internal);
        let port2 = switch.add_port("eth1", PortType::Internal);
        let port3 = switch.add_port("eth2", PortType::Internal);

        let frame = EthernetFrame {
            dst_mac: MacAddress::BROADCAST,
            src_mac: MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_ARP,
            payload: vec![],
        };

        let output = switch.process_frame(port1, &frame);
        // Should flood to port2 and port3
        assert_eq!(output.len(), 2);

        let port_ids: Vec<_> = output.iter().map(|(id, _)| *id).collect();
        assert!(port_ids.contains(&port2));
        assert!(port_ids.contains(&port3));
    }

    #[test]
    fn test_virtual_switch_mac_learning() {
        let switch = VirtualSwitch::new("test");
        let port1 = switch.add_port("eth0", PortType::Internal);
        let _port2 = switch.add_port("eth1", PortType::Internal);

        let src_mac = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let frame = EthernetFrame {
            dst_mac: MacAddress::BROADCAST,
            src_mac,
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_ARP,
            payload: vec![],
        };

        switch.process_frame(port1, &frame);

        // Check MAC was learned
        let entries = switch.mac_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].mac, src_mac);
        assert_eq!(entries[0].port, port1);
    }

    #[test]
    fn test_virtual_switch_unicast_forwarding() {
        let switch = VirtualSwitch::new("test");
        let port1 = switch.add_port("eth0", PortType::Internal);
        let port2 = switch.add_port("eth1", PortType::Internal);

        let mac1 = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let mac2 = MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x66]);

        // Learn MAC1 on port1
        switch.add_static_mac(mac1, port1, VlanId::DEFAULT);
        // Learn MAC2 on port2
        switch.add_static_mac(mac2, port2, VlanId::DEFAULT);

        // Send unicast from port1 to port2
        let frame = EthernetFrame {
            dst_mac: mac2,
            src_mac: mac1,
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_IPV4,
            payload: vec![0x45],
        };

        let output = switch.process_frame(port1, &frame);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].0, port2);
    }

    #[test]
    fn test_virtual_switch_vlan_isolation() {
        let switch = VirtualSwitch::new("test");
        let port1 = switch.add_port("eth0", PortType::Internal);
        let port2 = switch.add_port("eth1", PortType::Internal);

        switch.set_port_vlan(port1, VlanMode::Access(VlanId(10)));
        switch.set_port_vlan(port2, VlanMode::Access(VlanId(20)));

        let frame = EthernetFrame {
            dst_mac: MacAddress::BROADCAST,
            src_mac: MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_ARP,
            payload: vec![],
        };

        let output = switch.process_frame(port1, &frame);
        // Should not forward to port2 (different VLAN)
        assert!(output.is_empty());
    }

    #[test]
    fn test_virtual_switch_mirror() {
        let switch = VirtualSwitch::new("test");
        let port1 = switch.add_port("eth0", PortType::Internal);
        let port2 = switch.add_port("eth1", PortType::Internal);
        let mirror_port = switch.add_port("mirror", PortType::Mirror);

        switch.set_mirror(vec![port1], mirror_port);

        let frame = EthernetFrame {
            dst_mac: MacAddress::BROADCAST,
            src_mac: MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_ARP,
            payload: vec![],
        };

        let output = switch.process_frame(port1, &frame);
        // Should include mirror copy
        let port_ids: Vec<_> = output.iter().map(|(id, _)| *id).collect();
        assert!(port_ids.contains(&mirror_port));
    }

    #[test]
    fn test_virtual_switch_port_down() {
        let switch = VirtualSwitch::new("test");
        let port1 = switch.add_port("eth0", PortType::Internal);
        let port2 = switch.add_port("eth1", PortType::Internal);

        switch.set_port_enabled(port2, false);

        let frame = EthernetFrame {
            dst_mac: MacAddress::BROADCAST,
            src_mac: MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_ARP,
            payload: vec![],
        };

        let output = switch.process_frame(port1, &frame);
        // Should not forward to disabled port
        assert!(output.is_empty());
    }

    #[test]
    fn test_virtual_switch_stats() {
        let switch = VirtualSwitch::new("test");
        let port1 = switch.add_port("eth0", PortType::Internal);
        let _port2 = switch.add_port("eth1", PortType::Internal);

        let frame = EthernetFrame {
            dst_mac: MacAddress::BROADCAST,
            src_mac: MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            vlan_tag: None,
            ethertype: EthernetFrame::ETHERTYPE_ARP,
            payload: vec![],
        };

        switch.process_frame(port1, &frame);

        let stats = switch.stats();
        assert_eq!(stats.rx_frames, 1);
        assert_eq!(stats.flooded_broadcast, 1);
    }

    #[test]
    fn test_stp_state() {
        let switch = VirtualSwitch::new("test");
        assert!(!switch.is_stp_enabled());

        switch.enable_stp();
        assert!(switch.is_stp_enabled());

        switch.disable_stp();
        assert!(!switch.is_stp_enabled());
    }

    #[test]
    fn test_port_stats() {
        let mut stats = PortStats::default();
        stats.add_rx(100, false, false);
        stats.add_rx(200, true, false);
        stats.add_tx(150);

        assert_eq!(stats.rx_packets, 2);
        assert_eq!(stats.rx_bytes, 300);
        assert_eq!(stats.rx_multicast, 1);
        assert_eq!(stats.tx_packets, 1);
        assert_eq!(stats.tx_bytes, 150);
    }
}
