//! A virtio-net device a guest can actually drive.
//!
//! # Why this exists when two `VirtioNet` types already do
//!
//! Neither of them can be attached to a VM, and the reason is the same for
//! both: they are not the kind of device the MMIO transport carries.
//!
//! [`VirtioMmioTransport`](super::virtio_mmio::VirtioMmioTransport) takes an
//! `Arc<Mutex<dyn VirtioMmioDevice>>`, and a
//! `VirtioMmioDevice` keeps its virtqueues as [`GuestQueue`]s — descriptor,
//! available and used ring *addresses*, which the device walks in guest memory
//! on every kick. That is what makes a virtio device a device rather than a
//! description of one.
//!
//! - `devices::virtio::VirtioNet` implements the older `VirtioDevice` trait and
//!   keeps its rings as `Vec<VirtqDesc>` — host-side copies, with a `tx_buffer`
//!   its own comment marks "for testing". A guest writing to its ring in guest
//!   memory would not be talking to it at all.
//! - `hv2_net::virtio::VirtioNet` is closer: it has a `guest_memory` field. But
//!   it is built against `hv2_net`'s own `GuestMemory` *trait*, not this
//!   crate's type, and it implements neither virtio trait here.
//!
//! So before a VM can have a network device at all, there has to be one shaped
//! like [`VsockDevice`](super::virtio_vsock::VsockDevice) — the only other
//! device in this crate that a guest can really drive. This is that, and it is
//! deliberately the same shape, because the next person comparing them should
//! find two devices that differ in what they carry and not in how they are
//! built.
//!
//! # What it does and does not do
//!
//! Two queues, which is what a guest expects: queue 0 is receive, queue 1 is
//! transmit, both named from the *driver's* point of view. A frame the guest
//! sends arrives on the transmit queue and is held until the host takes it with
//! [`VirtioNetMmio::take_transmitted`]; a frame the host hands to
//! [`VirtioNetMmio::queue_received`] waits until the driver has posted a buffer
//! to put it in.
//!
//! There is no backend here. Wiring this to a TAP device or to `hv2_net`'s NAT
//! is the next piece and belongs outside: a device that opened its own sockets
//! would be two things at once, and the vsock device does not do that either.
//!
//! Checksum and segmentation offloads are not offered. A guest that is told a
//! device can do them will send frames that assume it, and nothing here would
//! make that true.

use std::collections::VecDeque;
use std::sync::Arc;

use super::virtio::{VIRTIO_NET_F_MAC, VIRTIO_NET_S_LINK_UP};
use super::virtio_mmio::{VirtioMmioDevice, VIRTIO_F_VERSION_1};
use super::virtio_queue::GuestQueue;
use crate::error::Result;
use crate::memory::GuestMemory;

/// Virtio device ID for a network card.
///
/// The number virtio assigns the device *type*, which is 1. Not
/// `VIRTIO_ID_NET` from `devices::virtio`, which is `0x1000` — that is a PCI
/// vendor/device identifier for the legacy transport, and a guest probing an
/// MMIO window expects the type here.
pub const VIRTIO_ID_NET: u32 = 1;

/// Receive queue, in the driver's direction: frames the guest reads.
const RX_QUEUE: usize = 0;
/// Transmit queue: frames the guest writes.
const TX_QUEUE: usize = 1;

/// Queue depth offered to the driver.
const QUEUE_SIZE: u16 = 256;

/// Bytes of `virtio_net_hdr` in front of every frame.
///
/// Twelve under `VIRTIO_F_VERSION_1`, which includes `num_buffers` whether or
/// not `VIRTIO_NET_F_MRG_RXBUF` was negotiated. A device that wrote ten would
/// put every frame two bytes out of place, which a guest reports as a stream of
/// malformed packets rather than as anything to do with this header.
pub const NET_HDR_LEN: usize = 12;

/// The largest frame this device will carry, header excluded.
///
/// Standard Ethernet. A frame longer than this from the guest is dropped rather
/// than truncated: half a packet is worse than no packet, because it looks like
/// corruption on the wire instead of a refusal here.
pub const MAX_FRAME_LEN: usize = 1514;

