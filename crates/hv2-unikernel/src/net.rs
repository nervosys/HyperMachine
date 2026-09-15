//! A virtio-net driver, the same size and shape as the vsock one beside it.
//!
//! The host gained a network device a guest can actually drive — split
//! virtqueues walked out of guest memory, rather than the host-side ring
//! copies the two older `VirtioNet` models keep. This is the other half: a
//! guest that finds it, brings it up, and moves frames across it.
//!
//! # What is deliberately not here
//!
//! No IP, no ARP, no checksums, no fragmentation. A driver's job ends at the
//! link layer: a frame goes out whole or it does not go out, and a frame comes
//! in whole or it is dropped at the door. Everything above that is a stack,
//! and a stack that lived in this file would be a stack nothing had asked for.
//!
//! No offloads are negotiated either, and that is a correctness matter rather
//! than a simplification: a driver that tells a device it can checksum will
//! send frames with the checksum left out, and the device on the other side
//! does not fill them in.
//!
//! # Where this differs from vsock
//!
//! Three things, and they are the whole diff worth reading:
//!
//! 1. **A twelve-byte header, not forty-four.** Written zero except for
//!    `num_buffers`, which is one, because this driver never splits a frame.
//! 2. **Receive buffers are sized for a frame**, 2 KiB rather than 4: an
//!    Ethernet frame is at most 1514 bytes and a page holds two of them.
//! 3. **The MAC is negotiated.** Configuration space carries it only if
//!    `VIRTIO_NET_F_MAC` was accepted, so a driver that skips the feature and
//!    reads the field anyway is reading a field the specification says may be
//!    anything.

use core::ptr::{read_volatile, write_volatile};

/// Where [`VM::attach_net`] puts the register window.
///
/// Fixed and agreed with the host, as the vsock window is, and deliberately
/// past it: a guest with both devices has two windows to find and no way to
/// enumerate either.
const MMIO_BASE: usize = 0xD002_0000;

/// virtio-mmio registers, version 2. The same set the vsock driver uses; a
/// second copy rather than a shared module because these two drivers are meant
/// to be readable one at a time.
mod reg {
    pub const MAGIC: u32 = 0x000;
    pub const VERSION: u32 = 0x004;
    pub const DEVICE_ID: u32 = 0x008;
    pub const DEVICE_FEATURES: u32 = 0x010;
    pub const DEVICE_FEATURES_SEL: u32 = 0x014;
    pub const DRIVER_FEATURES: u32 = 0x020;
    pub const DRIVER_FEATURES_SEL: u32 = 0x024;
    pub const QUEUE_SEL: u32 = 0x030;
    pub const QUEUE_NUM_MAX: u32 = 0x034;
    pub const QUEUE_NUM: u32 = 0x038;
    pub const QUEUE_READY: u32 = 0x044;
    pub const QUEUE_NOTIFY: u32 = 0x050;
    pub const INTERRUPT_STATUS: u32 = 0x060;
    pub const INTERRUPT_ACK: u32 = 0x064;
    pub const STATUS: u32 = 0x070;
    pub const QUEUE_DESC_LOW: u32 = 0x080;
    pub const QUEUE_DESC_HIGH: u32 = 0x084;
    pub const QUEUE_DRIVER_LOW: u32 = 0x090;
    pub const QUEUE_DRIVER_HIGH: u32 = 0x094;
    pub const QUEUE_DEVICE_LOW: u32 = 0x0a0;
    pub const QUEUE_DEVICE_HIGH: u32 = 0x0a4;
    pub const CONFIG: u32 = 0x100;
}

/// Device status bits, written in this order during bring-up.
mod status {
    pub const ACKNOWLEDGE: u32 = 1;
    pub const DRIVER: u32 = 2;
    pub const DRIVER_OK: u32 = 4;
    pub const FEATURES_OK: u32 = 8;
}

