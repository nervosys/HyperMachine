//! Disk Image Format Support
//!
//! This module provides support for various disk image formats:
//! - Raw: Direct sector-by-sector storage
//! - QCOW2: QEMU Copy-On-Write version 2
//! - VHDX: Microsoft Hyper-V virtual disk
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     DiskImage Trait                             │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │  read(offset, buf) -> Result<usize>                      │   │
//! │  │  write(offset, data) -> Result<usize>                    │   │
//! │  │  flush() -> Result<()>                                   │   │
//! │  │  size() -> u64                                           │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!         ┌───────────────────┼───────────────────┐
//!         ▼                   ▼                   ▼
//! ┌───────────────┐  ┌───────────────┐  ┌───────────────┐
//! │   RawImage    │  │  Qcow2Image   │  │   VhdxImage   │
//! │  (direct I/O) │  │ (sparse+COW)  │  │ (MS format)   │
//! └───────────────┘  └───────────────┘  └───────────────┘
//! ```

use crate::{Error, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::RwLock;

/// Sector size constant
pub const SECTOR_SIZE: u64 = 512;

/// Disk image trait
pub trait DiskImage: Send + Sync {
    /// Read data from the image at the given offset
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize>;

    /// Write data to the image at the given offset
    fn write(&self, offset: u64, data: &[u8]) -> Result<usize>;

    /// Flush pending writes
    fn flush(&self) -> Result<()>;

    /// Get total size in bytes
    fn size(&self) -> u64;

    /// Check if read-only
    fn is_read_only(&self) -> bool;

    /// Get format name
    fn format_name(&self) -> &'static str;
}

/// Raw disk image format
pub struct RawImage {
    /// File handle
    file: RwLock<File>,
    /// Total size
    size: u64,
    /// Read-only flag
    read_only: bool,
}

impl RawImage {
    /// Open an existing raw image
    pub fn open<P: AsRef<Path>>(path: P, read_only: bool) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(!read_only)
            .open(path.as_ref())
            .map_err(|e| Error::Device(format!("Failed to open image: {}", e)))?;

        let size = file
            .metadata()
            .map_err(|e| Error::Device(format!("Failed to get metadata: {}", e)))?
            .len();

        Ok(Self {
            file: RwLock::new(file),
            size,
            read_only,
        })
    }

    /// Create a new raw image
    pub fn create<P: AsRef<Path>>(path: P, size: u64) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path.as_ref())
            .map_err(|e| Error::Device(format!("Failed to create image: {}", e)))?;

        file.set_len(size)
            .map_err(|e| Error::Device(format!("Failed to set size: {}", e)))?;

        Ok(Self {
            file: RwLock::new(file),
            size,
            read_only: false,
        })
    }

    /// Create an in-memory raw image (for testing)
    pub fn in_memory(size: u64) -> InMemoryImage {
        InMemoryImage::new(size)
    }
}

impl DiskImage for RawImage {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        if offset >= self.size {
            return Ok(0);
        }

        let mut file = self.file.write().unwrap_or_else(|e| e.into_inner());
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| Error::Device(format!("Seek failed: {}", e)))?;

        let to_read = std::cmp::min(buf.len() as u64, self.size - offset) as usize;
        file.read(&mut buf[..to_read])
            .map_err(|e| Error::Device(format!("Read failed: {}", e)))
    }

    fn write(&self, offset: u64, data: &[u8]) -> Result<usize> {
        if self.read_only {
            return Err(Error::Device("Image is read-only".to_string()));
        }

        if offset >= self.size {
            return Ok(0);
        }

        let mut file = self.file.write().unwrap_or_else(|e| e.into_inner());
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| Error::Device(format!("Seek failed: {}", e)))?;

        let to_write = std::cmp::min(data.len() as u64, self.size - offset) as usize;
        file.write(&data[..to_write])
            .map_err(|e| Error::Device(format!("Write failed: {}", e)))
    }

    fn flush(&self) -> Result<()> {
        let file = self.file.write().unwrap_or_else(|e| e.into_inner());
        file.sync_all()
            .map_err(|e| Error::Device(format!("Flush failed: {}", e)))
    }

    fn size(&self) -> u64 {
        self.size
    }

    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn format_name(&self) -> &'static str {
        "raw"
    }
}

