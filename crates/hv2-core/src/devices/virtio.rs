//! Virtio Device Framework and Network Device
//!
//! This module implements the virtio specification for paravirtualized devices,
//! specifically focusing on virtio-net for network emulation.
//!
//! Virtio provides a standardized interface between guest and hypervisor
//! for efficient I/O with minimal VM exits.
//!
//! Key Components:
//! - VirtQueue: Descriptor ring buffer for data transfer
//! - VirtioDevice: Base trait for all virtio devices
//! - VirtioNet: Network interface implementation
//!
//! References:
//! - Virtio Spec: https://docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.html

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Virtio vendor ID
pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

/// Virtio device IDs
pub const VIRTIO_ID_NET: u16 = 0x1000; // Network device (legacy)
pub const VIRTIO_ID_BLOCK: u16 = 0x1001; // Block device (legacy)
pub const VIRTIO_ID_NET_MODERN: u16 = 0x1041;

/// Virtio device status bits
pub mod status {
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
    pub const DEVICE_NEEDS_RESET: u8 = 64;
    pub const FAILED: u8 = 128;
}

/// Virtio-net feature bits
pub const VIRTIO_NET_F_CSUM: u64 = 1 << 0; // Host handles checksum
pub const VIRTIO_NET_F_GUEST_CSUM: u64 = 1 << 1; // Guest handles checksum
pub const VIRTIO_NET_F_MAC: u64 = 1 << 5; // Device has MAC address
pub const VIRTIO_NET_F_STATUS: u64 = 1 << 16; // Device has link status
pub const VIRTIO_NET_F_MRG_RXBUF: u64 = 1 << 15; // Merge receive buffers
pub const VIRTIO_NET_F_CTRL_VQ: u64 = 1 << 17; // Control channel available
pub const VIRTIO_NET_F_CTRL_RX: u64 = 1 << 18; // Control RX mode
pub const VIRTIO_NET_F_CTRL_VLAN: u64 = 1 << 19; // Control VLAN filtering
pub const VIRTIO_NET_F_CTRL_RX_EXTRA: u64 = 1 << 20; // Extra RX mode bits

/// Virtio-net status bits
pub const VIRTIO_NET_S_LINK_UP: u16 = 1;
pub const VIRTIO_NET_S_ANNOUNCE: u16 = 2;

/// Virtqueue descriptor flags
pub mod desc_flags {
    pub const NEXT: u16 = 1; // Buffer continues via next field
    pub const WRITE: u16 = 2; // Buffer is write-only (for device)
    pub const INDIRECT: u16 = 4; // Buffer contains list of descriptors
}

/// Maximum virtqueue size
pub const MAX_QUEUE_SIZE: u16 = 256;

/// Virtqueue descriptor entry
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqDesc {
    /// Guest physical address of buffer
    pub addr: u64,
    /// Length of buffer
    pub len: u32,
    /// Descriptor flags
    pub flags: u16,
    /// Next descriptor index (if NEXT flag set)
    pub next: u16,
}

impl VirtqDesc {
    /// Check if this descriptor continues to another
    pub fn has_next(&self) -> bool {
        self.flags & desc_flags::NEXT != 0
    }

    /// Check if this is a write-only descriptor (device writes, guest reads)
    pub fn is_write_only(&self) -> bool {
        self.flags & desc_flags::WRITE != 0
    }

    /// Check if this is an indirect descriptor
    pub fn is_indirect(&self) -> bool {
        self.flags & desc_flags::INDIRECT != 0
    }
}

/// Virtqueue available ring
#[repr(C)]
#[derive(Debug)]
pub struct VirtqAvail {
    /// Flags (used for interrupt suppression)
    pub flags: u16,
    /// Index of next available entry
    pub idx: u16,
    /// Ring of available descriptor indices
    pub ring: Vec<u16>,
    /// Used event (for interrupt suppression)
    pub used_event: u16,
}

impl VirtqAvail {
    fn new(size: u16) -> Self {
        Self {
            flags: 0,
            idx: 0,
            ring: vec![0; size as usize],
            used_event: 0,
        }
    }
}

/// Virtqueue used ring element
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqUsedElem {
    /// Index of descriptor chain head
    pub id: u32,
    /// Total length written to descriptor chain
    pub len: u32,
}

