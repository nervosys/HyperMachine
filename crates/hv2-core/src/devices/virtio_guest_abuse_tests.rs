//! What a hostile virtio driver can do to the host, and that it cannot.
//!
//! hv2-sandboxd runs every VM's vCPU threads and devices in one process, built
//! with `overflow-checks = true` and `panic = "abort"`. A guest-reachable
//! arithmetic overflow, out-of-bounds index or unwrap therefore does not fail
//! one VM; it aborts the node. Each test here performs the guest's action
//! against the real device or transport and asserts the host neither panics
//! nor does unbounded work.
//!
//! They use only the public surface -- the transports' registers, a device's
//! `notify` -- so they say the same thing about the code before a fix as after
//! it. The dev profile has overflow checks on, which is what makes an
//! unchecked `+` here a failing test rather than a silent wrap.

use std::sync::Arc;

use parking_lot::Mutex;

use super::virtio_blk_mmio::VirtioBlockMmio;
use super::virtio_mmio::{
    status as mmio_status, VirtioMmioDevice, VirtioMmioTransport, VIRTIO_F_VERSION_1,
};
use super::virtio_net_mmio::VirtioNetMmio;
use super::virtio_pci::{VirtioPciTransport, NOTIFY_OFF_MULTIPLIER};
use super::virtio_queue::{desc_flags, GuestQueue};
use super::virtio_vsock::VsockDevice;
use crate::memory::GuestAddress;
use crate::{Device, GuestMemory};

const DESC: GuestAddress = 0x1000;
const AVAIL: GuestAddress = 0x2000;
const USED: GuestAddress = 0x3000;
const DATA: GuestAddress = 0x10000;
const RING: u16 = 8;
const RAM: u64 = 0x80000;

fn memory(size: u64) -> Arc<GuestMemory> {
    let mem = GuestMemory::new(size).expect("guest memory");
    mem.allocate_region(size, false).expect("region");
    Arc::new(mem)
}

fn write_desc(mem: &GuestMemory, idx: u16, addr: u64, len: u32, flags: u16, next: u16) {
    let mut b = [0u8; 16];
    b[0..8].copy_from_slice(&addr.to_le_bytes());
    b[8..12].copy_from_slice(&len.to_le_bytes());
    b[12..14].copy_from_slice(&flags.to_le_bytes());
    b[14..16].copy_from_slice(&next.to_le_bytes());
    mem.write_bytes(DESC + u64::from(idx) * 16, &b)
        .expect("descriptor");
}

/// Publish `head` in available slot `slot` and set `avail.idx` to `idx`.
fn publish(mem: &GuestMemory, slot: u16, head: u16, idx: u16) {
    mem.write_bytes(AVAIL + 4 + u64::from(slot) * 2, &head.to_le_bytes())
        .expect("avail entry");
    mem.write_bytes(AVAIL + 2, &idx.to_le_bytes())
        .expect("avail idx");
}

/// Point a device queue at the test rings, the way a driver's register writes
/// would once validated.
fn bring_up_queue(q: &mut GuestQueue) {
    q.set_size(RING);
    q.set_desc_addr(DESC);
    q.set_avail_addr(AVAIL);
    q.set_used_addr(USED);
    q.set_ready(true);
}

// ---------------------------------------------------------------------------
// H1: ring addresses
// ---------------------------------------------------------------------------

/// A ring at the very top of the address space must be an error at each use,
/// not an overflow.
#[test]
fn ring_addresses_at_the_top_of_the_address_space_are_errors_not_overflows() {
    let mem = memory(RAM);

    for top in [u64::MAX, u64::MAX - 1, u64::MAX - 3] {
        // avail.idx lives at avail + 2.
        let mut q = GuestQueue::new(256);
        bring_up_queue(&mut q);
        q.set_avail_addr(top);
        assert!(q.avail_idx(&mem).is_err());
        assert!(q.pop(&mem).is_err());
        assert!(!q.has_available(&mem).unwrap_or(false));

        // The descriptor table is read at desc + idx * 16; head 1 is the
        // first index that carries it past the end.
        let mut q = GuestQueue::new(256);
        bring_up_queue(&mut q);
        publish(&mem, 0, 1, 1);
        q.set_desc_addr(top);
        assert!(q.pop(&mem).is_err());

        // The used ring is written at used + 4 + slot * 8 and its index at
        // used + 2.
        let mut q = GuestQueue::new(256);
        bring_up_queue(&mut q);
        q.set_used_addr(top);
        assert!(q.add_used(&mem, 0, 0).is_err());
    }
}