/// In-memory disk image (for testing)
pub struct InMemoryImage {
    /// Data buffer
    data: RwLock<Vec<u8>>,
    /// Read-only flag
    read_only: bool,
}

impl InMemoryImage {
    /// Create a new in-memory image
    pub fn new(size: u64) -> Self {
        Self {
            data: RwLock::new(vec![0u8; size as usize]),
            read_only: false,
        }
    }

    /// Create from existing data
    pub fn from_data(data: Vec<u8>) -> Self {
        Self {
            data: RwLock::new(data),
            read_only: false,
        }
    }

    /// Set read-only
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }
}

impl DiskImage for InMemoryImage {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());

        if offset >= data.len() as u64 {
            return Ok(0);
        }

        let to_read = std::cmp::min(buf.len(), data.len() - offset as usize);
        buf[..to_read].copy_from_slice(&data[offset as usize..offset as usize + to_read]);
        Ok(to_read)
    }

    fn write(&self, offset: u64, data_to_write: &[u8]) -> Result<usize> {
        if self.read_only {
            return Err(Error::Device("Image is read-only".to_string()));
        }

        let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());

        if offset >= data.len() as u64 {
            return Ok(0);
        }

        let to_write = std::cmp::min(data_to_write.len(), data.len() - offset as usize);
        data[offset as usize..offset as usize + to_write]
            .copy_from_slice(&data_to_write[..to_write]);
        Ok(to_write)
    }

    fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn size(&self) -> u64 {
        self.data.read().unwrap_or_else(|e| e.into_inner()).len() as u64
    }

    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn format_name(&self) -> &'static str {
        "memory"
    }
}

/// QCOW2 magic number
pub const QCOW2_MAGIC: u32 = 0x514649FB; // "QFI\xfb"

/// QCOW2 version
pub const QCOW2_VERSION: u32 = 3;

/// QCOW2 header (v3)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Qcow2Header {
    /// Magic number (0x514649FB)
    pub magic: u32,
    /// Version (2 or 3)
    pub version: u32,
    /// Backing file offset
    pub backing_file_offset: u64,
    /// Backing file size
    pub backing_file_size: u32,
    /// Cluster bits (log2 of cluster size)
    pub cluster_bits: u32,
    /// Virtual size in bytes
    pub size: u64,
    /// Encryption method
    pub crypt_method: u32,
    /// L1 table size (number of entries)
    pub l1_size: u32,
    /// L1 table offset
    pub l1_table_offset: u64,
    /// Refcount table offset
    pub refcount_table_offset: u64,
    /// Refcount table clusters
    pub refcount_table_clusters: u32,
    /// Number of snapshots
    pub nb_snapshots: u32,
    /// Snapshots offset
    pub snapshots_offset: u64,
}

impl Default for Qcow2Header {
    fn default() -> Self {
        Self {
            magic: QCOW2_MAGIC,
            version: QCOW2_VERSION,
            backing_file_offset: 0,
            backing_file_size: 0,
            cluster_bits: 16, // 64KB clusters
            size: 0,
            crypt_method: 0,
            l1_size: 0,
            l1_table_offset: 0,
            refcount_table_offset: 0,
            refcount_table_clusters: 0,
            nb_snapshots: 0,
            snapshots_offset: 0,
        }
    }
}

impl Qcow2Header {
    /// Parse header from bytes (big-endian)
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 104 {
            return None;
        }

