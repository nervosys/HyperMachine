//! virtio-vsock: a socket channel between the host and a guest agent.
//!
//! # What this is for
//!
//! `execute_script` has always been described as running a command in the VM,
//! and has always evaluated a script on the host against a read-only view of
//! one. Closing that gap needs a way for a host process to talk to a program
//! running inside the guest, over something that exists before the guest has
//! networking and does not depend on the guest having an address. vsock is
//! that channel: an address is a context ID and a port, the host is always CID
//! 2, and the guest driver is in every stock Linux kernel.
//!
//! This module is the device half. It speaks the wire protocol to the guest
//! driver over [`GuestQueue`]s and offers the host a small connection API —
//! [`VsockDevice::connect`], [`VsockDevice::send`], [`VsockDevice::recv`].
//!
//! # Wire protocol
//!
//! Every packet is a 44-byte header, little-endian, optionally followed by a
//! payload:
//!
//! ```text
//! src_cid u64 | dst_cid u64 | src_port u32 | dst_port u32 | len u32
//! type u16 | op u16 | flags u32 | buf_alloc u32 | fwd_cnt u32
//! ```
//!
//! Three virtqueues: 0 is rx (device to driver), 1 is tx (driver to device),
//! 2 carries device events. The device fills rx buffers the driver supplied
//! and consumes packets the driver put on tx.
//!
//! # Flow control
//!
//! Every packet carries the sender's `buf_alloc` (how much it can hold) and
//! `fwd_cnt` (how much it has handed to its application). A sender may have
//! `peer_buf_alloc - (tx_cnt - peer_fwd_cnt)` bytes outstanding and no more.
//! This is the mechanism that stops one side from making the other buffer
//! without limit, so it is enforced here rather than assumed: an agent
//! streaming output from a guest is exactly the case where an unbounded
//! buffer becomes a host memory leak.
//!
//! # What is verified
//!
//! The tests below drive this device the way a driver does — they lay rings
//! out in guest memory and publish descriptors — so the packet encoding,
//! connection state machine and credit arithmetic are exercised end to end
//! against real memory. No test here boots a kernel, so the claim they support
//! is "the device implements the protocol", not "a Linux guest connected".

use crate::devices::virtio_mmio::{VirtioMmioDevice, VIRTIO_F_VERSION_1};
use crate::devices::virtio_queue::GuestQueue;
use crate::{Error, GuestMemory, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Virtio device ID for a socket device.
pub const VIRTIO_ID_VSOCK: u32 = 19;

/// The host's context ID. Fixed by the spec.
pub const VSOCK_HOST_CID: u64 = 2;

/// Lowest context ID a guest may be given. 0 and 1 are reserved for the
/// hypervisor and for a local-only address, 2 is the host.
pub const VSOCK_GUEST_CID_MIN: u64 = 3;

/// Size of the packet header, in bytes.
pub const VSOCK_HEADER_SIZE: usize = 44;

/// Bytes the device advertises it can buffer per connection, and the ceiling
/// it enforces on what it will hold.
///
/// This is the credit window the guest sees. It is also the reason a guest
/// cannot make the host allocate without limit: past this, the device stops
/// granting credit and a well-behaved guest stops sending.
pub const VSOCK_RX_BUF_SIZE: u32 = 256 * 1024;

/// Ceiling on packets queued for delivery to the guest.
///
/// The rx queue drains only when the driver offers buffers. A guest that never
/// does — one that is not running, or has hung — must not cost the host
/// unbounded memory per host-side write.
const MAX_PENDING_TX: usize = 1024;

/// Queue indices.
const RX_QUEUE: usize = 0;
const TX_QUEUE: usize = 1;
const EVENT_QUEUE: usize = 2;

/// Descriptors per queue.
const QUEUE_MAX_SIZE: u16 = 256;

/// Stream socket. The only type this device supports.
pub const VSOCK_TYPE_STREAM: u16 = 1;

/// Packet operations.
///
/// The values are the virtio specification's, section 5.10.6.1. They were
/// previously each one lower, which left `REQUEST` and `RESPONSE` correct by
/// coincidence and everything else wrong -- so a connection handshake worked
/// perfectly and no data ever moved. What the host sent as `RW` arrived at a
/// Linux guest as `SHUTDOWN` with flags of zero, which that stack processes by
/// doing nothing at all: no data, no reply, no reset, and nothing in the guest's
/// log. Do not renumber these to match anything but the specification.
pub mod op {
    /// Not a packet. Reserved by the specification and never sent.
    pub const INVALID: u16 = 0;
    /// Connection request.
    pub const REQUEST: u16 = 1;
    /// Connection accepted.
    pub const RESPONSE: u16 = 2;
    /// Reset — the connection is gone.
    pub const RST: u16 = 3;
    /// One or both directions are closing.
    pub const SHUTDOWN: u16 = 4;
    /// Payload.
    pub const RW: u16 = 5;
    /// Unsolicited credit report.
    pub const CREDIT_UPDATE: u16 = 6;
    /// Ask the peer for a credit report.
    pub const CREDIT_REQUEST: u16 = 7;
}

/// Shutdown flags on an [`op::SHUTDOWN`] packet.
pub mod shutdown_flags {
    /// The sender will read no more.
    pub const RECEIVE: u32 = 1;
    /// The sender will write no more.
    pub const SEND: u32 = 2;
}

/// Host ports handed out by [`VsockDevice::connect_ephemeral`].
///
/// The same range Linux uses for ephemeral ports, for the same reason: high
/// enough not to collide with anything a caller would name deliberately.
const EPHEMERAL_HOST_PORTS: std::ops::Range<u32> = 49152..65536;

/// Somewhere to say that a packet is waiting for the guest.
///
/// Queueing a packet is not delivering it: a host-side packet sits in this
/// device until something moves it into a receive buffer the driver posted and
/// signals the used queue. Nothing in the device can do that -- it needs guest
/// memory and the transport -- so it says a packet is waiting and the VM does
/// the rest.
///
/// Without this, delivery happens only when a caller remembers to ask for it,
/// which is a rule every future caller has to know and the compiler cannot
/// enforce. The boot probe knew it; the published `vm.exec` path did not, and
/// queued requests no guest ever saw.
pub trait PendingWake: Send + Sync + std::fmt::Debug {
    /// A packet is queued for the guest. Must not block: this is called with
    /// the device lock held.
    fn wake(&self);
}

/// A vsock packet header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VsockHeader {
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

impl VsockHeader {
    /// Encode to the 44 bytes on the wire.
    pub fn to_bytes(&self) -> [u8; VSOCK_HEADER_SIZE] {
        let mut b = [0u8; VSOCK_HEADER_SIZE];
        b[0..8].copy_from_slice(&self.src_cid.to_le_bytes());
        b[8..16].copy_from_slice(&self.dst_cid.to_le_bytes());
        b[16..20].copy_from_slice(&self.src_port.to_le_bytes());
        b[20..24].copy_from_slice(&self.dst_port.to_le_bytes());
        b[24..28].copy_from_slice(&self.len.to_le_bytes());
        b[28..30].copy_from_slice(&self.type_.to_le_bytes());
        b[30..32].copy_from_slice(&self.op.to_le_bytes());
        b[32..36].copy_from_slice(&self.flags.to_le_bytes());
        b[36..40].copy_from_slice(&self.buf_alloc.to_le_bytes());
        b[40..44].copy_from_slice(&self.fwd_cnt.to_le_bytes());
        b
    }

    /// Decode from the wire, refusing anything shorter than a header.
    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        if b.len() < VSOCK_HEADER_SIZE {
            return Err(Error::Device(format!(
                "vsock packet of {} bytes is shorter than a {VSOCK_HEADER_SIZE}-byte header",
                b.len()
            )));
        }
        Ok(Self {
            src_cid: u64::from_le_bytes(b[0..8].try_into().expect("8 bytes")),
            dst_cid: u64::from_le_bytes(b[8..16].try_into().expect("8 bytes")),
            src_port: u32::from_le_bytes(b[16..20].try_into().expect("4 bytes")),
            dst_port: u32::from_le_bytes(b[20..24].try_into().expect("4 bytes")),
            len: u32::from_le_bytes(b[24..28].try_into().expect("4 bytes")),
            type_: u16::from_le_bytes(b[28..30].try_into().expect("2 bytes")),
            op: u16::from_le_bytes(b[30..32].try_into().expect("2 bytes")),
            flags: u32::from_le_bytes(b[32..36].try_into().expect("4 bytes")),
            buf_alloc: u32::from_le_bytes(b[36..40].try_into().expect("4 bytes")),
            fwd_cnt: u32::from_le_bytes(b[40..44].try_into().expect("4 bytes")),
        })
    }
}