/// Virtqueue used ring
#[repr(C)]
#[derive(Debug)]
pub struct VirtqUsed {
    /// Flags (for interrupt suppression)
    pub flags: u16,
    /// Index of next used entry
    pub idx: u16,
    /// Ring of used elements
    pub ring: Vec<VirtqUsedElem>,
    /// Available event (for interrupt suppression)
    pub avail_event: u16,
}

impl VirtqUsed {
    fn new(size: u16) -> Self {
        Self {
            flags: 0,
            idx: 0,
            ring: vec![VirtqUsedElem::default(); size as usize],
            avail_event: 0,
        }
    }
}

/// Virtqueue implementation
#[derive(Debug)]
pub struct VirtQueue {
    /// Queue size (number of descriptors)
    size: u16,
    /// Descriptor table
    desc: Vec<VirtqDesc>,
    /// Available ring
    avail: VirtqAvail,
    /// Used ring
    used: VirtqUsed,
    /// Last processed available index
    last_avail_idx: u16,
    /// Queue enabled
    enabled: bool,
    /// Queue ready
    ready: bool,
    /// Guest physical address of descriptor table
    desc_addr: u64,
    /// Guest physical address of available ring
    avail_addr: u64,
    /// Guest physical address of used ring
    used_addr: u64,
}

impl VirtQueue {
    /// Create a new virtqueue with the given size
    pub fn new(size: u16) -> Self {
        let size = size.min(MAX_QUEUE_SIZE);
        Self {
            size,
            desc: vec![VirtqDesc::default(); size as usize],
            avail: VirtqAvail::new(size),
            used: VirtqUsed::new(size),
            last_avail_idx: 0,
            enabled: false,
            ready: false,
            desc_addr: 0,
            avail_addr: 0,
            used_addr: 0,
        }
    }

    /// Get queue size
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Check if queue is ready for operation
    pub fn is_ready(&self) -> bool {
        self.ready && self.enabled
    }

    /// Enable the queue
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Set queue as ready
    pub fn set_ready(&mut self) {
        self.ready = true;
    }

    /// Set descriptor table address
    pub fn set_desc_addr(&mut self, addr: u64) {
        self.desc_addr = addr;
    }

    /// Set available ring address
    pub fn set_avail_addr(&mut self, addr: u64) {
        self.avail_addr = addr;
    }

    /// Set used ring address
    pub fn set_used_addr(&mut self, addr: u64) {
        self.used_addr = addr;
    }

    /// Get descriptor table address
    pub fn desc_addr(&self) -> u64 {
        self.desc_addr
    }

    /// Get available ring address
    pub fn avail_addr(&self) -> u64 {
        self.avail_addr
    }

    /// Get used ring address
    pub fn used_addr(&self) -> u64 {
        self.used_addr
    }

    /// Check if there are available descriptors
    pub fn has_available(&self) -> bool {
        self.avail.idx != self.last_avail_idx
    }

    /// Get next available descriptor chain head index
    pub fn pop_available(&mut self) -> Option<u16> {
        if !self.has_available() {
            return None;
        }

        let idx = self.last_avail_idx % self.size;
        let desc_idx = self.avail.ring[idx as usize];
        self.last_avail_idx = self.last_avail_idx.wrapping_add(1);

        Some(desc_idx)
    }

    /// Add a used descriptor chain
    pub fn push_used(&mut self, desc_idx: u16, len: u32) {
        let idx = self.used.idx % self.size;
        self.used.ring[idx as usize] = VirtqUsedElem {
            id: desc_idx as u32,
            len,
        };
        self.used.idx = self.used.idx.wrapping_add(1);
    }

    /// Get a descriptor by index
    pub fn get_desc(&self, idx: u16) -> Option<&VirtqDesc> {
        if idx < self.size {
            Some(&self.desc[idx as usize])
        } else {
            None
        }
    }

    /// Set a descriptor (for testing/simulation)
    pub fn set_desc(&mut self, idx: u16, desc: VirtqDesc) {
        if (idx as usize) < self.desc.len() {
            self.desc[idx as usize] = desc;
        }
    }