/// Somewhere to be told that a frame is waiting.
///
/// The same shape as the vsock device's wake hook and for the same reason: a
/// frame queued by the host sits in `rx_pending` until a driver kick happens to
/// arrive, and on an idle guest that may be never. Whoever attaches this device
/// installs a hook that gets the VM to service the receive queue.
pub trait FrameWake: Send + Sync {
    /// A frame is waiting for the guest.
    fn wake(&self);
}

/// A virtio-net device backed by guest-memory virtqueues.
pub struct VirtioNetMmio {
    mac: [u8; 6],
    acked_features: u64,
    queues: Vec<GuestQueue>,
    /// Frames the guest has sent, waiting for the host to collect them.
    tx_pending: VecDeque<Vec<u8>>,
    /// Frames the host has offered, waiting for a buffer to put them in.
    rx_pending: VecDeque<Vec<u8>>,
    /// How many frames may wait in either direction before the oldest is
    /// dropped.
    ///
    /// A bound rather than a `VecDeque` that grows: a guest that stops posting
    /// receive buffers, or a host that stops collecting, would otherwise turn
    /// into unbounded host memory growth driven by the other side. Dropping is
    /// what a real link does when a queue fills.
    backlog: usize,
    wake: Option<Arc<dyn FrameWake>>,
    /// Frames dropped because the backlog was full, in each direction.
    dropped_rx: u64,
    dropped_tx: u64,
}

impl VirtioNetMmio {
    /// A device with `mac`, offering a 256-descriptor queue in each direction.
    pub fn new(mac: [u8; 6]) -> Self {
        Self {
            mac,
            acked_features: 0,
            queues: vec![GuestQueue::new(QUEUE_SIZE), GuestQueue::new(QUEUE_SIZE)],
            tx_pending: VecDeque::new(),
            rx_pending: VecDeque::new(),
            backlog: 256,
            wake: None,
            dropped_rx: 0,
            dropped_tx: 0,
        }
    }

    /// The MAC this device reports.
    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Install the hook that tells the VM a received frame is waiting.
    pub fn set_frame_wake(&mut self, wake: Arc<dyn FrameWake>) {
        self.wake = Some(wake);
    }

    /// How many frames may wait in one direction before the oldest is dropped.
    pub fn set_backlog(&mut self, frames: usize) {
        self.backlog = frames.max(1);
    }

    /// Hand the guest a frame to receive.
    ///
    /// Held until the driver has posted somewhere to put it. Over-long frames
    /// are refused here rather than sent on and truncated later.
    pub fn queue_received(&mut self, frame: Vec<u8>) -> bool {
        if frame.is_empty() || frame.len() > MAX_FRAME_LEN {
            return false;
        }
        if self.rx_pending.len() >= self.backlog {
            self.rx_pending.pop_front();
            self.dropped_rx += 1;
        }
        self.rx_pending.push_back(frame);
        if let Some(wake) = &self.wake {
            wake.wake();
        }
        true
    }

    /// Take one frame the guest has sent, if there is one.
    pub fn take_transmitted(&mut self) -> Option<Vec<u8>> {
        self.tx_pending.pop_front()
    }

    /// Frames waiting to be collected by the host.
    pub fn transmitted_len(&self) -> usize {
        self.tx_pending.len()
    }

    /// Frames waiting for the guest to post a buffer.
    pub fn pending_receive_len(&self) -> usize {
        self.rx_pending.len()
    }

    /// Frames dropped for a full backlog, receive and transmit.
    pub fn dropped(&self) -> (u64, u64) {
        (self.dropped_rx, self.dropped_tx)
    }

    /// Move everything the driver has put on the transmit queue into
    /// `tx_pending`.
    ///
    /// Returns whether anything was consumed, which is what the transport turns
    /// into an interrupt.
    fn drain_tx(&mut self, mem: &GuestMemory) -> Result<bool> {
        let mut consumed = false;
        while let Some(chain) = self.queues[TX_QUEUE].pop(mem)? {
            let bytes = chain.read_all(mem)?;
            // Everything before the header is the driver's business; a frame
            // shorter than the header is not a frame.
            if bytes.len() > NET_HDR_LEN {
                let frame = bytes[NET_HDR_LEN..].to_vec();
                if frame.len() <= MAX_FRAME_LEN {
                    if self.tx_pending.len() >= self.backlog {
                        self.tx_pending.pop_front();
                        self.dropped_tx += 1;
                    }
                    self.tx_pending.push_back(frame);
                } else {
                    self.dropped_tx += 1;
                    tracing::debug!(
                        "virtio-net: dropping a {}-byte frame from the guest, over the {MAX_FRAME_LEN}-byte limit",
                        frame.len()
                    );
                }
            }
            // The buffer goes back either way. A descriptor the device keeps
            // is a descriptor the driver never sees again, and a guest that
            // runs out stops sending with no error anywhere.
            self.queues[TX_QUEUE].add_used(mem, chain.head, 0)?;
            consumed = true;
        }
        Ok(consumed)
    }

