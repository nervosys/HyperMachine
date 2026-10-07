//! A virtio-blk device a guest can actually drive, backed by a file.
//!
//! # Why this exists when `VirtioBlock` already does
//!
//! For the reason [`virtio_net_mmio`](super::virtio_net_mmio) gives about the
//! older network types: [`VirtioBlock`](super::virtio_blk::VirtioBlock) parses
//! requests but has no virtqueues in guest memory, so a guest driver has
//! nothing to talk to. It also keeps the whole disk in a `Vec<u8>`, which is
//! the opposite of what a disk is for: what a guest writes must still be there
//! after the VM is gone, and must not cost host RAM while it runs.
//!
//! This device is the shape of [`VirtioNetMmio`](super::virtio_net_mmio::VirtioNetMmio)
//! — a [`VirtioMmioDevice`] whose one queue is a [`GuestQueue`] — with a raw
//! image file behind it.
//!
//! # What it does and does not do
//!
//! One request queue. Reads, writes, flushes and the device-ID request are
//! served synchronously on the kick, from the vCPU thread that made it: the
//! request is complete, and its status written, before the guest resumes.
//! There is no I/O thread and no batching. That is slower than a device with
//! its own thread, and it is also the simplest device that cannot reorder a
//! flush past a write.
//!
//! Every write reaches the file before its status does, and a flush is an
//! `fsync` of the file, so a guest that issued a flush and saw it complete has
//! its data on the host's disk. Write-back caching is not offered.
//!
//! Discard and write-zeroes are not offered either. A guest told a device can
//! do them will rely on it, and a raw file would have to punch holes in a way
//! that differs by platform.

use std::fs::File;
use std::path::Path;

use super::virtio_mmio::{VirtioMmioDevice, VIRTIO_F_VERSION_1};
use super::virtio_queue::{DescriptorChain, GuestQueue};
use crate::error::{Error, Result};
use crate::memory::GuestMemory;

/// Virtio device ID for a block device.
///
/// The device *type* virtio assigns, which is 2 — the same number as
/// [`VIRTIO_BLK_DEVICE_ID`](super::virtio_blk::VIRTIO_BLK_DEVICE_ID).
pub const VIRTIO_ID_BLOCK: u32 = 2;

/// Bytes per sector. Fixed by the virtio specification whatever the backing
/// file's own block size.
pub const SECTOR_SIZE: u64 = 512;

/// Queue depth offered to the driver.
const QUEUE_SIZE: u16 = 128;

/// Segments a driver may put in one request.
///
/// Two descriptors of every chain are the header and the status byte, so a
/// request can carry at most the rest of the queue.
const SEG_MAX: u32 = QUEUE_SIZE as u32 - 2;

/// Bytes of `virtio_blk_req` in front of every request: type, reserved, sector.
const REQ_HDR_LEN: usize = 16;

/// Length of the reply to a device-ID request.
const ID_LEN: usize = 20;

// Feature bits this device can offer.
const VIRTIO_BLK_F_SEG_MAX: u64 = 1 << 2;
const VIRTIO_BLK_F_RO: u64 = 1 << 5;
const VIRTIO_BLK_F_FLUSH: u64 = 1 << 9;

// Request types.
const T_IN: u32 = 0;
const T_OUT: u32 = 1;
const T_FLUSH: u32 = 4;
const T_GET_ID: u32 = 8;

// Status codes, the last byte the device writes.
const S_OK: u8 = 0;
const S_IOERR: u8 = 1;
const S_UNSUPP: u8 = 2;

/// A block device whose sectors are a file on the host.
pub struct VirtioBlockMmio {
    file: File,
    /// Capacity, in [`SECTOR_SIZE`] sectors. A file whose length is not a
    /// whole number of sectors loses the tail rather than exposing half a
    /// sector the guest could never write back.
    sectors: u64,
    read_only: bool,
    /// What the guest reads back from a device-ID request, and finds as the
    /// disk's serial (`/dev/disk/by-id/virtio-<id>`).
    id: [u8; ID_LEN],
    acked_features: u64,
    queues: [GuestQueue; 1],
}