/// The scenario as the driver performs it: program the registers, enable the
/// queue, kick it -- on the MMIO transport.
#[tokio::test]
async fn an_mmio_driver_publishing_a_wrapping_ring_cannot_abort_the_host() {
    let mem = memory(RAM);
    let device: Arc<Mutex<dyn VirtioMmioDevice>> =
        Arc::new(Mutex::new(VsockDevice::new(3).expect("vsock")));
    let mut t = VirtioMmioTransport::new("abuse-mmio", 0xd000_0000, mem, device);

    // Queue 1 is the vsock transmit queue.
    mmio_write(&mut t, 0x030, 1).await; // QueueSel
    mmio_write(&mut t, 0x038, u32::from(RING)).await; // QueueNum
    mmio_write(&mut t, 0x080, DESC as u32).await;
    mmio_write(&mut t, 0x090, 0xFFFF_FFFF).await; // QueueDriver low
    mmio_write(&mut t, 0x094, 0xFFFF_FFFF).await; // QueueDriver high
    mmio_write(&mut t, 0x0a0, USED as u32).await;
    mmio_write(&mut t, 0x044, 1).await; // QueueReady

    // Whether or not the enable was refused, the kick must not abort.
    mmio_write(
        &mut t,
        0x070,
        mmio_status::ACKNOWLEDGE
            | mmio_status::DRIVER
            | mmio_status::FEATURES_OK
            | mmio_status::DRIVER_OK,
    )
    .await;
    mmio_write(&mut t, 0x050, 1).await; // QueueNotify

    assert_eq!(
        mmio_read(&t, 0x044).await,
        0,
        "a queue whose ring is not in RAM is not ready"
    );
    assert_ne!(
        mmio_read(&t, 0x070).await & mmio_status::DEVICE_NEEDS_RESET,
        0,
        "the device asks for a reset"
    );
}

/// The same on the PCI transport, whose notify path did not require
/// DRIVER_OK: the driver never has to finish bring-up to reach the device.
#[tokio::test]
async fn a_pci_driver_publishing_a_wrapping_ring_cannot_abort_the_host() {
    let mem = memory(RAM);
    let device: Arc<Mutex<dyn VirtioMmioDevice>> =
        Arc::new(Mutex::new(VsockDevice::new(3).expect("vsock")));
    let mut t = VirtioPciTransport::new("abuse-pci", 0xd000_0000, mem, device);

    pci_write(&mut t, 0x16, &1u16.to_le_bytes()).await; // queue_select
    pci_write(&mut t, 0x18, &RING.to_le_bytes()).await; // queue_size
    pci_write(&mut t, 0x20, &DESC.to_le_bytes()).await; // queue_desc
    pci_write(&mut t, 0x28, &u64::MAX.to_le_bytes()).await; // queue_driver
    pci_write(&mut t, 0x30, &USED.to_le_bytes()).await; // queue_device
    pci_write(&mut t, 0x1C, &1u16.to_le_bytes()).await; // queue_enable

    // Kick queue 1 with no DRIVER_OK at all.
    pci_write(
        &mut t,
        0x2000 + u64::from(NOTIFY_OFF_MULTIPLIER),
        &1u16.to_le_bytes(),
    )
    .await;

    let mut enabled = [0u8; 2];
    t.read(0x1C, &mut enabled).await.expect("queue_enable");
    assert_eq!(
        u16::from_le_bytes(enabled),
        0,
        "a queue whose ring is not in RAM is not enabled"
    );
    let mut st = [0u8; 1];
    t.read(0x14, &mut st).await.expect("status");
    assert_ne!(st[0] & 0x40, 0, "NEEDS_RESET is set");
}

