//! A whole VM in a file: its memory and its vCPUs.
//!
//! # The format
//!
//! ```text
//!   magic     8 bytes   "HV2SNAP\0"
//!   version   u32 LE    2
//!   len       u32 LE    length of the header that follows
//!   header    JSON      Header, below
//!   bitmap    raw       one bit per page: is it in this file?
//!   pages     raw       only the pages whose bit is set
//! ```
//!
//! A JSON header in front of raw pages, rather than one encoding for
//! everything. The header is small, changes shape as this grows, and is the
//! part a human reads when a restore goes wrong; the pages are large, never
//! interpreted, and would be pure cost to encode. `serde_json` on a gigabyte
//! of guest RAM would be slower than the VM it is snapshotting.
//!
//! # Sparse, because most of a guest is zero
//!
//! A page that is entirely zero is recorded as a cleared bit and not written.
//! A freshly booted 64 MiB guest has touched a few megabytes, so this is most
//! of the file: measured at 92.4 ms to restore when every page was written,
//! against 8.7 ms to boot the same guest from its ELF -- a restore that lost
//! to booting by 10.6x, entirely on the cost of moving bytes that were zero
//! at both ends.
//!
//! On restore, a cleared bit means *write zeroes*, not *skip*. The
//! destination VM has its own memory, which has usually booted something, so
//! leaving those pages alone would restore a guest built half from the
//! snapshot and half from whatever was there before. Zeroing is a memset and
//! costs a fraction of the read it replaces.
//!
//! # What is not in it
//!
//! **Compression.** A different trade from sparseness -- CPU for I/O rather
//! than a pure saving -- and untaken. `MemorySnapshotConfig` in this crate
//! describes it and nothing applies it.
//!
//! **Device state.** Virtio queues here keep ring *addresses*, and the rings
//! themselves live in guest memory, so most of a device's state does travel
//! in the pages. What does not is the host-side bookkeeping --
//! `last_avail_idx`, `next_used_idx`, negotiated features -- and a vsock
//! device's connection table. A guest restored with I/O in flight will see
//! the device disagree with it. [`Snapshot::device_state_included`] answers
//! `false` so a caller can refuse rather than find out.
//!
//! **Anything about the host.** A snapshot does not record which kernel or
//! backend produced it beyond the version above, so restoring one into a
//! different machine's KVM is not checked and not supported.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::vcpu::VCpuSnapshot;
use crate::{Error, Result};

/// The first eight bytes of any snapshot.
const MAGIC: &[u8; 8] = b"HV2SNAP\0";

/// The format version. Bumped when the header's shape changes in a way an
/// older reader would misread rather than reject.
///
/// 2 added the page bitmap. A version 1 reader handed a version 2 file would
/// read the bitmap as the first pages of guest memory, which is why this is
/// checked rather than assumed.
const VERSION: u32 = 2;

/// The granularity of the bitmap, matching [`super::memory::PAGE_SIZE`].
pub const PAGE_SIZE: u64 = 4096;

/// A cap on the header, so a corrupt length cannot ask for an allocation the
/// size of the address space before anything has validated it.
const MAX_HEADER_BYTES: u32 = 1 << 20;

/// One region of guest memory, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionRecord {
    /// Where the guest sees it.
    pub guest_addr: u64,
    pub size: u64,
    /// Read-only regions are recorded but not written back on restore: they
    /// are shared host pages (a model's weights, say) that the restoring VM
    /// maps for itself, and writing to them would either fail or privately
    /// copy something every other guest is still sharing.
    pub readonly: bool,
}

/// What a snapshot says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    /// The VM's name when it was taken, for a human reading the file.
    pub vm_name: String,
    pub memory_size: u64,
    pub regions: Vec<RegionRecord>,
    pub vcpus: Vec<VCpuSnapshot>,
    /// How many pages the bitmap covers: every page of every writable region,
    /// in the regions' order.
    pub total_pages: u64,
    /// How many of them are actually in the file. Recorded so a reader can
    /// check its own arithmetic against the writer's, and so a human can see
    /// at a glance how much of the guest was worth storing.
    pub present_pages: u64,
    /// Whether host-side device state was captured. Always `false` today;
    /// see this module's own documentation.
    pub device_state_included: bool,
}

