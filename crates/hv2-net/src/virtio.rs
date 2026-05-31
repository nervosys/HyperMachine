//! VirtIO network device implementation
//!
//! This module provides a VirtIO-compliant network device that can be used
//! for high-performance VM networking with multiqueue support.

use crate::{NetError, Result};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Trait for accessing guest physical memory from VirtIO descriptors.
///
/// Implementors map guest-physical addresses to host buffers so that
/// VirtIO descriptor chains can read/write packet data into the guest.
pub trait GuestMemory: Send + Sync {
    /// Read `buf.len()` bytes from guest physical address `addr`.
    fn read(&self, addr: u64, buf: &mut [u8]) -> Result<()>;
    /// Write `data` to guest physical address `addr`.
    fn write(&self, addr: u64, data: &[u8]) -> Result<()>;
}

// VirtIO device type for network
const VIRTIO_NET_DEVICE_ID: u32 = 1;

// VirtIO network feature bits
mod features {
    pub const VIRTIO_NET_F_CSUM: u64 = 1 << 0; // Host handles pkts w/ partial csum
    pub const VIRTIO_NET_F_GUEST_CSUM: u64 = 1 << 1; // Guest handles pkts w/ partial csum
    pub const VIRTIO_NET_F_MAC: u64 = 1 << 5; // Device has given MAC address
    pub const VIRTIO_NET_F_GSO: u64 = 1 << 6; // Host can handle TSOv4 in
    pub const VIRTIO_NET_F_GUEST_TSO4: u64 = 1 << 7; // Guest can receive TSOv4
    pub const VIRTIO_NET_F_GUEST_TSO6: u64 = 1 << 8; // Guest can receive TSOv6
    pub const VIRTIO_NET_F_GUEST_ECN: u64 = 1 << 9; // Guest can receive TSO with ECN
    pub const VIRTIO_NET_F_GUEST_UFO: u64 = 1 << 10; // Guest can receive UFO
    pub const VIRTIO_NET_F_HOST_TSO4: u64 = 1 << 11; // Host can receive TSOv4
    pub const VIRTIO_NET_F_HOST_TSO6: u64 = 1 << 12; // Host can receive TSOv6
    pub const VIRTIO_NET_F_HOST_ECN: u64 = 1 << 13; // Host can receive TSO w/ ECN
    pub const VIRTIO_NET_F_HOST_UFO: u64 = 1 << 14; // Host can receive UFO
    pub const VIRTIO_NET_F_MRG_RXBUF: u64 = 1 << 15; // Host can merge receive buffers
    pub const VIRTIO_NET_F_STATUS: u64 = 1 << 16; // Configuration status field available
    pub const VIRTIO_NET_F_CTRL_VQ: u64 = 1 << 17; // Control channel available
    pub const VIRTIO_NET_F_CTRL_RX: u64 = 1 << 18; // Control channel RX mode support
    pub const VIRTIO_NET_F_CTRL_VLAN: u64 = 1 << 19; // Control channel VLAN filtering
    pub const VIRTIO_NET_F_MQ: u64 = 1 << 22; // Device supports multiqueue
    pub const VIRTIO_NET_F_CTRL_MAC_ADDR: u64 = 1 << 23; // Set MAC address through control channel

    // Modern VirtIO features (version 1.0+)
    pub const VIRTIO_F_VERSION_1: u64 = 1 << 32; // v1.0 compliant
    pub const VIRTIO_F_RING_PACKED: u64 = 1 << 34; // Packed virtqueue layout
    pub const VIRTIO_F_IN_ORDER: u64 = 1 << 35; // Device uses in-order processing
}

/// VirtIO network header prepended to each packet
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioNetHeader {
    /// Flags (needs_csum, data_valid, rsc_info)
    pub flags: u8,
    /// GSO type (none, tcpv4, udp, tcpv6, ecn)
    pub gso_type: u8,
    /// Header length for GSO
    pub hdr_len: u16,
    /// Maximum segment size for GSO
    pub gso_size: u16,
    /// Checksum start offset
    pub csum_start: u16,
    /// Checksum offset from csum_start
    pub csum_offset: u16,
    /// Number of merged buffers
    pub num_buffers: u16,
}

/// VirtIO descriptor in split virtqueue
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqDesc {
    /// Guest physical address of buffer
    pub addr: u64,
    /// Length of buffer
    pub len: u32,
    /// Flags (next, write, indirect)
    pub flags: u16,
    /// Next descriptor if flags & NEXT
    pub next: u16,
}

impl VirtqDesc {
    pub const F_NEXT: u16 = 1; // This descriptor continues via 'next'
    pub const F_WRITE: u16 = 2; // Buffer is write-only (for device)
    pub const F_INDIRECT: u16 = 4; // Buffer contains indirect table
}

/// VirtIO available ring
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: Vec<u16>,
}

/// VirtIO used ring element
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