        Some(Self {
            magic: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            version: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            backing_file_offset: u64::from_be_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            backing_file_size: u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            cluster_bits: u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
            size: u64::from_be_bytes([
                bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30],
                bytes[31],
            ]),
            crypt_method: u32::from_be_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]),
            l1_size: u32::from_be_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]),
            l1_table_offset: u64::from_be_bytes([
                bytes[40], bytes[41], bytes[42], bytes[43], bytes[44], bytes[45], bytes[46],
                bytes[47],
            ]),
            refcount_table_offset: u64::from_be_bytes([
                bytes[48], bytes[49], bytes[50], bytes[51], bytes[52], bytes[53], bytes[54],
                bytes[55],
            ]),
            refcount_table_clusters: u32::from_be_bytes([
                bytes[56], bytes[57], bytes[58], bytes[59],
            ]),
            nb_snapshots: u32::from_be_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]),
            snapshots_offset: u64::from_be_bytes([
                bytes[64], bytes[65], bytes[66], bytes[67], bytes[68], bytes[69], bytes[70],
                bytes[71],
            ]),
        })
    }

    /// Get cluster size
    pub fn cluster_size(&self) -> u64 {
        1 << self.cluster_bits
    }

    /// Validate the header
    pub fn is_valid(&self) -> bool {
        self.magic == QCOW2_MAGIC && (self.version == 2 || self.version == 3)
    }
}

/// VHDX file type identifier
pub const VHDX_SIGNATURE: u64 = 0x656C696678646876; // "vhdxfile"

/// VHDX header signature
pub const VHDX_HEADER_SIGNATURE: u32 = 0x64616568; // "head"

/// VHDX file header
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VhdxFileHeader {
    /// Signature ("vhdxfile")
    pub signature: u64,
    /// Creator application
    pub creator: [u8; 512],
}

impl VhdxFileHeader {
    /// Parse from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 520 {
            return None;
        }

        let signature = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);

        let mut creator = [0u8; 512];
        creator.copy_from_slice(&bytes[8..520]);

        Some(Self { signature, creator })
    }

    /// Check if valid
    pub fn is_valid(&self) -> bool {
        self.signature == VHDX_SIGNATURE
    }
}

/// VHDX region table entry
#[derive(Debug, Clone, Copy)]
pub struct VhdxRegion {
    /// Region GUID
    pub guid: [u8; 16],
    /// File offset
    pub file_offset: u64,
    /// Length in bytes
    pub length: u32,
    /// Required flag
    pub required: bool,
}

/// Disk image format detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Raw image
    Raw,
    /// QCOW2 image
    Qcow2,
    /// VHDX image
    Vhdx,
    /// Unknown format
    Unknown,
}

impl ImageFormat {
    /// Detect format from file header
    pub fn detect(header: &[u8]) -> Self {
        if header.len() >= 8 {
            // Check VHDX
            let sig = u64::from_le_bytes([
                header[0], header[1], header[2], header[3], header[4], header[5], header[6],
                header[7],
            ]);
            if sig == VHDX_SIGNATURE {
                return Self::Vhdx;
            }
        }

        if header.len() >= 4 {
            // Check QCOW2
            let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
            if magic == QCOW2_MAGIC {
                return Self::Qcow2;
            }
        }

        // Assume raw if no magic detected
        Self::Raw
    }

    /// Get format name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Qcow2 => "qcow2",
            Self::Vhdx => "vhdx",
            Self::Unknown => "unknown",
        }
    }
}

/// Open a disk image, auto-detecting format
pub fn open_image<P: AsRef<Path>>(path: P, read_only: bool) -> Result<Box<dyn DiskImage>> {
    // For now, only support raw images
    // QCOW2 and VHDX would require full implementation
    let image = RawImage::open(path, read_only)?;
    Ok(Box::new(image))
}

