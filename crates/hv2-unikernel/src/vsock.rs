//! A virtio-vsock driver, small enough to read in one sitting.
//!
//! The host has had a complete virtio-vsock device for some time — split
//! virtqueues walked out of guest memory, connection state, credit accounting.
//! What it never had was a guest that could talk to it. A swarm message could
//! be *routed* correctly and still only ever arrive at a serial port, because
//! a serial port is one `out` instruction and a virtqueue is this file.
//!
//! # What a driver has to agree with the device about
//!
//! Three things, and getting any of them wrong looks the same from outside —
//! a guest that boots and never answers:
//!
//! 1. **Where the registers are.** A virtio-mmio window, found at a fixed
//!    address rather than by enumeration, because a unikernel has no
//!    device tree and no PCI bus walk.
//! 2. **Where the rings are.** Descriptor table, available ring and used ring,
//!    all in guest memory at addresses the driver picks and tells the device.
//! 3. **What a packet is.** A 44-byte header and its payload, in one
//!    descriptor, which is what this device's `flush_rx` and `drain_tx`
//!    expect.
//!
//! # Polling, not interrupts
//!
//! There is no interrupt handler here and no IDT. The driver spins on the used
//! ring, which is the simplest thing that can work and costs a host core while
//! it runs. That is only tolerable because a spinning vCPU can now be
//! interrupted — until `stop()` learned to kick one out of `KVM_RUN`, a guest
//! that never halted owned its thread until the process died, and this driver
//! would have been unstoppable rather than merely busy.

use core::ptr::{read_volatile, write_volatile};

/// Where [`VM::attach_vsock`] puts the register window.
///
/// A fixed address, agreed with the host rather than discovered: 3.25 GiB, above
/// any conventional low-memory layout and below the 4 GiB line.
const MMIO_BASE: u32 = 0xD000_0000;

/// The host's context ID, fixed by the specification.
pub const HOST_CID: u64 = 2;

/// Stream socket — the only type this device supports.
const TYPE_STREAM: u16 = 1;

/// Packet operations, from the virtio specification section 5.10.6.1.
pub mod op {
    pub const REQUEST: u16 = 1;
    pub const RESPONSE: u16 = 2;
    pub const RST: u16 = 3;
    pub const SHUTDOWN: u16 = 4;
    pub const RW: u16 = 5;
    pub const CREDIT_UPDATE: u16 = 6;
    pub const CREDIT_REQUEST: u16 = 7;
}

/// virtio-mmio registers, version 2.
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
/// The virtio device ID for vsock.
const DEVICE_ID_VSOCK: u32 = 19;
/// Bit 32 of the feature bits. A version-2 mmio device requires it.
const F_VERSION_1_HIGH: u32 = 1;

/// This descriptor is written by the device, not read by it.
const DESC_F_WRITE: u16 = 2;

/// Ring slots per queue. Eight is more than a request/response conversation
/// needs and keeps every ring inside one page.
const QUEUE_SIZE: u16 = 8;
/// Bytes per packet buffer: a header, plus room for a message.
const BUF_SIZE: u32 = 4096;

/// Queue indices, fixed by the device.
const RX_QUEUE: u32 = 0;
const TX_QUEUE: u32 = 1;

/// Guest memory for the rings and buffers.
///
/// Fixed addresses at 2 MiB, chosen because the image is linked at 1 MiB and is
/// a few kilobytes: nothing else in this guest allocates, so a static map is
/// both sufficient and the only thing that could be verified by reading it.
mod mem {
    pub const RX_DESC: u32 = 0x0020_0000;
    pub const RX_AVAIL: u32 = 0x0020_1000;
    pub const RX_USED: u32 = 0x0020_2000;
    pub const TX_DESC: u32 = 0x0020_3000;
    pub const TX_AVAIL: u32 = 0x0020_4000;
    pub const TX_USED: u32 = 0x0020_5000;
    /// Eight 4 KiB receive buffers.
    pub const RX_BUFS: u32 = 0x0021_0000;
    /// One transmit buffer, reused: this driver sends one packet at a time and
    /// waits for the device to consume it.
    pub const TX_BUF: u32 = 0x0022_0000;
}