/// `0x74726976` — "virt" in little-endian ASCII.
const VIRTIO_MAGIC: u32 = 0x7472_6976;
/// The virtio device type for a network card.
const DEVICE_ID_NET: u32 = 1;
/// Bit 32 of the feature bits, so bit 0 of the high bank.
const F_VERSION_1_HIGH: u32 = 1;
/// Bit 5 of the low bank: configuration space carries a MAC address.
const F_MAC_LOW: u32 = 1 << 5;

/// This descriptor is written by the device, not read by it.
const DESC_F_WRITE: u16 = 2;

/// Ring slots per queue.
const QUEUE_SIZE: u16 = 8;
/// Bytes per frame buffer: a header, a full-size frame, and room to spare.
const BUF_SIZE: usize = 2048;
/// The virtio-net header that precedes every frame in both directions.
pub const HEADER_SIZE: usize = 12;
/// The largest Ethernet frame this driver will send, header excluded.
pub const MAX_FRAME_LEN: usize = 1514;

/// Queue indices, named from the driver's side, which is what the device
/// expects: queue 0 is what the driver receives on.
const RX_QUEUE: u32 = 0;
const TX_QUEUE: u32 = 1;

/// Guest memory for the rings and buffers.
///
/// At 3 MiB, past the vsock driver's map at 2 MiB and its buffers, because a
/// guest may carry both devices and two drivers writing one page is not a
/// failure either of them could report.
mod mem {
    pub const RX_DESC: usize = 0x0030_0000;
    pub const RX_AVAIL: usize = 0x0030_1000;
    pub const RX_USED: usize = 0x0030_2000;
    pub const TX_DESC: usize = 0x0030_3000;
    pub const TX_AVAIL: usize = 0x0030_4000;
    pub const TX_USED: usize = 0x0030_5000;
    /// Eight 2 KiB receive buffers.
    pub const RX_BUFS: usize = 0x0031_0000;
    /// Eight 2 KiB transmit buffers, one per ring slot — for the reason the
    /// vsock driver's `TX_BUFS` gives at length: a descriptor handed to a
    /// device names memory the device owns until it says otherwise, and one
    /// shared buffer makes that true of every queued frame at once.
    pub const TX_BUFS: usize = 0x0031_8000;
}

/// A cursor that writes little-endian integers to guest physical memory.
struct Writer {
    at: usize,
}

impl Writer {
    fn u16(&mut self, v: u16) {
        // SAFETY: every address this is used with is inside the fixed ring and
        // buffer map above, which is guest RAM this program alone owns.
        unsafe { write_volatile(self.at as *mut u16, v) };
        self.at += 2;
    }
    fn u32(&mut self, v: u32) {
        // SAFETY: as above.
        unsafe { write_volatile(self.at as *mut u32, v) };
        self.at += 4;
    }
    fn u64(&mut self, v: u64) {
        // SAFETY: as above.
        unsafe { write_volatile(self.at as *mut u64, v) };
        self.at += 8;
    }
}

/// A cursor that reads little-endian integers from guest physical memory.
struct Reader {
    at: usize,
}

impl Reader {
    fn u16(&mut self) -> u16 {
        // SAFETY: as in `Writer`.
        let v = unsafe { read_volatile(self.at as *const u16) };
        self.at += 2;
        v
    }
    fn u32(&mut self) -> u32 {
        // SAFETY: as in `Writer`.
        let v = unsafe { read_volatile(self.at as *const u32) };
        self.at += 4;
        v
    }
}

/// Read a device register.
fn reg_read(offset: u32) -> u32 {
    // SAFETY: `MMIO_BASE + offset` is inside the register window the host
    // registered for this VM. Every access is a naturally aligned 32-bit one,
    // which is what the transport accepts.
    unsafe { read_volatile((MMIO_BASE + offset as usize) as *const u32) }
}