    /// Update available ring index (simulating guest update)
    pub fn set_avail_idx(&mut self, idx: u16) {
        self.avail.idx = idx;
    }

    /// Add descriptor to available ring (for testing)
    pub fn add_available(&mut self, desc_idx: u16) {
        let idx = self.avail.idx % self.size;
        self.avail.ring[idx as usize] = desc_idx;
        self.avail.idx = self.avail.idx.wrapping_add(1);
    }

    /// Get current used index
    pub fn used_idx(&self) -> u16 {
        self.used.idx
    }

    /// Reset the queue
    pub fn reset(&mut self) {
        self.last_avail_idx = 0;
        self.avail.idx = 0;
        self.avail.flags = 0;
        self.used.idx = 0;
        self.used.flags = 0;
        self.enabled = false;
        self.ready = false;
        self.desc_addr = 0;
        self.avail_addr = 0;
        self.used_addr = 0;
    }
}

/// Trait for virtio devices
pub trait VirtioDevice: Send + Sync {
    /// Get device ID
    fn device_id(&self) -> u16;

    /// Get supported features
    fn features(&self) -> u64;

    /// Acknowledge features from guest
    fn ack_features(&mut self, features: u64);

    /// Get device status
    fn status(&self) -> u8;

    /// Set device status
    fn set_status(&mut self, status: u8);

    /// Get device config (device-specific)
    fn read_config(&self, offset: u64) -> u8;

    /// Set device config
    fn write_config(&mut self, offset: u64, value: u8);

    /// Get number of virtqueues
    fn num_queues(&self) -> usize;

    /// Get virtqueue by index
    fn queue(&self, idx: usize) -> Option<&VirtQueue>;

    /// Get mutable virtqueue by index
    fn queue_mut(&mut self, idx: usize) -> Option<&mut VirtQueue>;

    /// Process available descriptors
    fn process_queues(&mut self);

    /// Check if device has pending interrupt
    fn has_interrupt(&self) -> bool;

    /// Acknowledge interrupt
    fn ack_interrupt(&mut self);

    /// Reset device
    fn reset(&mut self);
}

/// Virtio-net header prepended to packets
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioNetHeader {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
    pub num_buffers: u16,
}

impl VirtioNetHeader {
    pub const SIZE: usize = 12;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0] = self.flags;
        bytes[1] = self.gso_type;
        bytes[2..4].copy_from_slice(&self.hdr_len.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.gso_size.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.csum_start.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.csum_offset.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.num_buffers.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            flags: bytes[0],
            gso_type: bytes[1],
            hdr_len: u16::from_le_bytes([bytes[2], bytes[3]]),
            gso_size: u16::from_le_bytes([bytes[4], bytes[5]]),
            csum_start: u16::from_le_bytes([bytes[6], bytes[7]]),
            csum_offset: u16::from_le_bytes([bytes[8], bytes[9]]),
            num_buffers: u16::from_le_bytes([bytes[10], bytes[11]]),
        })
    }
}

/// Virtio network device
#[derive(Debug)]
pub struct VirtioNet {
    /// Device features
    features: u64,
    /// Acknowledged features
    acked_features: u64,
    /// Device status
    device_status: u8,
    /// MAC address
    mac: [u8; 6],
    /// Link status
    link_status: u16,
    /// RX queue (guest receives)
    rx_queue: VirtQueue,
    /// TX queue (guest sends)
    tx_queue: VirtQueue,
    /// Pending RX packets
    rx_buffer: VecDeque<Vec<u8>>,
    /// Transmitted packets (for testing)
    tx_buffer: VecDeque<Vec<u8>>,
    /// Interrupt pending
    interrupt_pending: bool,
    /// Maximum packet size
    mtu: u16,
}

impl VirtioNet {
    /// Create a new virtio-net device
    pub fn new(mac: [u8; 6]) -> Self {
        Self {
            features: VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS | VIRTIO_NET_F_CSUM,
            acked_features: 0,
            device_status: 0,
            mac,
            link_status: VIRTIO_NET_S_LINK_UP,
            rx_queue: VirtQueue::new(MAX_QUEUE_SIZE),
            tx_queue: VirtQueue::new(MAX_QUEUE_SIZE),
            rx_buffer: VecDeque::new(),
            tx_buffer: VecDeque::new(),
            interrupt_pending: false,
            mtu: 1500,
        }
    }

