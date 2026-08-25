//! Split virtqueues backed by real guest memory.
//!
//! # Why this exists alongside [`crate::devices::virtio::VirtQueue`]
//!
//! That type keeps the descriptor table, available ring and used ring in host
//! `Vec`s that a test populates directly. It models the shape of a virtqueue
//! but is never the queue a guest driver writes to, because nothing in it
//! reads guest memory. That is fine for the device models which only ever talk
//! to tests, and useless for a device that must serve a real driver.
//!
//! [`GuestQueue`] is the other half: it holds only the three guest physical
//! addresses the driver published, plus the two indices the device owns, and
//! reads every ring entry out of [`GuestMemory`] on demand. A driver running
//! in a booted guest and this type are talking about the same bytes.
//!
//! # Layout
//!
//! The split virtqueue of the virtio 1.x spec, little-endian throughout:
//!
//! ```text
//! desc[i]  : addr u64 | len u32 | flags u16 | next u16          (16 bytes)
//! avail    : flags u16 | idx u16 | ring[size] u16 | used_event u16
//! used     : flags u16 | idx u16 | ring[size]{id u32, len u32} | avail_event u16
//! ```
//!
//! # Defensive posture
//!
//! Every field read here is guest-controlled. A descriptor chain can be a
//! cycle, an index can point past the table, a length can claim more bytes
//! than the guest has memory for. None of that is an error in the ordinary
//! sense — it is what a buggy or hostile driver produces — so each is bounded
//! rather than trusted, and the device sees either a well-formed chain or an
//! error.

use crate::memory::GuestAddress;
use crate::{Error, GuestMemory, Result};

/// Size of one descriptor table entry, in bytes.
const DESC_SIZE: u64 = 16;

/// Largest queue size the spec permits.
pub const MAX_QUEUE_SIZE: u16 = 32768;

/// Ceiling on the number of descriptors in any one chain, used where the
/// negotiated queue size is not the relevant bound (indirect tables).
///
/// A chain longer than its table cannot be well-formed: it must have revisited
/// a descriptor, which means the guest built a cycle. Bounding the walk turns
/// "the device hangs" into "this chain is rejected".
const MAX_CHAIN_LEN: usize = MAX_QUEUE_SIZE as usize;

/// Ceiling on the total bytes one chain may describe (64 MiB).
///
/// The device allocates against this when it gathers a chain, so it is the
/// difference between a guest asking for a large transfer and a guest asking
/// the host to allocate 4 GiB per request.
pub const MAX_CHAIN_BYTES: usize = 64 * 1024 * 1024;

/// Descriptor flags.
pub mod desc_flags {
    /// The chain continues at `next`.
    pub const NEXT: u16 = 1;
    /// The device writes this buffer; the driver reads it.
    pub const WRITE: u16 = 2;
    /// The buffer is itself a table of descriptors.
    pub const INDIRECT: u16 = 4;
}

/// One descriptor, as the guest wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    pub addr: GuestAddress,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl Descriptor {
    /// Decode from the 16 bytes at the start of `bytes`.
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            addr: u64::from_le_bytes(bytes[0..8].try_into().expect("8 bytes")),
            len: u32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes")),
            flags: u16::from_le_bytes(bytes[12..14].try_into().expect("2 bytes")),
            next: u16::from_le_bytes(bytes[14..16].try_into().expect("2 bytes")),
        }
    }

    /// Whether the device writes this buffer, as opposed to reading it.
    pub fn is_write_only(&self) -> bool {
        self.flags & desc_flags::WRITE != 0
    }

    fn has_next(&self) -> bool {
        self.flags & desc_flags::NEXT != 0
    }
}

/// A validated descriptor chain the driver made available.
///
/// The buffers arrive split the way every virtio device wants them: the device
/// *reads* `readable` (the request the driver wrote) and *writes* `writable`
/// (the space the driver left for a reply). A device that mixes the two up
/// produces garbage that is hard to trace, so the split happens once, here.
#[derive(Debug, Clone)]
pub struct DescriptorChain {
    /// Index of the head descriptor — what goes in the used ring.
    pub head: u16,
    /// Device-readable buffers, in chain order.
    pub readable: Vec<(GuestAddress, u32)>,
    /// Device-writable buffers, in chain order.
    pub writable: Vec<(GuestAddress, u32)>,
}