/// Which pages a snapshot carries.
///
/// One bit per page, least-significant bit first within each byte. Stored
/// outside the JSON header because it grows with the guest: a 16 GiB VM needs
/// half a megabyte of it, which would not fit under the header size limit
/// however the header were encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMap {
    bits: Vec<u8>,
    pages: u64,
}

impl PageMap {
    /// A map with every page absent.
    #[must_use]
    pub fn empty(pages: u64) -> Self {
        Self {
            bits: vec![0u8; Self::byte_len(pages)],
            pages,
        }
    }

    /// How many bytes a map of `pages` pages occupies.
    #[must_use]
    pub fn byte_len(pages: u64) -> usize {
        // Rounded up: a guest whose page count is not a multiple of eight
        // still needs a bit for its last page.
        pages.div_ceil(8) as usize
    }

    /// How many pages this map covers.
    #[must_use]
    pub fn pages(&self) -> u64 {
        self.pages
    }

    /// The raw bits, for writing.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bits
    }

    /// Read a map of `pages` pages.
    ///
    /// # Errors
    ///
    /// Propagates a read failure.
    pub fn read<R: Read>(file: &mut R, pages: u64) -> Result<Self> {
        let mut bits = vec![0u8; Self::byte_len(pages)];
        file.read_exact(&mut bits)
            .map_err(|e| Error::Config(format!("reading a snapshot's page map: {e}")))?;
        Ok(Self { bits, pages })
    }

    /// Record that page `index` is in the file.
    pub fn set(&mut self, index: u64) {
        if index < self.pages {
            self.bits[(index / 8) as usize] |= 1 << (index % 8);
        }
    }

    /// Is page `index` in the file?
    #[must_use]
    pub fn contains(&self, index: u64) -> bool {
        index < self.pages && self.bits[(index / 8) as usize] & (1 << (index % 8)) != 0
    }

    /// How many pages are present.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.bits.iter().map(|b| u64::from(b.count_ones())).sum()
    }
}

/// A snapshot on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub header: Header,
}

impl Snapshot {
    /// Whether this snapshot carries host-side device state.
    ///
    /// A caller restoring a VM with devices that had I/O in flight should
    /// check, because the alternative is a guest that resumes disagreeing
    /// with its own virtqueues.
    #[must_use]
    pub fn device_state_included(&self) -> bool {
        self.header.device_state_included
    }

    /// Read a snapshot's header, leaving `file` positioned at the first
    /// region's bytes.
    ///
    /// # Errors
    ///
    /// Fails on a file that is not a snapshot, one written by a newer format,
    /// or a header that will not parse.
    pub fn read_header<R: Read>(file: &mut R) -> Result<Self> {
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)
            .map_err(|e| Error::Config(format!("reading a snapshot's magic: {e}")))?;
        if &magic != MAGIC {
            return Err(Error::Config(
                "that file is not a HyperMachine snapshot".to_string(),
            ));
        }

        let version = read_u32(file)?;
        if version != VERSION {
            // Refused rather than attempted: a header this reader half
            // understands produces a VM restored from values that mean
            // something else, which fails later and somewhere unrelated.
            return Err(Error::Config(format!(
                "this snapshot is version {version} and this build reads version {VERSION}"
            )));
        }

        let header_len = read_u32(file)?;
        if header_len > MAX_HEADER_BYTES {
            return Err(Error::Config(format!(
                "that snapshot claims a {header_len}-byte header, over the {MAX_HEADER_BYTES} \
                 limit; the file is corrupt"
            )));
        }