/// Write a device register.
fn reg_write(offset: u32, value: u32) {
    // SAFETY: as in `reg_read`.
    unsafe { write_volatile((MMIO_BASE + offset as usize) as *mut u32, value) }
}

/// Why bring-up failed, in the words a reader would want.
#[derive(Clone, Copy)]
pub enum InitError {
    /// Nothing that answers like a virtio device is at the window.
    NoDevice,
    /// A virtio device, but not version 2.
    WrongVersion,
    /// A virtio device of some other kind.
    NotNet,
    /// The device refused the features offered, so it cannot be driven.
    FeaturesRejected,
    /// The device's queues are smaller than this driver's rings.
    QueueTooSmall,
}

impl InitError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoDevice => "no virtio device at the network window",
            Self::WrongVersion => "the virtio device is not version 2",
            Self::NotNet => "the virtio device is not a network device",
            Self::FeaturesRejected => "the device refused VIRTIO_F_VERSION_1",
            Self::QueueTooSmall => "the device's queues are too small for this driver",
        }
    }
}

/// A frame received from the device.
pub struct Frame {
    /// Where the frame is, and how much of it there is — the header already
    /// stepped over. Left in the receive buffer rather than copied: there is
    /// no allocator here, and the caller reads it before the buffer is
    /// returned to the device.
    pub at: usize,
    pub len: u32,
}

impl Frame {
    /// Byte `i` of the frame, or zero past its end.
    ///
    /// Indexed rather than handed out as a slice because the bytes live in
    /// guest physical memory the device may write again the moment the buffer
    /// is released, and a slice would outlive that guarantee.
    #[must_use]
    pub fn byte(&self, i: usize) -> u8 {
        if i >= self.len as usize {
            return 0;
        }
        // SAFETY: inside a receive buffer this driver owns until `release`.
        unsafe { read_volatile((self.at + i) as *const u8) }
    }
}

/// The driver.
pub struct Net {
    /// This device's MAC address, read from configuration space.
    mac: [u8; 6],
    /// Next slot to use in the receive available ring.
    rx_avail: u16,
    /// Last used-ring index this driver has consumed, for the receive queue.
    rx_used_seen: u16,
    /// Next slot to use in the transmit available ring.
    tx_avail: u16,
}

impl Net {
    /// Bring the device up and post receive buffers.
    ///
    /// The status sequence is the specification's, and the order matters for
    /// the reason the vsock driver gives: a device that sees `DRIVER_OK`
    /// before its queues are configured cannot tell that from a driver that
    /// configured them badly.
    pub fn init() -> Result<Self, InitError> {
        if reg_read(reg::MAGIC) != VIRTIO_MAGIC {
            return Err(InitError::NoDevice);
        }
        if reg_read(reg::VERSION) != 2 {
            return Err(InitError::WrongVersion);
        }
        if reg_read(reg::DEVICE_ID) != DEVICE_ID_NET {
            return Err(InitError::NotNet);
        }

        reg_write(reg::STATUS, 0);
        reg_write(reg::STATUS, status::ACKNOWLEDGE);
        reg_write(reg::STATUS, status::ACKNOWLEDGE | status::DRIVER);

        // Two banks, and both are read before either is written. The MAC bit
        // is in the low bank and VERSION_1 in the high one, so unlike the
        // vsock driver this one has something to say in both.
        reg_write(reg::DEVICE_FEATURES_SEL, 0);
        let low = reg_read(reg::DEVICE_FEATURES);
        reg_write(reg::DEVICE_FEATURES_SEL, 1);
        let high = reg_read(reg::DEVICE_FEATURES);

        // Accept only what this driver implements. Anything else accepted is a
        // promise made on its behalf: an offload bit taken here means frames
        // going out with a checksum nothing computed.
        reg_write(reg::DRIVER_FEATURES_SEL, 0);
        reg_write(reg::DRIVER_FEATURES, low & F_MAC_LOW);
        reg_write(reg::DRIVER_FEATURES_SEL, 1);
        reg_write(reg::DRIVER_FEATURES, high & F_VERSION_1_HIGH);

        reg_write(
            reg::STATUS,
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK,
        );
        if reg_read(reg::STATUS) & status::FEATURES_OK == 0 {
            return Err(InitError::FeaturesRejected);
        }

        setup_queue(RX_QUEUE, mem::RX_DESC, mem::RX_AVAIL, mem::RX_USED)?;
        setup_queue(TX_QUEUE, mem::TX_DESC, mem::TX_AVAIL, mem::TX_USED)?;

        reg_write(
            reg::STATUS,
            status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK,
        );

        // Six bytes of MAC at the start of configuration space, read one byte
        // at a time because the field is not aligned as a wider load.
        let mut mac = [0u8; 6];
        for (i, byte) in mac.iter_mut().enumerate() {
            // SAFETY: inside the configuration window of a device that has
            // just reported itself ready.
            *byte = unsafe { read_volatile((MMIO_BASE + reg::CONFIG as usize + i) as *const u8) };
        }

        let mut driver = Self {
            mac,
            rx_avail: 0,
            rx_used_seen: 0,
            tx_avail: 0,
        };

        // Until this happens the device has nowhere to put a frame, and its
        // `flush_rx` queues rather than delivering — which presents as a link
        // that is up and carries nothing.
        for slot in 0..QUEUE_SIZE {
            driver.post_rx(slot);
        }
        notify(RX_QUEUE);

        Ok(driver)
    }