/// Rings that are in RAM but misaligned, short, or partly outside it are
/// refused at enable.
#[tokio::test]
async fn rings_that_are_misaligned_or_leave_ram_are_refused_at_enable() {
    let mem = memory(RAM);
    let device: Arc<Mutex<dyn VirtioMmioDevice>> =
        Arc::new(Mutex::new(VsockDevice::new(3).expect("vsock")));
    let mut t = VirtioMmioTransport::new("abuse-bounds", 0xd000_0000, mem, device);

    let cases: [(&str, u64, u64, u64); 4] = [
        ("misaligned descriptor table", DESC + 8, AVAIL, USED),
        ("misaligned used ring", DESC, AVAIL, USED + 2),
        // The table fits but its tail runs off the end of RAM.
        ("table past the end of RAM", RAM - 16, AVAIL, USED),
        ("used ring in the MMIO hole", DESC, AVAIL, 0xd000_0000),
    ];
    for (what, desc, avail, used) in cases {
        mmio_write(&mut t, 0x070, 0).await; // reset clears NEEDS_RESET
        mmio_write(&mut t, 0x030, 1).await;
        mmio_write(&mut t, 0x038, u32::from(RING)).await;
        mmio_write(&mut t, 0x080, desc as u32).await;
        mmio_write(&mut t, 0x084, (desc >> 32) as u32).await;
        mmio_write(&mut t, 0x090, avail as u32).await;
        mmio_write(&mut t, 0x094, (avail >> 32) as u32).await;
        mmio_write(&mut t, 0x0a0, used as u32).await;
        mmio_write(&mut t, 0x0a4, (used >> 32) as u32).await;
        mmio_write(&mut t, 0x044, 1).await;
        assert_eq!(mmio_read(&t, 0x044).await, 0, "{what}: not enabled");
    }

    // And a well-formed one is accepted, so the above is not a blanket refusal.
    mmio_write(&mut t, 0x070, 0).await;
    mmio_write(&mut t, 0x030, 1).await;
    mmio_write(&mut t, 0x038, u32::from(RING)).await;
    mmio_write(&mut t, 0x080, DESC as u32).await;
    mmio_write(&mut t, 0x090, AVAIL as u32).await;
    mmio_write(&mut t, 0x0a0, USED as u32).await;
    mmio_write(&mut t, 0x044, 1).await;
    assert_eq!(mmio_read(&t, 0x044).await, 1);
    assert_eq!(
        mmio_read(&t, 0x070).await & mmio_status::DEVICE_NEEDS_RESET,
        0
    );
}

// ---------------------------------------------------------------------------
// Feature masking
// ---------------------------------------------------------------------------

/// A device that offers a few bits and records what it was told was accepted.
struct FeatureProbe {
    queues: Vec<GuestQueue>,
    acked: u64,
}

impl VirtioMmioDevice for FeatureProbe {
    fn device_id(&self) -> u32 {
        19
    }
    fn device_features(&self) -> u64 {
        VIRTIO_F_VERSION_1 | 0b1010
    }
    fn ack_features(&mut self, features: u64) {
        self.acked = features;
    }
    fn queues(&mut self) -> &mut [GuestQueue] {
        &mut self.queues
    }
    fn read_config(&self, _offset: u64, _data: &mut [u8]) {}
    fn write_config(&mut self, _offset: u64, _data: &[u8]) {}
    fn notify(&mut self, _queue: u16, _mem: &GuestMemory) -> crate::Result<bool> {
        Ok(false)
    }
    fn reset(&mut self) {
        self.acked = 0;
    }
}