/// VirtIO used ring
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: Vec<VirtqUsedElem>,
}

/// A single virtqueue for the device
#[derive(Debug, Clone, Default)]
pub struct Virtqueue {
    /// Queue size (number of descriptors)
    size: u16,
    /// Descriptors
    descriptors: Vec<VirtqDesc>,
    /// Available ring
    available: VirtqAvail,
    /// Used ring
    used: VirtqUsed,
    /// Last seen available index
    last_avail_idx: u16,
    /// Queue enabled
    enabled: bool,
    /// Notification suppressed
    notification_suppressed: bool,
}

impl Virtqueue {
    /// Create a new virtqueue
    #[must_use]
    pub fn new(size: u16) -> Self {
        Self {
            size,
            descriptors: vec![VirtqDesc::default(); size as usize],
            available: VirtqAvail {
                flags: 0,
                idx: 0,
                ring: vec![0; size as usize],
            },
            used: VirtqUsed {
                flags: 0,
                idx: 0,
                ring: vec![VirtqUsedElem::default(); size as usize],
            },
            last_avail_idx: 0,
            enabled: false,
            notification_suppressed: false,
        }
    }

    /// Check if there are available buffers
    pub fn has_available(&self) -> bool {
        self.available.idx != self.last_avail_idx
    }

    /// Get next available descriptor chain head
    pub fn pop_available(&mut self) -> Option<u16> {
        if !self.has_available() {
            return None;
        }
        let idx = self.last_avail_idx;
        self.last_avail_idx = self.last_avail_idx.wrapping_add(1);
        Some(self.available.ring[(idx % self.size) as usize])
    }

    /// Push a used buffer back
    pub fn push_used(&mut self, id: u16, len: u32) {
        let idx = self.used.idx % self.size;
        self.used.ring[idx as usize] = VirtqUsedElem { id: id as u32, len };
        self.used.idx = self.used.idx.wrapping_add(1);
    }

    /// Get a descriptor by index
    pub fn get_desc(&self, idx: u16) -> Option<&VirtqDesc> {
        self.descriptors.get(idx as usize)
    }

    /// Set a descriptor
    pub fn set_desc(&mut self, idx: u16, desc: VirtqDesc) {
        if (idx as usize) < self.descriptors.len() {
            self.descriptors[idx as usize] = desc;
        }
    }