    /// This device's MAC address, as it reported it.
    #[must_use]
    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Hand receive buffer `slot` to the device.
    fn post_rx(&mut self, slot: u16) {
        let desc = mem::RX_DESC + usize::from(slot) * 16;
        let mut w = Writer { at: desc };
        w.u64((mem::RX_BUFS + usize::from(slot) * BUF_SIZE) as u64);
        w.u32(BUF_SIZE as u32);
        w.u16(DESC_F_WRITE);
        w.u16(0); // no chaining: one descriptor is one frame

        let ring = mem::RX_AVAIL + 4 + usize::from(self.rx_avail % QUEUE_SIZE) * 2;
        Writer { at: ring }.u16(slot);
        self.rx_avail = self.rx_avail.wrapping_add(1);
        // The index is published after the ring entry it refers to, for the
        // reason the vsock driver's `post_rx` records.
        Writer {
            at: mem::RX_AVAIL + 2,
        }
        .u16(self.rx_avail);
    }

    /// Take one frame from the device, or `None` if it has sent nothing.
    ///
    /// The frame stays in the receive buffer; the buffer goes back to the
    /// device in [`Self::release`], which the caller calls when it is done
    /// reading.
    pub fn recv(&mut self) -> Option<Frame> {
        let used_idx = Reader {
            at: mem::RX_USED + 2,
        }
        .u16();
        if used_idx == self.rx_used_seen {
            return None;
        }

        let entry = mem::RX_USED + 4 + usize::from(self.rx_used_seen % QUEUE_SIZE) * 8;
        let mut r = Reader { at: entry };
        let slot = r.u32() as u16;
        let written = r.u32();
        self.rx_used_seen = self.rx_used_seen.wrapping_add(1);

        let buf = mem::RX_BUFS + usize::from(slot) * BUF_SIZE;
        if written as usize <= HEADER_SIZE {
            // A header and nothing behind it is not a frame. Give the buffer
            // straight back rather than reporting an empty one upward.
            self.post_rx(slot);
            notify(RX_QUEUE);
            return None;
        }

        Some(Frame {
            at: buf + HEADER_SIZE,
            len: written - HEADER_SIZE as u32,
        })
    }

    /// Return a consumed receive buffer to the device.
    pub fn release(&mut self, frame: &Frame) {
        let slot = ((frame.at - HEADER_SIZE - mem::RX_BUFS) / BUF_SIZE) as u16;
        self.post_rx(slot);
        notify(RX_QUEUE);
    }