/// A header and its payload.
#[derive(Debug, Clone)]
pub struct VsockPacket {
    pub header: VsockHeader,
    pub data: Vec<u8>,
}

impl VsockPacket {
    fn encoded(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(VSOCK_HEADER_SIZE + self.data.len());
        out.extend_from_slice(&self.header.to_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

/// Identifies one connection by the two ports it joins.
///
/// The context IDs are not part of the key: this device serves exactly one
/// guest, so the only pair that varies is the ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VsockConnectionId {
    /// Port on the host side.
    pub host_port: u32,
    /// Port on the guest side.
    pub guest_port: u32,
}

/// Where a connection is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VsockConnectionState {
    /// The host sent a REQUEST and is waiting for the guest to answer.
    Connecting,
    /// Both sides agreed; payload may flow.
    Established,
    /// One side sent SHUTDOWN; the connection is draining.
    Closing,
    /// Finished, by close or by reset. Bytes already received are still
    /// readable — a connection that closes after answering must not lose the
    /// answer.
    Closed,
}

/// One connection's state, including both directions of flow control.
#[derive(Debug)]
struct Connection {
    state: VsockConnectionState,
    /// Bytes received from the guest and not yet read by the host.
    rx: VecDeque<u8>,
    /// Bytes the host has read — what the guest sees as our `fwd_cnt`.
    fwd_cnt: u32,
    /// Bytes sent to the guest.
    tx_cnt: u32,
    /// The guest's advertised buffer size.
    peer_buf_alloc: u32,
    /// The guest's advertised forward count.
    peer_fwd_cnt: u32,
}

impl Connection {
    fn new(state: VsockConnectionState) -> Self {
        Self {
            state,
            rx: VecDeque::new(),
            fwd_cnt: 0,
            tx_cnt: 0,
            peer_buf_alloc: 0,
            peer_fwd_cnt: 0,
        }
    }

    /// Bytes this side may still send before the guest has to make room.
    fn credit(&self) -> u32 {
        let outstanding = self.tx_cnt.wrapping_sub(self.peer_fwd_cnt);
        self.peer_buf_alloc.saturating_sub(outstanding)
    }
}

/// A virtio socket device serving one guest.
#[derive(Debug)]
pub struct VsockDevice {
    guest_cid: u64,
    queues: Vec<GuestQueue>,
    acked_features: u64,
    connections: HashMap<VsockConnectionId, Connection>,
    /// Host ports on which a guest-initiated connection is accepted.
    listeners: HashSet<u32>,
    /// Guest-initiated connections that arrived on a listening port and the
    /// host has not yet taken.
    accepted: VecDeque<VsockConnectionId>,
    /// Packets waiting for the driver to offer an rx buffer.
    pending: VecDeque<VsockPacket>,
    /// Told when a packet is queued, so the VM can deliver it. `None` means
    /// nothing is listening and delivery waits for a caller to ask.
    pending_wake: Option<Arc<dyn PendingWake>>,
    /// Where [`Self::connect_ephemeral`] starts looking.
    ///
    /// It rotates rather than restarting at the bottom of the range. Forgetting
    /// a connection frees the port here, but the *guest* may still hold the
    /// other end for a moment -- an agent has the socket until it finishes
    /// serving it -- and a new request reusing that same pair lands on a
    /// four-tuple the guest thinks is already connected, so it goes unanswered.
    next_host_port: u32,
    /// Packets dropped because [`MAX_PENDING_TX`] was reached, so a caller can
    /// tell "the guest is not reading" from "nothing happened".
    dropped: u64,
}

impl VsockDevice {
    /// Create a device for a guest with context ID `guest_cid`.
    pub fn new(guest_cid: u64) -> Result<Self> {
        if guest_cid < VSOCK_GUEST_CID_MIN {
            return Err(Error::Device(format!(
                "guest CID {guest_cid} is reserved; the first usable CID is {VSOCK_GUEST_CID_MIN}"
            )));
        }
        Ok(Self {
            guest_cid,
            queues: vec![
                GuestQueue::new(QUEUE_MAX_SIZE),
                GuestQueue::new(QUEUE_MAX_SIZE),
                GuestQueue::new(QUEUE_MAX_SIZE),
            ],
            acked_features: 0,
            connections: HashMap::new(),
            listeners: HashSet::new(),
            accepted: VecDeque::new(),
            pending: VecDeque::new(),
            pending_wake: None,
            next_host_port: EPHEMERAL_HOST_PORTS.start,
            dropped: 0,
        })
    }