impl VirtioBlockMmio {
    /// Open the raw image at `path` as a disk.
    ///
    /// `id` is truncated to twenty bytes, the most the request carries.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, or is smaller than one
    /// sector.
    pub fn open(path: &Path, read_only: bool, id: &str) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(!read_only)
            .open(path)
            .map_err(|e| Error::Device(format!("cannot open disk {}: {e}", path.display())))?;
        Self::from_file(file, read_only, id)
    }

    /// Use an already-open file as a disk.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub fn from_file(file: File, read_only: bool, id: &str) -> Result<Self> {
        let len = file
            .metadata()
            .map_err(|e| Error::Device(format!("cannot size disk: {e}")))?
            .len();
        let sectors = len / SECTOR_SIZE;
        if sectors == 0 {
            return Err(Error::Device(format!(
                "a {len}-byte disk is smaller than one sector"
            )));
        }
        let mut serial = [0u8; ID_LEN];
        let take = id.len().min(ID_LEN);
        serial[..take].copy_from_slice(&id.as_bytes()[..take]);
        Ok(Self {
            file,
            sectors,
            read_only,
            id: serial,
            acked_features: 0,
            queues: [GuestQueue::new(QUEUE_SIZE)],
        })
    }

    /// Capacity in bytes.
    pub fn capacity_bytes(&self) -> u64 {
        self.sectors * SECTOR_SIZE
    }

    /// Whether the guest sees the disk as read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Serve every request the driver has made available.
    fn drain(&mut self, mem: &GuestMemory) -> Result<bool> {
        let mut consumed = false;
        while let Some(chain) = self.queues[0].pop(mem)? {
            let written = self.serve(&chain, mem)?;
            self.queues[0].add_used(mem, chain.head, written)?;
            consumed = true;
        }
        Ok(consumed)
    }

    /// Serve one request, and return how many bytes went into its writable
    /// buffers, status byte included.
    ///
    /// A malformed request is answered with an error status rather than
    /// dropped: a request the device keeps is a request the guest waits on
    /// forever.
    fn serve(&mut self, chain: &DescriptorChain, mem: &GuestMemory) -> Result<u32> {
        let writable = chain.writable_len();
        if writable == 0 {
            // Nowhere to put a status. Nothing to answer with, so the chain
            // goes back empty and the guest's driver reports the I/O error.
            tracing::debug!("virtio-blk: a request with no status byte");
            return Ok(0);
        }
        let readable = chain.read_all(mem)?;
        if readable.len() < REQ_HDR_LEN {
            return answer(chain, mem, &[], S_IOERR);
        }
        let kind = u32::from_le_bytes(readable[0..4].try_into().expect("4 bytes"));
        let sector = u64::from_le_bytes(readable[8..16].try_into().expect("8 bytes"));
        // Everything writable but the last byte is data space; the last byte
        // is the status.
        let data_room = writable - 1;

        match kind {
            T_IN => {
                let mut data = vec![0u8; data_room];
                let status = if self.in_bounds(sector, data.len()) {
                    match read_at(&self.file, &mut data, sector * SECTOR_SIZE) {
                        Ok(()) => S_OK,
                        Err(e) => {
                            tracing::warn!("virtio-blk: read of sector {sector} failed: {e}");
                            S_IOERR
                        }
                    }
                } else {
                    S_IOERR
                };
                if status == S_OK {
                    answer(chain, mem, &data, status)
                } else {
                    answer(chain, mem, &[], status)
                }
            }
            T_OUT => {
                let data = &readable[REQ_HDR_LEN..];
                let status = if self.read_only || !self.in_bounds(sector, data.len()) {
                    S_IOERR
                } else {
                    match write_at(&self.file, data, sector * SECTOR_SIZE) {
                        Ok(()) => S_OK,
                        Err(e) => {
                            tracing::warn!("virtio-blk: write of sector {sector} failed: {e}");
                            S_IOERR
                        }
                    }
                };
                answer(chain, mem, &[], status)
            }
            T_FLUSH => {
                let status = if self.read_only {
                    S_OK
                } else {
                    match self.file.sync_data() {
                        Ok(()) => S_OK,
                        Err(e) => {
                            tracing::warn!("virtio-blk: flush failed: {e}");
                            S_IOERR
                        }
                    }
                };
                answer(chain, mem, &[], status)
            }
            T_GET_ID => {
                let take = data_room.min(ID_LEN);
                let id = self.id;
                answer(chain, mem, &id[..take], S_OK)
            }
            _ => answer(chain, mem, &[], S_UNSUPP),
        }
    }

    /// Whether `len` bytes from `sector` lie inside the disk, in whole sectors.
    fn in_bounds(&self, sector: u64, len: usize) -> bool {
        let len = len as u64;
        len.is_multiple_of(SECTOR_SIZE)
            && sector
                .checked_add(len / SECTOR_SIZE)
                .is_some_and(|end| end <= self.sectors)
    }
}

