//! VirtIO network device implementation
//!
//! This module provides a VirtIO-compliant network device that can be used
//! for high-performance VM networking with multiqueue support.

use crate::{NetError, Result};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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

    /// Trigger interrupt for a queue
    async fn trigger_interrupt(&self, queue_idx: u16) {
        if let Some(cb) = self.interrupt_cb.read().await.as_ref() {
            cb(queue_idx);
        }
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

        while let Some(packet) = pending.pop_front() {
            if let Some(desc_idx) = queue.pop_available() {
                // In a real implementation, we would copy data to guest memory
                // using the descriptor chain. Here we simulate success.
                let len = packet.data.len() as u32;
                queue.push_used(desc_idx, len);

                self.stats.rx_packets.fetch_add(1, Ordering::Relaxed);
                self.stats.rx_bytes.fetch_add(len as u64, Ordering::Relaxed);
            } else {
                // No available buffers, put packet back
                pending.push_front(packet);
                self.stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }

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
        let mut packets = Vec::new();

        while let Some(desc_idx) = queue.pop_available() {
            // In a real implementation, we would read data from guest memory
            // using the descriptor chain. Here we simulate with empty packets.
            if let Some(desc) = queue.get_desc(desc_idx) {
                let len = desc.len;

                // Simulate reading packet data (would come from guest memory)
                let packet_data = vec![0u8; len as usize];
                packets.push(packet_data);

                queue.push_used(desc_idx, len);

                self.stats.tx_packets.fetch_add(1, Ordering::Relaxed);
                self.stats.tx_bytes.fetch_add(len as u64, Ordering::Relaxed);
            }
        }

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
}