impl DescriptorChain {
    /// Total bytes the device may read from this chain.
    pub fn readable_len(&self) -> usize {
        self.readable.iter().map(|(_, len)| *len as usize).sum()
    }

    /// Total bytes the device may write into this chain.
    pub fn writable_len(&self) -> usize {
        self.writable.iter().map(|(_, len)| *len as usize).sum()
    }

    /// Gather every device-readable buffer into one contiguous `Vec`.
    pub fn read_all(&self, mem: &GuestMemory) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.readable_len());
        for (addr, len) in &self.readable {
            out.extend_from_slice(&mem.read_bytes(*addr, *len as usize)?);
        }
        Ok(out)
    }

    /// Scatter `data` across the device-writable buffers, in chain order.
    ///
    /// Returns the number of bytes written, which is `data.len()` unless the
    /// driver left less room than the device had to say. A short write is the
    /// driver's to notice through the used-ring length, not a device error.
    pub fn write_all(&self, mem: &GuestMemory, data: &[u8]) -> Result<usize> {
        let mut written = 0usize;
        for (addr, len) in &self.writable {
            if written >= data.len() {
                break;
            }
            let take = (*len as usize).min(data.len() - written);
            mem.write_bytes(*addr, &data[written..written + take])?;
            written += take;
        }
        Ok(written)
    }
}

/// A split virtqueue whose rings live in guest memory.
///
/// The device owns `last_avail_idx` and `next_used_idx`; every other field is
/// published by the driver through the transport's queue registers.
#[derive(Debug, Clone)]
pub struct GuestQueue {
    /// Negotiated size, in descriptors. Zero until the driver sets it.
    size: u16,
    /// Largest size this device will accept.
    max_size: u16,
    desc_addr: GuestAddress,
    avail_addr: GuestAddress,
    used_addr: GuestAddress,
    /// Set by the driver's write to QueueReady.
    ready: bool,
    /// Next available-ring slot the device has not consumed.
    last_avail_idx: u16,
    /// Next used-ring slot the device will fill.
    next_used_idx: u16,
}

impl GuestQueue {
    /// Create a queue that will accept up to `max_size` descriptors.
    pub fn new(max_size: u16) -> Self {
        Self {
            size: 0,
            max_size: max_size.min(MAX_QUEUE_SIZE),
            desc_addr: 0,
            avail_addr: 0,
            used_addr: 0,
            ready: false,
            last_avail_idx: 0,
            next_used_idx: 0,
        }
    }

    /// Largest size this device advertises, for QueueNumMax.
    pub fn max_size(&self) -> u16 {
        self.max_size
    }

    /// Size the driver negotiated, or 0 if it has not yet.
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Set the negotiated size.
    ///
    /// Zero, values above the maximum, and non powers of two are refused: the
    /// ring index arithmetic below is only correct for a power-of-two size,
    /// and the spec requires one.
    pub fn set_size(&mut self, size: u16) {
        if size > 0 && size <= self.max_size && size.is_power_of_two() {
            self.size = size;
        }
    }

    pub fn set_desc_addr(&mut self, addr: GuestAddress) {
        self.desc_addr = addr;
    }

    pub fn set_avail_addr(&mut self, addr: GuestAddress) {
        self.avail_addr = addr;
    }

    pub fn set_used_addr(&mut self, addr: GuestAddress) {
        self.used_addr = addr;
    }

    pub fn desc_addr(&self) -> GuestAddress {
        self.desc_addr
    }

    pub fn avail_addr(&self) -> GuestAddress {
        self.avail_addr
    }

    pub fn used_addr(&self) -> GuestAddress {
        self.used_addr
    }

    /// Whether the driver has marked the queue ready *and* published every
    /// address the device needs.
    ///
    /// A driver that sets QueueReady without a descriptor table gets a queue
    /// the device declines to touch, rather than one that reads address zero.
    pub fn is_ready(&self) -> bool {
        self.ready
            && self.size > 0
            && self.desc_addr != 0
            && self.avail_addr != 0
            && self.used_addr != 0
    }