    /// Send one frame.
    ///
    /// Returns whether it was queued. A frame over [`MAX_FRAME_LEN`] is
    /// refused rather than truncated: half a frame on the wire is a frame the
    /// receiver discards after doing the work of reading it, and the sender
    /// never finds out.
    pub fn send(&mut self, frame: &[u8]) -> bool {
        if frame.is_empty() || frame.len() > MAX_FRAME_LEN {
            return false;
        }

        let slot = self.tx_avail % QUEUE_SIZE;
        let buf = mem::TX_BUFS + usize::from(slot) * BUF_SIZE;

        // The header, zero except for `num_buffers`: this driver never splits
        // a frame, so there is always exactly one.
        for i in 0..HEADER_SIZE {
            // SAFETY: inside a transmit buffer this driver owns.
            unsafe { write_volatile((buf + i) as *mut u8, 0) };
        }
        // SAFETY: as above; byte 10 is the low half of `num_buffers`.
        unsafe { write_volatile((buf + 10) as *mut u8, 1) };

        for (i, byte) in frame.iter().enumerate() {
            // SAFETY: bounded by `MAX_FRAME_LEN` above, which with the header
            // is well inside `BUF_SIZE`.
            unsafe { write_volatile((buf + HEADER_SIZE + i) as *mut u8, *byte) };
        }

        let desc = mem::TX_DESC + usize::from(slot) * 16;
        let mut w = Writer { at: desc };
        w.u64(buf as u64);
        w.u32((HEADER_SIZE + frame.len()) as u32);
        w.u16(0); // device-readable
        w.u16(0);

        let ring = mem::TX_AVAIL + 4 + usize::from(slot) * 2;
        Writer { at: ring }.u16(slot);
        self.tx_avail = self.tx_avail.wrapping_add(1);
        Writer {
            at: mem::TX_AVAIL + 2,
        }
        .u16(self.tx_avail);

        notify(TX_QUEUE);
        true
    }

    /// Acknowledge a device interrupt, if one is pending.
    pub fn ack_interrupt(&self) {
        ack_interrupt_raw();
    }
}

/// Point one queue at its rings and mark it ready.
fn setup_queue(queue: u32, desc: usize, avail: usize, used: usize) -> Result<(), InitError> {
    reg_write(reg::QUEUE_SEL, queue);
    if reg_read(reg::QUEUE_NUM_MAX) < u32::from(QUEUE_SIZE) {
        return Err(InitError::QueueTooSmall);
    }
    reg_write(reg::QUEUE_NUM, u32::from(QUEUE_SIZE));

    // Every high half is zero because this guest is entirely below 4 GiB, and
    // written anyway: a stale high half would point the device at memory that
    // does not exist.
    reg_write(reg::QUEUE_DESC_LOW, desc as u32);
    reg_write(reg::QUEUE_DESC_HIGH, 0);
    reg_write(reg::QUEUE_DRIVER_LOW, avail as u32);
    reg_write(reg::QUEUE_DRIVER_HIGH, 0);
    reg_write(reg::QUEUE_DEVICE_LOW, used as u32);
    reg_write(reg::QUEUE_DEVICE_HIGH, 0);

    reg_write(reg::QUEUE_READY, 1);
    Ok(())
}

/// Tell the device a queue has new entries.
fn notify(queue: u32) {
    reg_write(reg::QUEUE_NOTIFY, queue);
}

/// Acknowledge a device interrupt, without a driver in hand.
///
/// The virtio line is level-triggered and held until `InterruptACK` is
/// written, so a handler that skips this is re-entered immediately and
/// forever.
pub fn ack_interrupt_raw() {
    let pending = reg_read(reg::INTERRUPT_STATUS);
    if pending != 0 {
        reg_write(reg::INTERRUPT_ACK, pending);
    }
}
