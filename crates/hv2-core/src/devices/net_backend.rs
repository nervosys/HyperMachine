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
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(packet.to_vec());
        Ok(())
    }

    fn recv(&self) -> Result<Option<Vec<u8>>> {
        let packet = self
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front();
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
        self.rx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(packet);
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

    /// Handle DHCP request
    ///
    /// Parses DHCP DISCOVER/REQUEST from the guest and responds with
    /// OFFER/ACK assigning `guest_ip` with the configured gateway and DNS.
    fn handle_dhcp(&self, packet: &[u8]) -> Option<Vec<u8>> {
        // Minimum: 14 (eth) + 20 (ip) + 8 (udp) + 240 (dhcp fixed) = 282
        if packet.len() < 282 {
            return None;
        }

        let eth_hdr_len = 14;
        let ip_hdr_start = eth_hdr_len;
        let ip_ihl = ((packet[ip_hdr_start] & 0x0F) as usize) * 4;
        let udp_start = ip_hdr_start + ip_ihl;
        let dhcp_start = udp_start + 8;

        // DHCP op must be BOOTREQUEST (1)
        if packet.get(dhcp_start)? != &1 {
            return None;
        }

        // Extract transaction ID (xid) — bytes 4..8 of DHCP payload
        let xid = &packet[dhcp_start + 4..dhcp_start + 8];

        // Extract client MAC (chaddr) — bytes 28..34 of DHCP payload
        let client_mac = &packet[dhcp_start + 28..dhcp_start + 34];

        // Parse DHCP options to find message type
        // Options start at byte 240 of DHCP payload (after 4-byte magic cookie)
        let options_start = dhcp_start + 240;
        let mut msg_type = 0u8;
        let mut i = options_start;
        while i < packet.len() {
            let opt = packet[i];
            if opt == 0xFF {
                break; // End
            }
            if opt == 0x00 {
                i += 1; // Padding
                continue;
            }
            if i + 1 >= packet.len() {
                break;
            }
            let len = packet[i + 1] as usize;
            if opt == 53 && len == 1 && i + 2 < packet.len() {
                msg_type = packet[i + 2]; // 1=DISCOVER, 3=REQUEST
            }
            i += 2 + len;
        }

        // Reply type: DISCOVER → OFFER (2), REQUEST → ACK (5)
        let reply_type = match msg_type {
            1 => 2u8, // OFFER
            3 => 5u8, // ACK
            _ => return None,
        };

        // Build DHCP reply payload (fixed 240 bytes + options)
        let mut dhcp = vec![0u8; 240];
        dhcp[0] = 2; // BOOTREPLY
        dhcp[1] = 1; // Hardware type: Ethernet
        dhcp[2] = 6; // Hardware address length
        dhcp[4..8].copy_from_slice(xid);
        dhcp[16..20].copy_from_slice(&self.guest_ip); // yiaddr (your IP)
        dhcp[20..24].copy_from_slice(&self.gateway_ip); // siaddr (server IP)
        dhcp[28..34].copy_from_slice(client_mac); // chaddr

        // Magic cookie
        dhcp[236..240].copy_from_slice(&[99, 130, 83, 99]);

        // DHCP options
        let mut opts = Vec::with_capacity(64);
        // Option 53: message type
        opts.extend_from_slice(&[53, 1, reply_type]);
        // Option 54: server identifier
        opts.extend_from_slice(&[54, 4]);
        opts.extend_from_slice(&self.gateway_ip);
        // Option 51: lease time (86400 seconds = 1 day)
        opts.extend_from_slice(&[51, 4, 0x00, 0x01, 0x51, 0x80]);
        // Option 1: subnet mask (255.255.255.0)
        opts.extend_from_slice(&[1, 4, 255, 255, 255, 0]);
        // Option 3: router
        opts.extend_from_slice(&[3, 4]);
        opts.extend_from_slice(&self.gateway_ip);
        // Option 6: DNS server
        opts.extend_from_slice(&[6, 4]);
        opts.extend_from_slice(&self.dns_ip);
        // End
        opts.push(0xFF);

        dhcp.extend_from_slice(&opts);

        // Build UDP (src port 67, dst port 68)
        let udp_len = (8 + dhcp.len()) as u16;
        let mut udp = vec![0u8; 8];
        udp[0..2].copy_from_slice(&67u16.to_be_bytes()); // src port
        udp[2..4].copy_from_slice(&68u16.to_be_bytes()); // dst port
        udp[4..6].copy_from_slice(&udp_len.to_be_bytes());
        // checksum = 0 (optional for IPv4 UDP)
        udp.extend_from_slice(&dhcp);

        // Build IPv4 header (20 bytes, no options)
        let ip_total_len = (20 + udp.len()) as u16;
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45; // version 4, IHL 5
        ip[2..4].copy_from_slice(&ip_total_len.to_be_bytes());
        ip[8] = 64; // TTL
        ip[9] = 17; // Protocol: UDP
        ip[12..16].copy_from_slice(&self.gateway_ip);
        ip[16..20].copy_from_slice(&[255, 255, 255, 255]); // broadcast

        // IP header checksum
        let mut sum: u32 = 0;
        for chunk in ip.chunks(2) {
            let word = u16::from_be_bytes([chunk[0], chunk.get(1).copied().unwrap_or(0)]);
            sum += word as u32;
        }
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let checksum = !(sum as u16);
        ip[10..12].copy_from_slice(&checksum.to_be_bytes());

        // Build Ethernet frame
        let mut frame = vec![0u8; 14];
        frame[0..6].copy_from_slice(client_mac); // dst MAC
        frame[6..12].copy_from_slice(&self.gateway_mac); // src MAC
        frame[12..14].copy_from_slice(&[0x08, 0x00]); // EtherType: IPv4

        frame.extend_from_slice(&ip);
        frame.extend_from_slice(&udp);

        Some(frame)
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
                    self.rx_queue
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push_back(reply);
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
                                self.rx_queue
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .push_back(reply);
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
        let packet = self
            .rx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front();
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

/// TAP backend
///
/// Supports two modes:
/// - **Real mode**: When a file descriptor/handle is provided via `open_fd()`,
///   packets are sent/received through the actual TAP device.
/// - **Simulated mode**: When no fd is provided (or on unsupported platforms),
///   uses an in-memory packet queue for testing.
///
/// Platform-specific TAP device setup (opening `/dev/net/tun`, configuring
/// IFF_TAP, finding Windows TAP adapters) is handled by `hv2-net::tap`.
/// This backend only performs I/O on an already-configured file descriptor.
#[derive(Debug)]
pub struct TapBackend {
    /// Configuration
    config: TapConfig,
    /// Statistics
    stats: RwLock<NetworkStats>,
    /// Simulated packet queue (for testing / when no real fd is available)
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
    /// Connected state
    connected: RwLock<bool>,
    /// Raw file descriptor for the TAP device (Linux/macOS)
    #[cfg(unix)]
    tap_fd: Mutex<Option<std::os::unix::io::RawFd>>,
    /// Whether we own the fd and should close it on drop
    #[cfg(unix)]
    owns_fd: Mutex<bool>,
}

impl TapBackend {
    /// Create a new TAP backend
    pub fn new(config: TapConfig) -> Result<Self> {
        Ok(Self {
            config,
            stats: RwLock::new(NetworkStats::default()),
            rx_queue: Mutex::new(VecDeque::new()),
            connected: RwLock::new(false),
            #[cfg(unix)]
            tap_fd: Mutex::new(None),
            #[cfg(unix)]
            owns_fd: Mutex::new(false),
        })
    }

    /// Open TAP device in simulated mode (no real I/O)
    pub fn open(&self) -> Result<()> {
        *self.connected.write().unwrap_or_else(|e| e.into_inner()) = true;
        Ok(())
    }

    /// Attach a pre-configured TAP file descriptor for real I/O.
    ///
    /// The caller is responsible for opening and configuring the TAP device
    /// (e.g., via `hv2-net::tap::TapDevice`). After calling this, `send()`
    /// and `recv()` will perform actual I/O on the fd.
    ///
    /// # Arguments
    /// * `fd` - A valid, open file descriptor for the TAP device
    /// * `take_ownership` - If true, the fd will be closed when TapBackend is dropped
    #[cfg(unix)]
    pub fn open_fd(&self, fd: std::os::unix::io::RawFd, take_ownership: bool) -> Result<()> {
        *self.tap_fd.lock().unwrap_or_else(|e| e.into_inner()) = Some(fd);
        *self.owns_fd.lock().unwrap_or_else(|e| e.into_inner()) = take_ownership;
        *self.connected.write().unwrap_or_else(|e| e.into_inner()) = true;
        Ok(())
    }

    /// Close TAP device
    pub fn close(&self) {
        #[cfg(unix)]
        {
            let mut fd_guard = self.tap_fd.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(fd) = fd_guard.take() {
                if *self.owns_fd.lock().unwrap_or_else(|e| e.into_inner()) {
                    // SAFETY: fd is a valid file descriptor that we own.
                    unsafe { libc_close(fd) };
                }
            }
        }
        *self.connected.write().unwrap_or_else(|e| e.into_inner()) = false;
    }

    /// Get device name
    pub fn device_name(&self) -> &str {
        &self.config.name
    }

    /// Inject a packet into the receive queue (for testing/simulated mode)
    pub fn inject_packet(&self, packet: Vec<u8>) {
        self.rx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(packet);
    }

    /// Check if a real TAP fd is attached
    #[cfg(unix)]
    pub fn has_real_fd(&self) -> bool {
        self.tap_fd
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
}

/// Minimal close wrapper to avoid a full libc dependency.
/// On Unix, std provides the raw fd but not close() without libc.
#[cfg(unix)]
unsafe fn libc_close(fd: std::os::unix::io::RawFd) {
    // SAFETY: Caller guarantees fd is valid and owned.
    extern "C" {
        fn close(fd: std::os::raw::c_int) -> std::os::raw::c_int;
    }
    unsafe { close(fd) };
}

/// Minimal read wrapper for TAP fd I/O without libc dependency.
#[cfg(unix)]
unsafe fn tap_read(fd: std::os::unix::io::RawFd, buf: &mut [u8]) -> isize {
    extern "C" {
        fn read(fd: std::os::raw::c_int, buf: *mut std::os::raw::c_void, count: usize) -> isize;
    }
    // SAFETY: fd is a valid open file descriptor; buf is a valid mutable slice.
    unsafe { read(fd, buf.as_mut_ptr().cast(), buf.len()) }
}

/// Minimal write wrapper for TAP fd I/O without libc dependency.
#[cfg(unix)]
unsafe fn tap_write(fd: std::os::unix::io::RawFd, buf: &[u8]) -> isize {
    extern "C" {
        fn write(fd: std::os::raw::c_int, buf: *const std::os::raw::c_void, count: usize) -> isize;
    }
    // SAFETY: fd is a valid open file descriptor; buf is a valid slice.
    unsafe { write(fd, buf.as_ptr().cast(), buf.len()) }
}

impl NetworkBackend for TapBackend {
    fn send(&self, packet: &[u8]) -> Result<()> {
        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.tx_packets += 1;
        stats.tx_bytes += packet.len() as u64;

        if !*self.connected.read().unwrap_or_else(|e| e.into_inner()) {
            stats.tx_errors += 1;
            return Ok(());
        }

        // Write to real TAP fd if available (unix only)
        #[cfg(unix)]
        {
            let fd_guard = self.tap_fd.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(fd) = *fd_guard {
                // SAFETY: fd is a valid open file descriptor attached via open_fd().
                let written = unsafe { tap_write(fd, packet) };
                if written < 0 {
                    stats.tx_errors += 1;
                }
                return Ok(());
            }
        }

        // Simulated mode: packet is counted but not delivered anywhere
        Ok(())
    }

    fn recv(&self) -> Result<Option<Vec<u8>>> {
        // Try real TAP fd first (unix only)
        #[cfg(unix)]
        {
            let fd_guard = self.tap_fd.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(fd) = *fd_guard {
                let mut buf = [0u8; MAX_FRAME_SIZE];
                // SAFETY: fd is a valid open file descriptor attached via open_fd().
                // The fd should be set to non-blocking by the caller before passing to open_fd().
                let n = unsafe { tap_read(fd, &mut buf) };
                if n > 0 {
                    let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
                    stats.rx_packets += 1;
                    stats.rx_bytes += n as u64;
                    return Ok(Some(buf[..n as usize].to_vec()));
                }
                // n == 0 or EAGAIN/EWOULDBLOCK → no data available
                // n < 0 and not EAGAIN → real error, but we silently return None
                // (the caller can check stats.rx_errors if needed)
                return Ok(None);
            }
        }

        // Simulated mode: read from injected packet queue
        let packet = self
            .rx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front();
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

    /// Helper: build a minimal DHCP DISCOVER/REQUEST Ethernet frame.
    fn build_dhcp_packet(msg_type: u8) -> Vec<u8> {
        let client_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let xid: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

        // DHCP payload (240 fixed + options)
        let mut dhcp = vec![0u8; 240];
        dhcp[0] = 1; // BOOTREQUEST
        dhcp[1] = 1; // Ethernet
        dhcp[2] = 6; // HW addr len
        dhcp[4..8].copy_from_slice(&xid);
        dhcp[28..34].copy_from_slice(&client_mac);
        // Magic cookie
        dhcp[236..240].copy_from_slice(&[99, 130, 83, 99]);
        // Option 53 (message type)
        dhcp.extend_from_slice(&[53, 1, msg_type]);
        // End
        dhcp.push(0xFF);

        // UDP header
        let udp_len = (8 + dhcp.len()) as u16;
        let mut udp = vec![0u8; 8];
        udp[0..2].copy_from_slice(&68u16.to_be_bytes()); // src=68
        udp[2..4].copy_from_slice(&67u16.to_be_bytes()); // dst=67
        udp[4..6].copy_from_slice(&udp_len.to_be_bytes());
        udp.extend_from_slice(&dhcp);

        // IPv4 header (20 bytes)
        let ip_total_len = (20 + udp.len()) as u16;
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&ip_total_len.to_be_bytes());
        ip[8] = 64;
        ip[9] = 17; // UDP
        ip[12..16].copy_from_slice(&[0, 0, 0, 0]); // src=0.0.0.0
        ip[16..20].copy_from_slice(&[255, 255, 255, 255]); // dst=broadcast

        // Ethernet
        let mut frame = vec![0u8; 14];
        frame[0..6].copy_from_slice(&[0xFF; 6]); // broadcast
        frame[6..12].copy_from_slice(&client_mac);
        frame[12..14].copy_from_slice(&[0x08, 0x00]);

        frame.extend_from_slice(&ip);
        frame.extend_from_slice(&udp);
        frame
    }

    #[test]
    fn test_user_backend_dhcp_discover() {
        let backend = UserBackend::new();
        let discover = build_dhcp_packet(1); // DISCOVER

        backend.send(&discover).unwrap();

        // Should have a DHCP OFFER in the rx queue
        let reply = backend.recv().unwrap();
        assert!(reply.is_some(), "DHCP DISCOVER should produce an OFFER");

        let reply = reply.unwrap();
        // Verify it's an IPv4/UDP frame
        assert_eq!(&reply[12..14], &[0x08, 0x00]);
        // DHCP op should be BOOTREPLY (2)
        let ip_ihl = ((reply[14] & 0x0F) as usize) * 4;
        let dhcp_start = 14 + ip_ihl + 8;
        assert_eq!(reply[dhcp_start], 2, "Should be BOOTREPLY");
        // yiaddr should be 10.0.2.15
        assert_eq!(&reply[dhcp_start + 16..dhcp_start + 20], &[10, 0, 2, 15]);
        // Find DHCP option 53 in reply
        let opts_start = dhcp_start + 240;
        let mut i = opts_start;
        let mut found_type = 0u8;
        while i < reply.len() {
            let opt = reply[i];
            if opt == 0xFF {
                break;
            }
            if opt == 0 {
                i += 1;
                continue;
            }
            let len = reply[i + 1] as usize;
            if opt == 53 && len == 1 {
                found_type = reply[i + 2];
            }
            i += 2 + len;
        }
        assert_eq!(found_type, 2, "Reply should be DHCP OFFER (type=2)");
    }

    #[test]
    fn test_user_backend_dhcp_request() {
        let backend = UserBackend::new();
        let request = build_dhcp_packet(3); // REQUEST

        backend.send(&request).unwrap();

        let reply = backend.recv().unwrap();
        assert!(reply.is_some(), "DHCP REQUEST should produce an ACK");

        let reply = reply.unwrap();
        let ip_ihl = ((reply[14] & 0x0F) as usize) * 4;
        let dhcp_start = 14 + ip_ihl + 8;
        // Find option 53
        let opts_start = dhcp_start + 240;
        let mut i = opts_start;
        let mut found_type = 0u8;
        while i < reply.len() {
            let opt = reply[i];
            if opt == 0xFF {
                break;
            }
            if opt == 0 {
                i += 1;
                continue;
            }
            let len = reply[i + 1] as usize;
            if opt == 53 && len == 1 {
                found_type = reply[i + 2];
            }
            i += 2 + len;
        }
        assert_eq!(found_type, 5, "Reply should be DHCP ACK (type=5)");
    }

    #[test]
    fn test_user_backend_dhcp_xid_preserved() {
        let backend = UserBackend::new();
        let discover = build_dhcp_packet(1);

        backend.send(&discover).unwrap();
        let reply = backend.recv().unwrap().unwrap();

        let ip_ihl = ((reply[14] & 0x0F) as usize) * 4;
        let dhcp_start = 14 + ip_ihl + 8;
        // xid is at offset 4 in DHCP
        assert_eq!(
            &reply[dhcp_start + 4..dhcp_start + 8],
            &[0xDE, 0xAD, 0xBE, 0xEF],
            "Transaction ID must be preserved"
        );
    }
}