        let mut header_bytes = vec![0u8; header_len as usize];
        file.read_exact(&mut header_bytes)
            .map_err(|e| Error::Config(format!("reading a snapshot's header: {e}")))?;
        let header: Header = serde_json::from_slice(&header_bytes)
            .map_err(|e| Error::Config(format!("parsing a snapshot's header: {e}")))?;

        Ok(Self { header })
    }

    /// Write a header, leaving `file` positioned where the regions go.
    ///
    /// # Errors
    ///
    /// Propagates a write failure, or a header that will not encode.
    pub fn write_header<W: Write>(header: &Header, file: &mut W) -> Result<()> {
        let encoded = serde_json::to_vec(header)
            .map_err(|e| Error::Config(format!("encoding a snapshot's header: {e}")))?;
        let len = u32::try_from(encoded.len())
            .map_err(|_| Error::Config("a snapshot header larger than 4 GiB".to_string()))?;
        if len > MAX_HEADER_BYTES {
            return Err(Error::Config(format!(
                "this header is {len} bytes, over the {MAX_HEADER_BYTES} a reader will accept"
            )));
        }

        file.write_all(MAGIC)
            .and_then(|()| file.write_all(&VERSION.to_le_bytes()))
            .and_then(|()| file.write_all(&len.to_le_bytes()))
            .and_then(|()| file.write_all(&encoded))
            .map_err(|e| Error::Config(format!("writing a snapshot's header: {e}")))
    }

    /// The bytes a snapshot with this header occupies, header included.
    ///
    /// Only the pages the bitmap says are present are counted, which is the
    /// whole point of the bitmap.
    #[must_use]
    pub fn expected_len(header: &Header, header_len: u64) -> u64 {
        8 + 4
            + 4
            + header_len
            + PageMap::byte_len(header.total_pages) as u64
            + header.present_pages * PAGE_SIZE
    }

    /// Check that a file is as long as its header says it should be.
    ///
    /// A snapshot truncated by a full disk parses perfectly -- the header is
    /// at the front -- and then restores a guest whose last pages are
    /// whatever the new VM happened to have. This is what stops that being
    /// discovered by the guest.
    ///
    /// # Errors
    ///
    /// Fails if the file is shorter than the regions it promises.
    pub fn check_length<R: Read + Seek>(&self, file: &mut R, header_len: u64) -> Result<()> {
        let actual = file
            .seek(SeekFrom::End(0))
            .map_err(|e| Error::Config(format!("measuring a snapshot: {e}")))?;
        let expected = Self::expected_len(&self.header, header_len);
        if actual < expected {
            return Err(Error::Config(format!(
                "that snapshot is {actual} bytes and its header describes {expected}; it was \
                 truncated, most likely by a full disk while it was written"
            )));
        }
        Ok(())
    }
}

fn read_u32<R: Read>(file: &mut R) -> Result<u32> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes)
        .map_err(|e| Error::Config(format!("reading a snapshot: {e}")))?;
    Ok(u32::from_le_bytes(bytes))
}

/// How long a header encodes to, for [`Snapshot::expected_len`].
///
/// # Errors
///
/// Fails if the header will not encode.
pub fn header_len(header: &Header) -> Result<u64> {
    serde_json::to_vec(header)
        .map(|encoded| encoded.len() as u64)
        .map_err(|e| Error::Config(format!("encoding a snapshot's header: {e}")))
}

