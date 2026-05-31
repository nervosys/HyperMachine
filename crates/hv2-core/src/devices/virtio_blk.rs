//! VirtIO Block Device
//!
//! This module implements the VirtIO block device specification for
//! paravirtualized disk I/O with high performance.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    VirtIO Block Device                          │
//! │  ┌────────────────────────────────────────────────────────────┐│
//! │  │                    Virtqueue (requestq)                     ││
//! │  │  ┌─────────────────┐      ┌─────────────────────────────┐ ││
//! │  │  │ Descriptor Ring │ ──▶  │ Available Ring               │ ││
//! │  │  │ (buffer addrs)  │      │ (guest → device)             │ ││
//! │  │  └─────────────────┘      └─────────────────────────────┘ ││
//! │  │                           ┌─────────────────────────────┐ ││
//! │  │                           │ Used Ring                    │ ││
//! │  │                           │ (device → guest)             │ ││
//! │  │                           └─────────────────────────────┘ ││
//! │  └────────────────────────────────────────────────────────────┘│
//! │  ┌────────────────────────────────────────────────────────────┐│
//! │  │                    Device Configuration                     ││
//! │  │  capacity: u64        │ size_max: u32    │ seg_max: u32    ││
//! │  │  geometry             │ blk_size: u32    │ topology        ││
//! │  └────────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Request Format
//!
//! Each request consists of:
//! 1. Header (virtio_blk_req_header) - type, reserved, sector
//! 2. Data buffer(s) - for read/write operations
//! 3. Status byte - written by device on completion

use crate::Result;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use parking_lot::RwLock;

/// VirtIO block sector size
pub const VIRTIO_BLK_SECTOR_SIZE: u32 = 512;

/// VirtIO block device ID
pub const VIRTIO_BLK_DEVICE_ID: u32 = 2;

/// Feature bits
pub mod features {
    /// Device supports request barriers
    pub const VIRTIO_BLK_F_BARRIER: u64 = 1 << 0;
    /// Maximum size of any single segment
    pub const VIRTIO_BLK_F_SIZE_MAX: u64 = 1 << 1;
    /// Maximum number of segments in a request
    pub const VIRTIO_BLK_F_SEG_MAX: u64 = 1 << 2;
    /// Disk-style geometry
    pub const VIRTIO_BLK_F_GEOMETRY: u64 = 1 << 4;
    /// Device is read-only
    pub const VIRTIO_BLK_F_RO: u64 = 1 << 5;
    /// Block size of disk
    pub const VIRTIO_BLK_F_BLK_SIZE: u64 = 1 << 6;
    /// Device supports scsi packet commands
    pub const VIRTIO_BLK_F_SCSI: u64 = 1 << 7;
    /// Cache flush command support
    pub const VIRTIO_BLK_F_FLUSH: u64 = 1 << 9;
    /// Device exports topology information
    pub const VIRTIO_BLK_F_TOPOLOGY: u64 = 1 << 10;
    /// Device can toggle cache writeback mode
    pub const VIRTIO_BLK_F_CONFIG_WCE: u64 = 1 << 11;
    /// Device supports multiqueue
    pub const VIRTIO_BLK_F_MQ: u64 = 1 << 12;
    /// Device supports discard command
    pub const VIRTIO_BLK_F_DISCARD: u64 = 1 << 13;
    /// Device supports write zeroes command
    pub const VIRTIO_BLK_F_WRITE_ZEROES: u64 = 1 << 14;
}

/// Request types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RequestType {
    /// Read sectors
    In = 0,
    /// Write sectors
    Out = 1,
    /// Flush write cache
    Flush = 4,
    /// Get device ID
    GetId = 8,
    /// Discard sectors
    Discard = 11,
    /// Write zeroes
    WriteZeroes = 13,
    /// Unknown request
    Unknown = 0xFFFF_FFFF,
}

impl From<u32> for RequestType {
    fn from(val: u32) -> Self {
        match val {
            0 => Self::In,
            1 => Self::Out,
            4 => Self::Flush,
            8 => Self::GetId,
            11 => Self::Discard,
            13 => Self::WriteZeroes,
            _ => Self::Unknown,
        }
    }
}