    /// The guest's context ID.
    pub fn guest_cid(&self) -> u64 {
        self.guest_cid
    }

    /// Open a connection from `host_port` to `guest_port`.
    ///
    /// This queues a REQUEST; the connection is [`VsockConnectionState::Connecting`]
    /// until the guest answers. A caller that needs an established connection
    /// must wait for the state to change rather than assume this succeeded —
    /// nothing here can know whether an agent is listening in the guest.
    pub fn connect(&mut self, host_port: u32, guest_port: u32) -> Result<VsockConnectionId> {
        let id = VsockConnectionId {
            host_port,
            guest_port,
        };
        if self.connections.contains_key(&id) {
            return Err(Error::Device(format!(
                "a connection from host port {host_port} to guest port {guest_port} already exists"
            )));
        }

        self.connections
            .insert(id, Connection::new(VsockConnectionState::Connecting));
        self.enqueue(id, op::REQUEST, 0, Vec::new());
        Ok(id)
    }

    /// Open a connection from a host port nothing else is using.
    ///
    /// A caller that names its own port has to pick one, and a caller with one
    /// job to do picks a constant -- which works exactly once. A closed
    /// connection keeps its port pair until it is forgotten, so the second call
    /// is refused for colliding with the first, and that is not a concurrency
    /// limit but a "you may do this once" limit that reads as the guest having
    /// gone away.
    ///
    /// # Errors
    ///
    /// [`Error::Device`] when every port in the ephemeral range is in use --
    /// which means connections are being opened and never forgotten, and is
    /// worth saying rather than papering over by reusing a live port.
    pub fn connect_ephemeral(&mut self, guest_port: u32) -> Result<VsockConnectionId> {
        let span = EPHEMERAL_HOST_PORTS.len() as u32;
        for offset in 0..span {
            let host_port = EPHEMERAL_HOST_PORTS.start
                + (self.next_host_port - EPHEMERAL_HOST_PORTS.start + offset) % span;
            let id = VsockConnectionId {
                host_port,
                guest_port,
            };
            if !self.connections.contains_key(&id) {
                self.next_host_port = EPHEMERAL_HOST_PORTS.start
                    + (host_port - EPHEMERAL_HOST_PORTS.start + 1) % span;
                return self.connect(host_port, guest_port);
            }
        }
        Err(Error::Device(format!(
            "no free host port for a connection to guest port {guest_port}: all {} in the              ephemeral range are in use",
            EPHEMERAL_HOST_PORTS.len()
        )))
    }

    /// Accept guest-initiated connections to `host_port`.
    pub fn listen(&mut self, host_port: u32) {
        self.listeners.insert(host_port);
    }

    /// Stop accepting guest-initiated connections to `host_port`.
    pub fn unlisten(&mut self, host_port: u32) {
        self.listeners.remove(&host_port);
    }

    /// Take the next guest-initiated connection, if one arrived.
    pub fn accept(&mut self) -> Option<VsockConnectionId> {
        self.accepted.pop_front()
    }

    /// State of `id`, or `None` if there is no such connection.
    pub fn state(&self, id: VsockConnectionId) -> Option<VsockConnectionState> {
        self.connections.get(&id).map(|c| c.state)
    }

    /// Every connection this device knows about, in a stable order.
    ///
    /// Sorted rather than in hash order: a caller listing connections twice
    /// should see the same sequence, and a hash-ordered list passes a single
    /// run by luck.
    pub fn connections(&self) -> Vec<(VsockConnectionId, VsockConnectionState)> {
        let mut out: Vec<_> = self
            .connections
            .iter()
            .map(|(id, conn)| (*id, conn.state))
            .collect();
        out.sort_by_key(|(id, _)| *id);
        out
    }

    /// Send `data` to the guest on an established connection.
    ///
    /// Returns how many bytes were accepted, which is less than `data.len()`
    /// when the guest has not granted enough credit — and zero when it has
    /// granted none. A caller must look at the return value rather than assume
    /// the whole buffer went.
    pub fn send(&mut self, id: VsockConnectionId, data: &[u8]) -> Result<usize> {
        let conn = self.connections.get(&id).ok_or_else(|| {
            Error::Device(format!(
                "no vsock connection {:?}",
                (id.host_port, id.guest_port)
            ))
        })?;
        if conn.state != VsockConnectionState::Established {
            return Err(Error::Device(format!(
                "vsock connection {}->{} is {:?}, not established",
                id.host_port, id.guest_port, conn.state
            )));
        }

        let credit = conn.credit() as usize;
        let take = data.len().min(credit);
        if take == 0 {
            return Ok(0);
        }

        self.enqueue(id, op::RW, 0, data[..take].to_vec());
        if let Some(conn) = self.connections.get_mut(&id) {
            conn.tx_cnt = conn.tx_cnt.wrapping_add(take as u32);
        }
        Ok(take)
    }

    /// Read what the guest has sent on `id`, consuming it.
    ///
    /// Consuming is what moves the flow-control window: the bytes are gone
    /// from the host buffer, so the guest is told it may send that much more.
    pub fn recv(&mut self, id: VsockConnectionId) -> Result<Vec<u8>> {
        let conn = self.connections.get_mut(&id).ok_or_else(|| {
            Error::Device(format!(
                "no vsock connection {:?}",
                (id.host_port, id.guest_port)
            ))
        })?;
        let data: Vec<u8> = conn.rx.drain(..).collect();
        if data.is_empty() {
            return Ok(data);
        }
        conn.fwd_cnt = conn.fwd_cnt.wrapping_add(data.len() as u32);
        let established = conn.state == VsockConnectionState::Established;

        // Tell the guest the window moved. Without this a guest that filled
        // our buffer never learns it may continue.
        if established {
            self.enqueue(id, op::CREDIT_UPDATE, 0, Vec::new());
        }
        Ok(data)
    }