#[tokio::test]
async fn a_driver_cannot_accept_features_the_device_never_offered_on_mmio() {
    let probe = Arc::new(Mutex::new(FeatureProbe {
        queues: vec![GuestQueue::new(8)],
        acked: 0,
    }));
    let mut t = VirtioMmioTransport::new("abuse-feat", 0xd000_0000, memory(0x1000), probe.clone());

    for sel in [0u32, 1] {
        mmio_write(&mut t, 0x024, sel).await;
        mmio_write(&mut t, 0x020, 0xFFFF_FFFF).await;
    }
    assert_eq!(
        probe.lock().acked,
        VIRTIO_F_VERSION_1 | 0b1010,
        "only offered bits reach the device"
    );
}

#[tokio::test]
async fn a_driver_cannot_accept_features_the_device_never_offered_on_pci() {
    let probe = Arc::new(Mutex::new(FeatureProbe {
        queues: vec![GuestQueue::new(8)],
        acked: 0,
    }));
    let mut t = VirtioPciTransport::new("abuse-feat", 0xd000_0000, memory(0x1000), probe.clone());

    for sel in [0u32, 1] {
        pci_write(&mut t, 0x08, &sel.to_le_bytes()).await;
        pci_write(&mut t, 0x0C, &0xFFFF_FFFFu32.to_le_bytes()).await;
    }
    assert_eq!(
        probe.lock().acked,
        VIRTIO_F_VERSION_1 | 0b1010,
        "only offered bits reach the device"
    );
}

// ---------------------------------------------------------------------------
// H2: checksum offload
// ---------------------------------------------------------------------------

const NET_HDR: usize = 12;

/// Put one transmit chain of `hdr + payload` in slot 0 of the net device's
/// transmit queue.
fn net_tx_chain(mem: &GuestMemory, flags: u8, payload: &[u8]) {
    let mut buf = vec![0u8; NET_HDR];
    buf[0] = flags;
    buf.extend_from_slice(payload);
    mem.write_bytes(DATA, &buf).expect("frame");
    write_desc(mem, 0, DATA, buf.len() as u32, 0, 0);
    publish(mem, 0, 0, 1);
}

/// 200 KiB of 0xFF is 102 400 words of 0xFFFF: past what a u32 sum holds.
const BIG_FRAME: usize = 200 * 1024;

#[test]
fn a_huge_frame_cannot_overflow_a_checksum_the_device_never_offered() {
    // The device offers no offloads, but the driver says it accepted CSUM.
    // Through a transport that is masked away; here the device is handed the
    // bit directly, as the old transports did, so the device must hold on its
    // own.
    let mem = memory(RAM);
    let mut dev = VirtioNetMmio::new([0x52, 0x54, 0, 0, 0, 1]);
    dev.ack_features(
        VIRTIO_F_VERSION_1 | 1 /* CSUM */ | (1 << 11), /* HOST_TSO4 */
    );
    bring_up_queue(&mut dev.queues()[1]);

    net_tx_chain(&mem, 1 /* NEEDS_CSUM */, &vec![0xFF; BIG_FRAME]);
    // A chain this size may be refused outright; either way no panic, and no
    // frame reaches the host.
    let _ = dev.notify(1, &mem);

    assert_eq!(dev.transmitted_len(), 0, "an oversize frame is dropped");
}

#[test]
fn a_legitimately_offloaded_oversize_frame_is_dropped_before_it_is_summed() {
    let mem = memory(RAM);
    let mut dev = VirtioNetMmio::new([0x52, 0x54, 0, 0, 0, 1]);
    dev.set_host_offloads(true);
    dev.ack_features(dev.device_features());
    bring_up_queue(&mut dev.queues()[1]);

    // 150 000 bytes of 0xFF after the header: over the 65 549-byte offloaded
    // limit, and enough words to overflow a 32-bit sum.
    net_tx_chain(&mem, 1, &vec![0xFF; 150_000]);
    let _ = dev.notify(1, &mem);

    assert_eq!(dev.transmitted_len(), 0, "an oversize frame is dropped");
}