/// Write `data` and then `status` into the chain's writable buffers.
///
/// The status goes in the *last* writable byte, wherever the data ended: the
/// driver looks for it there, and a short read that put it straight after the
/// data would be read as a stray data byte and a missing status.
fn answer(chain: &DescriptorChain, mem: &GuestMemory, data: &[u8], status: u8) -> Result<u32> {
    let writable = chain.writable_len();
    let written = chain.write_all(mem, &data[..data.len().min(writable - 1)])?;
    let mut remaining = writable - 1;
    for (addr, len) in &chain.writable {
        let len = *len as usize;
        if remaining < len {
            mem.write_bytes(addr + remaining as u64, &[status])?;
            break;
        }
        remaining -= len;
    }
    Ok((written + 1) as u32)
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(windows)]
fn read_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        match std::os::windows::fs::FileExt::seek_read(file, buf, offset)? {
            0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            n => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn write_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        match std::os::windows::fs::FileExt::seek_write(file, buf, offset)? {
            0 => return Err(std::io::ErrorKind::WriteZero.into()),
            n => {
                buf = &buf[n..];
                offset += n as u64;
            }
        }
    }
    Ok(())
}

impl VirtioMmioDevice for VirtioBlockMmio {
    fn device_id(&self) -> u32 {
        VIRTIO_ID_BLOCK
    }

    fn device_features(&self) -> u64 {
        let ro = if self.read_only { VIRTIO_BLK_F_RO } else { 0 };
        VIRTIO_F_VERSION_1 | VIRTIO_BLK_F_SEG_MAX | VIRTIO_BLK_F_FLUSH | ro
    }

    fn ack_features(&mut self, features: u64) {
        self.acked_features = features;
    }

    fn queues(&mut self) -> &mut [GuestQueue] {
        &mut self.queues
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        // capacity (u64), size_max (u32, not offered), seg_max (u32).
        let mut config = [0u8; 16];
        config[0..8].copy_from_slice(&self.sectors.to_le_bytes());
        config[12..16].copy_from_slice(&SEG_MAX.to_le_bytes());
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = config.get(offset as usize + i).copied().unwrap_or(0);
        }
    }

    fn write_config(&mut self, _offset: u64, _data: &[u8]) {
        // Only the write-back toggle is driver-writable, and it is not offered.
        tracing::debug!("virtio-blk: ignoring a driver write to read-only config space");
    }

    fn notify(&mut self, queue: u16, mem: &GuestMemory) -> Result<bool> {
        if queue == 0 {
            self.drain(mem)
        } else {
            tracing::warn!("virtio-blk: notify for queue {queue}, which does not exist");
            Ok(false)
        }
    }

    fn reset(&mut self) {
        for queue in &mut self.queues {
            queue.reset();
        }
        self.acked_features = 0;
    }
}