/// The 44-byte packet header.
///
/// Field for field the same as the host's `VsockHeader`, and laid out by hand
/// rather than by `repr(C)` so that the wire format is visible here rather than
/// implied by a struct definition somewhere else.
#[derive(Clone, Copy, Default)]
pub struct Header {
    pub src_cid: u64,
    pub dst_cid: u64,
    pub src_port: u32,
    pub dst_port: u32,
    pub len: u32,
    pub type_: u16,
    pub op: u16,
    pub flags: u32,
    pub buf_alloc: u32,
    pub fwd_cnt: u32,
}

/// Bytes in a header on the wire.
pub const HEADER_SIZE: usize = 44;

impl Header {
    fn write_to(&self, at: u32) {
        let mut w = Writer { at };
        w.u64(self.src_cid);
        w.u64(self.dst_cid);
        w.u32(self.src_port);
        w.u32(self.dst_port);
        w.u32(self.len);
        w.u16(self.type_);
        w.u16(self.op);
        w.u32(self.flags);
        w.u32(self.buf_alloc);
        w.u32(self.fwd_cnt);
    }

    fn read_from(at: u32) -> Self {
        let mut r = Reader { at };
        Self {
            src_cid: r.u64(),
            dst_cid: r.u64(),
            src_port: r.u32(),
            dst_port: r.u32(),
            len: r.u32(),
            type_: r.u16(),
            op: r.u16(),
            flags: r.u32(),
            buf_alloc: r.u32(),
            fwd_cnt: r.u32(),
        }
    }
}

/// A cursor that writes little-endian integers to guest physical memory.
struct Writer {
    at: u32,
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
    at: u32,
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
    fn u64(&mut self) -> u64 {
        // SAFETY: as in `Writer`.
        let v = unsafe { read_volatile(self.at as *const u64) };
        self.at += 8;
        v
    }
}

/// Read a device register.
fn reg_read(offset: u32) -> u32 {
    // SAFETY: `MMIO_BASE + offset` is inside the register window the host
    // registered for this VM. Every access is a naturally aligned 32-bit one,
    // which is what the transport accepts.
    unsafe { read_volatile((MMIO_BASE + offset) as *const u32) }
}

/// Write a device register.
fn reg_write(offset: u32, value: u32) {
    // SAFETY: as in `reg_read`.
    unsafe { write_volatile((MMIO_BASE + offset) as *mut u32, value) }
}

/// Why bring-up failed, in the words a reader would want.
#[derive(Clone, Copy)]
pub enum InitError {
    /// Nothing that answers like a virtio device is at the window.
    NoDevice,
    /// A virtio device, but not version 2.
    WrongVersion,
    /// A virtio device of some other kind.
    NotVsock,
    /// The device refused the features offered, so it cannot be driven.
    FeaturesRejected,
    /// The device's queues are smaller than this driver's rings.
    QueueTooSmall,
}

impl InitError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoDevice => "no virtio device at the vsock window",
            Self::WrongVersion => "the virtio device is not version 2",
            Self::NotVsock => "the virtio device is not a vsock device",
            Self::FeaturesRejected => "the device refused VIRTIO_F_VERSION_1",
            Self::QueueTooSmall => "the device's queues are too small for this driver",
        }
    }
}

/// A packet received from the host.
pub struct Packet {
    pub header: Header,
    /// Where the payload is, and how much of it there is. Left in the receive
    /// buffer rather than copied: there is no allocator here, and the caller
    /// reads it before the buffer is returned to the device.
    pub payload_at: u32,
    pub payload_len: u32,
}

/// The driver.
pub struct Vsock {
    /// This guest's context ID, read from device configuration space.
    cid: u64,
    /// Next slot to use in the receive available ring.
    rx_avail: u16,
    /// Last used-ring index this driver has consumed, for the receive queue.
    rx_used_seen: u16,
    /// Next slot to use in the transmit available ring.
    tx_avail: u16,
    /// Bytes received on the connection so far, reported back as `fwd_cnt` so
    /// the host knows its credit has been returned.
    fwd_cnt: u32,
}

