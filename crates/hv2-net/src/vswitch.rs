//! Virtual Switch with MAC Learning
//!
//! Provides a software-defined Layer 2 switch that bridges multiple virtual
//! ports (one per VM) using a MAC address learning table. The switch:
//!
//! - **Learns** source MAC addresses on ingress and associates them with ports
//! - **Forwards** known unicast traffic directly to the correct port
//! - **Floods** unknown unicast and broadcast traffic to all ports
//! - **Ages** stale MAC entries after a configurable timeout
//! - **Supports** optional VLAN tagging per port (access or trunk mode)
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
//! │   VM Port 0  │  │   VM Port 1  │  │   VM Port 2  │
//! └──────┬───────┘  └──────┬───────┘  └──────┬───────┘
//!        │                 │                 │
//!        └────────────┬────┴────────────┬────┘
//!                     │  VirtualSwitch  │
//!                     │ ┌────────────┐  │
//!                     │ │ MAC Table   │ │
//!                     │ └────────────┘  │
//!                     └─────────────────┘
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// MAC address (6 bytes)
pub type MacAddress = [u8; 6];

/// Broadcast MAC address
pub const BROADCAST_MAC: MacAddress = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

/// Port identifier (index into the switch's port table)
pub type PortId = u32;

// ============================================================================
// VLAN Support
// ============================================================================

/// VLAN identifier (1-4094)
pub type VlanId = u16;

/// Port VLAN mode
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VlanMode {
    /// Access port: member of a single VLAN (untagged traffic)
    Access(VlanId),
    /// Trunk port: carries multiple VLANs (tagged traffic)
    Trunk(Vec<VlanId>),
    /// No VLAN filtering (all traffic passes through)
    #[default]
    None,
}

// ============================================================================
// Port
// ============================================================================

/// A virtual switch port representing one connected VM
#[derive(Debug, Clone)]
pub struct SwitchPort {
    /// Unique port identifier
    pub id: PortId,
    /// Human-readable name
    pub name: String,
    /// MAC address associated with this port (optional — learned dynamically)
    pub mac: Option<MacAddress>,
    /// VLAN mode
    pub vlan_mode: VlanMode,
    /// Port is enabled
    pub enabled: bool,
    /// Packet counters
    pub stats: PortStats,
}

/// Per-port statistics
#[derive(Debug, Clone, Default)]
pub struct PortStats {
    /// Packets received on this port
    pub rx_packets: u64,
    /// Bytes received on this port
    pub rx_bytes: u64,
    /// Packets transmitted from this port
    pub tx_packets: u64,
    /// Bytes transmitted from this port
    pub tx_bytes: u64,
    /// Packets dropped (e.g., VLAN mismatch)
    pub dropped: u64,
}

impl SwitchPort {
    /// Create a new port
    pub fn new(id: PortId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            mac: None,
            vlan_mode: VlanMode::default(),
            enabled: true,
            stats: PortStats::default(),
        }
    }

    /// Set static MAC address
    pub fn with_mac(mut self, mac: MacAddress) -> Self {
        self.mac = Some(mac);
        self
    }

    /// Set VLAN mode
    pub fn with_vlan(mut self, mode: VlanMode) -> Self {
        self.vlan_mode = mode;
        self
    }

    /// Check if this port accepts a given VLAN
    pub fn accepts_vlan(&self, vlan: Option<VlanId>) -> bool {
        match (&self.vlan_mode, vlan) {
            (VlanMode::None, _) => true,
            (VlanMode::Access(_port_vlan), None) => true, // Untagged → access VLAN
            (VlanMode::Access(port_vlan), Some(v)) => *port_vlan == v,
            (VlanMode::Trunk(vlans), Some(v)) => vlans.contains(&v),
            (VlanMode::Trunk(_), None) => false, // Trunk expects tagged traffic
        }
    }
}

// ============================================================================
// MAC Table Entry
// ============================================================================

/// Entry in the MAC learning table
#[derive(Debug, Clone)]
struct MacEntry {
    /// Port where this MAC was last seen
    port_id: PortId,
    /// VLAN where this MAC was learned (if any)
    vlan: Option<VlanId>,
    /// When this entry was last seen
    last_seen: Instant,
}

// ============================================================================
// Virtual Switch
// ============================================================================