#[test]
fn an_offloaded_frame_at_the_limit_still_gets_its_checksum() {
    let mem = memory(RAM);
    let mut dev = VirtioNetMmio::new([0x52, 0x54, 0, 0, 0, 1]);
    dev.set_host_offloads(true);
    dev.ack_features(dev.device_features());
    bring_up_queue(&mut dev.queues()[1]);

    // Not IPv4, so the generic partial-checksum path: sum from csum_start on,
    // stored at csum_start + csum_offset. 65 000 bytes of 0xFF is within the
    // limit and 32 500 words of 0xFFFF.
    let mut payload = vec![0xFFu8; 65_000];
    payload[12..14].copy_from_slice(&[0x86, 0xdd]); // not 0x0800
    let mut buf = vec![0u8; NET_HDR];
    buf[0] = 1;
    buf[6..8].copy_from_slice(&20u16.to_le_bytes()); // csum_start
    buf[8..10].copy_from_slice(&16u16.to_le_bytes()); // csum_offset
    buf.extend_from_slice(&payload);
    mem.write_bytes(DATA, &buf).expect("frame");
    write_desc(&mem, 0, DATA, buf.len() as u32, 0, 0);
    publish(&mem, 0, 0, 1);

    assert!(dev.notify(1, &mem).expect("notify"));
    let frame = dev.take_transmitted().expect("the frame was sent");
    assert_eq!(frame.len(), 65_000);
}

// ---------------------------------------------------------------------------
// H3: descriptor addresses
// ---------------------------------------------------------------------------

fn disk() -> (tempfile::NamedTempFile, VirtioBlockMmio) {
    let file = tempfile::NamedTempFile::new().expect("temp file");
    file.as_file().set_len(64 * 512).expect("size");
    let dev = VirtioBlockMmio::open(file.path(), false, "abuse").expect("open");
    (file, dev)
}

/// A header descriptor then a writable one at `status_addr` of `status_len`.
fn blk_request(mem: &GuestMemory, kind: u32, status_addr: u64, status_len: u32) {
    let mut hdr = [0u8; 16];
    hdr[0..4].copy_from_slice(&kind.to_le_bytes());
    mem.write_bytes(DATA, &hdr).expect("header");
    write_desc(mem, 0, DATA, 16, desc_flags::NEXT, 1);
    write_desc(mem, 1, status_addr, status_len, desc_flags::WRITE, 0);
    publish(mem, 0, 0, 1);
}

#[test]
fn a_status_descriptor_at_the_top_of_the_address_space_is_refused() {
    let mem = memory(RAM);
    let (_file, mut dev) = disk();
    bring_up_queue(&mut dev.queues()[0]);

    // A flush needs no data, so the device goes straight to writing the
    // status byte at `addr + (len - 1)`: with `addr` of u64::MAX that is the
    // sum that overflowed.
    blk_request(&mem, 4 /* flush */, u64::MAX, 2);
    assert!(dev.notify(0, &mem).is_err(), "the request is refused");
}

#[test]
fn a_descriptor_whose_end_wraps_or_leaves_ram_is_refused() {
    let mem = memory(RAM);
    for (addr, len) in [
        (u64::MAX - 1, 16u32), // addr + len wraps
        (RAM - 4, 16),         // runs past the end of RAM
        (0xd000_0000, 16),     // not RAM at all
        (u64::MAX, u32::MAX),  // both
    ] {
        let (_file, mut dev) = disk();
        bring_up_queue(&mut dev.queues()[0]);
        blk_request(&mem, 4, addr, len);
        assert!(
            dev.notify(0, &mem).is_err(),
            "descriptor at {addr:#x} of {len} bytes must be refused"
        );
    }
}

#[test]
fn a_well_formed_blk_request_is_still_served() {
    let mem = memory(RAM);
    let (_file, mut dev) = disk();
    bring_up_queue(&mut dev.queues()[0]);

    blk_request(&mem, 4, DATA + 0x100, 1);
    assert!(dev.notify(0, &mem).expect("notify"));
    assert_eq!(mem.read_bytes(DATA + 0x100, 1).unwrap(), [0], "S_OK");
}