impl std::fmt::Debug for VirtioBlockMmio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtioBlockMmio")
            .field("sectors", &self.sectors)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::virtio_queue::{desc_flags, Descriptor};
    use crate::memory::GuestAddress;

    const DESC: GuestAddress = 0x1000;
    const AVAIL: GuestAddress = 0x1400;
    const USED: GuestAddress = 0x1800;
    const HDR: GuestAddress = 0x2000;
    const DATA: GuestAddress = 0x3000;
    const STATUS: GuestAddress = 0x4000;
    const RING_SIZE: u16 = 8;

    fn memory() -> GuestMemory {
        let mem = GuestMemory::new(0x10000).expect("guest memory");
        mem.allocate_region(0x10000, false).expect("region");
        mem
    }

    fn disk(sectors: u64, read_only: bool) -> (tempfile::NamedTempFile, VirtioBlockMmio) {
        let file = tempfile::NamedTempFile::new().expect("temp file");
        file.as_file().set_len(sectors * SECTOR_SIZE).expect("size");
        let mut dev = VirtioBlockMmio::open(file.path(), read_only, "vol-test").expect("open");
        let q = &mut dev.queues()[0];
        q.set_size(RING_SIZE);
        q.set_desc_addr(DESC);
        q.set_avail_addr(AVAIL);
        q.set_used_addr(USED);
        q.set_ready(true);
        (file, dev)
    }

    fn write_desc(mem: &GuestMemory, idx: u16, desc: Descriptor) {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&desc.addr.to_le_bytes());
        bytes[8..12].copy_from_slice(&desc.len.to_le_bytes());
        bytes[12..14].copy_from_slice(&desc.flags.to_le_bytes());
        bytes[14..16].copy_from_slice(&desc.next.to_le_bytes());
        mem.write_bytes(DESC + u64::from(idx) * 16, &bytes)
            .expect("descriptor");
    }

    /// Lay out a three-descriptor request the way Linux does — header, data,
    /// status — make it available in `slot`, and kick.
    fn submit(
        mem: &GuestMemory,
        dev: &mut VirtioBlockMmio,
        slot: u16,
        kind: u32,
        sector: u64,
        data_len: u32,
        data_writable: bool,
    ) -> u8 {
        let mut hdr = [0u8; REQ_HDR_LEN];
        hdr[0..4].copy_from_slice(&kind.to_le_bytes());
        hdr[8..16].copy_from_slice(&sector.to_le_bytes());
        mem.write_bytes(HDR, &hdr).expect("header");
        mem.write_bytes(STATUS, &[0xff]).expect("status");

        write_desc(
            mem,
            0,
            Descriptor {
                addr: HDR,
                len: REQ_HDR_LEN as u32,
                flags: desc_flags::NEXT,
                next: 1,
            },
        );
        let mut status_idx = 1;
        if data_len > 0 {
            let flags = desc_flags::NEXT | if data_writable { desc_flags::WRITE } else { 0 };
            write_desc(
                mem,
                1,
                Descriptor {
                    addr: DATA,
                    len: data_len,
                    flags,
                    next: 2,
                },
            );
            status_idx = 2;
        }
        write_desc(
            mem,
            status_idx,
            Descriptor {
                addr: STATUS,
                len: 1,
                flags: desc_flags::WRITE,
                next: 0,
            },
        );
        mem.write_bytes(AVAIL + 4 + u64::from(slot) * 2, &0u16.to_le_bytes())
            .expect("ring entry");
        mem.write_bytes(AVAIL + 2, &(slot + 1).to_le_bytes())
            .expect("avail idx");

        assert!(
            dev.notify(0, mem).expect("notify"),
            "a request owes an interrupt"
        );
        mem.read_bytes(STATUS, 1).expect("status")[0]
    }

    /// What the guest writes reaches the file, and reads back through the
    /// device: the whole point of a disk.
    #[test]
    fn a_write_lands_in_the_file_and_reads_back() {
        let mem = memory();
        let (file, mut dev) = disk(16, false);

        let pattern: Vec<u8> = (0..1024u32).map(|i| (i % 251) as u8).collect();
        mem.write_bytes(DATA, &pattern).expect("data");
        assert_eq!(submit(&mem, &mut dev, 0, T_OUT, 3, 1024, false), S_OK);

        let on_disk = std::fs::read(file.path()).expect("read back");
        assert_eq!(&on_disk[3 * 512..3 * 512 + 1024], &pattern[..]);

        mem.write_bytes(DATA, &[0u8; 1024]).expect("clear");
        assert_eq!(submit(&mem, &mut dev, 1, T_IN, 3, 1024, true), S_OK);
        assert_eq!(mem.read_bytes(DATA, 1024).expect("data"), pattern);
    }

    /// A request past the end is an error, not a short read or a file that
    /// grows: the capacity the guest was told is the capacity it gets.
    #[test]
    fn a_request_past_the_end_is_refused() {
        let mem = memory();
        let (file, mut dev) = disk(4, false);
        assert_eq!(submit(&mem, &mut dev, 0, T_OUT, 3, 1024, false), S_IOERR);
        assert_eq!(submit(&mem, &mut dev, 1, T_IN, 4, 512, true), S_IOERR);
        assert_eq!(
            std::fs::metadata(file.path()).expect("stat").len(),
            4 * 512,
            "the file must not grow"
        );
    }

    #[test]
    fn a_read_only_disk_refuses_writes_and_says_so() {
        let mem = memory();
        let (_file, mut dev) = disk(4, true);
        assert_ne!(dev.device_features() & VIRTIO_BLK_F_RO, 0);
        assert_eq!(submit(&mem, &mut dev, 0, T_OUT, 0, 512, false), S_IOERR);
    }

    #[test]
    fn flush_and_id_are_answered() {
        let mem = memory();
        let (_file, mut dev) = disk(4, false);
        assert_eq!(submit(&mem, &mut dev, 0, T_FLUSH, 0, 0, false), S_OK);
        assert_eq!(submit(&mem, &mut dev, 1, T_GET_ID, 0, 20, true), S_OK);
        assert_eq!(&mem.read_bytes(DATA, 8).expect("id")[..], b"vol-test");
        assert_eq!(submit(&mem, &mut dev, 2, 99, 0, 0, false), S_UNSUPP);
    }

    #[test]
    fn config_space_carries_capacity_and_seg_max() {
        let (_file, dev) = disk(32, false);
        let mut cap = [0u8; 8];
        dev.read_config(0, &mut cap);
        assert_eq!(u64::from_le_bytes(cap), 32);
        let mut seg = [0u8; 4];
        dev.read_config(12, &mut seg);
        assert_eq!(u32::from_le_bytes(seg), SEG_MAX);
        assert_eq!(dev.device_id(), VIRTIO_ID_BLOCK);
    }

    #[test]
    fn an_empty_file_is_not_a_disk() {
        let file = tempfile::NamedTempFile::new().expect("temp file");
        assert!(VirtioBlockMmio::open(file.path(), false, "x").is_err());
    }
}