    /// Put waiting frames into receive buffers the driver has posted.
    fn flush_rx(&mut self, mem: &GuestMemory) -> Result<bool> {
        let mut published = false;
        while !self.rx_pending.is_empty() {
            let Some(chain) = self.queues[RX_QUEUE].pop(mem)? else {
                break;
            };
            // Taken only once a buffer exists, so a frame is never lost to a
            // queue that turned out to be empty.
            let frame = self
                .rx_pending
                .pop_front()
                .expect("the loop condition just checked this");

            let mut buffer = vec![0u8; NET_HDR_LEN + frame.len()];
            // The header stays zero except for `num_buffers`, which is one:
            // this device never splits a frame across descriptors.
            buffer[10] = 1;
            buffer[NET_HDR_LEN..].copy_from_slice(&frame);

            let written = chain.write_all(mem, &buffer)?;
            self.queues[RX_QUEUE].add_used(mem, chain.head, written as u32)?;
            published = true;

            if written < buffer.len() {
                tracing::debug!(
                    "virtio-net: a receive buffer took {written} of {} bytes; the frame was truncated",
                    buffer.len()
                );
            }
        }
        Ok(published)
    }
}

impl VirtioMmioDevice for VirtioNetMmio {
    fn device_id(&self) -> u32 {
        VIRTIO_ID_NET
    }

    fn device_features(&self) -> u64 {
        // The MAC, and modern virtio. No offloads: see the module header.
        VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC
    }

    fn ack_features(&mut self, features: u64) {
        self.acked_features = features;
    }

    fn queues(&mut self) -> &mut [GuestQueue] {
        &mut self.queues
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        // Six bytes of MAC, then a two-byte status with LINK_UP set.
        let mut config = [0u8; 8];
        config[..6].copy_from_slice(&self.mac);
        config[6..8].copy_from_slice(&VIRTIO_NET_S_LINK_UP.to_le_bytes());
        for (i, byte) in data.iter_mut().enumerate() {
            let idx = offset as usize + i;
            *byte = config.get(idx).copied().unwrap_or(0);
        }
    }

    fn write_config(&mut self, _offset: u64, _data: &[u8]) {
        // The MAC is the host's to choose. A driver writing it is confused or
        // hostile, and either way the value does not move.
        tracing::debug!("virtio-net: ignoring a driver write to read-only config space");
    }

    fn notify(&mut self, queue: u16, mem: &GuestMemory) -> Result<bool> {
        match queue as usize {
            // A kick on either queue can create work on the other, exactly as
            // for vsock: fresh receive buffers may be what a waiting frame
            // needed, and draining transmit is cheap enough to do anyway.
            TX_QUEUE => {
                let consumed = self.drain_tx(mem)?;
                let published = self.flush_rx(mem)?;
                Ok(consumed || published)
            }
            RX_QUEUE => self.flush_rx(mem),
            other => {
                tracing::warn!("virtio-net: notify for queue {other}, which does not exist");
                Ok(false)
            }
        }
    }