// ---------------------------------------------------------------------------
// H4: unbounded drain
// ---------------------------------------------------------------------------

/// `avail.idx` one behind the device's position is 65 535 chains published on
/// a ring of 8. Every slot points at the same chain of `chain_bytes`.
fn lie_about_avail_idx(mem: &GuestMemory, chain_bytes: u32, write: bool) {
    let flags = if write { desc_flags::WRITE } else { 0 };
    write_desc(mem, 0, DATA, chain_bytes, flags, 0);
    for slot in 0..RING {
        mem.write_bytes(AVAIL + 4 + u64::from(slot) * 2, &0u16.to_le_bytes())
            .unwrap();
    }
    // The device is at 0; idx = 0 - 1.
    mem.write_bytes(AVAIL + 2, &u16::MAX.to_le_bytes()).unwrap();
}

#[test]
fn a_lying_avail_idx_does_not_make_the_blk_device_serve_65535_requests() {
    let mem = memory(RAM);
    let (_file, mut dev) = disk();
    bring_up_queue(&mut dev.queues()[0]);

    // One 64 KiB readable chain: a header that is not a request, so the
    // device does the cheap thing with it -- 65 535 times in the old loop.
    lie_about_avail_idx(&mem, 0x10000, false);
    let _ = dev.notify(0, &mem);

    let q = &dev.queues()[0];
    assert!(
        q.progress().0 <= RING,
        "consumed {} chains from a ring of {RING}",
        q.progress().0
    );
    assert!(q.is_broken(), "the queue is stopped");
}

#[test]
fn a_lying_avail_idx_does_not_make_the_net_device_drain_65535_frames() {
    let mem = memory(RAM);
    let mut dev = VirtioNetMmio::new([0x52, 0x54, 0, 0, 0, 1]);
    bring_up_queue(&mut dev.queues()[1]);

    lie_about_avail_idx(&mem, 0x10000, false);
    let _ = dev.notify(1, &mem);

    let q = &dev.queues()[1];
    assert!(
        q.progress().0 <= RING,
        "consumed {} chains from a ring of {RING}",
        q.progress().0
    );
    assert!(q.is_broken());
}

#[test]
fn a_lying_avail_idx_does_not_make_the_vsock_device_drain_65535_packets() {
    let mem = memory(RAM);
    let mut dev = VsockDevice::new(3).expect("vsock");
    bring_up_queue(&mut dev.queues()[1]);

    lie_about_avail_idx(&mem, 0x10000, false);
    let _ = dev.notify(1, &mem);

    let q = &dev.queues()[1];
    assert!(
        q.progress().0 <= RING,
        "consumed {} chains from a ring of {RING}",
        q.progress().0
    );
    assert!(q.is_broken());
}

#[tokio::test]
async fn a_broken_queue_reports_needs_reset_and_a_reset_clears_it() {
    let mem = memory(RAM);
    let device: Arc<Mutex<dyn VirtioMmioDevice>> =
        Arc::new(Mutex::new(VirtioNetMmio::new([0x52, 0x54, 0, 0, 0, 1])));
    let mut t = VirtioMmioTransport::new("abuse-reset", 0xd000_0000, mem.clone(), device);

    // Queue 1 is the transmit queue.
    mmio_write(&mut t, 0x030, 1).await;
    mmio_write(&mut t, 0x038, u32::from(RING)).await;
    mmio_write(&mut t, 0x080, DESC as u32).await;
    mmio_write(&mut t, 0x090, AVAIL as u32).await;
    mmio_write(&mut t, 0x0a0, USED as u32).await;
    mmio_write(&mut t, 0x044, 1).await;
    assert_eq!(mmio_read(&t, 0x044).await, 1, "a sound queue is enabled");
    mmio_write(
        &mut t,
        0x070,
        mmio_status::ACKNOWLEDGE
            | mmio_status::DRIVER
            | mmio_status::FEATURES_OK
            | mmio_status::DRIVER_OK,
    )
    .await;

    lie_about_avail_idx(&mem, 64, false);
    mmio_write(&mut t, 0x050, 1).await;
    assert_ne!(
        mmio_read(&t, 0x070).await & mmio_status::DEVICE_NEEDS_RESET,
        0
    );

    mmio_write(&mut t, 0x070, 0).await;
    assert_eq!(
        mmio_read(&t, 0x070).await & mmio_status::DEVICE_NEEDS_RESET,
        0,
        "a reset clears it"
    );
}