/// Open a file for writing a snapshot, refusing to overwrite one.
///
/// # Errors
///
/// Fails if `path` exists. Overwriting is refused rather than defaulted:
/// these are large files named by a human, and the cost of a mistaken
/// overwrite is a guest that no longer exists anywhere.
pub fn create_new(path: &Path) -> Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Error::Config(format!("creating {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn header() -> Header {
        Header {
            vm_name: "test".to_string(),
            memory_size: 8192,
            regions: vec![
                RegionRecord {
                    guest_addr: 0,
                    size: 4096,
                    readonly: false,
                },
                RegionRecord {
                    guest_addr: 4096,
                    size: 4096,
                    readonly: true,
                },
            ],
            vcpus: vec![VCpuSnapshot::default()],
            // One writable region of 4096 bytes: one page, and say it is
            // stored. The read-only one has no pages in the file.
            total_pages: 1,
            present_pages: 1,
            device_state_included: false,
        }
    }

    /// A file with a header, a page map, and `present` pages after it.
    fn snapshot_bytes(header: &Header, present: u64) -> Vec<u8> {
        let mut buffer = Vec::new();
        Snapshot::write_header(header, &mut buffer).expect("write");
        let mut map = PageMap::empty(header.total_pages);
        for index in 0..present {
            map.set(index);
        }
        buffer.extend_from_slice(map.as_bytes());
        buffer.extend_from_slice(&vec![0u8; (present * PAGE_SIZE) as usize]);
        buffer
    }

    #[test]
    fn a_header_survives_the_round_trip() {
        let mut buffer = Vec::new();
        Snapshot::write_header(&header(), &mut buffer).expect("write");
        let read = Snapshot::read_header(&mut Cursor::new(&buffer)).expect("read");
        assert_eq!(read.header, header());
    }

    #[test]
    fn the_reader_stops_where_the_regions_begin() {
        // The caller reads pages from wherever the header left the cursor, so
        // a reader that consumed one byte too many or too few would shift
        // every page of guest memory.
        let mut buffer = Vec::new();
        Snapshot::write_header(&header(), &mut buffer).expect("write");
        let payload = b"the first region's bytes";
        buffer.extend_from_slice(payload);

        let mut cursor = Cursor::new(&buffer);
        Snapshot::read_header(&mut cursor).expect("read");
        let mut rest = Vec::new();
        cursor.read_to_end(&mut rest).expect("read the rest");
        assert_eq!(rest, payload);
    }

    #[test]
    fn a_file_that_is_not_a_snapshot_is_refused() {
        let mut cursor = Cursor::new(b"not a snapshot at all, just some bytes".to_vec());
        let error = Snapshot::read_header(&mut cursor).expect_err("should refuse");
        assert!(
            format!("{error}").contains("not a HyperMachine snapshot"),
            "{error}"
        );
    }

    #[test]
    fn a_newer_version_is_refused_rather_than_guessed_at() {
        // Reading a header this build half understands restores a VM from
        // values that mean something else, and the failure surfaces later,
        // somewhere unrelated to the file.
        let mut buffer = Vec::new();
        Snapshot::write_header(&header(), &mut buffer).expect("write");
        buffer[8..12].copy_from_slice(&99u32.to_le_bytes());

        let error = Snapshot::read_header(&mut Cursor::new(&buffer)).expect_err("should refuse");
        assert!(format!("{error}").contains("version 99"), "{error}");
    }

    #[test]
    fn a_corrupt_header_length_does_not_become_an_allocation() {
        // The length is read from the file before anything has checked it. A
        // reader that believed it would try to allocate 4 GiB on a one-byte
        // corruption.
        let mut buffer = Vec::new();
        Snapshot::write_header(&header(), &mut buffer).expect("write");
        buffer[12..16].copy_from_slice(&u32::MAX.to_le_bytes());

        let error = Snapshot::read_header(&mut Cursor::new(&buffer)).expect_err("should refuse");
        assert!(format!("{error}").contains("corrupt"), "{error}");
    }

    #[test]
    fn a_truncated_snapshot_is_caught_before_it_is_restored() {
        // The header is at the front, so a file cut short by a full disk
        // parses perfectly and then hands the guest whatever the new VM's
        // memory happened to contain.
        let header = header();
        let len = header_len(&header).expect("len");
        let mut buffer = snapshot_bytes(&header, 1);
        buffer.truncate(buffer.len() - 100);

        let mut cursor = Cursor::new(buffer);
        let snapshot = Snapshot::read_header(&mut cursor).expect("read");
        let error = snapshot
            .check_length(&mut cursor, len)
            .expect_err("should refuse");
        assert!(format!("{error}").contains("truncated"), "{error}");
    }

    #[test]
    fn a_complete_snapshot_passes_the_length_check() {
        let header = header();
        let len = header_len(&header).expect("len");
        let buffer = snapshot_bytes(&header, 1);
        let mut cursor = Cursor::new(buffer);
        let snapshot = Snapshot::read_header(&mut cursor).expect("read");
        snapshot.check_length(&mut cursor, len).expect("complete");
    }

    #[test]
    fn a_sparse_snapshot_is_shorter_than_a_full_one() {
        // The whole reason for the bitmap: a guest that has touched one page
        // of two writes one page, not two.
        let mut sparse = header();
        sparse.total_pages = 2;
        sparse.present_pages = 1;
        let mut full = sparse.clone();
        full.present_pages = 2;

        let len = header_len(&sparse).expect("len");
        assert!(
            Snapshot::expected_len(&sparse, len) < Snapshot::expected_len(&full, len),
            "a snapshot storing fewer pages must be a smaller file"
        );
        assert_eq!(
            Snapshot::expected_len(&full, len) - Snapshot::expected_len(&sparse, len),
            PAGE_SIZE,
            "and smaller by exactly the page it did not store"
        );
    }

    #[test]
    fn a_page_map_remembers_exactly_which_pages() {
        let mut map = PageMap::empty(20);
        for index in [0u64, 7, 8, 19] {
            map.set(index);
        }
        assert_eq!(map.count(), 4);
        for index in 0..20u64 {
            assert_eq!(
                map.contains(index),
                matches!(index, 0 | 7 | 8 | 19),
                "page {index}"
            );
        }
        // The byte boundary at 7/8 is where an off-by-one in the shift shows
        // up, and it would otherwise restore the wrong page's contents.
        assert!(map.contains(7) && map.contains(8));
        assert!(!map.contains(6) && !map.contains(9));
    }

    #[test]
    fn a_page_map_covers_a_count_that_is_not_a_multiple_of_eight() {
        // 20 pages needs three bytes, not two: the last four pages would
        // otherwise have no bit, and every one of them would restore as
        // absent -- silently zeroed rather than restored.
        assert_eq!(PageMap::byte_len(20), 3);
        assert_eq!(PageMap::byte_len(8), 1);
        assert_eq!(PageMap::byte_len(9), 2);
        assert_eq!(PageMap::byte_len(0), 0);

        let mut map = PageMap::empty(20);
        map.set(19);
        assert!(map.contains(19));
    }

    #[test]
    fn a_page_map_survives_the_round_trip() {
        let mut map = PageMap::empty(100);
        for index in (0..100).step_by(3) {
            map.set(index);
        }
        let read = PageMap::read(&mut Cursor::new(map.as_bytes().to_vec()), 100).expect("read");
        assert_eq!(read, map);
        assert_eq!(read.count(), map.count());
    }

    #[test]
    fn a_page_beyond_the_map_is_not_recorded_and_does_not_panic() {
        // The index comes from counting regions, and a mismatch between the
        // count and the map is a bug -- but one that should not take the
        // process down in the middle of writing a snapshot.
        let mut map = PageMap::empty(4);
        map.set(99);
        assert_eq!(map.count(), 0);
        assert!(!map.contains(99));
    }

    #[test]
    fn a_snapshot_says_it_has_no_device_state() {
        // Until host-side virtio bookkeeping is captured, a caller restoring
        // a VM with I/O in flight needs to be able to find that out from the
        // file rather than from the guest.
        let mut buffer = Vec::new();
        Snapshot::write_header(&header(), &mut buffer).expect("write");
        let read = Snapshot::read_header(&mut Cursor::new(&buffer)).expect("read");
        assert!(!read.device_state_included());
    }
}