    fn reset(&mut self) {
        for queue in &mut self.queues {
            queue.reset();
        }
        self.acked_features = 0;
        // Frames in flight belong to a link that no longer exists. Keeping them
        // would deliver a pre-reset packet to a post-reset driver.
        self.rx_pending.clear();
        self.tx_pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::virtio_queue::{desc_flags, Descriptor};
    use crate::memory::GuestAddress;

    // One ring per direction, so both queues can be published at once and a
    // frame can be watched crossing from one to the other.
    const RX_DESC: GuestAddress = 0x1000;
    const RX_AVAIL: GuestAddress = 0x1400;
    const RX_USED: GuestAddress = 0x1800;
    const RX_DATA: GuestAddress = 0x1c00;
    const TX_DESC: GuestAddress = 0x2000;
    const TX_AVAIL: GuestAddress = 0x2400;
    const TX_USED: GuestAddress = 0x2800;
    const TX_DATA: GuestAddress = 0x2c00;
    const RING_SIZE: u16 = 8;

    fn memory() -> GuestMemory {
        let mem = GuestMemory::new(0x10000).expect("guest memory");
        mem.allocate_region(0x10000, false).expect("region");
        mem
    }

    /// A device whose driver has published both rings, as a booted guest would.
    fn ready_device() -> VirtioNetMmio {
        let mut dev = VirtioNetMmio::new([0x52, 0x54, 0x00, 0xaa, 0xbb, 0xcc]);
        for (i, (desc, avail, used)) in [(RX_DESC, RX_AVAIL, RX_USED), (TX_DESC, TX_AVAIL, TX_USED)]
            .into_iter()
            .enumerate()
        {
            let q = &mut dev.queues()[i];
            q.set_size(RING_SIZE);
            q.set_desc_addr(desc);
            q.set_avail_addr(avail);
            q.set_used_addr(used);
            q.set_ready(true);
        }
        dev
    }

    fn write_desc(mem: &GuestMemory, base: GuestAddress, idx: u16, desc: Descriptor) {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&desc.addr.to_le_bytes());
        bytes[8..12].copy_from_slice(&desc.len.to_le_bytes());
        bytes[12..14].copy_from_slice(&desc.flags.to_le_bytes());
        bytes[14..16].copy_from_slice(&desc.next.to_le_bytes());
        mem.write_bytes(base + u64::from(idx) * 16, &bytes)
            .expect("descriptor");
    }

    fn make_available(mem: &GuestMemory, avail: GuestAddress, slot: u16, head: u16) {
        mem.write_bytes(avail + 4 + u64::from(slot) * 2, &head.to_le_bytes())
            .expect("ring entry");
        mem.write_bytes(avail + 2, &(slot + 1).to_le_bytes())
            .expect("avail idx");
    }

    fn used_idx(mem: &GuestMemory, used: GuestAddress) -> u16 {
        let bytes = mem.read_bytes(used + 2, 2).expect("used idx");
        u16::from_le_bytes([bytes[0], bytes[1]])
    }

    /// The point of this device: a frame the guest writes into its own ring
    /// comes out on the host side.
    ///
    /// Neither existing `VirtioNet` can do this. Their rings are host-side
    /// structures, so a guest writing to guest memory would be talking to
    /// nothing.
    #[test]
    fn a_frame_the_guest_writes_into_its_ring_reaches_the_host() {
        let mem = memory();
        let mut dev = ready_device();

        // A header and a frame, laid out as a driver lays them out.
        let frame = [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02];
        let mut buffer = vec![0u8; NET_HDR_LEN];
        buffer.extend_from_slice(&frame);
        mem.write_bytes(TX_DATA, &buffer).expect("frame");

        write_desc(
            &mem,
            TX_DESC,
            0,
            Descriptor {
                addr: TX_DATA,
                len: buffer.len() as u32,
                flags: 0,
                next: 0,
            },
        );
        make_available(&mem, TX_AVAIL, 0, 0);

        let owed = dev.notify(1, &mem).expect("notify");
        assert!(owed, "consuming a buffer owes the driver an interrupt");
        assert_eq!(
            dev.take_transmitted().as_deref(),
            Some(&frame[..]),
            "the frame should arrive without its header"
        );
        assert_eq!(
            used_idx(&mem, TX_USED),
            1,
            "and the descriptor should go back to the driver, or it runs out"
        );
    }

    /// And the other direction, which is the half a one-sided test would miss.
    #[test]
    fn a_frame_from_the_host_lands_in_a_buffer_the_guest_posted() {
        let mem = memory();
        let mut dev = ready_device();

        write_desc(
            &mem,
            RX_DESC,
            0,
            Descriptor {
                addr: RX_DATA,
                len: 128,
                flags: desc_flags::WRITE,
                next: 0,
            },
        );
        make_available(&mem, RX_AVAIL, 0, 0);

        let frame = [0xaa, 0xbb, 0xcc, 0xdd];
        assert!(dev.queue_received(frame.to_vec()), "accepted");

        let owed = dev.notify(0, &mem).expect("notify");
        assert!(owed, "publishing a frame owes the driver an interrupt");

        let got = mem
            .read_bytes(RX_DATA, NET_HDR_LEN + frame.len())
            .expect("read back");
        assert_eq!(
            &got[NET_HDR_LEN..],
            &frame[..],
            "the frame should be in the buffer, after the header"
        );
        assert_eq!(got[10], 1, "num_buffers is one: this device never splits");
        assert_eq!(used_idx(&mem, RX_USED), 1, "and the buffer is returned");
    }