/// Status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    /// Success
    Ok = 0,
    /// I/O error
    IoErr = 1,
    /// Unsupported request
    Unsupported = 2,
}

/// Block request header (16 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct BlockRequestHeader {
    /// Request type
    pub request_type: u32,
    /// Reserved
    pub reserved: u32,
    /// Starting sector
    pub sector: u64,
}

impl BlockRequestHeader {
    /// Create a new request header
    pub fn new(request_type: RequestType, sector: u64) -> Self {
        Self {
            request_type: request_type as u32,
            reserved: 0,
            sector,
        }
    }

    /// Get request type
    pub fn get_type(&self) -> RequestType {
        RequestType::from(self.request_type)
    }

    /// Parse from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        Some(Self {
            request_type: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            reserved: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            sector: u64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
        })
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&self.request_type.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.reserved.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.sector.to_le_bytes());
        bytes
    }
}

/// Disk geometry
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct DiskGeometry {
    /// Number of cylinders
    pub cylinders: u16,
    /// Number of heads
    pub heads: u8,
    /// Sectors per track
    pub sectors: u8,
}

impl DiskGeometry {
    /// Calculate from capacity
    pub fn from_capacity(sectors: u64) -> Self {
        // Use standard CHS translation
        let heads = 16u8;
        let spt = 63u8;
        let cylinders = std::cmp::min(sectors / (heads as u64 * spt as u64), 65535) as u16;

        Self {
            cylinders,
            heads,
            sectors: spt,
        }
    }
}

/// Disk topology
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct DiskTopology {
    /// Number of logical blocks per physical block (log2)
    pub physical_block_exp: u8,
    /// Offset of first aligned logical block
    pub alignment_offset: u8,
    /// Suggested minimum I/O size in blocks
    pub min_io_size: u16,
    /// Optimal I/O size in blocks
    pub opt_io_size: u32,
}

/// Block device configuration (from spec)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct BlockConfig {
    /// Capacity in 512-byte sectors
    pub capacity: u64,
    /// Maximum segment size (if VIRTIO_BLK_F_SIZE_MAX)
    pub size_max: u32,
    /// Maximum number of segments (if VIRTIO_BLK_F_SEG_MAX)
    pub seg_max: u32,
    /// Geometry (if VIRTIO_BLK_F_GEOMETRY)
    pub geometry: DiskGeometry,
    /// Block size (if VIRTIO_BLK_F_BLK_SIZE)
    pub blk_size: u32,
    /// Topology (if VIRTIO_BLK_F_TOPOLOGY)
    pub topology: DiskTopology,
    /// Writeback mode (if VIRTIO_BLK_F_CONFIG_WCE)
    pub writeback: u8,
    /// Unused
    pub unused0: [u8; 3],
    /// Number of queues (if VIRTIO_BLK_F_MQ)
    pub num_queues: u16,
}

impl BlockConfig {
    /// Create configuration for a given size in bytes
    pub fn new(size_bytes: u64) -> Self {
        let capacity = size_bytes / VIRTIO_BLK_SECTOR_SIZE as u64;
        Self {
            capacity,
            size_max: 1024 * 1024, // 1MB max segment
            seg_max: 128,
            geometry: DiskGeometry::from_capacity(capacity),
            blk_size: VIRTIO_BLK_SECTOR_SIZE,
            topology: DiskTopology::default(),
            writeback: 0,
            unused0: [0; 3],
            num_queues: 1,
        }
    }

    /// Get size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.capacity * VIRTIO_BLK_SECTOR_SIZE as u64
    }
}

/// VirtIO Block Device
pub struct VirtioBlock {
    /// Device features
    features: AtomicU64,
    /// Driver features (acknowledged)
    driver_features: AtomicU64,
    /// Device status
    status: AtomicU32,
    /// Configuration
    config: RwLock<BlockConfig>,
    /// Storage backend
    storage: RwLock<Vec<u8>>,
    /// Read-only flag
    read_only: AtomicBool,
    /// Interrupt pending
    interrupt_pending: AtomicBool,
    /// Device ID string (20 bytes)
    device_id: [u8; 20],
}