/// Configuration for the virtual switch
#[derive(Debug, Clone)]
pub struct SwitchConfig {
    /// Maximum MAC table entries
    pub max_mac_entries: usize,
    /// MAC entry aging timeout
    pub mac_aging_timeout: Duration,
    /// Switch name
    pub name: String,
    /// Enable promiscuous mode (forward all traffic to all ports)
    pub promiscuous: bool,
}

impl Default for SwitchConfig {
    fn default() -> Self {
        Self {
            max_mac_entries: 8192,
            mac_aging_timeout: Duration::from_secs(300),
            name: "vswitch0".into(),
            promiscuous: false,
        }
    }
}

impl SwitchConfig {
    /// Create a named switch config
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }
}

/// A Layer 2 virtual switch with MAC address learning
///
/// Frames ingressing on a port are learned (source MAC → port mapping),
/// then forwarded to the destination port if known, or flooded to all
/// other ports if unknown. Broadcast frames always flood.
pub struct VirtualSwitch {
    /// Switch configuration
    config: SwitchConfig,
    /// Connected ports
    ports: HashMap<PortId, SwitchPort>,
    /// MAC address learning table: MAC → entry
    mac_table: HashMap<MacAddress, MacEntry>,
    /// Next port ID
    next_port_id: PortId,
    /// Global statistics
    stats: SwitchStats,
}

/// Global switch statistics
#[derive(Debug, Clone, Default)]
pub struct SwitchStats {
    /// Total frames forwarded (unicast hit)
    pub forwarded: u64,
    /// Total frames flooded (unknown unicast or broadcast)
    pub flooded: u64,
    /// Total frames dropped (VLAN mismatch, disabled port, etc.)
    pub dropped: u64,
    /// MAC learning events
    pub mac_learned: u64,
    /// MAC aging events
    pub mac_aged: u64,
}

/// Result of forwarding a frame
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardAction {
    /// Send to a specific port
    Unicast(PortId),
    /// Flood to all ports except the source
    Flood(Vec<PortId>),
    /// Drop the frame
    Drop,
}