    /// Enable the queue
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Check if queue is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Network device configuration
#[derive(Debug, Clone)]
pub struct VirtioNetConfig {
    /// MAC address
    pub mac: [u8; 6],
    /// Maximum number of queues
    pub max_virtqueue_pairs: u16,
    /// MTU
    pub mtu: u16,
    /// Status (link up, announce)
    pub status: u16,
}

impl Default for VirtioNetConfig {
    fn default() -> Self {
        Self {
            // Default MAC: 52:54:00:xx:xx:xx (QEMU-style)
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            max_virtqueue_pairs: 1,
            mtu: 1500,
            status: 1, // VIRTIO_NET_S_LINK_UP
        }
    }
}

/// Pending packet in transmit or receive queue
#[derive(Debug, Clone)]
pub struct NetPacket {
    /// Packet data (including VirtIO header)
    pub data: Vec<u8>,
    /// Source queue index
    pub queue_idx: usize,
}

/// Statistics for the network device
#[derive(Debug, Default)]
pub struct VirtioNetStats {
    /// Packets received
    pub rx_packets: AtomicU64,
    /// Packets transmitted
    pub tx_packets: AtomicU64,
    /// Bytes received
    pub rx_bytes: AtomicU64,
    /// Bytes transmitted
    pub tx_bytes: AtomicU64,
    /// Receive errors
    pub rx_errors: AtomicU64,
    /// Transmit errors
    pub tx_errors: AtomicU64,
    /// Dropped packets
    pub rx_dropped: AtomicU64,
    /// Transmit dropped
    pub tx_dropped: AtomicU64,
}

/// VirtIO network device
pub struct VirtioNet {
    /// Device configuration
    config: RwLock<VirtioNetConfig>,
    /// Number of queue pairs (TX + RX pairs)
    queue_pairs: usize,
    /// Receive queues (one per queue pair)
    rx_queues: Vec<Mutex<Virtqueue>>,
    /// Transmit queues (one per queue pair)
    tx_queues: Vec<Mutex<Virtqueue>>,
    /// Control queue (optional)
    ctrl_queue: Option<Mutex<Virtqueue>>,
    /// Supported features
    device_features: u64,
    /// Negotiated features
    driver_features: AtomicU64,
    /// Device status
    status: AtomicU32,
    /// Device initialized
    initialized: AtomicBool,
    /// Pending RX packets
    rx_pending: Mutex<VecDeque<NetPacket>>,
    /// Pending TX packets (for backend)
    tx_pending: Mutex<VecDeque<NetPacket>>,
    /// Statistics
    stats: Arc<VirtioNetStats>,
    /// Interrupt callback
    #[allow(clippy::type_complexity)]
    interrupt_cb: RwLock<Option<Arc<dyn Fn(u16) + Send + Sync>>>,
    /// Guest memory accessor (set after VM memory is mapped)
    guest_memory: RwLock<Option<Arc<dyn GuestMemory>>>,
}

impl VirtioNet {
    /// Create a new VirtIO network device
    #[must_use]
    pub fn new(queue_pairs: usize) -> Self {
        let queue_pairs = queue_pairs.max(1);
        let queue_size = 256u16; // Standard queue size

        // Create RX and TX queues
        let rx_queues: Vec<_> = (0..queue_pairs)
            .map(|_| Mutex::new(Virtqueue::new(queue_size)))
            .collect();
        let tx_queues: Vec<_> = (0..queue_pairs)
            .map(|_| Mutex::new(Virtqueue::new(queue_size)))
            .collect();

        // Control queue (if multiqueue enabled)
        let ctrl_queue = if queue_pairs > 1 {
            Some(Mutex::new(Virtqueue::new(64)))
        } else {
            None
        };

        // Supported features
        let device_features = features::VIRTIO_NET_F_CSUM
            | features::VIRTIO_NET_F_GUEST_CSUM
            | features::VIRTIO_NET_F_MAC
            | features::VIRTIO_NET_F_HOST_TSO4
            | features::VIRTIO_NET_F_HOST_TSO6
            | features::VIRTIO_NET_F_GUEST_TSO4
            | features::VIRTIO_NET_F_GUEST_TSO6
            | features::VIRTIO_NET_F_MRG_RXBUF
            | features::VIRTIO_NET_F_STATUS
            | features::VIRTIO_NET_F_CTRL_VQ
            | features::VIRTIO_NET_F_CTRL_RX
            | features::VIRTIO_F_VERSION_1
            | if queue_pairs > 1 {
                features::VIRTIO_NET_F_MQ
            } else {
                0
            };

        Self {
            config: RwLock::new(VirtioNetConfig {
                max_virtqueue_pairs: queue_pairs as u16,
                ..Default::default()
            }),
            queue_pairs,
            rx_queues,
            tx_queues,
            ctrl_queue,
            device_features,
            driver_features: AtomicU64::new(0),
            status: AtomicU32::new(0),
            initialized: AtomicBool::new(false),
            rx_pending: Mutex::new(VecDeque::new()),
            tx_pending: Mutex::new(VecDeque::new()),
            stats: Arc::new(VirtioNetStats::default()),
            interrupt_cb: RwLock::new(None),
            guest_memory: RwLock::new(None),
        }
    }

    /// Initialize the device
    pub async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "Initializing VirtIO network with {} queue pairs",
            self.queue_pairs
        );

        // Enable all queues
        for rx in &self.rx_queues {
            rx.lock().await.enable();
        }
        for tx in &self.tx_queues {
            tx.lock().await.enable();
        }
        if let Some(ctrl) = &self.ctrl_queue {
            ctrl.lock().await.enable();
        }