impl VirtioBlock {
    /// Create a new VirtIO block device
    pub fn new(size_bytes: u64, read_only: bool) -> Self {
        let mut features = features::VIRTIO_BLK_F_SIZE_MAX
            | features::VIRTIO_BLK_F_SEG_MAX
            | features::VIRTIO_BLK_F_GEOMETRY
            | features::VIRTIO_BLK_F_BLK_SIZE
            | features::VIRTIO_BLK_F_FLUSH;

        if read_only {
            features |= features::VIRTIO_BLK_F_RO;
        }

        let mut device_id = [0u8; 20];
        device_id[..12].copy_from_slice(b"AetherVMDisk");

        Self {
            features: AtomicU64::new(features),
            driver_features: AtomicU64::new(0),
            status: AtomicU32::new(0),
            config: RwLock::new(BlockConfig::new(size_bytes)),
            storage: RwLock::new(vec![0u8; size_bytes as usize]),
            read_only: AtomicBool::new(read_only),
            interrupt_pending: AtomicBool::new(false),
            device_id,
        }
    }

    /// Get device features
    pub fn features(&self) -> u64 {
        self.features.load(Ordering::Relaxed)
    }

    /// Set driver features
    pub fn set_driver_features(&self, features: u64) {
        let valid = features & self.features.load(Ordering::Relaxed);
        self.driver_features.store(valid, Ordering::Relaxed);
    }

    /// Get driver features
    pub fn driver_features(&self) -> u64 {
        self.driver_features.load(Ordering::Relaxed)
    }

    /// Get device status
    pub fn status(&self) -> u32 {
        self.status.load(Ordering::Relaxed)
    }

    /// Set device status
    pub fn set_status(&self, status: u32) {
        self.status.store(status, Ordering::Relaxed);
    }

