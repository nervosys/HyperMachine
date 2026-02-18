//! Network Backend Abstraction
//!
//! This module provides network backend implementations for connecting
//! virtual network devices to the host network.
//!
//! # Backends
//!
//! - **TAP/TUN**: Direct kernel interface for high-performance networking
//! - **User-mode**: NAT-based networking without root privileges
//! - **Null**: Discard all packets (for testing)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Virtual NIC (E1000/VirtIO)                   │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      NetworkBackend Trait                        │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │  send(packet) -> Result<()>                              │   │
//! │  │  recv() -> Result<Option<Vec<u8>>>                       │   │
//! │  │  mac_address() -> [u8; 6]                                │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!         ┌─────────────────────┼─────────────────────┐
//!         ▼                     ▼                     ▼
//! ┌───────────────┐    ┌───────────────┐    ┌───────────────┐
//! │  TapBackend   │    │ UserBackend   │    │ NullBackend   │
//! │  (kernel tap) │    │ (NAT/SLIRP)   │    │ (discard)     │
//! └───────────────┘    └───────────────┘    └───────────────┘
//! ```

use crate::Result;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};

/// Maximum Transmission Unit
pub const MTU: usize = 1500;

/// Maximum Ethernet frame size (MTU + headers)
pub const MAX_FRAME_SIZE: usize = MTU + 18; // 14 byte header + 4 byte FCS

/// Network backend trait
pub trait NetworkBackend: Send + Sync {
    /// Send a packet to the network
    fn send(&self, packet: &[u8]) -> Result<()>;

    /// Receive a packet from the network (non-blocking)
    fn recv(&self) -> Result<Option<Vec<u8>>>;

    /// Get the MAC address
    fn mac_address(&self) -> [u8; 6];

    /// Check if the backend is connected/ready
    fn is_connected(&self) -> bool;

    /// Get backend name
    fn name(&self) -> &'static str;

    /// Get statistics
    fn stats(&self) -> NetworkStats;
}

/// Network statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct NetworkStats {
    /// Packets transmitted
    pub tx_packets: u64,
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// Transmit errors
    pub tx_errors: u64,
    /// Receive errors
    pub rx_errors: u64,
    /// Packets dropped
    pub dropped: u64,
}

/// Null backend (discards all packets)
#[derive(Debug)]
pub struct NullBackend {
    /// MAC address
    mac: [u8; 6],
    /// Statistics
    stats: RwLock<NetworkStats>,
}

impl NullBackend {
    /// Create a new null backend
    pub fn new() -> Self {
        Self::with_mac([0x52, 0x54, 0x00, 0x00, 0x00, 0x01])
    }

    /// Create with specific MAC
    pub fn with_mac(mac: [u8; 6]) -> Self {
        Self {
            mac,
            stats: RwLock::new(NetworkStats::default()),
        }
    }
}

impl Default for NullBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkBackend for NullBackend {
    fn send(&self, packet: &[u8]) -> Result<()> {
        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.tx_packets += 1;
        stats.tx_bytes += packet.len() as u64;
        stats.dropped += 1; // Null backend drops everything
        Ok(())
    }

    fn recv(&self) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn is_connected(&self) -> bool {
        true // Always "connected"
    }