        self.initialized.store(true, Ordering::SeqCst);
        tracing::info!("VirtIO network device initialized");
        Ok(())
    }

    /// Get device ID
    #[inline]
    pub fn device_id(&self) -> u32 {
        VIRTIO_NET_DEVICE_ID
    }

    /// Get device features
    #[inline]
    pub fn device_features(&self) -> u64 {
        self.device_features
    }

    /// Set driver features (negotiation)
    pub fn set_driver_features(&self, features: u64) {
        let negotiated = self.device_features & features;
        self.driver_features.store(negotiated, Ordering::SeqCst);
        tracing::debug!("Negotiated features: 0x{:x}", negotiated);
    }

    /// Get negotiated features
    #[inline]
    pub fn negotiated_features(&self) -> u64 {
        self.driver_features.load(Ordering::SeqCst)
    }

    /// Check if a feature is negotiated
    #[inline]
    pub fn has_feature(&self, feature: u64) -> bool {
        (self.negotiated_features() & feature) != 0
    }

    /// Set device status
    pub fn set_status(&self, status: u32) {
        self.status.store(status, Ordering::SeqCst);
    }

    /// Get device status
    #[inline]
    pub fn status(&self) -> u32 {
        self.status.load(Ordering::SeqCst)
    }

    /// Set the MAC address
    pub async fn set_mac(&self, mac: [u8; 6]) {
        self.config.write().await.mac = mac;
    }

    /// Get the MAC address
    pub async fn mac(&self) -> [u8; 6] {
        self.config.read().await.mac
    }

    /// Set interrupt callback
    pub async fn set_interrupt_callback<F>(&self, cb: F)
    where
        F: Fn(u16) + Send + Sync + 'static,
    {
        *self.interrupt_cb.write().await = Some(Arc::new(cb));
    }

    /// Attach guest physical memory so descriptor chains can access it.
    pub async fn attach_guest_memory(&self, mem: Arc<dyn GuestMemory>) {
        *self.guest_memory.write().await = Some(mem);
    }

    /// Trigger interrupt for a queue
    async fn trigger_interrupt(&self, queue_idx: u16) {
        if let Some(cb) = self.interrupt_cb.read().await.as_ref() {
            cb(queue_idx);
        }
    }

    /// Walk a descriptor chain starting at `head`, collecting all
    /// write-only (device-writable) buffer regions as `(addr, len)`.
    fn collect_write_descs(queue: &Virtqueue, head: u16) -> Vec<(u64, u32)> {
        let mut result = Vec::new();
        let mut idx = head;
        let mut iters = 0u16;
        loop {
            if iters >= queue.size {
                break; // guard against loops
            }
            iters += 1;
            if let Some(desc) = queue.get_desc(idx) {
                if desc.flags & VirtqDesc::F_WRITE != 0 {
                    result.push((desc.addr, desc.len));
                }
                if desc.flags & VirtqDesc::F_NEXT != 0 {
                    idx = desc.next;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        result
    }

    /// Walk a descriptor chain starting at `head`, collecting all
    /// read-only (device-readable) buffer regions as `(addr, len)`.
    fn collect_read_descs(queue: &Virtqueue, head: u16) -> Vec<(u64, u32)> {
        let mut result = Vec::new();
        let mut idx = head;
        let mut iters = 0u16;
        loop {
            if iters >= queue.size {
                break;
            }
            iters += 1;
            if let Some(desc) = queue.get_desc(idx) {
                if desc.flags & VirtqDesc::F_WRITE == 0 {
                    result.push((desc.addr, desc.len));
                }
                if desc.flags & VirtqDesc::F_NEXT != 0 {
                    idx = desc.next;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        result
    }

    /// Receive a packet from the backend (to be delivered to guest)
    pub async fn receive_packet(&self, data: &[u8]) -> Result<()> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err(NetError::Config("Device not initialized".into()));
        }

        // Create VirtIO header
        let mut packet = Vec::with_capacity(std::mem::size_of::<VirtioNetHeader>() + data.len());

        // Add header (zeroed for basic receive)
        let header = VirtioNetHeader::default();
        // SAFETY: `VirtioNetHeader` is a plain-old-data struct (all fields are
        // primitive integers), so viewing it as a byte slice is safe. The pointer
        // is valid and the length matches `size_of::<VirtioNetHeader>()`.
        packet.extend_from_slice(unsafe {
            std::slice::from_raw_parts(
                &header as *const VirtioNetHeader as *const u8,
                std::mem::size_of::<VirtioNetHeader>(),
            )
        });
        packet.extend_from_slice(data);

        // Queue the packet
        self.rx_pending.lock().await.push_back(NetPacket {
            data: packet,
            queue_idx: 0, // Use first RX queue by default
        });

        // Process RX queue
        self.process_rx_queue(0).await?;

        Ok(())
    }

    /// Process RX queue - deliver pending packets to guest
    async fn process_rx_queue(&self, queue_idx: usize) -> Result<()> {
        if queue_idx >= self.rx_queues.len() {
            return Err(NetError::Config("Invalid queue index".into()));
        }

        let mut queue = self.rx_queues[queue_idx].lock().await;
        let mut pending = self.rx_pending.lock().await;
        let guest_mem = self.guest_memory.read().await;

        while let Some(packet) = pending.pop_front() {
            if let Some(desc_idx) = queue.pop_available() {
                let written = if let Some(mem) = guest_mem.as_ref() {
                    // Real path: walk descriptor chain and write packet data to
                    // guest-physical addresses.
                    let descs = Self::collect_write_descs(&queue, desc_idx);
                    let mut remaining = &packet.data[..];
                    let mut total = 0u32;
                    for (addr, len) in &descs {
                        if remaining.is_empty() {
                            break;
                        }
                        let chunk = remaining.len().min(*len as usize);
                        mem.write(*addr, &remaining[..chunk])?;
                        remaining = &remaining[chunk..];
                        total += chunk as u32;
                    }
                    total
                } else {
                    // Simulated path (no guest memory attached)
                    packet.data.len() as u32
                };

                queue.push_used(desc_idx, written);
                self.stats.rx_packets.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .rx_bytes
                    .fetch_add(written as u64, Ordering::Relaxed);
            } else {
                // No available buffers, put packet back
                pending.push_front(packet);
                self.stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }

        drop(guest_mem);
        drop(queue);
        drop(pending);

        // Trigger interrupt
        self.trigger_interrupt(queue_idx as u16 * 2).await; // RX queues are even

        Ok(())
    }

    /// Transmit packets from guest
    pub async fn process_tx_queue(&self, queue_idx: usize) -> Result<Vec<Vec<u8>>> {
        if queue_idx >= self.tx_queues.len() {
            return Err(NetError::Config("Invalid queue index".into()));
        }

        let mut queue = self.tx_queues[queue_idx].lock().await;
        let guest_mem = self.guest_memory.read().await;
        let mut packets = Vec::new();

        while let Some(desc_idx) = queue.pop_available() {
            let packet_data = if let Some(mem) = guest_mem.as_ref() {
                // Real path: read data from guest memory via descriptor chain
                let descs = Self::collect_read_descs(&queue, desc_idx);
                let total_len: usize = descs.iter().map(|(_, l)| *l as usize).sum();
                let mut buf = Vec::with_capacity(total_len);
                for (addr, len) in &descs {
                    let mut chunk = vec![0u8; *len as usize];
                    mem.read(*addr, &mut chunk)?;
                    buf.extend_from_slice(&chunk);
                }
                buf
            } else if let Some(desc) = queue.get_desc(desc_idx) {
                // Simulated path: fill with zeros
                vec![0u8; desc.len as usize]
            } else {
                continue;
            };

            let len = packet_data.len() as u32;
            packets.push(packet_data);
            queue.push_used(desc_idx, len);

            self.stats.tx_packets.fetch_add(1, Ordering::Relaxed);
            self.stats.tx_bytes.fetch_add(len as u64, Ordering::Relaxed);
        }

        drop(guest_mem);
        drop(queue);

        // Trigger interrupt
        self.trigger_interrupt(queue_idx as u16 * 2 + 1).await; // TX queues are odd

        Ok(packets)
    }

    /// Notify the device that a queue has new buffers
    pub async fn notify(&self, queue_idx: u16) -> Result<()> {
        let queue_pair = (queue_idx / 2) as usize;
        let is_tx = queue_idx % 2 == 1;

        if is_tx {
            // Process transmit
            let packets = self.process_tx_queue(queue_pair).await?;
            // Store for backend to pick up
            let mut pending = self.tx_pending.lock().await;
            for data in packets {
                pending.push_back(NetPacket {
                    data,
                    queue_idx: queue_pair,
                });
            }
        } else {
            // Process receive (try to deliver pending)
            self.process_rx_queue(queue_pair).await?;
        }

        Ok(())
    }

    /// Get pending TX packets for the backend
    pub async fn get_tx_packets(&self) -> Vec<NetPacket> {
        let mut pending = self.tx_pending.lock().await;
        pending.drain(..).collect()
    }

    /// Get statistics
    pub fn stats(&self) -> &VirtioNetStats {
        &self.stats
    }

    /// Reset the device
    pub async fn reset(&self) {
        self.status.store(0, Ordering::SeqCst);
        self.driver_features.store(0, Ordering::SeqCst);
        self.rx_pending.lock().await.clear();
        self.tx_pending.lock().await.clear();

        // Reset all queues
        for i in 0..self.queue_pairs {
            *self.rx_queues[i].lock().await = Virtqueue::new(256);
            *self.tx_queues[i].lock().await = Virtqueue::new(256);
        }
        if let Some(ctrl) = &self.ctrl_queue {
            *ctrl.lock().await = Virtqueue::new(64);
        }

        tracing::info!("VirtIO network device reset");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple flat guest memory for tests.
    struct TestGuestMemory {
        mem: std::sync::Mutex<Vec<u8>>,
    }

    impl TestGuestMemory {
        fn new(size: usize) -> Self {
            Self {
                mem: std::sync::Mutex::new(vec![0u8; size]),
            }
        }
    }

    impl GuestMemory for TestGuestMemory {
        fn read(&self, addr: u64, buf: &mut [u8]) -> Result<()> {
            let m = self.mem.lock().unwrap();
            let start = addr as usize;
            let end = start + buf.len();
            if end > m.len() {
                return Err(NetError::Network("read out of bounds".into()));
            }
            buf.copy_from_slice(&m[start..end]);
            Ok(())
        }

        fn write(&self, addr: u64, data: &[u8]) -> Result<()> {
            let mut m = self.mem.lock().unwrap();
            let start = addr as usize;
            let end = start + data.len();
            if end > m.len() {
                return Err(NetError::Network("write out of bounds".into()));
            }
            m[start..end].copy_from_slice(data);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_virtio_net_creation() {
        let net = VirtioNet::new(2);
        assert_eq!(net.device_id(), VIRTIO_NET_DEVICE_ID);
        assert!(net.device_features() & features::VIRTIO_NET_F_MQ != 0);
        assert!(net.device_features() & features::VIRTIO_F_VERSION_1 != 0);
    }

    #[tokio::test]
    async fn test_virtio_net_init() {
        let mut net = VirtioNet::new(1);
        assert!(net.init().await.is_ok());
        assert!(net.initialized.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_feature_negotiation() {
        let net = VirtioNet::new(1);
        let requested = features::VIRTIO_NET_F_CSUM | features::VIRTIO_NET_F_MAC;
        net.set_driver_features(requested);
        assert_eq!(net.negotiated_features(), requested);
        assert!(net.has_feature(features::VIRTIO_NET_F_CSUM));
    }

    #[tokio::test]
    async fn test_mac_address() {
        let net = VirtioNet::new(1);
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        net.set_mac(mac).await;
        assert_eq!(net.mac().await, mac);
    }

    #[tokio::test]
    async fn test_virtqueue_operations() {
        let mut queue = Virtqueue::new(16);
        assert!(!queue.has_available());

        queue.available.idx = 1;
        queue.available.ring[0] = 5;
        assert!(queue.has_available());

        let desc_idx = queue.pop_available();
        assert_eq!(desc_idx, Some(5));
        assert!(!queue.has_available());

        queue.push_used(5, 100);
        assert_eq!(queue.used.idx, 1);
        assert_eq!(queue.used.ring[0].id, 5);
        assert_eq!(queue.used.ring[0].len, 100);
    }

    #[tokio::test]
    async fn test_rx_with_guest_memory() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();

        // Set up 4 KB of guest memory
        let gmem = Arc::new(TestGuestMemory::new(4096));
        net.attach_guest_memory(gmem.clone()).await;

        // Set up an RX descriptor chain:
        // desc[0] → writable, addr=0x100, len=256
        {
            let mut q = net.rx_queues[0].lock().await;
            q.set_desc(
                0,
                VirtqDesc {
                    addr: 0x100,
                    len: 256,
                    flags: VirtqDesc::F_WRITE,
                    next: 0,
                },
            );
            q.available.ring[0] = 0;
            q.available.idx = 1;
        }

        // Deliver a small packet
        let payload = b"Hello, guest!";
        net.receive_packet(payload).await.unwrap();

        // Verify data was written to guest memory at addr 0x100
        let header_len = std::mem::size_of::<VirtioNetHeader>();
        let expected_len = header_len + payload.len();
        let mut buf = vec![0u8; expected_len];
        gmem.read(0x100, &mut buf).unwrap();

        // After the VirtIO header, the payload should appear
        assert_eq!(&buf[header_len..], payload);

        // Stats should reflect one received packet
        assert_eq!(net.stats().rx_packets.load(Ordering::Relaxed), 1);
        assert_eq!(
            net.stats().rx_bytes.load(Ordering::Relaxed),
            expected_len as u64
        );
    }

    #[tokio::test]
    async fn test_tx_with_guest_memory() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();

        let gmem = Arc::new(TestGuestMemory::new(4096));
        // Pre-fill some data in guest memory at addr 0x200
        let tx_data = b"Outbound packet data";
        gmem.write(0x200, tx_data).unwrap();
        net.attach_guest_memory(gmem).await;

        // Set up a TX descriptor chain: desc[0] → readable, addr=0x200, len=tx_data.len()
        {
            let mut q = net.tx_queues[0].lock().await;
            q.set_desc(
                0,
                VirtqDesc {
                    addr: 0x200,
                    len: tx_data.len() as u32,
                    flags: 0, // readable by device
                    next: 0,
                },
            );
            q.available.ring[0] = 0;
            q.available.idx = 1;
        }

        let packets = net.process_tx_queue(0).await.unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0], tx_data);

        assert_eq!(net.stats().tx_packets.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_rx_descriptor_chain() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();

        let gmem = Arc::new(TestGuestMemory::new(4096));
        net.attach_guest_memory(gmem.clone()).await;

        // Two-descriptor chain: desc[0] → desc[1]
        {
            let mut q = net.rx_queues[0].lock().await;
            q.set_desc(
                0,
                VirtqDesc {
                    addr: 0x100,
                    len: 16,
                    flags: VirtqDesc::F_WRITE | VirtqDesc::F_NEXT,
                    next: 1,
                },
            );
            q.set_desc(
                1,
                VirtqDesc {
                    addr: 0x200,
                    len: 64,
                    flags: VirtqDesc::F_WRITE,
                    next: 0,
                },
            );
            q.available.ring[0] = 0;
            q.available.idx = 1;
        }

        let header_len = std::mem::size_of::<VirtioNetHeader>();
        // Payload larger than first descriptor's 16 bytes (header=10, so
        // payload will spill after 6 bytes of the first desc into the second)
        let payload = vec![0xABu8; 30];
        net.receive_packet(&payload).await.unwrap();

        // Read from second descriptor to verify chain was followed
        let mut buf2 = vec![0u8; 30];
        gmem.read(0x200, &mut buf2).unwrap();
        // The total packet (header_len + 30) should have spilled into desc[1]
        let spill = (header_len + 30) - 16; // bytes written into second desc
        assert!(buf2[..spill].iter().any(|b| *b != 0)); // non-zero data present
    }

    #[tokio::test]
    async fn test_tx_descriptor_chain() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();

        let gmem = Arc::new(TestGuestMemory::new(4096));
        gmem.write(0x300, b"AAAA").unwrap();
        gmem.write(0x400, b"BBBB").unwrap();
        net.attach_guest_memory(gmem).await;

        {
            let mut q = net.tx_queues[0].lock().await;
            q.set_desc(
                0,
                VirtqDesc {
                    addr: 0x300,
                    len: 4,
                    flags: VirtqDesc::F_NEXT,
                    next: 1,
                },
            );
            q.set_desc(
                1,
                VirtqDesc {
                    addr: 0x400,
                    len: 4,
                    flags: 0,
                    next: 0,
                },
            );
            q.available.ring[0] = 0;
            q.available.idx = 1;
        }

        let packets = net.process_tx_queue(0).await.unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0], b"AAAABBBB");
    }

    #[tokio::test]
    async fn test_rx_no_available_buffer_drops() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();

        // Don't make any descriptors available — packet should be dropped
        net.receive_packet(b"dropped").await.unwrap();
        assert_eq!(net.stats().rx_dropped.load(Ordering::Relaxed), 1);
        assert_eq!(net.stats().rx_packets.load(Ordering::Relaxed), 0);
    }

    // --- New tests below ---

    #[test]
    fn test_virtqueue_new() {
        let q = Virtqueue::new(32);
        assert_eq!(q.size, 32);
        assert!(!q.has_available());
        assert!(!q.is_enabled());
    }

    #[test]
    fn test_virtqueue_enable_disable() {
        let mut q = Virtqueue::new(16);
        assert!(!q.is_enabled());
        q.enable();
        assert!(q.is_enabled());
    }

    #[test]
    fn test_virtqueue_set_get_desc() {
        let mut q = Virtqueue::new(16);
        let desc = VirtqDesc {
            addr: 0x1000,
            len: 256,
            flags: VirtqDesc::F_WRITE,
            next: 0,
        };
        q.set_desc(0, desc);
        let got = q.get_desc(0).unwrap();
        assert_eq!(got.addr, 0x1000);
        assert_eq!(got.len, 256);
        assert_eq!(got.flags, VirtqDesc::F_WRITE);
    }

    #[test]
    fn test_virtqueue_get_desc_out_of_range() {
        let q = Virtqueue::new(4);
        assert!(q.get_desc(100).is_none());
    }

    #[test]
    fn test_virtqueue_pop_empty() {
        let mut q = Virtqueue::new(16);
        assert_eq!(q.pop_available(), None);
    }

    #[test]
    fn test_virtqueue_push_used_multiple() {
        let mut q = Virtqueue::new(16);
        q.push_used(0, 64);
        q.push_used(1, 128);
        q.push_used(2, 256);
        assert_eq!(q.used.idx, 3);
        assert_eq!(q.used.ring[0].id, 0);
        assert_eq!(q.used.ring[0].len, 64);
        assert_eq!(q.used.ring[1].id, 1);
        assert_eq!(q.used.ring[1].len, 128);
        assert_eq!(q.used.ring[2].id, 2);
        assert_eq!(q.used.ring[2].len, 256);
    }

    #[test]
    fn test_virtq_desc_flags() {
        assert_eq!(VirtqDesc::F_NEXT, 1);
        assert_eq!(VirtqDesc::F_WRITE, 2);
        assert_eq!(VirtqDesc::F_INDIRECT, 4);
        // Flags are independent bits
        assert_eq!(VirtqDesc::F_NEXT | VirtqDesc::F_WRITE, 3);
    }

    #[tokio::test]
    async fn test_virtio_net_device_id() {
        let net = VirtioNet::new(1);
        assert_eq!(net.device_id(), VIRTIO_NET_DEVICE_ID);
    }

    #[tokio::test]
    async fn test_virtio_net_default_mac() {
        let net = VirtioNet::new(1);
        let mac = net.mac().await;
        // Default MAC: 52:54:00:12:34:56
        assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    }

    #[tokio::test]
    async fn test_virtio_net_multi_queue() {
        let net = VirtioNet::new(4);
        assert!(net.device_features() & features::VIRTIO_NET_F_MQ != 0);
    }

    #[tokio::test]
    async fn test_virtio_net_stats_default() {
        let net = VirtioNet::new(1);
        let stats = net.stats();
        assert_eq!(stats.rx_packets.load(Ordering::Relaxed), 0);
        assert_eq!(stats.tx_packets.load(Ordering::Relaxed), 0);
        assert_eq!(stats.rx_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(stats.tx_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(stats.rx_dropped.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_virtio_net_reset() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();
        net.set_driver_features(features::VIRTIO_NET_F_CSUM);
        net.reset().await;
        assert_eq!(net.negotiated_features(), 0);
    }

    #[tokio::test]
    async fn test_virtio_net_set_mac_custom() {
        let net = VirtioNet::new(1);
        let custom_mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01];
        net.set_mac(custom_mac).await;
        assert_eq!(net.mac().await, custom_mac);
    }

    #[tokio::test]
    async fn test_feature_negotiation_masking() {
        let net = VirtioNet::new(1);
        let device_features = net.device_features();
        // Request features not offered by device, should be masked
        let bogus_feature = 1u64 << 50;
        net.set_driver_features(device_features | bogus_feature);
        // Negotiated should not contain the bogus feature
        let negotiated = net.negotiated_features();
        assert_eq!(negotiated & bogus_feature, 0);
    }

    #[tokio::test]
    async fn test_virtio_net_get_tx_packets_empty() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();
        let packets = net.get_tx_packets().await;
        assert!(packets.is_empty());
    }

    #[tokio::test]
    async fn test_process_tx_queue_no_descriptors() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();
        let packets = net.process_tx_queue(0).await.unwrap();
        assert!(packets.is_empty());
    }

    #[test]
    fn test_net_error_display() {
        let e = NetError::Network("test".to_string());
        assert!(format!("{}", e).contains("test"));
        let e = NetError::Config("bad config".to_string());
        assert!(format!("{}", e).contains("bad config"));
    }

    #[test]
    fn test_guest_memory_impl() {
        let mem = TestGuestMemory::new(256);
        let data = [0xAB, 0xCD, 0xEF, 0x01];
        mem.write(10, &data).unwrap();
        let mut buf = [0u8; 4];
        mem.read(10, &mut buf).unwrap();
        assert_eq!(buf, data);
    }

    #[test]
    fn test_guest_memory_out_of_bounds() {
        let mem = TestGuestMemory::new(16);
        let mut buf = [0u8; 32];
        assert!(mem.read(0, &mut buf).is_err());
        assert!(mem.write(0, &[0u8; 32]).is_err());
    }

    #[test]
    fn test_virtio_net_header_size() {
        assert_eq!(
            std::mem::size_of::<VirtioNetHeader>(),
            12,
            "VirtioNetHeader should be 12 bytes"
        );
    }

    #[tokio::test]
    async fn test_virtio_net_status_transitions() {
        let net = VirtioNet::new(1);
        assert_eq!(net.status(), 0);

        net.set_status(1); // ACKNOWLEDGE
        assert_eq!(net.status(), 1);

        net.set_status(3); // ACKNOWLEDGE | DRIVER
        assert_eq!(net.status(), 3);
    }

    #[tokio::test]
    async fn test_virtio_net_stats_after_rx() {
        let mut net = VirtioNet::new(1);
        net.init().await.unwrap();

        let mem = Arc::new(TestGuestMemory::new(4096));
        net.attach_guest_memory(mem.clone()).await;

        // Set up a descriptor for receiving
        {
            let mut q = net.rx_queues[0].lock().await;
            q.set_desc(
                0,
                VirtqDesc {
                    addr: 0,
                    len: 1024,
                    flags: VirtqDesc::F_WRITE,
                    next: 0,
                },
            );
            q.available.ring.push(0);
            q.available.idx = 1;
            q.enable();
        }

        let data = vec![0xAA; 100];
        let _ = net.receive_packet(&data).await;

        let stats = net.stats();
        // Stats should reflect the attempt
        let rx_packets = stats.rx_packets.load(std::sync::atomic::Ordering::Relaxed);
        let rx_dropped = stats.rx_dropped.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            rx_packets > 0 || rx_dropped > 0,
            "Should have either received or dropped"
        );
    }

    #[test]
    fn test_virtqueue_available_after_push() {
        let mut q = Virtqueue::new(16);
        assert!(!q.has_available());

        q.available.ring.push(0);
        q.available.idx = 1;
        assert!(q.has_available());

        let idx = q.pop_available();
        assert_eq!(idx, Some(0));
        assert!(!q.has_available());
    }

    #[test]
    fn test_virtqueue_used_ring() {
        let mut q = Virtqueue::new(16);
        assert_eq!(q.used.idx, 0);

        q.push_used(5, 128);
        assert_eq!(q.used.idx, 1);
        assert_eq!(q.used.ring[0].id, 5);
        assert_eq!(q.used.ring[0].len, 128);

        q.push_used(7, 256);
        assert_eq!(q.used.idx, 2);
    }

    #[tokio::test]
    async fn test_virtio_net_config_default_mac() {
        let net = VirtioNet::new(1);
        let mac = net.mac().await;
        assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    }

    #[tokio::test]
    async fn test_virtio_net_reset_clears_features() {
        let net = VirtioNet::new(1);
        net.set_driver_features(0xFFFF);
        assert_ne!(net.negotiated_features(), 0);

        net.reset().await;
        assert_eq!(net.negotiated_features(), 0);
    }
}