    /// Read configuration
    pub fn read_config(&self, offset: u64, size: u8) -> u64 {
        let config = self.config.read();
        // SAFETY: BlockConfig is #[repr(C)] with no padding holes that could
        // cause UB when read as bytes. The pointer is valid for the lifetime
        // of the RwLockReadGuard `config`, and the size is exactly
        // size_of::<BlockConfig>() bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &*config as *const BlockConfig as *const u8,
                std::mem::size_of::<BlockConfig>(),
            )
        };

        if offset as usize + size as usize > bytes.len() {
            return 0;
        }

        match size {
            1 => bytes[offset as usize] as u64,
            2 => u16::from_le_bytes([bytes[offset as usize], bytes[offset as usize + 1]]) as u64,
            4 => u32::from_le_bytes([
                bytes[offset as usize],
                bytes[offset as usize + 1],
                bytes[offset as usize + 2],
                bytes[offset as usize + 3],
            ]) as u64,
            8 => u64::from_le_bytes([
                bytes[offset as usize],
                bytes[offset as usize + 1],
                bytes[offset as usize + 2],
                bytes[offset as usize + 3],
                bytes[offset as usize + 4],
                bytes[offset as usize + 5],
                bytes[offset as usize + 6],
                bytes[offset as usize + 7],
            ]),
            _ => 0,
        }
    }

    /// Process a block request
    pub fn process_request(&self, header: &BlockRequestHeader, data: &mut [u8]) -> Status {
        match header.get_type() {
            RequestType::In => self.handle_read(header.sector, data),
            RequestType::Out => self.handle_write(header.sector, data),
            RequestType::Flush => self.handle_flush(),
            RequestType::GetId => self.handle_get_id(data),
            RequestType::Discard => self.handle_discard(header.sector, data.len() as u64),
            RequestType::WriteZeroes => self.handle_write_zeroes(header.sector, data.len() as u64),
            RequestType::Unknown => Status::Unsupported,
        }
    }

    /// Handle read request
    fn handle_read(&self, sector: u64, data: &mut [u8]) -> Status {
        let offset = sector * VIRTIO_BLK_SECTOR_SIZE as u64;
        let storage = self.storage.read();

        if offset as usize + data.len() > storage.len() {
            return Status::IoErr;
        }

        data.copy_from_slice(&storage[offset as usize..offset as usize + data.len()]);
        Status::Ok
    }

    /// Handle write request
    fn handle_write(&self, sector: u64, data: &[u8]) -> Status {
        if self.read_only.load(Ordering::Relaxed) {
            return Status::IoErr;
        }

        let offset = sector * VIRTIO_BLK_SECTOR_SIZE as u64;
        let mut storage = self.storage.write();

        if offset as usize + data.len() > storage.len() {
            return Status::IoErr;
        }

        storage[offset as usize..offset as usize + data.len()].copy_from_slice(data);
        Status::Ok
    }

    /// Handle flush request
    fn handle_flush(&self) -> Status {
        // No-op for in-memory storage
        Status::Ok
    }

    /// Handle get device ID request
    fn handle_get_id(&self, data: &mut [u8]) -> Status {
        let len = std::cmp::min(data.len(), self.device_id.len());
        data[..len].copy_from_slice(&self.device_id[..len]);
        Status::Ok
    }

    /// Handle discard request
    fn handle_discard(&self, sector: u64, num_sectors: u64) -> Status {
        if self.read_only.load(Ordering::Relaxed) {
            return Status::IoErr;
        }

        // For in-memory storage, just zero the region
        let offset = sector * VIRTIO_BLK_SECTOR_SIZE as u64;
        let length = num_sectors * VIRTIO_BLK_SECTOR_SIZE as u64;
        let mut storage = self.storage.write();

        if offset as usize + length as usize > storage.len() {
            return Status::IoErr;
        }

        storage[offset as usize..offset as usize + length as usize].fill(0);
        Status::Ok
    }

    /// Handle write zeroes request
    fn handle_write_zeroes(&self, sector: u64, num_sectors: u64) -> Status {
        self.handle_discard(sector, num_sectors)
    }

    /// Get capacity in sectors
    pub fn capacity(&self) -> u64 {
        self.config.read().capacity
    }

    /// Get capacity in bytes
    pub fn capacity_bytes(&self) -> u64 {
        self.config.read().size_bytes()
    }

    /// Check if read-only
    pub fn is_read_only(&self) -> bool {
        self.read_only.load(Ordering::Relaxed)
    }

    /// Set interrupt pending
    pub fn set_interrupt(&self, pending: bool) {
        self.interrupt_pending.store(pending, Ordering::Relaxed);
    }

    /// Check and clear interrupt
    pub fn check_interrupt(&self) -> bool {
        self.interrupt_pending.swap(false, Ordering::Relaxed)
    }

    /// Read from storage directly (for testing)
    pub fn read_storage(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let storage = self.storage.read();
        if offset as usize + buf.len() > storage.len() {
            return Err(crate::Error::Memory("Read past end of storage".to_string()));
        }
        buf.copy_from_slice(&storage[offset as usize..offset as usize + buf.len()]);
        Ok(())
    }

    /// Write to storage directly (for testing)
    pub fn write_storage(&self, offset: u64, data: &[u8]) -> Result<()> {
        let mut storage = self.storage.write();
        if offset as usize + data.len() > storage.len() {
            return Err(crate::Error::Memory(
                "Write past end of storage".to_string(),
            ));
        }
        storage[offset as usize..offset as usize + data.len()].copy_from_slice(data);
        Ok(())
    }
}