impl VirtualSwitch {
    /// Create a new virtual switch with the given configuration
    pub fn new(config: SwitchConfig) -> Self {
        Self {
            config,
            ports: HashMap::new(),
            mac_table: HashMap::new(),
            next_port_id: 0,
            stats: SwitchStats::default(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SwitchConfig::default())
    }

    /// Add a port to the switch, returns the assigned port ID
    pub fn add_port(&mut self, name: impl Into<String>) -> PortId {
        let id = self.next_port_id;
        self.next_port_id += 1;
        let port = SwitchPort::new(id, name);
        self.ports.insert(id, port);
        id
    }

    /// Add a port with a specific MAC and VLAN configuration
    pub fn add_port_with_config(
        &mut self,
        name: impl Into<String>,
        mac: Option<MacAddress>,
        vlan_mode: VlanMode,
    ) -> PortId {
        let id = self.next_port_id;
        self.next_port_id += 1;
        let mut port = SwitchPort::new(id, name);
        port.mac = mac;
        port.vlan_mode = vlan_mode;
        self.ports.insert(id, port);
        id
    }

    /// Remove a port and any associated MAC entries
    pub fn remove_port(&mut self, port_id: PortId) -> Option<SwitchPort> {
        // Remove all MAC entries for this port
        self.mac_table.retain(|_, entry| entry.port_id != port_id);
        self.ports.remove(&port_id)
    }

    /// Get a port by ID
    pub fn get_port(&self, port_id: PortId) -> Option<&SwitchPort> {
        self.ports.get(&port_id)
    }

    /// Get a mutable port by ID
    pub fn get_port_mut(&mut self, port_id: PortId) -> Option<&mut SwitchPort> {
        self.ports.get_mut(&port_id)
    }

    /// List all ports
    pub fn ports(&self) -> Vec<&SwitchPort> {
        self.ports.values().collect()
    }

    /// Number of connected ports
    pub fn port_count(&self) -> usize {
        self.ports.len()
    }

    /// Number of learned MAC addresses
    pub fn mac_count(&self) -> usize {
        self.mac_table.len()
    }

    /// Get switch statistics
    pub fn stats(&self) -> &SwitchStats {
        &self.stats
    }

    /// Get switch config
    pub fn config(&self) -> &SwitchConfig {
        &self.config
    }

    /// Process an ingress frame: learn source MAC and determine forwarding
    ///
    /// - `src_port` — the port the frame arrived on
    /// - `src_mac` — Ethernet source MAC address (bytes 6-11)
    /// - `dst_mac` — Ethernet destination MAC address (bytes 0-5)
    /// - `vlan` — VLAN tag from 802.1Q header (if present)
    /// - `frame_len` — total frame length in bytes
    ///
    /// Returns the forwarding action.
    pub fn process_frame(
        &mut self,
        src_port: PortId,
        src_mac: MacAddress,
        dst_mac: MacAddress,
        vlan: Option<VlanId>,
        frame_len: u64,
    ) -> ForwardAction {
        // Validate source port
        let port_enabled = self
            .ports
            .get(&src_port)
            .map(|p| p.enabled)
            .unwrap_or(false);

        if !port_enabled {
            self.stats.dropped += 1;
            return ForwardAction::Drop;
        }

        // Check VLAN on ingress port
        if let Some(port) = self.ports.get(&src_port) {
            if !port.accepts_vlan(vlan) {
                self.stats.dropped += 1;
                if let Some(port) = self.ports.get_mut(&src_port) {
                    port.stats.dropped += 1;
                }
                return ForwardAction::Drop;
            }
        }

        // Update source port stats
        if let Some(port) = self.ports.get_mut(&src_port) {
            port.stats.rx_packets += 1;
            port.stats.rx_bytes += frame_len;
        }

        // Learn source MAC
        self.learn_mac(src_mac, src_port, vlan);

        // Promiscuous mode: flood everything
        if self.config.promiscuous {
            return self.flood(src_port, frame_len);
        }

        // Broadcast or multicast → flood
        if dst_mac == BROADCAST_MAC || dst_mac[0] & 0x01 != 0 {
            return self.flood(src_port, frame_len);
        }

        // Unicast lookup
        if let Some(entry) = self.mac_table.get(&dst_mac) {
            let dst_port = entry.port_id;

            // Don't send back to the source port
            if dst_port == src_port {
                return ForwardAction::Drop;
            }

            // Check destination port VLAN
            if let Some(dst_p) = self.ports.get(&dst_port) {
                if !dst_p.enabled || !dst_p.accepts_vlan(vlan) {
                    self.stats.dropped += 1;
                    return ForwardAction::Drop;
                }
            }

            // Update destination port stats
            if let Some(port) = self.ports.get_mut(&dst_port) {
                port.stats.tx_packets += 1;
                port.stats.tx_bytes += frame_len;
            }

            self.stats.forwarded += 1;
            ForwardAction::Unicast(dst_port)
        } else {
            // Unknown unicast → flood
            self.flood(src_port, frame_len)
        }
    }

    /// Learn a MAC address on a port
    fn learn_mac(&mut self, mac: MacAddress, port_id: PortId, vlan: Option<VlanId>) {
        // Don't learn broadcast/multicast
        if mac == BROADCAST_MAC || mac[0] & 0x01 != 0 {
            return;
        }

        let now = Instant::now();

        match self.mac_table.get_mut(&mac) {
            Some(entry) => {
                // Update existing entry
                entry.port_id = port_id;
                entry.vlan = vlan;
                entry.last_seen = now;
            }
            None => {
                // New entry — check table capacity
                if self.mac_table.len() >= self.config.max_mac_entries {
                    // Evict oldest entry
                    if let Some(oldest_mac) = self
                        .mac_table
                        .iter()
                        .min_by_key(|(_, e)| e.last_seen)
                        .map(|(mac, _)| *mac)
                    {
                        self.mac_table.remove(&oldest_mac);
                        self.stats.mac_aged += 1;
                    }
                }

                self.mac_table.insert(
                    mac,
                    MacEntry {
                        port_id,
                        vlan,
                        last_seen: now,
                    },
                );
                self.stats.mac_learned += 1;
            }
        }
    }

    /// Flood a frame to all ports except the source
    fn flood(&mut self, src_port: PortId, frame_len: u64) -> ForwardAction {
        let targets: Vec<PortId> = self
            .ports
            .iter()
            .filter(|(&id, port)| id != src_port && port.enabled)
            .map(|(&id, _)| id)
            .collect();

        // Update stats for each target port
        for &port_id in &targets {
            if let Some(port) = self.ports.get_mut(&port_id) {
                port.stats.tx_packets += 1;
                port.stats.tx_bytes += frame_len;
            }
        }

        self.stats.flooded += 1;

        if targets.is_empty() {
            ForwardAction::Drop
        } else {
            ForwardAction::Flood(targets)
        }
    }

    /// Age out expired MAC entries
    pub fn age_mac_table(&mut self) -> usize {
        let timeout = self.config.mac_aging_timeout;
        let now = Instant::now();
        let before = self.mac_table.len();

        self.mac_table
            .retain(|_, entry| now.duration_since(entry.last_seen) < timeout);

        let aged = before - self.mac_table.len();
        self.stats.mac_aged += aged as u64;
        aged
    }

    /// Lookup which port a MAC address is on
    pub fn lookup_mac(&self, mac: &MacAddress) -> Option<PortId> {
        self.mac_table.get(mac).map(|e| e.port_id)
    }

    /// Clear the MAC table
    pub fn flush_mac_table(&mut self) {
        self.mac_table.clear();
    }

    /// Enable or disable a port
    pub fn set_port_enabled(&mut self, port_id: PortId, enabled: bool) -> bool {
        if let Some(port) = self.ports.get_mut(&port_id) {
            port.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Get the MAC table as a vector of (MAC, PortId, age_secs) for debugging
    pub fn mac_table_dump(&self) -> Vec<(MacAddress, PortId, f64)> {
        let now = Instant::now();
        self.mac_table
            .iter()
            .map(|(mac, entry)| {
                (
                    *mac,
                    entry.port_id,
                    now.duration_since(entry.last_seen).as_secs_f64(),
                )
            })
            .collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(b: u8) -> MacAddress {
        [0x02, 0x00, 0x00, 0x00, 0x00, b]
    }

    #[test]
    fn test_switch_creation() {
        let sw = VirtualSwitch::with_defaults();
        assert_eq!(sw.port_count(), 0);
        assert_eq!(sw.mac_count(), 0);
        assert_eq!(sw.config().name, "vswitch0");
    }

    #[test]
    fn test_add_remove_ports() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");
        let _p2 = sw.add_port("vm-2");

        assert_eq!(sw.port_count(), 3);
        assert_eq!(sw.get_port(p0).unwrap().name, "vm-0");
        assert_eq!(sw.get_port(p1).unwrap().name, "vm-1");

        sw.remove_port(p1);
        assert_eq!(sw.port_count(), 2);
        assert!(sw.get_port(p1).is_none());
    }

    #[test]
    fn test_mac_learning() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");

        // Send a frame from vm-0
        let action = sw.process_frame(p0, mac(1), mac(2), None, 64);

        // MAC(1) should be learned on port 0
        assert_eq!(sw.lookup_mac(&mac(1)), Some(p0));
        assert_eq!(sw.mac_count(), 1);
        assert_eq!(sw.stats().mac_learned, 1);

        // dst MAC(2) is unknown → flood
        match action {
            ForwardAction::Flood(ports) => {
                assert!(ports.contains(&p1));
                assert!(!ports.contains(&p0)); // Not back to source
            }
            _ => panic!("Expected flood, got {:?}", action),
        }
    }

    #[test]
    fn test_unicast_forwarding() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");

        // Learn MAC(2) on p1
        sw.process_frame(p1, mac(2), BROADCAST_MAC, None, 64);

        // Now send from p0 to MAC(2) — should unicast to p1
        let action = sw.process_frame(p0, mac(1), mac(2), None, 128);
        assert_eq!(action, ForwardAction::Unicast(p1));
        assert_eq!(sw.stats().forwarded, 1);
    }

    #[test]
    fn test_broadcast_floods() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");
        let p2 = sw.add_port("vm-2");

        let action = sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);
        match action {
            ForwardAction::Flood(ports) => {
                assert_eq!(ports.len(), 2);
                assert!(ports.contains(&p1));
                assert!(ports.contains(&p2));
            }
            _ => panic!("Expected flood"),
        }
    }

    #[test]
    fn test_disabled_port_drops() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        sw.set_port_enabled(p0, false);

        let action = sw.process_frame(p0, mac(1), mac(2), None, 64);
        assert_eq!(action, ForwardAction::Drop);
        assert_eq!(sw.stats().dropped, 1);
    }

    #[test]
    fn test_mac_aging() {
        let mut sw = VirtualSwitch::new(SwitchConfig {
            mac_aging_timeout: Duration::from_millis(1),
            ..Default::default()
        });

        let p0 = sw.add_port("vm-0");
        let _p1 = sw.add_port("vm-1");

        sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);
        assert_eq!(sw.mac_count(), 1);

        // Wait for aging
        std::thread::sleep(Duration::from_millis(5));
        let aged = sw.age_mac_table();
        assert_eq!(aged, 1);
        assert_eq!(sw.mac_count(), 0);
    }

    #[test]
    fn test_mac_table_flush() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");

        sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);
        sw.process_frame(p0, mac(2), BROADCAST_MAC, None, 64);
        assert_eq!(sw.mac_count(), 2);

        sw.flush_mac_table();
        assert_eq!(sw.mac_count(), 0);
    }

    #[test]
    fn test_vlan_access_port() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port_with_config("vm-0", None, VlanMode::Access(100));
        let p1 = sw.add_port_with_config("vm-1", None, VlanMode::Access(100));
        let _p2 = sw.add_port_with_config("vm-2", None, VlanMode::Access(200));

        // Frame on VLAN 100 from p0
        let action = sw.process_frame(p0, mac(1), BROADCAST_MAC, Some(100), 64);
        match action {
            ForwardAction::Flood(ports) => {
                // p1 accepts VLAN 100, p2 does not
                assert!(ports.contains(&p1));
            }
            _ => panic!("Expected flood"),
        }
    }

    #[test]
    fn test_vlan_mismatch_drops() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port_with_config("vm-0", None, VlanMode::Access(100));

        // Frame on VLAN 200 from p0 — drops
        let action = sw.process_frame(p0, mac(1), mac(2), Some(200), 64);
        assert_eq!(action, ForwardAction::Drop);
    }

    #[test]
    fn test_trunk_port() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port_with_config("uplink", None, VlanMode::Trunk(vec![100, 200, 300]));
        let p1 = sw.add_port_with_config("vm-0", None, VlanMode::Access(100));

        // Tagged frame on VLAN 100 from trunk → should be accepted
        let action = sw.process_frame(p0, mac(1), BROADCAST_MAC, Some(100), 64);
        match action {
            ForwardAction::Flood(ports) => {
                assert!(ports.contains(&p1));
            }
            _ => panic!("Expected flood"),
        }
    }

    #[test]
    fn test_port_stats() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");

        // Learn MAC(2) on p1
        sw.process_frame(p1, mac(2), BROADCAST_MAC, None, 64);

        // Send unicast from p0 to p1
        sw.process_frame(p0, mac(1), mac(2), None, 128);

        let port0 = sw.get_port(p0).unwrap();
        assert_eq!(port0.stats.rx_packets, 1);
        assert_eq!(port0.stats.rx_bytes, 128);

        let port1 = sw.get_port(p1).unwrap();
        assert_eq!(port1.stats.tx_packets, 1);
        assert_eq!(port1.stats.tx_bytes, 128);
    }

    #[test]
    fn test_mac_table_capacity() {
        let mut sw = VirtualSwitch::new(SwitchConfig {
            max_mac_entries: 2,
            ..Default::default()
        });
        let p0 = sw.add_port("vm-0");

        sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);
        sw.process_frame(p0, mac(2), BROADCAST_MAC, None, 64);
        assert_eq!(sw.mac_count(), 2);

        // Third entry should evict the oldest
        sw.process_frame(p0, mac(3), BROADCAST_MAC, None, 64);
        assert_eq!(sw.mac_count(), 2);
        assert!(sw.lookup_mac(&mac(3)).is_some());
    }

    #[test]
    fn test_mac_table_dump() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");

        sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);
        sw.process_frame(p0, mac(2), BROADCAST_MAC, None, 64);

        let dump = sw.mac_table_dump();
        assert_eq!(dump.len(), 2);
    }

    #[test]
    fn test_promiscuous_mode() {
        let mut sw = VirtualSwitch::new(SwitchConfig {
            promiscuous: true,
            ..Default::default()
        });
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");

        // Even unicast with known MAC → should flood in promiscuous mode
        sw.process_frame(p1, mac(2), BROADCAST_MAC, None, 64);
        let action = sw.process_frame(p0, mac(1), mac(2), None, 64);
        match action {
            ForwardAction::Flood(_) => {}
            _ => panic!("Expected flood in promiscuous mode"),
        }
    }

    #[test]
    fn test_multicast_floods() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");

        // Multicast MAC (bit 0 of first byte is 1)
        let mcast_mac: MacAddress = [0x01, 0x00, 0x5E, 0x00, 0x00, 0x01];
        let action = sw.process_frame(p0, mac(1), mcast_mac, None, 64);
        match action {
            ForwardAction::Flood(ports) => {
                assert!(ports.contains(&p1));
            }
            _ => panic!("Expected flood for multicast"),
        }
    }

    #[test]
    fn test_same_port_unicast_drops() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");

        // Learn MAC(1) on p0
        sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);

        // Send from p0 to MAC(1) — same port, should drop
        let action = sw.process_frame(p0, mac(2), mac(1), None, 64);
        assert_eq!(action, ForwardAction::Drop);
    }

    #[test]
    fn test_port_builder_methods() {
        let port = SwitchPort::new(1, "test-port")
            .with_mac([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])
            .with_vlan(VlanMode::Access(100));
        assert_eq!(port.id, 1);
        assert_eq!(port.name, "test-port");
        assert_eq!(port.mac, Some([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]));
        assert!(port.enabled);
    }

    #[test]
    fn test_port_accepts_vlan_access() {
        let port = SwitchPort::new(1, "p").with_vlan(VlanMode::Access(100));
        assert!(port.accepts_vlan(Some(100)));
        assert!(!port.accepts_vlan(Some(200)));
        assert!(port.accepts_vlan(None)); // untagged accepted on access port
    }

    #[test]
    fn test_port_accepts_vlan_trunk() {
        let port = SwitchPort::new(1, "p").with_vlan(VlanMode::Trunk(vec![100, 200, 300]));
        assert!(port.accepts_vlan(Some(100)));
        assert!(port.accepts_vlan(Some(200)));
        assert!(!port.accepts_vlan(Some(999)));
    }

    #[test]
    fn test_port_accepts_vlan_none_mode() {
        let port = SwitchPort::new(1, "p"); // VlanMode::None
        assert!(port.accepts_vlan(None));
        assert!(port.accepts_vlan(Some(42)));
    }

    #[test]
    fn test_set_port_enabled_nonexistent() {
        let mut sw = VirtualSwitch::with_defaults();
        let result = sw.set_port_enabled(999, false);
        assert!(!result); // returns false for nonexistent port
    }

    #[test]
    fn test_set_port_enabled_toggle() {
        let mut sw = VirtualSwitch::with_defaults();
        let p = sw.add_port("vm-0");

        assert!(sw.set_port_enabled(p, false));
        assert!(!sw.get_port(p).unwrap().enabled);

        assert!(sw.set_port_enabled(p, true));
        assert!(sw.get_port(p).unwrap().enabled);
    }

    #[test]
    fn test_config_accessor() {
        let config = SwitchConfig {
            max_mac_entries: 512,
            mac_aging_timeout: std::time::Duration::from_secs(60),
            name: "my-switch".into(),
            promiscuous: true,
        };
        let sw = VirtualSwitch::new(config);
        assert_eq!(sw.config().name, "my-switch");
        assert_eq!(sw.config().max_mac_entries, 512);
        assert!(sw.config().promiscuous);
    }

    #[test]
    fn test_mac_count_after_learning() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let p1 = sw.add_port("vm-1");

        assert_eq!(sw.mac_count(), 0);

        sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);
        assert_eq!(sw.mac_count(), 1);

        sw.process_frame(p1, mac(2), BROADCAST_MAC, None, 64);
        assert_eq!(sw.mac_count(), 2);

        sw.flush_mac_table();
        assert_eq!(sw.mac_count(), 0);
    }

    #[test]
    fn test_lookup_mac() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");

        assert!(sw.lookup_mac(&mac(1)).is_none());

        sw.process_frame(p0, mac(1), BROADCAST_MAC, None, 64);
        assert_eq!(sw.lookup_mac(&mac(1)), Some(p0));
    }

    #[test]
    fn test_remove_port() {
        let mut sw = VirtualSwitch::with_defaults();
        let p0 = sw.add_port("vm-0");
        let _p1 = sw.add_port("vm-1");
        assert_eq!(sw.port_count(), 2);

        let removed = sw.remove_port(p0);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "vm-0");
        assert_eq!(sw.port_count(), 1);

        // Removing again returns None
        assert!(sw.remove_port(p0).is_none());
    }

    #[test]
    fn test_switch_stats_initial() {
        let sw = VirtualSwitch::with_defaults();
        let stats = sw.stats();
        assert_eq!(stats.forwarded, 0);
        assert_eq!(stats.flooded, 0);
        assert_eq!(stats.dropped, 0);
        assert_eq!(stats.mac_learned, 0);
    }
}
