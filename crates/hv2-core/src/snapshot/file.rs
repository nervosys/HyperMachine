//! A whole VM in a file: its memory and its vCPUs.
//!
//! # The format
//!
//! ```text
//!   magic     8 bytes   "HV2SNAP\0"
//!   version   u32 LE    1
//!   len       u32 LE    length of the header that follows
//!   header    JSON      Header, below
//!   regions   raw       each region's bytes, in the header's order
//! ```
//!
//! A JSON header in front of raw pages, rather than one encoding for
//! everything. The header is small, changes shape as this grows, and is the
//! part a human reads when a restore goes wrong; the pages are large, never
//! interpreted, and would be pure cost to encode. `serde_json` on a gigabyte
//! of guest RAM would be slower than the VM it is snapshotting.
//!
//! # What is not in it
//!
//! **Compression, and any sparseness.** A 64 MiB guest writes a 64 MiB file
//! whether or not it has touched a single page. `MemorySnapshotConfig` in
//! this crate already describes compression that nothing applies here, and
//! dirty-page tracking exists in [`super::memory`]; using either is the next
//! thing this wants, and pretending otherwise would hide the cost.
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
const VERSION: u32 = 1;

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
    /// Whether host-side device state was captured. Always `false` today;
    /// see this module's own documentation.
    pub device_state_included: bool,
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
    /// Read-only regions are counted: they are recorded so a restore can tell
    /// that a region it is about to skip was skipped deliberately.
    #[must_use]
    pub fn expected_len(header: &Header, header_len: u64) -> u64 {
        8 + 4 + 4 + header_len + header.regions.iter().map(|r| r.size).sum::<u64>()
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
            device_state_included: false,
        }
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
        let mut buffer = Vec::new();
        Snapshot::write_header(&header, &mut buffer).expect("write");
        buffer.extend_from_slice(&vec![0u8; 4096]); // one region, not two

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
        let mut buffer = Vec::new();
        Snapshot::write_header(&header, &mut buffer).expect("write");
        buffer.extend_from_slice(&vec![0u8; 8192]); // both regions

        let mut cursor = Cursor::new(buffer);
        let snapshot = Snapshot::read_header(&mut cursor).expect("read");
        snapshot.check_length(&mut cursor, len).expect("complete");
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