impl Vsock {
    /// Bring the device up and post receive buffers.
    ///
    /// The status sequence is the specification's and the order matters: a
    /// device that sees `DRIVER_OK` before its queues are configured has no
    /// way to tell that from a driver that configured them badly.
    pub fn init() -> Result<Self, InitError> {
        if reg_read(reg::MAGIC) != VIRTIO_MAGIC {
            return Err(InitError::NoDevice);
        }
        if reg_read(reg::VERSION) != 2 {
            return Err(InitError::WrongVersion);
        }
        if reg_read(reg::DEVICE_ID) != DEVICE_ID_VSOCK {
            return Err(InitError::NotVsock);
        }

        // Reset, then acknowledge in the order the specification gives.
        reg_write(reg::STATUS, 0);
        reg_write(reg::STATUS, status::ACKNOWLEDGE);
        reg_write(reg::STATUS, status::ACKNOWLEDGE | status::DRIVER);

        // VIRTIO_F_VERSION_1 is bit 32, so it lives in the high bank and the
        // low bank is empty. Both banks are written: a device that only ever
        // sees the high one cannot tell an empty low bank from an unwritten
        // one.
        reg_write(reg::DEVICE_FEATURES_SEL, 1);
        let high = reg_read(reg::DEVICE_FEATURES);
        reg_write(reg::DRIVER_FEATURES_SEL, 0);
        reg_write(reg::DRIVER_FEATURES, 0);
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

        // The guest CID is the first eight bytes of configuration space.
        let cid = Reader {
            at: MMIO_BASE + reg::CONFIG,
        }
        .u64();

        let mut driver = Self {
            cid,
            rx_avail: 0,
            rx_used_seen: 0,
            tx_avail: 0,
            fwd_cnt: 0,
        };

        // Offer every receive buffer. Until this happens the device has
        // nowhere to put a packet, and its `flush_rx` quietly queues rather
        // than delivering -- which presents as a host that connects and a
        // guest that never hears about it.
        for slot in 0..QUEUE_SIZE {
            driver.post_rx(slot);
        }
        notify(RX_QUEUE);

        Ok(driver)
    }

    /// This guest's context ID, as the device reported it.
    pub fn cid(&self) -> u64 {
        self.cid
    }

    /// Hand receive buffer `slot` to the device.
    fn post_rx(&mut self, slot: u16) {
        let desc = mem::RX_DESC + u32::from(slot) * 16;
        let mut w = Writer { at: desc };
        w.u64(u64::from(mem::RX_BUFS + u32::from(slot) * BUF_SIZE));
        w.u32(BUF_SIZE);
        w.u16(DESC_F_WRITE);
        w.u16(0); // no chaining: one descriptor is one packet

        let ring = mem::RX_AVAIL + 4 + u32::from(self.rx_avail % QUEUE_SIZE) * 2;
        Writer { at: ring }.u16(slot);
        self.rx_avail = self.rx_avail.wrapping_add(1);
        // The index is published after the ring entry it refers to. On x86
        // stores are ordered, so this needs no explicit barrier -- but the
        // order of the two writes is still load-bearing and is why they are
        // not one function.
        Writer {
            at: mem::RX_AVAIL + 2,
        }
        .u16(self.rx_avail);
    }

    /// Take one packet from the device, or `None` if it has sent nothing.
    ///
    /// The payload stays in the receive buffer; the buffer is returned to the
    /// device by [`Self::release`], which the caller calls when it is done
    /// reading.
    pub fn recv(&mut self) -> Option<Packet> {
        let used_idx = Reader {
            at: mem::RX_USED + 2,
        }
        .u16();
        if used_idx == self.rx_used_seen {
            return None;
        }

        let entry = mem::RX_USED + 4 + u32::from(self.rx_used_seen % QUEUE_SIZE) * 8;
        let mut r = Reader { at: entry };
        let slot = r.u32() as u16;
        let written = r.u32();
        self.rx_used_seen = self.rx_used_seen.wrapping_add(1);

        let buf = mem::RX_BUFS + u32::from(slot) * BUF_SIZE;
        if written < HEADER_SIZE as u32 {
            // Not a packet. Give the buffer straight back rather than reading
            // a header that was never written.
            self.post_rx(slot);
            notify(RX_QUEUE);
            return None;
        }

        let header = Header::read_from(buf);
        let payload_len = header.len.min(written - HEADER_SIZE as u32);
        self.fwd_cnt = self.fwd_cnt.wrapping_add(payload_len);

        Some(Packet {
            header,
            payload_at: buf + HEADER_SIZE as u32,
            payload_len,
        })
    }