    /// Read what the guest has sent without consuming it.
    ///
    /// The non-destructive counterpart to [`Self::recv`], for the same reason
    /// the serial console has one: a caller polling a connection must not eat
    /// the bytes another caller is waiting for.
    pub fn peek(&self, id: VsockConnectionId) -> Option<Vec<u8>> {
        self.connections
            .get(&id)
            .map(|conn| conn.rx.iter().copied().collect())
    }

    /// Close `id`, telling the guest both directions are done.
    pub fn close(&mut self, id: VsockConnectionId) -> Result<()> {
        if !self.connections.contains_key(&id) {
            return Err(Error::Device(format!(
                "no vsock connection {:?}",
                (id.host_port, id.guest_port)
            )));
        }
        self.enqueue(
            id,
            op::SHUTDOWN,
            shutdown_flags::SEND | shutdown_flags::RECEIVE,
            Vec::new(),
        );
        if let Some(conn) = self.connections.get_mut(&id) {
            conn.state = VsockConnectionState::Closing;
        }
        Ok(())
    }

    /// Forget a closed connection and whatever it still held.
    pub fn forget(&mut self, id: VsockConnectionId) {
        self.connections.remove(&id);
    }

    /// Packets dropped because the guest was not draining the rx queue.
    pub fn dropped_packets(&self) -> u64 {
        self.dropped
    }

    /// Whether the device has packets waiting for an rx buffer.
    /// Install the handle that is told when a packet is queued for the guest.
    ///
    /// The VM does this when it attaches the device. A device without one still
    /// works, but only for a caller that asks for delivery itself.
    pub fn set_pending_wake(&mut self, wake: Arc<dyn PendingWake>) {
        self.pending_wake = Some(wake);
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Queue a packet for the guest, stamping the current flow-control state.
    fn enqueue(&mut self, id: VsockConnectionId, operation: u16, flags: u32, data: Vec<u8>) {
        let (buf_alloc, fwd_cnt) = self
            .connections
            .get(&id)
            .map(|c| (VSOCK_RX_BUF_SIZE, c.fwd_cnt))
            .unwrap_or((VSOCK_RX_BUF_SIZE, 0));

        let packet = VsockPacket {
            header: VsockHeader {
                src_cid: VSOCK_HOST_CID,
                dst_cid: self.guest_cid,
                src_port: id.host_port,
                dst_port: id.guest_port,
                len: data.len() as u32,
                type_: VSOCK_TYPE_STREAM,
                op: operation,
                flags,
                buf_alloc,
                fwd_cnt,
            },
            data,
        };

        if self.pending.len() >= MAX_PENDING_TX {
            // Drop the oldest: a guest that has stopped reading is not going
            // to want the backlog, and the alternative is unbounded growth.
            self.pending.pop_front();
            self.dropped += 1;
        }
        self.pending.push_back(packet);

        // Say so now. A packet that nobody is told about is delivered when
        // something unrelated happens to ask, which is indistinguishable from
        // a guest that stopped answering.
        if let Some(wake) = &self.pending_wake {
            wake.wake();
        }
    }

    /// Publish anything the host has queued for the guest, now.
    ///
    /// The device only ever moved packets when the *guest* kicked the receive
    /// queue, which is the wrong trigger for a host-initiated message: the
    /// guest kicks when it posts buffers and then waits, so a connection
    /// request queued afterwards sat in `pending` with nobody to move it. The
    /// host side calls this after queueing, and the caller signals the used
    /// queue if this returns true.
    ///
    /// # Errors
    ///
    /// Propagates a queue error from reading the driver's descriptors.
    pub fn deliver_pending(&mut self, mem: &GuestMemory) -> Result<bool> {
        self.flush_rx(mem)
    }

    /// Move queued packets into rx buffers the driver supplied.
    ///
    /// Returns whether anything was published.
    fn flush_rx(&mut self, mem: &GuestMemory) -> Result<bool> {
        let mut published = false;

        while !self.pending.is_empty() {
            let Some(chain) = self.queues[RX_QUEUE].pop(mem)? else {
                break;
            };
            let packet = self.pending.pop_front().expect("checked non-empty");
            let bytes = packet.encoded();

            if chain.writable_len() < bytes.len() {
                // The driver offered a buffer too small for this packet.
                // Returning it unused and dropping the packet is better than
                // truncating: a partial header is not a packet, and a driver
                // reading one would desynchronise.
                tracing::warn!(
                    "vsock: rx buffer of {} bytes cannot hold a {}-byte packet",
                    chain.writable_len(),
                    bytes.len()
                );
                self.queues[RX_QUEUE].add_used(mem, chain.head, 0)?;
                self.dropped += 1;
                published = true;
                continue;
            }

            tracing::debug!(
                "vsock: to guest op={} len={} src={}:{} dst={}:{} type={} buf_alloc={} fwd_cnt={}                  encoded={} into {} writable bytes",
                packet.header.op,
                packet.header.len,
                packet.header.src_cid,
                packet.header.src_port,
                packet.header.dst_cid,
                packet.header.dst_port,
                packet.header.type_,
                packet.header.buf_alloc,
                packet.header.fwd_cnt,
                bytes.len(),
                chain.writable_len(),
            );

            let written = chain.write_all(mem, &bytes)?;
            self.queues[RX_QUEUE].add_used(mem, chain.head, written as u32)?;
            published = true;
        }

        Ok(published)
    }

    /// Consume packets the driver placed on the tx queue.
    fn drain_tx(&mut self, mem: &GuestMemory) -> Result<bool> {
        let mut consumed = false;

        while let Some(chain) = self.queues[TX_QUEUE].pop(mem)? {
            let bytes = chain.read_all(mem)?;
            self.queues[TX_QUEUE].add_used(mem, chain.head, 0)?;
            consumed = true;

            match VsockHeader::from_bytes(&bytes) {
                Ok(header) => {
                    let len = header.len as usize;
                    let available = bytes.len() - VSOCK_HEADER_SIZE;
                    if len > available {
                        tracing::warn!(
                            "vsock: packet claims {len} payload bytes but carries {available}"
                        );
                        continue;
                    }
                    let data = bytes[VSOCK_HEADER_SIZE..VSOCK_HEADER_SIZE + len].to_vec();
                    self.handle_packet(header, data);
                }
                Err(e) => tracing::warn!("vsock: {e}"),
            }
        }

        Ok(consumed)
    }

    /// Act on one packet from the guest.
    fn handle_packet(&mut self, header: VsockHeader, data: Vec<u8>) {
        if header.dst_cid != VSOCK_HOST_CID {
            tracing::warn!(
                "vsock: packet addressed to CID {}, which is not the host",
                header.dst_cid
            );
            return;
        }

        // From the host's side, the guest's source port is the guest port.
        let id = VsockConnectionId {
            host_port: header.dst_port,
            guest_port: header.src_port,
        };

        // Every packet carries a credit report; record it before anything
        // else, including on packets that then close the connection.
        if let Some(conn) = self.connections.get_mut(&id) {
            conn.peer_buf_alloc = header.buf_alloc;
            conn.peer_fwd_cnt = header.fwd_cnt;
        }

        // Which packets the guest actually sends is the difference between
        // "it never answered" and "it answered something we discarded".
        tracing::debug!(
            "vsock: from guest op={} len={} src_port={} dst_port={}",
            header.op,
            header.len,
            header.src_port,
            header.dst_port
        );

        match header.op {
            op::REQUEST => self.handle_request(id),
            op::RESPONSE => self.handle_response(id),
            op::RW => self.handle_payload(id, data),
            op::SHUTDOWN => self.handle_shutdown(id),
            op::RST => {
                if let Some(conn) = self.connections.get_mut(&id) {
                    conn.state = VsockConnectionState::Closed;
                }
            }
            op::CREDIT_UPDATE => {}
            op::CREDIT_REQUEST => {
                if self.connections.contains_key(&id) {
                    self.enqueue(id, op::CREDIT_UPDATE, 0, Vec::new());
                }
            }
            other => tracing::warn!("vsock: unknown operation {other}"),
        }
    }

    fn handle_request(&mut self, id: VsockConnectionId) {
        if !self.listeners.contains(&id.host_port) {
            // Nothing is listening. RST is the answer that lets the guest fail
            // immediately instead of waiting out a timeout.
            self.enqueue(id, op::RST, 0, Vec::new());
            return;
        }
        self.connections
            .insert(id, Connection::new(VsockConnectionState::Established));
        self.accepted.push_back(id);
        self.enqueue(id, op::RESPONSE, 0, Vec::new());
    }

    fn handle_response(&mut self, id: VsockConnectionId) {
        match self.connections.get_mut(&id) {
            Some(conn) if conn.state == VsockConnectionState::Connecting => {
                conn.state = VsockConnectionState::Established;
            }
            Some(_) => {}
            None => self.enqueue(id, op::RST, 0, Vec::new()),
        }
    }

    fn handle_payload(&mut self, id: VsockConnectionId, data: Vec<u8>) {
        let Some(conn) = self.connections.get_mut(&id) else {
            self.enqueue(id, op::RST, 0, Vec::new());
            return;
        };
        if conn.state != VsockConnectionState::Established {
            return;
        }

        // The guest should not exceed the credit it was granted. If it does,
        // keep what fits and drop the rest: the alternative is honouring an
        // overrun and letting the guest choose the host's memory use.
        let room = VSOCK_RX_BUF_SIZE as usize - conn.rx.len().min(VSOCK_RX_BUF_SIZE as usize);
        if data.len() > room {
            tracing::warn!(
                "vsock: guest sent {} bytes with room for {room}; the excess is dropped",
                data.len()
            );
        }
        conn.rx.extend(data.into_iter().take(room));
    }

    fn handle_shutdown(&mut self, id: VsockConnectionId) {
        let Some(conn) = self.connections.get_mut(&id) else {
            return;
        };
        conn.state = VsockConnectionState::Closed;
        // The spec has the peer answer a shutdown with a reset once it is
        // done, which is what frees the guest's socket.
        self.enqueue(id, op::RST, 0, Vec::new());
    }
}

impl VirtioMmioDevice for VsockDevice {
    fn device_id(&self) -> u32 {
        VIRTIO_ID_VSOCK
    }