    fn name(&self) -> &'static str {
        "null"
    }

    fn stats(&self) -> NetworkStats {
        *self.stats.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// Loopback backend (echoes packets back)
#[derive(Debug)]
pub struct LoopbackBackend {
    /// MAC address
    mac: [u8; 6],
    /// Packet queue
    queue: Mutex<VecDeque<Vec<u8>>>,
    /// Statistics
    stats: RwLock<NetworkStats>,
}

impl LoopbackBackend {
    /// Create a new loopback backend
    pub fn new() -> Self {
        Self::with_mac([0x52, 0x54, 0x00, 0x00, 0x00, 0x02])
    }

    /// Create with specific MAC
    pub fn with_mac(mac: [u8; 6]) -> Self {
        Self {
            mac,
            queue: Mutex::new(VecDeque::new()),
            stats: RwLock::new(NetworkStats::default()),
        }
    }

    /// Clear the packet queue
    pub fn clear(&self) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Get queue length
    pub fn queue_len(&self) -> usize {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Default for LoopbackBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkBackend for LoopbackBackend {
    fn send(&self, packet: &[u8]) -> Result<()> {
        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.tx_packets += 1;
        stats.tx_bytes += packet.len() as u64;

        // Echo back to receive queue
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(packet.to_vec());
        Ok(())
    }

    fn recv(&self) -> Result<Option<Vec<u8>>> {
        let packet = self.queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
        if let Some(ref p) = packet {
            let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
            stats.rx_packets += 1;
            stats.rx_bytes += p.len() as u64;
        }
        Ok(packet)
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "loopback"
    }

    fn stats(&self) -> NetworkStats {
        *self.stats.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// User-mode networking backend (simplified NAT)
///
/// This provides basic NAT functionality without requiring root privileges.
/// Packets are translated between the guest's private network and the host.
#[derive(Debug)]
pub struct UserBackend {
    /// Guest MAC address
    guest_mac: [u8; 6],
    /// Gateway MAC address (virtual)
    gateway_mac: [u8; 6],
    /// Guest IP address
    guest_ip: [u8; 4],
    /// Gateway IP address
    gateway_ip: [u8; 4],
    /// DNS server IP
    dns_ip: [u8; 4],
    /// Receive queue (packets going to guest)
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
    /// Statistics
    stats: RwLock<NetworkStats>,
    /// Connected state
    connected: RwLock<bool>,
}

impl UserBackend {
    /// Create a new user-mode backend with default settings
    pub fn new() -> Self {
        Self {
            guest_mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            gateway_mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x01],
            guest_ip: [10, 0, 2, 15],
            gateway_ip: [10, 0, 2, 2],
            dns_ip: [10, 0, 2, 3],
            rx_queue: Mutex::new(VecDeque::new()),
            stats: RwLock::new(NetworkStats::default()),
            connected: RwLock::new(true),
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        guest_mac: [u8; 6],
        guest_ip: [u8; 4],
        gateway_ip: [u8; 4],
        dns_ip: [u8; 4],
    ) -> Self {
        Self {
            guest_mac,
            gateway_mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x01],
            guest_ip,
            gateway_ip,
            dns_ip,
            rx_queue: Mutex::new(VecDeque::new()),
            stats: RwLock::new(NetworkStats::default()),
            connected: RwLock::new(true),
        }
    }

    /// Get guest IP address
    pub fn guest_ip(&self) -> [u8; 4] {
        self.guest_ip
    }

    /// Get gateway IP address
    pub fn gateway_ip(&self) -> [u8; 4] {
        self.gateway_ip
    }

    /// Get DNS server IP
    pub fn dns_ip(&self) -> [u8; 4] {
        self.dns_ip
    }

    /// Queue a packet for the guest to receive
    pub fn inject_packet(&self, packet: Vec<u8>) {
        self.rx_queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(packet);
    }

    /// Set connected state
    pub fn set_connected(&self, connected: bool) {
        *self.connected.write().unwrap_or_else(|e| e.into_inner()) = connected;
    }

    /// Handle ARP request
    fn handle_arp(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() < 42 {
            return None;
        }

        // Check if it's an ARP request (opcode = 1)
        let opcode = u16::from_be_bytes([packet[20], packet[21]]);
        if opcode != 1 {
            return None;
        }

        // Get target IP
        let target_ip = [packet[38], packet[39], packet[40], packet[41]];

        // Respond if asking for gateway or DNS
        let response_mac = if target_ip == self.gateway_ip || target_ip == self.dns_ip {
            self.gateway_mac
        } else {
            return None;
        };

        // Build ARP reply
        let mut reply = vec![0u8; 42];

        // Ethernet header
        reply[0..6].copy_from_slice(&packet[6..12]); // Destination = sender
        reply[6..12].copy_from_slice(&response_mac); // Source = our MAC
        reply[12..14].copy_from_slice(&[0x08, 0x06]); // EtherType = ARP

        // ARP header
        reply[14..16].copy_from_slice(&[0x00, 0x01]); // Hardware type = Ethernet
        reply[16..18].copy_from_slice(&[0x08, 0x00]); // Protocol type = IPv4
        reply[18] = 6; // Hardware size
        reply[19] = 4; // Protocol size
        reply[20..22].copy_from_slice(&[0x00, 0x02]); // Opcode = reply

        // Sender hardware/protocol address
        reply[22..28].copy_from_slice(&response_mac);
        reply[28..32].copy_from_slice(&target_ip);

        // Target hardware/protocol address
        reply[32..38].copy_from_slice(&packet[22..28]);
        reply[38..42].copy_from_slice(&packet[28..32]);

        Some(reply)
    }

    /// Handle DHCP request (simplified)
    fn handle_dhcp(&self, _packet: &[u8]) -> Option<Vec<u8>> {
        // In a full implementation, we would:
        // 1. Parse DHCP request
        // 2. Generate DHCP offer/ack with IP assignment
        // For now, return None and let static IP be used
        None
    }
}

impl Default for UserBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkBackend for UserBackend {
    fn send(&self, packet: &[u8]) -> Result<()> {
        if packet.len() < 14 {
            return Ok(()); // Invalid frame
        }

        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.tx_packets += 1;
        stats.tx_bytes += packet.len() as u64;

        // Get EtherType
        let ethertype = u16::from_be_bytes([packet[12], packet[13]]);

        match ethertype {
            0x0806 => {
                // ARP
                if let Some(reply) = self.handle_arp(packet) {
                    self.rx_queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(reply);
                }
            }
            0x0800 => {
                // IPv4
                if packet.len() >= 34 {
                    let protocol = packet[23];
                    if protocol == 17 {
                        // UDP
                        let dst_port = u16::from_be_bytes([packet[36], packet[37]]);
                        if dst_port == 67 || dst_port == 68 {
                            // DHCP
                            if let Some(reply) = self.handle_dhcp(packet) {
                                self.rx_queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(reply);
                            }
                        }
                    }
                }
                // Other IPv4 packets would be NAT'd in a full implementation
            }
            _ => {
                // Unknown protocol, drop
                stats.dropped += 1;
            }
        }

        Ok(())
    }

    fn recv(&self) -> Result<Option<Vec<u8>>> {
        let packet = self.rx_queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
        if let Some(ref p) = packet {
            let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
            stats.rx_packets += 1;
            stats.rx_bytes += p.len() as u64;
        }
        Ok(packet)
    }

    fn mac_address(&self) -> [u8; 6] {
        self.guest_mac
    }

    fn is_connected(&self) -> bool {
        *self.connected.read().unwrap_or_else(|e| e.into_inner())
    }

    fn name(&self) -> &'static str {
        "user"
    }

    fn stats(&self) -> NetworkStats {
        *self.stats.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// TAP backend configuration
#[derive(Debug, Clone)]
pub struct TapConfig {
    /// TAP device name
    pub name: String,
    /// MAC address
    pub mac: [u8; 6],
    /// MTU
    pub mtu: usize,
}

impl Default for TapConfig {
    fn default() -> Self {
        Self {
            name: "tap0".to_string(),
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            mtu: MTU,
        }
    }
}

/// TAP backend placeholder
///
/// The actual TAP implementation would use platform-specific APIs:
/// - Linux: /dev/net/tun with IFF_TAP
/// - Windows: OpenVPN TAP driver or Wintun
/// - macOS: utun devices
#[derive(Debug)]
pub struct TapBackend {
    /// Configuration
    config: TapConfig,
    /// Statistics
    stats: RwLock<NetworkStats>,
    /// Simulated packet queue (for testing)
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
    /// Connected state
    connected: RwLock<bool>,
}

impl TapBackend {
    /// Create a new TAP backend (placeholder)
    pub fn new(config: TapConfig) -> Result<Self> {
        Ok(Self {
            config,
            stats: RwLock::new(NetworkStats::default()),
            rx_queue: Mutex::new(VecDeque::new()),
            connected: RwLock::new(false), // Not actually connected
        })
    }

    /// Open TAP device (placeholder - would need platform implementation)
    pub fn open(&self) -> Result<()> {
        // In a real implementation:
        // Linux: open("/dev/net/tun"), ioctl(TUNSETIFF)
        // Windows: Open TAP-Windows adapter
        *self.connected.write().unwrap_or_else(|e| e.into_inner()) = true;
        Ok(())
    }

    /// Close TAP device
    pub fn close(&self) {
        *self.connected.write().unwrap_or_else(|e| e.into_inner()) = false;
    }

    /// Get device name
    pub fn device_name(&self) -> &str {
        &self.config.name
    }

    /// Inject a packet (for testing)
    pub fn inject_packet(&self, packet: Vec<u8>) {
        self.rx_queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(packet);
    }
}

impl NetworkBackend for TapBackend {
    fn send(&self, packet: &[u8]) -> Result<()> {
        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.tx_packets += 1;
        stats.tx_bytes += packet.len() as u64;

        if !*self.connected.read().unwrap_or_else(|e| e.into_inner()) {
            stats.tx_errors += 1;
        }

        // In a real implementation, write to TAP fd
        Ok(())
    }

    fn recv(&self) -> Result<Option<Vec<u8>>> {
        // In a real implementation, read from TAP fd (non-blocking)
        let packet = self.rx_queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
        if let Some(ref p) = packet {
            let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
            stats.rx_packets += 1;
            stats.rx_bytes += p.len() as u64;
        }
        Ok(packet)
    }

    fn mac_address(&self) -> [u8; 6] {
        self.config.mac
    }

    fn is_connected(&self) -> bool {
        *self.connected.read().unwrap_or_else(|e| e.into_inner())
    }

    fn name(&self) -> &'static str {
        "tap"
    }

    fn stats(&self) -> NetworkStats {
        *self.stats.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// Shared network backend
pub type SharedNetworkBackend = Arc<dyn NetworkBackend>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_backend() {
        let backend = NullBackend::new();

        assert!(backend.is_connected());
        assert_eq!(backend.name(), "null");

        // Send a packet
        backend.send(&[0u8; 64]).unwrap();

        // Receive returns None
        assert!(backend.recv().unwrap().is_none());

        let stats = backend.stats();
        assert_eq!(stats.tx_packets, 1);
        assert_eq!(stats.dropped, 1);
    }

    #[test]
    fn test_loopback_backend() {
        let backend = LoopbackBackend::new();

        assert!(backend.is_connected());
        assert_eq!(backend.name(), "loopback");

        // Send a packet
        let packet = vec![0x42u8; 64];
        backend.send(&packet).unwrap();

        // Should be able to receive it back
        let received = backend.recv().unwrap();
        assert_eq!(received, Some(packet));

        // Queue should be empty now
        assert!(backend.recv().unwrap().is_none());

        let stats = backend.stats();
        assert_eq!(stats.tx_packets, 1);
        assert_eq!(stats.rx_packets, 1);
    }

    #[test]
    fn test_loopback_queue() {
        let backend = LoopbackBackend::new();

        backend.send(&[1u8; 10]).unwrap();
        backend.send(&[2u8; 20]).unwrap();
        backend.send(&[3u8; 30]).unwrap();

        assert_eq!(backend.queue_len(), 3);

        backend.clear();
        assert_eq!(backend.queue_len(), 0);
    }

    #[test]
    fn test_user_backend_creation() {
        let backend = UserBackend::new();

        assert!(backend.is_connected());
        assert_eq!(backend.name(), "user");
        assert_eq!(backend.guest_ip(), [10, 0, 2, 15]);
        assert_eq!(backend.gateway_ip(), [10, 0, 2, 2]);
        assert_eq!(backend.dns_ip(), [10, 0, 2, 3]);
    }

    #[test]
    fn test_user_backend_arp() {
        let backend = UserBackend::new();

        // Create ARP request for gateway
        let mut arp_request = vec![0u8; 42];
        // Ethernet header
        arp_request[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // Broadcast
        arp_request[6..12].copy_from_slice(&backend.guest_mac); // Source
        arp_request[12..14].copy_from_slice(&[0x08, 0x06]); // ARP
                                                            // ARP payload
        arp_request[14..16].copy_from_slice(&[0x00, 0x01]); // Ethernet
        arp_request[16..18].copy_from_slice(&[0x08, 0x00]); // IPv4
        arp_request[18] = 6; // HW size
        arp_request[19] = 4; // Proto size
        arp_request[20..22].copy_from_slice(&[0x00, 0x01]); // Request
        arp_request[22..28].copy_from_slice(&backend.guest_mac);
        arp_request[28..32].copy_from_slice(&backend.guest_ip());
        arp_request[38..42].copy_from_slice(&backend.gateway_ip());

        backend.send(&arp_request).unwrap();

        // Should have ARP reply queued
        let reply = backend.recv().unwrap();
        assert!(reply.is_some());

        let reply = reply.unwrap();
        // Check it's an ARP reply
        assert_eq!(&reply[12..14], &[0x08, 0x06]); // ARP
        assert_eq!(&reply[20..22], &[0x00, 0x02]); // Reply opcode
    }

    #[test]
    fn test_user_backend_inject() {
        let backend = UserBackend::new();

        let packet = vec![0xAB; 100];
        backend.inject_packet(packet.clone());

        let received = backend.recv().unwrap();
        assert_eq!(received, Some(packet));
    }

    #[test]
    fn test_user_backend_disconnect() {
        let backend = UserBackend::new();

        assert!(backend.is_connected());
        backend.set_connected(false);
        assert!(!backend.is_connected());
    }

    #[test]
    fn test_tap_backend() {
        let config = TapConfig::default();
        let backend = TapBackend::new(config).unwrap();

        assert!(!backend.is_connected());
        assert_eq!(backend.name(), "tap");
        assert_eq!(backend.device_name(), "tap0");

        backend.open().unwrap();
        assert!(backend.is_connected());

        backend.close();
        assert!(!backend.is_connected());
    }

    #[test]
    fn test_tap_inject() {
        let config = TapConfig::default();
        let backend = TapBackend::new(config).unwrap();
        backend.open().unwrap();

        let packet = vec![0x42u8; 64];
        backend.inject_packet(packet.clone());

        let received = backend.recv().unwrap();
        assert_eq!(received, Some(packet));
    }

    #[test]
    fn test_network_stats() {
        let backend = LoopbackBackend::new();

        for i in 0..5 {
            backend.send(&vec![0u8; 100 + i]).unwrap();
        }

        for _ in 0..3 {
            backend.recv().unwrap();
        }

        let stats = backend.stats();
        assert_eq!(stats.tx_packets, 5);
        assert_eq!(stats.rx_packets, 3);
    }

    #[test]
    fn test_custom_mac() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

        let null = NullBackend::with_mac(mac);
        assert_eq!(null.mac_address(), mac);

        let loopback = LoopbackBackend::with_mac(mac);
        assert_eq!(loopback.mac_address(), mac);
    }

    #[test]
    fn test_shared_backend() {
        let backend: SharedNetworkBackend = Arc::new(LoopbackBackend::new());
        let backend_clone = Arc::clone(&backend);

        backend.send(&[1, 2, 3]).unwrap();

        let packet = backend_clone.recv().unwrap();
        assert_eq!(packet, Some(vec![1, 2, 3]));
    }
}