    /// Create with default MAC
    pub fn with_default_mac() -> Self {
        // Use locally administered MAC: 52:54:00:xx:xx:xx (QEMU style)
        Self::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])
    }

    /// Get MAC address
    pub fn mac(&self) -> &[u8; 6] {
        &self.mac
    }

    /// Set link status
    pub fn set_link_up(&mut self, up: bool) {
        if up {
            self.link_status |= VIRTIO_NET_S_LINK_UP;
        } else {
            self.link_status &= !VIRTIO_NET_S_LINK_UP;
        }
    }

    /// Check if link is up
    pub fn is_link_up(&self) -> bool {
        self.link_status & VIRTIO_NET_S_LINK_UP != 0
    }

    /// Queue a packet for reception by guest
    pub fn receive_packet(&mut self, packet: Vec<u8>) {
        // Prepend virtio-net header
        let header = VirtioNetHeader::default();
        let mut full_packet = header.to_bytes().to_vec();
        full_packet.extend_from_slice(&packet);

        self.rx_buffer.push_back(full_packet);
    }

    /// Get next transmitted packet (for backend)
    pub fn get_transmitted_packet(&mut self) -> Option<Vec<u8>> {
        self.tx_buffer.pop_front()
    }

    /// Get number of packets pending reception
    pub fn rx_pending(&self) -> usize {
        self.rx_buffer.len()
    }

    /// Get number of packets transmitted
    pub fn tx_count(&self) -> usize {
        self.tx_buffer.len()
    }

    /// Process RX queue - deliver packets to guest
    fn process_rx(&mut self) {
        while let Some(packet) = self.rx_buffer.front() {
            if !self.rx_queue.is_ready() || !self.rx_queue.has_available() {
                break;
            }

            if let Some(desc_idx) = self.rx_queue.pop_available() {
                // In a real implementation, we'd copy to guest memory
                // For now, just mark as used
                let len = packet.len() as u32;
                self.rx_queue.push_used(desc_idx, len);
                self.rx_buffer.pop_front();
                self.interrupt_pending = true;
            }
        }
    }

    /// Process TX queue - send packets from guest
    fn process_tx(&mut self) {
        while self.tx_queue.is_ready() && self.tx_queue.has_available() {
            if let Some(desc_idx) = self.tx_queue.pop_available() {
                // In a real implementation, we'd read from guest memory
                // For testing, we'll create a dummy packet
                if let Some(desc) = self.tx_queue.get_desc(desc_idx) {
                    // Simulate packet transmission
                    let packet = vec![0u8; desc.len as usize];
                    self.tx_buffer.push_back(packet);
                    self.tx_queue.push_used(desc_idx, 0);
                    self.interrupt_pending = true;
                }
            }
        }
    }
}

impl VirtioDevice for VirtioNet {
    fn device_id(&self) -> u16 {
        VIRTIO_ID_NET
    }

    fn features(&self) -> u64 {
        self.features
    }

    fn ack_features(&mut self, features: u64) {
        self.acked_features = features & self.features;
    }

    fn status(&self) -> u8 {
        self.device_status
    }

    fn set_status(&mut self, status: u8) {
        self.device_status = status;

        if status == 0 {
            self.reset();
        }
    }

    fn read_config(&self, offset: u64) -> u8 {
        match offset {
            // MAC address (bytes 0-5)
            0..=5 => self.mac[offset as usize],
            // Status (bytes 6-7)
            6 => self.link_status as u8,
            7 => (self.link_status >> 8) as u8,
            _ => 0,
        }
    }

    fn write_config(&mut self, offset: u64, value: u8) {
        // MAC address is read-only in most implementations
        // Status is also typically read-only
        let _ = (offset, value);
    }

    fn num_queues(&self) -> usize {
        2 // RX and TX
    }

    fn queue(&self, idx: usize) -> Option<&VirtQueue> {
        match idx {
            0 => Some(&self.rx_queue),
            1 => Some(&self.tx_queue),
            _ => None,
        }
    }

    fn queue_mut(&mut self, idx: usize) -> Option<&mut VirtQueue> {
        match idx {
            0 => Some(&mut self.rx_queue),
            1 => Some(&mut self.tx_queue),
            _ => None,
        }
    }