    fn device_features(&self) -> u64 {
        VIRTIO_F_VERSION_1
    }

    fn ack_features(&mut self, features: u64) {
        self.acked_features = features;
    }

    fn queues(&mut self) -> &mut [GuestQueue] {
        &mut self.queues
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        // Configuration space is the 8-byte guest CID and nothing else.
        let cid = self.guest_cid.to_le_bytes();
        for (i, byte) in data.iter_mut().enumerate() {
            let idx = offset as usize + i;
            *byte = cid.get(idx).copied().unwrap_or(0);
        }
    }

    fn write_config(&mut self, _offset: u64, _data: &[u8]) {
        // The guest CID is set by the host. A driver writing it is either
        // confused or hostile; either way the value does not change.
        tracing::debug!("vsock: ignoring a driver write to read-only config space");
    }

    fn notify(&mut self, queue: u16, mem: &GuestMemory) -> Result<bool> {
        match queue as usize {
            // A kick on either data queue can create work on the other: a
            // request arriving on tx produces a response that needs an rx
            // buffer, and a freshly offered rx buffer may be what a queued
            // response was waiting for. Servicing both is what keeps a
            // request/response exchange from stalling for a kick that never
            // comes.
            TX_QUEUE => {
                let consumed = self.drain_tx(mem)?;
                let published = self.flush_rx(mem)?;
                Ok(consumed || published)
            }
            RX_QUEUE => self.flush_rx(mem),
            EVENT_QUEUE => Ok(false),
            other => {
                tracing::warn!("vsock: notify for queue {other}, which does not exist");
                Ok(false)
            }
        }
    }

    fn reset(&mut self) {
        for queue in &mut self.queues {
            queue.reset();
        }
        self.acked_features = 0;
        self.connections.clear();
        self.accepted.clear();
        self.pending.clear();
        // Listeners survive a reset: they are host-side intent, not
        // negotiated state, and a guest rebooting should find the same ports
        // open that it found the first time.
    }
}

#[cfg(test)]
mod tests {

    /// The wire numbers, written out rather than derived from the constants.
    ///
    /// A test that says `op::RW == op::RW` passes against any table, which is
    /// how a table one short of the specification survived: the device agreed
    /// with itself, and only a real guest disagreed. These are the values in
    /// section 5.10.6.1, typed in from the specification.
    #[test]
    fn the_operation_numbers_are_the_ones_on_the_wire() {
        assert_eq!(op::INVALID, 0);
        assert_eq!(op::REQUEST, 1);
        assert_eq!(op::RESPONSE, 2);
        assert_eq!(op::RST, 3);
        assert_eq!(op::SHUTDOWN, 4);
        assert_eq!(op::RW, 5);
        assert_eq!(op::CREDIT_UPDATE, 6);
        assert_eq!(op::CREDIT_REQUEST, 7);
    }
    use super::*;
    use crate::devices::virtio_queue::desc_flags;