    /// A frame with nowhere to go waits rather than being lost.
    #[test]
    fn a_frame_with_no_buffer_waits_instead_of_vanishing() {
        let mem = memory();
        let mut dev = ready_device();

        assert!(dev.queue_received(vec![1, 2, 3]));
        assert_eq!(dev.pending_receive_len(), 1);

        // No descriptor posted, so nothing can be published yet.
        assert!(!dev.notify(0, &mem).expect("notify"));
        assert_eq!(
            dev.pending_receive_len(),
            1,
            "the frame should still be waiting, not dropped on the floor"
        );

        // Now the driver posts one, and the waiting frame goes out.
        write_desc(
            &mem,
            RX_DESC,
            0,
            Descriptor {
                addr: RX_DATA,
                len: 128,
                flags: desc_flags::WRITE,
                next: 0,
            },
        );
        make_available(&mem, RX_AVAIL, 0, 0);
        assert!(dev.notify(0, &mem).expect("notify"));
        assert_eq!(dev.pending_receive_len(), 0);
    }

    /// The backlog is a bound, not a suggestion.
    #[test]
    fn a_full_backlog_drops_the_oldest_rather_than_growing() {
        let mut dev = ready_device();
        dev.set_backlog(2);

        for i in 0..4u8 {
            assert!(dev.queue_received(vec![i, i, i]));
        }
        assert_eq!(dev.pending_receive_len(), 2, "the bound holds");
        assert_eq!(dev.dropped(), (2, 0), "and says how many it cost");
    }

    /// An over-long frame is refused at the door.
    #[test]
    fn an_oversized_frame_is_refused_rather_than_truncated() {
        let mut dev = ready_device();
        assert!(!dev.queue_received(vec![0u8; MAX_FRAME_LEN + 1]));
        assert!(!dev.queue_received(Vec::new()), "and so is an empty one");
        assert_eq!(dev.pending_receive_len(), 0);
    }

    /// What the driver reads out of configuration space.
    #[test]
    fn config_space_carries_the_mac_and_a_live_link() {
        let dev = VirtioNetMmio::new([1, 2, 3, 4, 5, 6]);
        let mut config = [0u8; 8];
        dev.read_config(0, &mut config);
        assert_eq!(&config[..6], &[1, 2, 3, 4, 5, 6]);
        assert_eq!(
            u16::from_le_bytes([config[6], config[7]]),
            VIRTIO_NET_S_LINK_UP
        );

        // A read past the end is zero rather than a panic: a driver may read
        // whatever width it likes.
        let mut past = [0xffu8; 4];
        dev.read_config(64, &mut past);
        assert_eq!(past, [0, 0, 0, 0]);
    }

    /// A device that says it is a network card, with the type number rather
    /// than the legacy PCI identifier.
    #[test]
    fn it_identifies_as_a_network_device() {
        let dev = VirtioNetMmio::new([0; 6]);
        assert_eq!(dev.device_id(), 1, "virtio device type 1 is a network card");
        assert!(dev.device_features() & VIRTIO_F_VERSION_1 != 0);
        assert!(dev.device_features() & VIRTIO_NET_F_MAC != 0);
    }

    /// A reset drops frames in flight: they belong to a link that is gone.
    #[test]
    fn a_reset_drops_what_was_in_flight() {
        let mut dev = ready_device();
        dev.queue_received(vec![1, 2, 3]);
        assert_eq!(dev.pending_receive_len(), 1);

        dev.reset();
        assert_eq!(dev.pending_receive_len(), 0);
        assert_eq!(dev.transmitted_len(), 0);
        assert!(!dev.queues()[0].is_ready(), "and the rings are unpublished");
    }

    /// A kick for a queue that does not exist is refused, not indexed.
    #[test]
    fn a_kick_for_a_queue_that_does_not_exist_is_ignored() {
        let mem = memory();
        let mut dev = ready_device();
        assert!(!dev.notify(7, &mem).expect("notify"));
    }
}