    fn process_queues(&mut self) {
        self.process_rx();
        self.process_tx();
    }

    fn has_interrupt(&self) -> bool {
        self.interrupt_pending
    }

    fn ack_interrupt(&mut self) {
        self.interrupt_pending = false;
    }

    fn reset(&mut self) {
        self.device_status = 0;
        self.acked_features = 0;
        self.rx_queue.reset();
        self.tx_queue.reset();
        self.rx_buffer.clear();
        self.tx_buffer.clear();
        self.interrupt_pending = false;
        self.link_status = VIRTIO_NET_S_LINK_UP;
    }
}

/// Thread-safe wrapper for VirtioNet
#[derive(Debug, Clone)]
pub struct SharedVirtioNet {
    inner: Arc<Mutex<VirtioNet>>,
}

impl SharedVirtioNet {
    /// Create a new shared virtio-net
    pub fn new(net: VirtioNet) -> Self {
        Self {
            inner: Arc::new(Mutex::new(net)),
        }
    }

    /// Get inner device
    pub fn lock(&self) -> std::sync::MutexGuard<'_, VirtioNet> {
        self.inner.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtqueue_creation() {
        let vq = VirtQueue::new(256);
        assert_eq!(vq.size(), 256);
        assert!(!vq.is_ready());
        assert!(!vq.has_available());
    }

    #[test]
    fn test_virtqueue_enable() {
        let mut vq = VirtQueue::new(64);
        vq.enable();
        vq.set_ready();
        assert!(vq.is_ready());
    }

    #[test]
    fn test_virtqueue_descriptors() {
        let mut vq = VirtQueue::new(16);

        let desc = VirtqDesc {
            addr: 0x1000,
            len: 512,
            flags: desc_flags::WRITE,
            next: 0,
        };
        vq.set_desc(0, desc);

        let retrieved = vq.get_desc(0).unwrap();
        assert_eq!(retrieved.addr, 0x1000);
        assert_eq!(retrieved.len, 512);
        assert!(retrieved.is_write_only());
        assert!(!retrieved.has_next());
    }

    #[test]
    fn test_virtqueue_available_ring() {
        let mut vq = VirtQueue::new(16);
        vq.enable();
        vq.set_ready();

        vq.add_available(5);
        assert!(vq.has_available());

        let idx = vq.pop_available();
        assert_eq!(idx, Some(5));
        assert!(!vq.has_available());
    }

    #[test]
    fn test_virtqueue_used_ring() {
        let mut vq = VirtQueue::new(16);

        assert_eq!(vq.used_idx(), 0);
        vq.push_used(3, 256);
        assert_eq!(vq.used_idx(), 1);
    }

    #[test]
    fn test_virtqueue_reset() {
        let mut vq = VirtQueue::new(16);
        vq.enable();
        vq.set_ready();
        vq.add_available(0);
        vq.push_used(0, 100);

        vq.reset();

        assert!(!vq.is_ready());
        assert!(!vq.has_available());
        assert_eq!(vq.used_idx(), 0);
    }

    #[test]
    fn test_virtio_net_creation() {
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let net = VirtioNet::new(mac);

        assert_eq!(net.device_id(), VIRTIO_ID_NET);
        assert_eq!(net.mac(), &mac);
        assert!(net.is_link_up());
        assert_eq!(net.num_queues(), 2);
    }

    #[test]
    fn test_virtio_net_features() {
        let mut net = VirtioNet::with_default_mac();

        let features = net.features();
        assert!(features & VIRTIO_NET_F_MAC != 0);
        assert!(features & VIRTIO_NET_F_STATUS != 0);

        net.ack_features(VIRTIO_NET_F_MAC);
        // Only MAC should be acknowledged
    }

    #[test]
    fn test_virtio_net_config() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let net = VirtioNet::new(mac);

        // Read MAC address from config
        for i in 0..6 {
            assert_eq!(net.read_config(i), mac[i as usize]);
        }