    /// Return a consumed receive buffer to the device.
    pub fn release(&mut self, packet: &Packet) {
        let slot = ((packet.payload_at - HEADER_SIZE as u32 - mem::RX_BUFS) / BUF_SIZE) as u16;
        self.post_rx(slot);
        notify(RX_QUEUE);
    }

    /// Send a packet to the host.
    ///
    /// `to` is the packet being answered: the reply's ports are its ports
    /// swapped, which is how a driver with no connection table at all still
    /// answers the right socket.
    pub fn reply(&mut self, to: &Header, op: u16, payload: &[u8]) {
        let header = Header {
            src_cid: self.cid,
            dst_cid: HOST_CID,
            src_port: to.dst_port,
            dst_port: to.src_port,
            len: payload.len() as u32,
            type_: TYPE_STREAM,
            op,
            flags: 0,
            // Every packet carries a credit report. `buf_alloc` is what this
            // driver can hold and `fwd_cnt` is what it has already consumed;
            // a host that never sees them stops sending once it believes the
            // window is full.
            buf_alloc: BUF_SIZE,
            fwd_cnt: self.fwd_cnt,
        };
        header.write_to(mem::TX_BUF);

        let base = mem::TX_BUF + HEADER_SIZE as u32;
        for (i, byte) in payload.iter().enumerate() {
            // SAFETY: the transmit buffer is 4 KiB of guest RAM this program
            // owns, and the caller's payload is bounded by the receive buffer
            // it came from.
            unsafe { write_volatile((base + i as u32) as *mut u8, *byte) };
        }

        let slot = self.tx_avail % QUEUE_SIZE;
        let desc = mem::TX_DESC + u32::from(slot) * 16;
        let mut w = Writer { at: desc };
        w.u64(u64::from(mem::TX_BUF));
        w.u32(HEADER_SIZE as u32 + payload.len() as u32);
        w.u16(0); // device-readable
        w.u16(0);

        let ring = mem::TX_AVAIL + 4 + u32::from(slot) * 2;
        Writer { at: ring }.u16(slot);
        self.tx_avail = self.tx_avail.wrapping_add(1);
        Writer {
            at: mem::TX_AVAIL + 2,
        }
        .u16(self.tx_avail);

        notify(TX_QUEUE);
    }

    /// Acknowledge a device interrupt, if one is pending.
    pub fn ack_interrupt(&self) {
        ack_interrupt_raw();
    }
}

/// Point one queue at its rings and mark it ready.
fn setup_queue(queue: u32, desc: u32, avail: u32, used: u32) -> Result<(), InitError> {
    reg_write(reg::QUEUE_SEL, queue);
    if reg_read(reg::QUEUE_NUM_MAX) < u32::from(QUEUE_SIZE) {
        return Err(InitError::QueueTooSmall);
    }
    reg_write(reg::QUEUE_NUM, u32::from(QUEUE_SIZE));

    // Addresses are 64-bit and the registers are 32. This guest is entirely
    // below 4 GiB, so every high half is zero -- written anyway, because a
    // stale high half from a previous configuration would point the device at
    // memory that does not exist.
    reg_write(reg::QUEUE_DESC_LOW, desc);
    reg_write(reg::QUEUE_DESC_HIGH, 0);
    reg_write(reg::QUEUE_DRIVER_LOW, avail);
    reg_write(reg::QUEUE_DRIVER_HIGH, 0);
    reg_write(reg::QUEUE_DEVICE_LOW, used);
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
/// The interrupt handler runs with no access to the `Vsock` value — it is
/// reached through the IDT, not called — and this is the only thing it needs to
/// do to the device. The virtio line is level-triggered and held until
/// `InterruptACK` is written, so a handler that skips this is re-entered
/// immediately and forever.
pub fn ack_interrupt_raw() {
    let pending = reg_read(reg::INTERRUPT_STATUS);
    if pending != 0 {
        reg_write(reg::INTERRUPT_ACK, pending);
    }
}