    // One ring set per data queue, plus buffers, all inside a single region.
    const RX_DESC: u64 = 0x1000;
    const RX_AVAIL: u64 = 0x2000;
    const RX_USED: u64 = 0x3000;
    const TX_DESC: u64 = 0x4000;
    const TX_AVAIL: u64 = 0x5000;
    const TX_USED: u64 = 0x6000;
    const RX_BUFS: u64 = 0x10000;
    const TX_BUFS: u64 = 0x18000;
    const BUF_SIZE: u32 = 0x800;
    const RING_SIZE: u16 = 8;

    const HOST_PORT: u32 = 1024;
    const GUEST_PORT: u32 = 5555;
    const GUEST_CID: u64 = 3;

    /// Stands in for the guest driver: owns the rings, offers rx buffers and
    /// submits tx packets exactly as a driver does, through guest memory.
    struct Driver {
        mem: GuestMemory,
        rx_avail: u16,
        tx_avail: u16,
        rx_used_seen: u16,
    }

    impl Driver {
        fn new(device: &mut VsockDevice) -> Self {
            let mem = GuestMemory::new(0x20000).expect("guest memory");
            mem.allocate_region(0x20000, false).expect("region");

            let queues = device.queues();
            for (idx, (desc, avail, used)) in
                [(RX_DESC, RX_AVAIL, RX_USED), (TX_DESC, TX_AVAIL, TX_USED)]
                    .into_iter()
                    .enumerate()
            {
                let q = &mut queues[idx];
                q.set_size(RING_SIZE);
                q.set_desc_addr(desc);
                q.set_avail_addr(avail);
                q.set_used_addr(used);
                q.set_ready(true);
            }

            Self {
                mem,
                rx_avail: 0,
                tx_avail: 0,
                rx_used_seen: 0,
            }
        }

        fn write_desc(&self, table: u64, idx: u16, addr: u64, len: u32, flags: u16) {
            let mut bytes = [0u8; 16];
            bytes[0..8].copy_from_slice(&addr.to_le_bytes());
            bytes[8..12].copy_from_slice(&len.to_le_bytes());
            bytes[12..14].copy_from_slice(&flags.to_le_bytes());
            self.mem
                .write_bytes(table + u64::from(idx) * 16, &bytes)
                .expect("descriptor");
        }

        fn publish(&self, ring: u64, slot: u16, head: u16, next_idx: u16) {
            self.mem
                .write_bytes(ring + 4 + u64::from(slot) * 2, &head.to_le_bytes())
                .expect("avail ring");
            self.mem
                .write_bytes(ring + 2, &next_idx.to_le_bytes())
                .expect("avail idx");
        }

        /// Offer one empty buffer for the device to write a packet into.
        fn offer_rx_buffer(&mut self) {
            let slot = self.rx_avail % RING_SIZE;
            let addr = RX_BUFS + u64::from(slot) * u64::from(BUF_SIZE);
            self.write_desc(RX_DESC, slot, addr, BUF_SIZE, desc_flags::WRITE);
            self.rx_avail += 1;
            self.publish(RX_AVAIL, slot, slot, self.rx_avail);
        }

        /// Read the next packet the device published on the rx queue.
        fn next_rx_packet(&mut self) -> Option<VsockPacket> {
            let used_idx = u16::from_le_bytes(
                self.mem
                    .read_bytes(RX_USED + 2, 2)
                    .expect("used idx")
                    .try_into()
                    .unwrap(),
            );
            if used_idx == self.rx_used_seen {
                return None;
            }
            let slot = self.rx_used_seen % RING_SIZE;
            let entry = self
                .mem
                .read_bytes(RX_USED + 4 + u64::from(slot) * 8, 8)
                .expect("used entry");
            let head = u32::from_le_bytes(entry[0..4].try_into().unwrap()) as u16;
            let len = u32::from_le_bytes(entry[4..8].try_into().unwrap()) as usize;
            self.rx_used_seen += 1;

            if len == 0 {
                return None;
            }
            let addr = RX_BUFS + u64::from(head) * u64::from(BUF_SIZE);
            let bytes = self.mem.read_bytes(addr, len).expect("packet");
            let header = VsockHeader::from_bytes(&bytes).expect("header");
            Some(VsockPacket {
                data: bytes[VSOCK_HEADER_SIZE..].to_vec(),
                header,
            })
        }

        /// Drain every packet currently published on the rx queue.
        fn rx_packets(&mut self) -> Vec<VsockPacket> {
            let mut out = Vec::new();
            while let Some(p) = self.next_rx_packet() {
                out.push(p);
            }
            out
        }

        /// Submit a packet from the guest to the device.
        fn submit_tx(&mut self, header: VsockHeader, data: &[u8]) {
            let mut bytes = header.to_bytes().to_vec();
            bytes.extend_from_slice(data);
            let slot = self.tx_avail % RING_SIZE;
            let addr = TX_BUFS + u64::from(slot) * u64::from(BUF_SIZE);
            self.mem.write_bytes(addr, &bytes).expect("packet bytes");
            self.write_desc(TX_DESC, slot, addr, bytes.len() as u32, 0);
            self.tx_avail += 1;
            self.publish(TX_AVAIL, slot, slot, self.tx_avail);
        }
    }

    /// A header for a packet the guest sends to the host.
    fn guest_header(op: u16, len: u32, buf_alloc: u32, fwd_cnt: u32) -> VsockHeader {
        VsockHeader {
            src_cid: GUEST_CID,
            dst_cid: VSOCK_HOST_CID,
            src_port: GUEST_PORT,
            dst_port: HOST_PORT,
            len,
            type_: VSOCK_TYPE_STREAM,
            op,
            flags: 0,
            buf_alloc,
            fwd_cnt,
        }
    }

    /// A device with an established connection, and the driver behind it.
    fn established() -> (VsockDevice, Driver, VsockConnectionId) {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);
        let id = device.connect(HOST_PORT, GUEST_PORT).expect("connect");

        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("rx notify");
        driver.rx_packets();

        driver.submit_tx(guest_header(op::RESPONSE, 0, 8192, 0), &[]);
        device.notify(1, &driver.mem).expect("tx notify");

        (device, driver, id)
    }