    /// Set the QueueReady bit as the driver wrote it.
    pub fn set_ready(&mut self, ready: bool) {
        self.ready = ready;
    }

    /// Return the queue to its post-reset state, keeping the advertised
    /// maximum.
    pub fn reset(&mut self) {
        *self = Self::new(self.max_size);
    }

    /// The driver's available-ring index.
    pub fn avail_idx(&self, mem: &GuestMemory) -> Result<u16> {
        let mut buf = [0u8; 2];
        mem.read_bytes_into(self.avail_addr + 2, &mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Whether the driver has made a chain available that the device has not
    /// yet consumed.
    pub fn has_available(&self, mem: &GuestMemory) -> Result<bool> {
        if !self.is_ready() {
            return Ok(false);
        }
        Ok(self.avail_idx(mem)? != self.last_avail_idx)
    }

    /// Take the next available chain, or `None` when the driver has published
    /// nothing new.
    pub fn pop(&mut self, mem: &GuestMemory) -> Result<Option<DescriptorChain>> {
        if !self.has_available(mem)? {
            return Ok(None);
        }

        // avail: flags u16, idx u16, then the ring.
        let slot = self.last_avail_idx % self.size;
        let ring_entry = self.avail_addr + 4 + u64::from(slot) * 2;
        let mut buf = [0u8; 2];
        mem.read_bytes_into(ring_entry, &mut buf)?;
        let head = u16::from_le_bytes(buf);

        // Consume the slot before validating the chain: one the device rejects
        // must not be re-read forever on every later call.
        self.last_avail_idx = self.last_avail_idx.wrapping_add(1);

        Ok(Some(self.walk_chain(mem, head)?))
    }

    /// Read one descriptor, refusing an index outside the negotiated table.
    fn read_desc(&self, mem: &GuestMemory, idx: u16) -> Result<Descriptor> {
        if idx >= self.size {
            return Err(Error::Device(format!(
                "virtqueue descriptor index {idx} is outside a table of {}",
                self.size
            )));
        }
        let mut buf = [0u8; DESC_SIZE as usize];
        mem.read_bytes_into(self.desc_addr + u64::from(idx) * DESC_SIZE, &mut buf)?;
        Ok(Descriptor::from_bytes(&buf))
    }

    /// Follow a chain from `head`, splitting it into readable and writable
    /// buffers.
    fn walk_chain(&self, mem: &GuestMemory, head: u16) -> Result<DescriptorChain> {
        let mut chain = DescriptorChain {
            head,
            readable: Vec::new(),
            writable: Vec::new(),
        };
        let mut total_bytes = 0usize;
        let mut visited = 0usize;
        let mut idx = head;

        loop {
            // A well-formed chain visits each descriptor at most once, so the
            // table size is the bound. Exceeding it means a cycle.
            visited += 1;
            if visited > self.size as usize {
                return Err(Error::Device(
                    "virtqueue descriptor chain does not terminate".to_string(),
                ));
            }

            let desc = self.read_desc(mem, idx)?;

            if desc.flags & desc_flags::INDIRECT != 0 {
                self.walk_indirect(mem, &desc, &mut chain, &mut total_bytes)?;
            } else {
                push_buffer(
                    &mut chain,
                    desc.addr,
                    desc.len,
                    desc.is_write_only(),
                    &mut total_bytes,
                )?;
            }

            if !desc.has_next() {
                break;
            }
            idx = desc.next;
        }

        Ok(chain)
    }

    /// Follow an indirect descriptor table.
    ///
    /// One level only, which is all the spec allows: an entry inside an
    /// indirect table may not itself be indirect.
    fn walk_indirect(
        &self,
        mem: &GuestMemory,
        desc: &Descriptor,
        chain: &mut DescriptorChain,
        total_bytes: &mut usize,
    ) -> Result<()> {
        if u64::from(desc.len) % DESC_SIZE != 0 {
            return Err(Error::Device(format!(
                "indirect descriptor table length {} is not a multiple of {DESC_SIZE}",
                desc.len
            )));
        }
        let count = (u64::from(desc.len) / DESC_SIZE) as usize;
        if count == 0 || count > MAX_CHAIN_LEN {
            return Err(Error::Device(format!(
                "indirect descriptor table of {count} entries is out of range"
            )));
        }

        let table = mem.read_bytes(desc.addr, desc.len as usize)?;
        let mut idx = 0usize;
        let mut visited = 0usize;

        loop {
            visited += 1;
            if visited > count {
                return Err(Error::Device(
                    "indirect descriptor chain does not terminate".to_string(),
                ));
            }
            if idx >= count {
                return Err(Error::Device(format!(
                    "indirect descriptor index {idx} is outside a table of {count}"
                )));
            }

            let entry = Descriptor::from_bytes(&table[idx * DESC_SIZE as usize..]);
            if entry.flags & desc_flags::INDIRECT != 0 {
                return Err(Error::Device(
                    "an indirect descriptor may not itself be indirect".to_string(),
                ));
            }
            push_buffer(
                chain,
                entry.addr,
                entry.len,
                entry.is_write_only(),
                total_bytes,
            )?;

            if !entry.has_next() {
                return Ok(());
            }
            idx = entry.next as usize;
        }
    }

    /// Publish a consumed chain in the used ring, `len` being the number of
    /// bytes the device wrote into it.
    pub fn add_used(&mut self, mem: &GuestMemory, head: u16, len: u32) -> Result<()> {
        if self.size == 0 {
            return Err(Error::Device(
                "cannot publish a used descriptor on a queue with no size".to_string(),
            ));
        }

        // used: flags u16, idx u16, then ring[size] of {id u32, len u32}.
        let slot = self.next_used_idx % self.size;
        let entry = self.used_addr + 4 + u64::from(slot) * 8;
        let mut bytes = [0u8; 8];
        bytes[0..4].copy_from_slice(&u32::from(head).to_le_bytes());
        bytes[4..8].copy_from_slice(&len.to_le_bytes());
        mem.write_bytes(entry, &bytes)?;

        // The index is published last: a driver that reads it must find the
        // entry it points past already written.
        self.next_used_idx = self.next_used_idx.wrapping_add(1);
        mem.write_bytes(self.used_addr + 2, &self.next_used_idx.to_le_bytes())?;
        Ok(())
    }

    /// The used index the device will write next.
    pub fn next_used_idx(&self) -> u16 {
        self.next_used_idx
    }
}

/// Add one buffer to the right half of a chain, enforcing the byte ceiling.
fn push_buffer(
    chain: &mut DescriptorChain,
    addr: GuestAddress,
    len: u32,
    write_only: bool,
    total_bytes: &mut usize,
) -> Result<()> {
    if len == 0 {
        return Ok(());
    }
    *total_bytes = total_bytes.saturating_add(len as usize);
    if *total_bytes > MAX_CHAIN_BYTES {
        return Err(Error::Device(format!(
            "virtqueue chain describes more than {MAX_CHAIN_BYTES} bytes"
        )));
    }
    if write_only {
        chain.writable.push((addr, len));
    } else {
        chain.readable.push((addr, len));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESC_BASE: GuestAddress = 0x1000;
    const AVAIL_BASE: GuestAddress = 0x2000;
    const USED_BASE: GuestAddress = 0x3000;
    const DATA_BASE: GuestAddress = 0x4000;
    const QUEUE_SIZE: u16 = 8;

    /// Guest memory with one region covering every address these tests use.
    fn memory() -> GuestMemory {
        let mem = GuestMemory::new(0x10000).expect("guest memory");
        mem.allocate_region(0x10000, false).expect("region");
        mem
    }

    /// A queue the driver has fully published.
    fn ready_queue() -> GuestQueue {
        let mut q = GuestQueue::new(256);
        q.set_size(QUEUE_SIZE);
        q.set_desc_addr(DESC_BASE);
        q.set_avail_addr(AVAIL_BASE);
        q.set_used_addr(USED_BASE);
        q.set_ready(true);
        q
    }

    fn write_desc(mem: &GuestMemory, idx: u16, desc: Descriptor) {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&desc.addr.to_le_bytes());
        bytes[8..12].copy_from_slice(&desc.len.to_le_bytes());
        bytes[12..14].copy_from_slice(&desc.flags.to_le_bytes());
        bytes[14..16].copy_from_slice(&desc.next.to_le_bytes());
        mem.write_bytes(DESC_BASE + u64::from(idx) * 16, &bytes)
            .expect("write descriptor");
    }

    /// Publish `head` in the available ring, as a driver would.
    fn make_available(mem: &GuestMemory, slot: u16, head: u16) {
        mem.write_bytes(AVAIL_BASE + 4 + u64::from(slot) * 2, &head.to_le_bytes())
            .expect("ring entry");
        mem.write_bytes(AVAIL_BASE + 2, &(slot + 1).to_le_bytes())
            .expect("avail idx");
    }

    #[test]
    fn a_chain_arrives_split_into_what_the_device_reads_and_writes() {
        let mem = memory();
        let mut q = ready_queue();

        // A request buffer followed by space for a reply — the shape of every
        // virtio request.
        write_desc(
            &mem,
            0,
            Descriptor {
                addr: DATA_BASE,
                len: 4,
                flags: desc_flags::NEXT,
                next: 1,
            },
        );
        write_desc(
            &mem,
            1,
            Descriptor {
                addr: DATA_BASE + 64,
                len: 8,
                flags: desc_flags::WRITE,
                next: 0,
            },
        );
        mem.write_bytes(DATA_BASE, b"ping").expect("payload");
        make_available(&mem, 0, 0);

        let chain = q.pop(&mem).expect("pop").expect("a chain was available");

        assert_eq!(chain.head, 0);
        assert_eq!(chain.readable, vec![(DATA_BASE, 4)]);
        assert_eq!(chain.writable, vec![(DATA_BASE + 64, 8)]);
        assert_eq!(chain.read_all(&mem).expect("read"), b"ping");

        let written = chain.write_all(&mem, b"pong").expect("write");
        assert_eq!(written, 4);
        assert_eq!(
            mem.read_bytes(DATA_BASE + 64, 4).expect("readback"),
            b"pong"
        );
    }

    #[test]
    fn pop_returns_none_when_the_driver_has_published_nothing_new() {
        let mem = memory();
        let mut q = ready_queue();

        assert!(q.pop(&mem).expect("pop").is_none());

        write_desc(
            &mem,
            0,
            Descriptor {
                addr: DATA_BASE,
                len: 1,
                flags: 0,
                next: 0,
            },
        );
        make_available(&mem, 0, 0);

        assert!(q.pop(&mem).expect("pop").is_some());
        // The same entry must not be handed out twice.
        assert!(q.pop(&mem).expect("pop").is_none());
    }

    #[test]
    fn add_used_publishes_the_head_and_length_the_driver_will_read() {
        let mem = memory();
        let mut q = ready_queue();

        q.add_used(&mem, 3, 12).expect("add_used");

        let entry = mem.read_bytes(USED_BASE + 4, 8).expect("used entry");
        assert_eq!(u32::from_le_bytes(entry[0..4].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(entry[4..8].try_into().unwrap()), 12);

        let idx = mem.read_bytes(USED_BASE + 2, 2).expect("used idx");
        assert_eq!(u16::from_le_bytes(idx.try_into().unwrap()), 1);
        assert_eq!(q.next_used_idx(), 1);
    }

    #[test]
    fn a_chain_that_cycles_is_rejected_rather_than_walked_forever() {
        let mem = memory();
        let mut q = ready_queue();

        // Descriptor 0 points at itself. Without a bound this is an infinite
        // loop inside the device, driven entirely by guest-written memory.
        write_desc(
            &mem,
            0,
            Descriptor {
                addr: DATA_BASE,
                len: 4,
                flags: desc_flags::NEXT,
                next: 0,
            },
        );
        make_available(&mem, 0, 0);

        let err = q.pop(&mem).expect_err("a cycle must be refused");
        assert!(
            err.to_string().contains("does not terminate"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_descriptor_index_outside_the_table_is_rejected() {
        let mem = memory();
        let mut q = ready_queue();

        make_available(&mem, 0, QUEUE_SIZE + 1);

        let err = q.pop(&mem).expect_err("out-of-range index must be refused");
        assert!(
            err.to_string().contains("outside a table"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn only_a_power_of_two_size_within_the_maximum_is_accepted() {
        let mut q = GuestQueue::new(256);

        q.set_size(0);
        assert_eq!(q.size(), 0, "zero is not a queue size");
        q.set_size(6);
        assert_eq!(q.size(), 0, "the ring arithmetic requires a power of two");
        q.set_size(512);
        assert_eq!(q.size(), 0, "above the advertised maximum");
        q.set_size(64);
        assert_eq!(q.size(), 64);
    }

    #[test]
    fn a_queue_is_not_ready_until_every_address_is_published() {
        let mut q = GuestQueue::new(256);
        q.set_size(QUEUE_SIZE);
        q.set_ready(true);

        // QueueReady alone would otherwise have the device reading GPA 0.
        assert!(!q.is_ready());
        q.set_desc_addr(DESC_BASE);
        assert!(!q.is_ready());
        q.set_avail_addr(AVAIL_BASE);
        assert!(!q.is_ready());
        q.set_used_addr(USED_BASE);
        assert!(q.is_ready());
    }

    #[test]
    fn an_indirect_table_is_followed_one_level() {
        let mem = memory();
        let mut q = ready_queue();

        // Two entries in a table the guest placed in its own memory.
        let table = DATA_BASE + 0x200;
        let mut entries = Vec::new();
        for (addr, len, flags, next) in [
            (DATA_BASE, 4u32, desc_flags::NEXT, 1u16),
            (DATA_BASE + 64, 8u32, desc_flags::WRITE, 0u16),
        ] {
            entries.extend_from_slice(&addr.to_le_bytes());
            entries.extend_from_slice(&len.to_le_bytes());
            entries.extend_from_slice(&flags.to_le_bytes());
            entries.extend_from_slice(&next.to_le_bytes());
        }
        mem.write_bytes(table, &entries).expect("indirect table");

        write_desc(
            &mem,
            0,
            Descriptor {
                addr: table,
                len: entries.len() as u32,
                flags: desc_flags::INDIRECT,
                next: 0,
            },
        );
        make_available(&mem, 0, 0);

        let chain = q.pop(&mem).expect("pop").expect("a chain was available");
        assert_eq!(chain.readable, vec![(DATA_BASE, 4)]);
        assert_eq!(chain.writable, vec![(DATA_BASE + 64, 8)]);
    }

    #[test]
    fn an_indirect_descriptor_may_not_nest() {
        let mem = memory();
        let mut q = ready_queue();

        let table = DATA_BASE + 0x200;
        let mut entry = Vec::new();
        entry.extend_from_slice(&DATA_BASE.to_le_bytes());
        entry.extend_from_slice(&4u32.to_le_bytes());
        entry.extend_from_slice(&desc_flags::INDIRECT.to_le_bytes());
        entry.extend_from_slice(&0u16.to_le_bytes());
        mem.write_bytes(table, &entry).expect("indirect table");

        write_desc(
            &mem,
            0,
            Descriptor {
                addr: table,
                len: 16,
                flags: desc_flags::INDIRECT,
                next: 0,
            },
        );
        make_available(&mem, 0, 0);

        let err = q.pop(&mem).expect_err("nested indirection must be refused");
        assert!(
            err.to_string().contains("may not itself be indirect"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_chain_claiming_more_than_the_byte_ceiling_is_refused() {
        let mem = memory();
        let mut q = ready_queue();

        // Two descriptors of 48 MiB each: individually plausible, together
        // past the ceiling. The device would otherwise allocate 96 MiB
        // because the guest asked it to.
        let big = 48 * 1024 * 1024u32;
        write_desc(
            &mem,
            0,
            Descriptor {
                addr: DATA_BASE,
                len: big,
                flags: desc_flags::NEXT,
                next: 1,
            },
        );
        write_desc(
            &mem,
            1,
            Descriptor {
                addr: DATA_BASE,
                len: big,
                flags: 0,
                next: 0,
            },
        );
        make_available(&mem, 0, 0);

        let err = q.pop(&mem).expect_err("the ceiling must bind");
        assert!(
            err.to_string().contains("more than"),
            "unexpected error: {err}"
        );
    }
}