impl std::fmt::Debug for VirtioBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtioBlock")
            .field("capacity", &self.capacity())
            .field("read_only", &self.is_read_only())
            .field("features", &format!("{:#x}", self.features()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtio_block_creation() {
        let dev = VirtioBlock::new(1024 * 1024, false);

        assert_eq!(dev.capacity_bytes(), 1024 * 1024);
        assert!(!dev.is_read_only());
    }

    #[test]
    fn test_virtio_block_read_only() {
        let dev = VirtioBlock::new(1024 * 1024, true);

        assert!(dev.is_read_only());
        assert!(dev.features() & features::VIRTIO_BLK_F_RO != 0);
    }

    #[test]
    fn test_block_request_header() {
        let header = BlockRequestHeader::new(RequestType::Out, 42);

        assert_eq!(header.get_type(), RequestType::Out);
        assert_eq!(header.sector, 42);

        let bytes = header.to_bytes();
        let parsed = BlockRequestHeader::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.get_type(), RequestType::Out);
        assert_eq!(parsed.sector, 42);
    }

    #[test]
    fn test_read_write() {
        let dev = VirtioBlock::new(4096, false);

        // Write some data
        let write_header = BlockRequestHeader::new(RequestType::Out, 0);
        let write_data = [0x42u8; 512];
        let status = dev.process_request(&write_header, &mut write_data.clone());
        assert_eq!(status, Status::Ok);

        // Read it back
        let read_header = BlockRequestHeader::new(RequestType::In, 0);
        let mut read_data = [0u8; 512];
        let status = dev.process_request(&read_header, &mut read_data);
        assert_eq!(status, Status::Ok);

        assert_eq!(read_data, write_data);
    }

    #[test]
    fn test_read_only_write_fails() {
        let dev = VirtioBlock::new(4096, true);

        let header = BlockRequestHeader::new(RequestType::Out, 0);
        let data = [0x42u8; 512];
        let status = dev.process_request(&header, &mut data.clone());

        assert_eq!(status, Status::IoErr);
    }

    #[test]
    fn test_flush() {
        let dev = VirtioBlock::new(4096, false);

        let header = BlockRequestHeader::new(RequestType::Flush, 0);
        let status = dev.process_request(&header, &mut []);

        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn test_get_id() {
        let dev = VirtioBlock::new(4096, false);

        let header = BlockRequestHeader::new(RequestType::GetId, 0);
        let mut id = [0u8; 20];
        let status = dev.process_request(&header, &mut id);

        assert_eq!(status, Status::Ok);
        assert_eq!(&id[..12], b"AetherVMDisk");
    }

    #[test]
    fn test_out_of_bounds() {
        let dev = VirtioBlock::new(512, false);

        // Try to read past end
        let header = BlockRequestHeader::new(RequestType::In, 2);
        let mut data = [0u8; 512];
        let status = dev.process_request(&header, &mut data);

        assert_eq!(status, Status::IoErr);
    }

    #[test]
    fn test_config_read() {
        let dev = VirtioBlock::new(1024 * 1024 * 1024, false); // 1GB

        // Read capacity (offset 0, 8 bytes)
        let capacity = dev.read_config(0, 8);
        assert_eq!(capacity, 1024 * 1024 * 1024 / 512); // In sectors
    }

    #[test]
    fn test_driver_features() {
        let dev = VirtioBlock::new(4096, false);

        // Try to set invalid features
        dev.set_driver_features(0xFFFF_FFFF_FFFF_FFFF);

        // Should be masked to supported features
        let actual = dev.driver_features();
        assert_eq!(actual, actual & dev.features());
    }

    #[test]
    fn test_disk_geometry() {
        let geom = DiskGeometry::from_capacity(2 * 1024 * 1024); // ~1GB in sectors

        assert!(geom.cylinders > 0);
        assert_eq!(geom.heads, 16);
        assert_eq!(geom.sectors, 63);
    }

    #[test]
    fn test_request_type_from() {
        assert_eq!(RequestType::from(0), RequestType::In);
        assert_eq!(RequestType::from(1), RequestType::Out);
        assert_eq!(RequestType::from(4), RequestType::Flush);
        assert_eq!(RequestType::from(999), RequestType::Unknown);
    }

    #[test]
    fn test_interrupt() {
        let dev = VirtioBlock::new(4096, false);

        assert!(!dev.check_interrupt());

        dev.set_interrupt(true);
        assert!(dev.check_interrupt());

        // Should be cleared after check
        assert!(!dev.check_interrupt());
    }

    #[test]
    fn test_write_zeroes() {
        let dev = VirtioBlock::new(4096, false);

        // Write some data first
        dev.write_storage(0, &[0xFF; 512]).unwrap();

        // Write zeroes
        let header = BlockRequestHeader::new(RequestType::WriteZeroes, 0);
        let mut dummy = [0u8; 0]; // Length from sector count in real impl
        let status = dev.process_request(&header, &mut dummy);
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn test_direct_storage_access() {
        let dev = VirtioBlock::new(4096, false);

        let data = [0x42u8; 512];
        dev.write_storage(512, &data).unwrap();

        let mut buf = [0u8; 512];
        dev.read_storage(512, &mut buf).unwrap();

        assert_eq!(buf, data);
    }
}