/// A correct driver with a full ring is drained in one kick.
#[test]
fn a_full_ring_of_ordinary_requests_is_drained_in_one_kick() {
    let mem = memory(RAM);
    let (_file, mut dev) = disk();
    bring_up_queue(&mut dev.queues()[0]);

    // Four requests, each a header descriptor and a one-byte status: eight
    // descriptors, which fills the table of a ring of eight.
    for n in 0..4u16 {
        let hdr = DATA + u64::from(n) * 0x40;
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(&4u32.to_le_bytes());
        mem.write_bytes(hdr, &h).unwrap();
        write_desc(&mem, 2 * n, hdr, 16, desc_flags::NEXT, 2 * n + 1);
        write_desc(&mem, 2 * n + 1, hdr + 0x20, 1, desc_flags::WRITE, 0);
        mem.write_bytes(AVAIL + 4 + u64::from(n) * 2, &(2 * n).to_le_bytes())
            .unwrap();
    }
    mem.write_bytes(AVAIL + 2, &4u16.to_le_bytes()).unwrap();

    assert!(dev.notify(0, &mem).expect("notify"));
    assert_eq!(dev.queues()[0].progress(), (4, 4));
    assert!(!dev.queues()[0].is_broken());
}

/// Chains that are each within the limit but together past the per-kick byte
/// budget are served in part; the rest waits for the next kick.
#[test]
fn a_kick_serves_a_bounded_number_of_bytes_and_leaves_the_rest() {
    // 64 MiB chain: header (readable) 16 bytes, then 64 MiB - 17 of
    // "readable" payload, i.e. one maximal chain, aliased by every slot.
    let size: u64 = 66 * 1024 * 1024;
    let mem = memory(size);
    let (_file, mut dev) = disk();
    bring_up_queue(&mut dev.queues()[0]);

    let big = (64 * 1024 * 1024 - 16) as u32;
    let data = 0x10000u64;
    write_desc(&mem, 0, data, big, desc_flags::NEXT, 1);
    write_desc(&mem, 1, data, 1, desc_flags::WRITE, 0);
    for slot in 0..RING {
        mem.write_bytes(AVAIL + 4 + u64::from(slot) * 2, &0u16.to_le_bytes())
            .unwrap();
    }
    mem.write_bytes(AVAIL + 2, &RING.to_le_bytes()).unwrap();

    let _ = dev.notify(0, &mem);
    let served = dev.queues()[0].progress().0;
    assert!(
        served < RING,
        "a ring of eight 64 MiB requests was served whole ({served})"
    );
    assert!(served >= 1, "the kick made progress");

    // The next kick takes up where this one stopped.
    let _ = dev.notify(0, &mem);
    assert!(dev.queues()[0].progress().0 > served);
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn mmio_write(t: &mut VirtioMmioTransport, offset: u64, value: u32) {
    t.write(offset, &value.to_le_bytes())
        .await
        .expect("mmio write");
}

async fn mmio_read(t: &VirtioMmioTransport, offset: u64) -> u32 {
    let mut b = [0u8; 4];
    t.read(offset, &mut b).await.expect("mmio read");
    u32::from_le_bytes(b)
}

async fn pci_write(t: &mut VirtioPciTransport, offset: u64, data: &[u8]) {
    t.write(offset, data).await.expect("pci write");
}