    #[test]
    fn a_reserved_context_id_is_refused() {
        // 0, 1 and 2 name the hypervisor, a local address and the host. A
        // guest given one of them would be unaddressable or would collide
        // with the host.
        for cid in [0, 1, 2] {
            assert!(
                VsockDevice::new(cid).is_err(),
                "CID {cid} must not be usable as a guest CID"
            );
        }
        assert!(VsockDevice::new(3).is_ok());
    }

    #[test]
    fn config_space_reports_the_guest_context_id() {
        let device = VsockDevice::new(42).expect("device");
        let mut data = [0u8; 8];
        device.read_config(0, &mut data);
        assert_eq!(u64::from_le_bytes(data), 42);
    }

    #[test]
    fn a_read_past_config_space_reads_as_zero_rather_than_panicking() {
        let device = VsockDevice::new(GUEST_CID).expect("device");
        let mut data = [0xffu8; 4];
        device.read_config(64, &mut data);
        assert_eq!(data, [0, 0, 0, 0]);
    }

    #[test]
    fn connecting_puts_a_request_on_the_rx_queue() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);

        let id = device.connect(HOST_PORT, GUEST_PORT).expect("connect");
        assert_eq!(device.state(id), Some(VsockConnectionState::Connecting));

        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("notify");

        let packets = driver.rx_packets();
        assert_eq!(packets.len(), 1, "exactly one REQUEST should be queued");
        let header = packets[0].header;
        assert_eq!(header.op, op::REQUEST);
        assert_eq!(header.src_cid, VSOCK_HOST_CID);
        assert_eq!(header.dst_cid, GUEST_CID);
        assert_eq!(header.src_port, HOST_PORT);
        assert_eq!(header.dst_port, GUEST_PORT);
        assert_eq!(header.buf_alloc, VSOCK_RX_BUF_SIZE);
    }

    #[test]
    fn a_connection_is_not_established_until_the_guest_answers() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);
        let id = device.connect(HOST_PORT, GUEST_PORT).expect("connect");

        // Sending the REQUEST is not the guest agreeing to anything. Nothing
        // on the host can know whether an agent is listening in there.
        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("notify");
        assert_eq!(device.state(id), Some(VsockConnectionState::Connecting));
        assert!(
            device.send(id, b"early").is_err(),
            "payload before RESPONSE must be refused"
        );

        driver.submit_tx(guest_header(op::RESPONSE, 0, 8192, 0), &[]);
        device.notify(1, &driver.mem).expect("notify");
        assert_eq!(device.state(id), Some(VsockConnectionState::Established));
    }

    #[test]
    fn payload_from_the_guest_reaches_the_host() {
        let (mut device, mut driver, id) = established();

        driver.submit_tx(guest_header(op::RW, 5, 8192, 0), b"hello");
        device.notify(1, &driver.mem).expect("notify");

        assert_eq!(device.peek(id).as_deref(), Some(&b"hello"[..]));
        assert_eq!(device.recv(id).expect("recv"), b"hello");
        assert_eq!(
            device.recv(id).expect("recv"),
            Vec::<u8>::new(),
            "recv consumes"
        );
    }

    #[test]
    fn peek_does_not_consume_what_recv_would_return() {
        let (mut device, mut driver, id) = established();

        driver.submit_tx(guest_header(op::RW, 3, 8192, 0), b"abc");
        device.notify(1, &driver.mem).expect("notify");

        // Two callers polling the same connection must both see the bytes;
        // a status check must not eat the answer another caller is waiting on.
        assert_eq!(device.peek(id).as_deref(), Some(&b"abc"[..]));
        assert_eq!(device.peek(id).as_deref(), Some(&b"abc"[..]));
        assert_eq!(device.recv(id).expect("recv"), b"abc");
    }

    #[test]
    fn payload_from_the_host_reaches_the_guest() {
        let (mut device, mut driver, id) = established();

        assert_eq!(device.send(id, b"run").expect("send"), 3);
        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("notify");

        let packets = driver.rx_packets();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].header.op, op::RW);
        assert_eq!(packets[0].header.len, 3);
        assert_eq!(packets[0].data, b"run");
    }

    #[test]
    fn a_send_is_bounded_by_the_credit_the_guest_granted() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);
        let id = device.connect(HOST_PORT, GUEST_PORT).expect("connect");

        // The guest says it can hold four bytes. Sending ten would make it
        // buffer six it never agreed to.
        driver.submit_tx(guest_header(op::RESPONSE, 0, 4, 0), &[]);
        device.notify(1, &driver.mem).expect("notify");

        assert_eq!(device.send(id, b"0123456789").expect("send"), 4);
        assert_eq!(
            device.send(id, b"more").expect("send"),
            0,
            "the window is used up until the guest forwards"
        );

        // The guest reports it consumed the four bytes; the window reopens.
        driver.submit_tx(guest_header(op::CREDIT_UPDATE, 0, 4, 4), &[]);
        device.notify(1, &driver.mem).expect("notify");
        assert_eq!(device.send(id, b"more").expect("send"), 4);
    }

    #[test]
    fn reading_from_the_host_tells_the_guest_the_window_moved() {
        let (mut device, mut driver, id) = established();

        driver.submit_tx(guest_header(op::RW, 4, 8192, 0), b"data");
        device.notify(1, &driver.mem).expect("notify");
        driver.rx_packets();

        device.recv(id).expect("recv");
        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("notify");

        let packets = driver.rx_packets();
        let update = packets
            .iter()
            .find(|p| p.header.op == op::CREDIT_UPDATE)
            .expect("a credit update should follow a read");
        assert_eq!(
            update.header.fwd_cnt, 4,
            "the guest must be told how much the host consumed"
        );
    }

    #[test]
    fn a_guest_request_to_a_port_nothing_listens_on_is_reset() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);

        driver.submit_tx(guest_header(op::REQUEST, 0, 8192, 0), &[]);
        device.notify(1, &driver.mem).expect("notify");
        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("notify");

        let packets = driver.rx_packets();
        assert_eq!(packets.len(), 1);
        assert_eq!(
            packets[0].header.op,
            op::RST,
            "a guest connecting to a closed port should fail at once, not time out"
        );
        assert!(device.connections().is_empty());
    }

    #[test]
    fn a_guest_request_to_a_listening_port_is_accepted() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);
        device.listen(HOST_PORT);

        driver.submit_tx(guest_header(op::REQUEST, 0, 8192, 0), &[]);
        device.notify(1, &driver.mem).expect("notify");
        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("notify");

        let packets = driver.rx_packets();
        assert_eq!(packets[0].header.op, op::RESPONSE);

        let id = device.accept().expect("an accepted connection");
        assert_eq!(id.host_port, HOST_PORT);
        assert_eq!(id.guest_port, GUEST_PORT);
        assert_eq!(device.state(id), Some(VsockConnectionState::Established));
    }

    #[test]
    fn a_guest_shutdown_closes_the_connection_but_keeps_what_it_said() {
        let (mut device, mut driver, id) = established();

        driver.submit_tx(guest_header(op::RW, 6, 8192, 0), b"answer");
        driver.submit_tx(guest_header(op::SHUTDOWN, 0, 8192, 0), &[]);
        device.notify(1, &driver.mem).expect("notify");

        assert_eq!(device.state(id), Some(VsockConnectionState::Closed));
        // A guest that answers and then hangs up has still answered. Dropping
        // the payload on close would lose exactly the reply the host asked for.
        assert_eq!(device.recv(id).expect("recv"), b"answer");
    }

    #[test]
    fn a_second_connection_on_the_same_ports_is_refused() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let _driver = Driver::new(&mut device);

        device.connect(HOST_PORT, GUEST_PORT).expect("connect");
        assert!(
            device.connect(HOST_PORT, GUEST_PORT).is_err(),
            "the port pair is the connection identity; two would alias"
        );
        assert!(device.connect(HOST_PORT + 1, GUEST_PORT).is_ok());
    }

    #[test]
    fn connections_are_listed_in_a_stable_order() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let _driver = Driver::new(&mut device);
        for port in [2000, 1000, 3000] {
            device.connect(port, GUEST_PORT).expect("connect");
        }

        // Hash order would pass a single run by luck.
        let ports: Vec<u32> = device
            .connections()
            .iter()
            .map(|(id, _)| id.host_port)
            .collect();
        assert_eq!(ports, vec![1000, 2000, 3000]);
        assert_eq!(ports, {
            let again: Vec<u32> = device
                .connections()
                .iter()
                .map(|(id, _)| id.host_port)
                .collect();
            again
        });
    }

    #[test]
    fn queued_packets_for_an_unresponsive_guest_are_bounded() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);
        let id = device.connect(HOST_PORT, GUEST_PORT).expect("connect");
        driver.submit_tx(guest_header(op::RESPONSE, 0, u32::MAX, 0), &[]);
        device.notify(1, &driver.mem).expect("notify");

        // The guest never offers an rx buffer. Without a ceiling every host
        // write would live in host memory forever.
        for _ in 0..MAX_PENDING_TX + 50 {
            device.send(id, b"x").expect("send");
        }

        assert!(device.dropped_packets() >= 50);
        driver.offer_rx_buffer();
        device.notify(0, &driver.mem).expect("notify");
        assert!(driver.next_rx_packet().is_some());
    }

    #[test]
    fn an_rx_buffer_too_small_for_a_packet_is_returned_rather_than_half_filled() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let mut driver = Driver::new(&mut device);
        device.connect(HOST_PORT, GUEST_PORT).expect("connect");

        // Sixteen bytes cannot hold a 44-byte header. A truncated header is
        // not a short packet, it is a driver reading garbage from then on.
        driver.write_desc(RX_DESC, 0, RX_BUFS, 16, desc_flags::WRITE);
        driver.rx_avail += 1;
        driver.publish(RX_AVAIL, 0, 0, driver.rx_avail);
        device.notify(0, &driver.mem).expect("notify");

        assert_eq!(device.dropped_packets(), 1);
        let used_len = u32::from_le_bytes(
            driver
                .mem
                .read_bytes(RX_USED + 8, 4)
                .expect("used entry")
                .try_into()
                .unwrap(),
        );
        assert_eq!(used_len, 0, "the buffer must come back marked unwritten");
    }

    #[test]
    fn a_packet_claiming_more_payload_than_it_carries_is_dropped() {
        let (mut device, mut driver, id) = established();

        // len says 100; the descriptor carries 4. Trusting the header would
        // read past the buffer.
        driver.submit_tx(guest_header(op::RW, 100, 8192, 0), b"tiny");
        device.notify(1, &driver.mem).expect("notify");

        assert_eq!(device.peek(id).as_deref(), Some(&[][..]));
    }

    #[test]
    fn a_reset_clears_connections_but_keeps_host_side_listeners() {
        let mut device = VsockDevice::new(GUEST_CID).expect("device");
        let _driver = Driver::new(&mut device);
        device.listen(HOST_PORT);
        let id = device.connect(HOST_PORT, GUEST_PORT).expect("connect");

        device.reset();

        assert_eq!(device.state(id), None, "negotiated state is gone");
        assert!(!device.has_pending());
        // A guest that reboots should find the same ports open it found the
        // first time: a listener is host-side intent, not negotiated state.
        assert!(device.listeners.contains(&HOST_PORT));
    }

    /// Successive connections must not reuse a port pair, even when the
    /// previous one has been forgotten. The host frees the port immediately;
    /// the guest may still hold the other end for a moment, and a request that
    /// reuses the pair lands on a four-tuple the guest thinks is connected and
    /// goes unanswered. This was a real failure: ping worked, and the exec
    /// after it hung until the caller's timeout.
    #[test]
    fn successive_ephemeral_connections_do_not_reuse_a_port() {
        let mut device = VsockDevice::new(3).expect("device");

        let first = device.connect_ephemeral(1024).expect("first connection");
        device.forget(first);
        let second = device.connect_ephemeral(1024).expect("second connection");

        assert_ne!(
            first.host_port, second.host_port,
            "a forgotten port must not be handed straight back out"
        );
    }

    /// A caller that opens several at once gets several ports, which is what
    /// makes concurrent use possible at all: a constant host port means the
    /// second caller collides with the first.
    #[test]
    fn concurrent_ephemeral_connections_each_get_their_own_port() {
        let mut device = VsockDevice::new(3).expect("device");

        let ports: Vec<u32> = (0..8)
            .map(|_| {
                device
                    .connect_ephemeral(1024)
                    .expect("connection")
                    .host_port
            })
            .collect();

        let unique: std::collections::HashSet<_> = ports.iter().collect();
        assert_eq!(
            unique.len(),
            ports.len(),
            "ports handed out twice: {ports:?}"
        );
    }
}