/// Create a new disk image
pub fn create_image<P: AsRef<Path>>(
    path: P,
    size: u64,
    format: ImageFormat,
) -> Result<Box<dyn DiskImage>> {
    match format {
        ImageFormat::Raw => {
            let image = RawImage::create(path, size)?;
            Ok(Box::new(image))
        }
        _ => Err(Error::Device(format!(
            "Unsupported format: {}",
            format.name()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_image() {
        let img = InMemoryImage::new(4096);

        assert_eq!(img.size(), 4096);
        assert!(!img.is_read_only());
        assert_eq!(img.format_name(), "memory");
    }

    #[test]
    fn test_in_memory_read_write() {
        let img = InMemoryImage::new(4096);

        let data = [0x42u8; 512];
        let written = img.write(0, &data).unwrap();
        assert_eq!(written, 512);

        let mut buf = [0u8; 512];
        let read = img.read(0, &mut buf).unwrap();
        assert_eq!(read, 512);
        assert_eq!(buf, data);
    }

    #[test]
    fn test_in_memory_offset() {
        let img = InMemoryImage::new(4096);

        let data = [0x42u8; 512];
        img.write(1024, &data).unwrap();

        let mut buf = [0u8; 512];
        img.read(1024, &mut buf).unwrap();
        assert_eq!(buf, data);

        // Verify other regions are zero
        let mut zero_buf = [0u8; 512];
        img.read(0, &mut zero_buf).unwrap();
        assert_eq!(zero_buf, [0u8; 512]);
    }

    #[test]
    fn test_in_memory_bounds() {
        let img = InMemoryImage::new(512);

        // Read past end returns 0 bytes
        let mut buf = [0u8; 512];
        let read = img.read(1024, &mut buf).unwrap();
        assert_eq!(read, 0);

        // Partial read at boundary
        let read = img.read(256, &mut buf).unwrap();
        assert_eq!(read, 256);
    }

    #[test]
    fn test_in_memory_read_only() {
        let mut img = InMemoryImage::new(4096);
        img.set_read_only(true);

        let data = [0x42u8; 512];
        assert!(img.write(0, &data).is_err());
    }

    #[test]
    fn test_from_data() {
        let data = vec![0x42u8; 1024];
        let img = InMemoryImage::from_data(data.clone());

        let mut buf = [0u8; 1024];
        img.read(0, &mut buf).unwrap();
        assert_eq!(&buf[..], &data[..]);
    }

    #[test]
    fn test_qcow2_header_parse() {
        let mut bytes = vec![0u8; 104];
        // Magic (big-endian)
        bytes[0..4].copy_from_slice(&QCOW2_MAGIC.to_be_bytes());
        // Version (big-endian)
        bytes[4..8].copy_from_slice(&3u32.to_be_bytes());
        // Cluster bits
        bytes[20..24].copy_from_slice(&16u32.to_be_bytes());
        // Size
        bytes[24..32].copy_from_slice(&(1024u64 * 1024 * 1024).to_be_bytes());

        let header = Qcow2Header::from_bytes(&bytes).unwrap();

        assert!(header.is_valid());
        assert_eq!(header.version, 3);
        assert_eq!(header.cluster_size(), 65536);
        assert_eq!(header.size, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_qcow2_header_invalid() {
        let bytes = vec![0u8; 104];
        let header = Qcow2Header::from_bytes(&bytes).unwrap();

        assert!(!header.is_valid());
    }

    #[test]
    fn test_vhdx_header_parse() {
        let mut bytes = vec![0u8; 520];
        bytes[0..8].copy_from_slice(&VHDX_SIGNATURE.to_le_bytes());

        let header = VhdxFileHeader::from_bytes(&bytes).unwrap();

        assert!(header.is_valid());
    }

    #[test]
    fn test_format_detect_qcow2() {
        let mut header = vec![0u8; 8];
        header[0..4].copy_from_slice(&QCOW2_MAGIC.to_be_bytes());

        assert_eq!(ImageFormat::detect(&header), ImageFormat::Qcow2);
    }

    #[test]
    fn test_format_detect_vhdx() {
        let mut header = vec![0u8; 8];
        header[0..8].copy_from_slice(&VHDX_SIGNATURE.to_le_bytes());

        assert_eq!(ImageFormat::detect(&header), ImageFormat::Vhdx);
    }

    #[test]
    fn test_format_detect_raw() {
        let header = vec![0u8; 8];

        assert_eq!(ImageFormat::detect(&header), ImageFormat::Raw);
    }

    #[test]
    fn test_format_name() {
        assert_eq!(ImageFormat::Raw.name(), "raw");
        assert_eq!(ImageFormat::Qcow2.name(), "qcow2");
        assert_eq!(ImageFormat::Vhdx.name(), "vhdx");
    }

    #[test]
    fn test_flush() {
        let img = InMemoryImage::new(4096);
        assert!(img.flush().is_ok());
    }
}