        // Read link status
        assert_eq!(net.read_config(6) & 1, 1); // Link up
    }

    #[test]
    fn test_virtio_net_status() {
        let mut net = VirtioNet::with_default_mac();

        assert_eq!(net.status(), 0);

        net.set_status(status::ACKNOWLEDGE);
        assert_eq!(net.status(), status::ACKNOWLEDGE);

        net.set_status(status::ACKNOWLEDGE | status::DRIVER);
        assert_eq!(net.status(), status::ACKNOWLEDGE | status::DRIVER);
    }

    #[test]
    fn test_virtio_net_link_status() {
        let mut net = VirtioNet::with_default_mac();

        assert!(net.is_link_up());

        net.set_link_up(false);
        assert!(!net.is_link_up());

        net.set_link_up(true);
        assert!(net.is_link_up());
    }

    #[test]
    fn test_virtio_net_receive_packet() {
        let mut net = VirtioNet::with_default_mac();

        let packet = vec![0xDE, 0xAD, 0xBE, 0xEF];
        net.receive_packet(packet);

        assert_eq!(net.rx_pending(), 1);
    }

    #[test]
    fn test_virtio_net_queues() {
        let mut net = VirtioNet::with_default_mac();

        // RX queue
        let rx = net.queue_mut(0).unwrap();
        rx.enable();
        rx.set_ready();
        assert!(net.queue(0).unwrap().is_ready());

        // TX queue
        let tx = net.queue_mut(1).unwrap();
        tx.enable();
        tx.set_ready();
        assert!(net.queue(1).unwrap().is_ready());

        // Invalid queue
        assert!(net.queue(2).is_none());
    }

    #[test]
    fn test_virtio_net_reset() {
        let mut net = VirtioNet::with_default_mac();

        net.set_status(status::DRIVER_OK);
        net.receive_packet(vec![1, 2, 3]);

        net.reset();

        assert_eq!(net.status(), 0);
        assert_eq!(net.rx_pending(), 0);
    }

    #[test]
    fn test_virtio_net_header() {
        let header = VirtioNetHeader {
            flags: 1,
            gso_type: 2,
            hdr_len: 0x1234,
            gso_size: 0x5678,
            csum_start: 0x9ABC,
            csum_offset: 0xDEF0,
            num_buffers: 3,
        };

        let bytes = header.to_bytes();
        let restored = VirtioNetHeader::from_bytes(&bytes).unwrap();

        // Copy fields to avoid unaligned access on packed struct
        let flags = restored.flags;
        let gso_type = restored.gso_type;
        let hdr_len = restored.hdr_len;
        let gso_size = restored.gso_size;
        let csum_start = restored.csum_start;
        let csum_offset = restored.csum_offset;
        let num_buffers = restored.num_buffers;

        assert_eq!(flags, 1);
        assert_eq!(gso_type, 2);
        assert_eq!(hdr_len, 0x1234);
        assert_eq!(gso_size, 0x5678);
        assert_eq!(csum_start, 0x9ABC);
        assert_eq!(csum_offset, 0xDEF0);
        assert_eq!(num_buffers, 3);
    }

    #[test]
    fn test_virtio_net_interrupt() {
        let mut net = VirtioNet::with_default_mac();

        assert!(!net.has_interrupt());

        // Setup and enable queues
        net.queue_mut(0).unwrap().enable();
        net.queue_mut(0).unwrap().set_ready();

        // Add available buffer and packet
        net.queue_mut(0).unwrap().add_available(0);
        net.receive_packet(vec![1, 2, 3, 4]);

        // Process should raise interrupt
        net.process_queues();
        assert!(net.has_interrupt());

        net.ack_interrupt();
        assert!(!net.has_interrupt());
    }

    #[test]
    fn test_shared_virtio_net() {
        let net = VirtioNet::with_default_mac();
        let shared = SharedVirtioNet::new(net);

        {
            let mut locked = shared.lock();
            locked.set_link_up(false);
        }

        {
            let locked = shared.lock();
            assert!(!locked.is_link_up());
        }
    }

    #[test]
    fn test_desc_flags() {
        let desc = VirtqDesc {
            addr: 0,
            len: 0,
            flags: desc_flags::NEXT | desc_flags::WRITE,
            next: 1,
        };

        assert!(desc.has_next());
        assert!(desc.is_write_only());
        assert!(!desc.is_indirect());
    }
}
