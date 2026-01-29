//! Snapshot types and core definitions
//!
//! This module provides the fundamental types used for VM state snapshots,
//! including snapshot identifiers, metadata, and state definitions.

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant, SystemTime};

/// Snapshot identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub u64);

impl SnapshotId {
    /// Create a new snapshot ID
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Generate a new unique ID based on timestamp
    pub fn generate() -> Self {
        use std::time::UNIX_EPOCH;
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self(duration.as_nanos() as u64)
    }
}

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "snap-{:016x}", self.0)
    }
}

impl From<u64> for SnapshotId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

/// Snapshot state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnapshotState {
    /// Snapshot is being created
    #[default]
    Creating,
    /// Snapshot is valid and complete
    Valid,
    /// Snapshot is being restored
    Restoring,
    /// Snapshot is corrupted or invalid
    Invalid,
    /// Snapshot is being deleted
    Deleting,
}

impl fmt::Display for SnapshotState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotState::Creating => write!(f, "creating"),
            SnapshotState::Valid => write!(f, "valid"),
            SnapshotState::Restoring => write!(f, "restoring"),
            SnapshotState::Invalid => write!(f, "invalid"),
            SnapshotState::Deleting => write!(f, "deleting"),
        }
    }
}

/// Snapshot type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnapshotType {
    /// Full snapshot with all state
    #[default]
    Full,
    /// Incremental snapshot (only changes since parent)
    Incremental,
    /// Memory-only snapshot (no device state)
    MemoryOnly,
    /// Checkpoint (lightweight, temporary)
    Checkpoint,
}

impl fmt::Display for SnapshotType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotType::Full => write!(f, "full"),
            SnapshotType::Incremental => write!(f, "incremental"),
            SnapshotType::MemoryOnly => write!(f, "memory-only"),
            SnapshotType::Checkpoint => write!(f, "checkpoint"),
        }
    }
}

/// Snapshot metadata
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// Unique snapshot identifier
    pub id: SnapshotId,
    /// User-provided name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Snapshot type
    pub snapshot_type: SnapshotType,
    /// Current state
    pub state: SnapshotState,
    /// Creation timestamp
    pub created_at: SystemTime,
    /// Parent snapshot ID (for incremental)
    pub parent_id: Option<SnapshotId>,
    /// Total size in bytes
    pub size_bytes: u64,
    /// Memory size in bytes
    pub memory_size: u64,
    /// Number of vCPUs
    pub vcpu_count: u32,
    /// Custom tags
    pub tags: HashMap<String, String>,
    /// VM name/identifier
    pub vm_name: String,
}

impl SnapshotInfo {
    /// Create new snapshot info
    pub fn new(id: SnapshotId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            description: None,
            snapshot_type: SnapshotType::Full,
            state: SnapshotState::Creating,
            created_at: SystemTime::now(),
            parent_id: None,
            size_bytes: 0,
            memory_size: 0,
            vcpu_count: 0,
            tags: HashMap::new(),
            vm_name: String::new(),
        }
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set snapshot type
    pub fn with_type(mut self, snapshot_type: SnapshotType) -> Self {
        self.snapshot_type = snapshot_type;
        self
    }

    /// Set parent snapshot
    pub fn with_parent(mut self, parent_id: SnapshotId) -> Self {
        self.parent_id = Some(parent_id);
        self.snapshot_type = SnapshotType::Incremental;
        self
    }

    /// Set VM name
    pub fn with_vm_name(mut self, name: impl Into<String>) -> Self {
        self.vm_name = name.into();
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Check if this is an incremental snapshot
    pub fn is_incremental(&self) -> bool {
        self.parent_id.is_some() || self.snapshot_type == SnapshotType::Incremental
    }

    /// Get age of snapshot
    pub fn age(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.created_at)
            .unwrap_or_default()
    }
}

/// CPU register state for snapshot
#[derive(Debug, Clone, Default)]
pub struct CpuSnapshot {
    /// vCPU ID
    pub vcpu_id: u32,
    /// General purpose registers
    pub gprs: [u64; 16],
    /// RIP (instruction pointer)
    pub rip: u64,
    /// RFLAGS
    pub rflags: u64,
    /// Control registers (CR0, CR2, CR3, CR4, CR8)
    pub crs: [u64; 5],
    /// Segment registers (CS, DS, ES, FS, GS, SS)
    pub segments: [SegmentSnapshot; 6],
    /// GDTR
    pub gdtr: DescriptorTableSnapshot,
    /// IDTR
    pub idtr: DescriptorTableSnapshot,
    /// LDTR
    pub ldtr: SegmentSnapshot,
    /// TR (task register)
    pub tr: SegmentSnapshot,
    /// MSRs
    pub msrs: Vec<(u32, u64)>,
    /// XCR0 (extended control register)
    pub xcr0: u64,
    /// FPU/SSE state
    pub fpu_state: Vec<u8>,
    /// APIC state
    pub apic_state: Vec<u8>,
}

impl CpuSnapshot {
    /// Create a new CPU snapshot
    pub fn new(vcpu_id: u32) -> Self {
        Self {
            vcpu_id,
            ..Default::default()
        }
    }

    /// Set general purpose register
    pub fn set_gpr(&mut self, index: usize, value: u64) {
        if index < 16 {
            self.gprs[index] = value;
        }
    }

    /// Get general purpose register
    pub fn gpr(&self, index: usize) -> u64 {
        self.gprs.get(index).copied().unwrap_or(0)
    }

    /// Set control register
    pub fn set_cr(&mut self, index: usize, value: u64) {
        if index < 5 {
            self.crs[index] = value;
        }
    }

    /// Add MSR value
    pub fn add_msr(&mut self, msr: u32, value: u64) {
        self.msrs.push((msr, value));
    }

    /// Get MSR value
    pub fn get_msr(&self, msr: u32) -> Option<u64> {
        self.msrs.iter().find(|(m, _)| *m == msr).map(|(_, v)| *v)
    }

    /// Estimated size in bytes
    pub fn size_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.msrs.len() * 12
            + self.fpu_state.len()
            + self.apic_state.len()
    }
}

/// Segment register snapshot
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentSnapshot {
    /// Selector
    pub selector: u16,
    /// Base address
    pub base: u64,
    /// Limit
    pub limit: u32,
    /// Access rights
    pub access_rights: u32,
}

impl SegmentSnapshot {
    /// Create a new segment snapshot
    pub fn new(selector: u16, base: u64, limit: u32, access_rights: u32) -> Self {
        Self {
            selector,
            base,
            limit,
            access_rights,
        }
    }
}

/// Descriptor table snapshot (GDTR/IDTR)
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorTableSnapshot {
    /// Base address
    pub base: u64,
    /// Limit
    pub limit: u16,
}

impl DescriptorTableSnapshot {
    /// Create a new descriptor table snapshot
    pub fn new(base: u64, limit: u16) -> Self {
        Self { base, limit }
    }
}

/// Memory region snapshot metadata
#[derive(Debug, Clone)]
pub struct MemoryRegionSnapshot {
    /// Guest physical address start
    pub gpa_start: u64,
    /// Region size in bytes
    pub size: u64,
    /// Offset in snapshot file
    pub file_offset: u64,
    /// Compressed size (if compressed)
    pub compressed_size: Option<u64>,
    /// Checksum
    pub checksum: u32,
    /// Is dirty (for incremental)
    pub is_dirty: bool,
    /// Compression algorithm used
    pub compression: CompressionType,
}

impl MemoryRegionSnapshot {
    /// Create a new memory region snapshot
    pub fn new(gpa_start: u64, size: u64) -> Self {
        Self {
            gpa_start,
            size,
            file_offset: 0,
            compressed_size: None,
            checksum: 0,
            is_dirty: true,
            compression: CompressionType::None,
        }
    }

    /// Set file offset
    pub fn with_offset(mut self, offset: u64) -> Self {
        self.file_offset = offset;
        self
    }

    /// Set compression
    pub fn with_compression(mut self, compression: CompressionType, compressed_size: u64) -> Self {
        self.compression = compression;
        self.compressed_size = Some(compressed_size);
        self
    }

    /// Get the actual size on disk
    pub fn disk_size(&self) -> u64 {
        self.compressed_size.unwrap_or(self.size)
    }

    /// Compression ratio (1.0 = no compression)
    pub fn compression_ratio(&self) -> f64 {
        if let Some(compressed) = self.compressed_size {
            if compressed > 0 {
                return self.size as f64 / compressed as f64;
            }
        }
        1.0
    }
}

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionType {
    /// No compression
    #[default]
    None,
    /// LZ4 fast compression
    Lz4,
    /// Zstd compression
    Zstd,
    /// Deflate/zlib compression
    Deflate,
}

impl fmt::Display for CompressionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompressionType::None => write!(f, "none"),
            CompressionType::Lz4 => write!(f, "lz4"),
            CompressionType::Zstd => write!(f, "zstd"),
            CompressionType::Deflate => write!(f, "deflate"),
        }
    }
}

/// Device state snapshot
#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    /// Device type identifier
    pub device_type: String,
    /// Device instance name
    pub name: String,
    /// Serialized device state
    pub state_data: Vec<u8>,
    /// State format version
    pub version: u32,
    /// Checksum
    pub checksum: u32,
}

impl DeviceSnapshot {
    /// Create a new device snapshot
    pub fn new(device_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            device_type: device_type.into(),
            name: name.into(),
            state_data: Vec::new(),
            version: 1,
            checksum: 0,
        }
    }

    /// Set state data
    pub fn with_state(mut self, data: Vec<u8>) -> Self {
        self.state_data = data;
        self
    }

    /// Set version
    pub fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Size in bytes
    pub fn size_bytes(&self) -> usize {
        self.state_data.len()
    }

    /// Calculate checksum
    pub fn calculate_checksum(&mut self) {
        self.checksum = crc32_checksum(&self.state_data);
    }

    /// Verify checksum
    pub fn verify_checksum(&self) -> bool {
        self.checksum == crc32_checksum(&self.state_data)
    }
}

/// Complete VM snapshot
#[derive(Debug, Clone)]
pub struct VmSnapshot {
    /// Snapshot metadata
    pub info: SnapshotInfo,
    /// CPU states
    pub cpus: Vec<CpuSnapshot>,
    /// Memory regions
    pub memory_regions: Vec<MemoryRegionSnapshot>,
    /// Device states
    pub devices: Vec<DeviceSnapshot>,
    /// Timestamp when snapshot capture started
    pub capture_start: Instant,
    /// Timestamp when snapshot capture completed
    pub capture_end: Option<Instant>,
}

impl VmSnapshot {
    /// Create a new VM snapshot
    pub fn new(info: SnapshotInfo) -> Self {
        Self {
            info,
            cpus: Vec::new(),
            memory_regions: Vec::new(),
            devices: Vec::new(),
            capture_start: Instant::now(),
            capture_end: None,
        }
    }

    /// Add CPU state
    pub fn add_cpu(&mut self, cpu: CpuSnapshot) {
        self.cpus.push(cpu);
        self.info.vcpu_count = self.cpus.len() as u32;
    }

    /// Add memory region
    pub fn add_memory_region(&mut self, region: MemoryRegionSnapshot) {
        self.info.memory_size += region.size;
        self.info.size_bytes += region.disk_size();
        self.memory_regions.push(region);
    }

    /// Add device state
    pub fn add_device(&mut self, device: DeviceSnapshot) {
        self.info.size_bytes += device.size_bytes() as u64;
        self.devices.push(device);
    }

    /// Mark snapshot as complete
    pub fn complete(&mut self) {
        self.capture_end = Some(Instant::now());
        self.info.state = SnapshotState::Valid;
    }

    /// Get capture duration
    pub fn capture_duration(&self) -> Duration {
        self.capture_end
            .map(|end| end.duration_since(self.capture_start))
            .unwrap_or_else(|| self.capture_start.elapsed())
    }

    /// Get CPU by ID
    pub fn get_cpu(&self, vcpu_id: u32) -> Option<&CpuSnapshot> {
        self.cpus.iter().find(|c| c.vcpu_id == vcpu_id)
    }

    /// Get device by name
    pub fn get_device(&self, name: &str) -> Option<&DeviceSnapshot> {
        self.devices.iter().find(|d| d.name == name)
    }

    /// Total size including overhead
    pub fn total_size(&self) -> u64 {
        self.info.size_bytes
    }
}

/// Snapshot operation statistics
#[derive(Debug, Clone, Default)]
pub struct SnapshotStats {
    /// Total snapshots created
    pub snapshots_created: u64,
    /// Total snapshots restored
    pub snapshots_restored: u64,
    /// Total snapshots deleted
    pub snapshots_deleted: u64,
    /// Failed snapshot operations
    pub failures: u64,
    /// Total bytes saved
    pub bytes_saved: u64,
    /// Total bytes restored
    pub bytes_restored: u64,
    /// Total time spent creating snapshots
    pub creation_time_us: u64,
    /// Total time spent restoring snapshots
    pub restore_time_us: u64,
    /// Current active snapshot operations
    pub active_operations: u32,
    /// Compression savings (bytes)
    pub compression_savings: u64,
}

impl SnapshotStats {
    /// Record a snapshot creation
    pub fn record_creation(&mut self, size: u64, duration: Duration) {
        self.snapshots_created += 1;
        self.bytes_saved += size;
        self.creation_time_us += duration.as_micros() as u64;
    }

    /// Record a snapshot restoration
    pub fn record_restore(&mut self, size: u64, duration: Duration) {
        self.snapshots_restored += 1;
        self.bytes_restored += size;
        self.restore_time_us += duration.as_micros() as u64;
    }

    /// Record a failure
    pub fn record_failure(&mut self) {
        self.failures += 1;
    }

    /// Record deletion
    pub fn record_deletion(&mut self) {
        self.snapshots_deleted += 1;
    }

    /// Average creation time
    pub fn avg_creation_time(&self) -> Duration {
        if self.snapshots_created == 0 {
            return Duration::ZERO;
        }
        Duration::from_micros(self.creation_time_us / self.snapshots_created)
    }

    /// Average restore time
    pub fn avg_restore_time(&self) -> Duration {
        if self.snapshots_restored == 0 {
            return Duration::ZERO;
        }
        Duration::from_micros(self.restore_time_us / self.snapshots_restored)
    }

    /// Success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.snapshots_created + self.snapshots_restored + self.failures;
        if total == 0 {
            return 1.0;
        }
        1.0 - (self.failures as f64 / total as f64)
    }
}

/// Simple CRC32 checksum
fn crc32_checksum(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB88320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_id_creation() {
        let id = SnapshotId::new(12345);
        assert_eq!(id.value(), 12345);
    }

    #[test]
    fn test_snapshot_id_generate() {
        let id1 = SnapshotId::generate();
        let id2 = SnapshotId::generate();
        // Should be different (unless generated in same nanosecond)
        assert!(id1.value() > 0);
        assert!(id2.value() > 0);
    }

    #[test]
    fn test_snapshot_id_display() {
        let id = SnapshotId::new(0x123456789ABCDEF0);
        let display = format!("{}", id);
        assert!(display.starts_with("snap-"));
    }

    #[test]
    fn test_snapshot_state_display() {
        assert_eq!(format!("{}", SnapshotState::Valid), "valid");
        assert_eq!(format!("{}", SnapshotState::Creating), "creating");
    }

    #[test]
    fn test_snapshot_type_display() {
        assert_eq!(format!("{}", SnapshotType::Full), "full");
        assert_eq!(format!("{}", SnapshotType::Incremental), "incremental");
    }

    #[test]
    fn test_snapshot_info_creation() {
        let id = SnapshotId::new(1);
        let info = SnapshotInfo::new(id, "test-snapshot")
            .with_description("Test description")
            .with_vm_name("test-vm")
            .with_tag("env", "test");

        assert_eq!(info.name, "test-snapshot");
        assert_eq!(info.description, Some("Test description".to_string()));
        assert_eq!(info.vm_name, "test-vm");
        assert_eq!(info.tags.get("env"), Some(&"test".to_string()));
    }

    #[test]
    fn test_snapshot_info_incremental() {
        let parent_id = SnapshotId::new(1);
        let id = SnapshotId::new(2);
        let info = SnapshotInfo::new(id, "child").with_parent(parent_id);

        assert!(info.is_incremental());
        assert_eq!(info.parent_id, Some(parent_id));
    }

    #[test]
    fn test_cpu_snapshot_creation() {
        let mut cpu = CpuSnapshot::new(0);
        cpu.set_gpr(0, 0x1234);
        cpu.rip = 0x7FFF0000;
        cpu.add_msr(0x1A0, 0x5678);

        assert_eq!(cpu.gpr(0), 0x1234);
        assert_eq!(cpu.rip, 0x7FFF0000);
        assert_eq!(cpu.get_msr(0x1A0), Some(0x5678));
        assert_eq!(cpu.get_msr(0x1A1), None);
    }

    #[test]
    fn test_cpu_snapshot_size() {
        let cpu = CpuSnapshot::new(0);
        assert!(cpu.size_bytes() > 0);
    }

    #[test]
    fn test_segment_snapshot() {
        let seg = SegmentSnapshot::new(0x10, 0x1000, 0xFFFF, 0x93);
        assert_eq!(seg.selector, 0x10);
        assert_eq!(seg.base, 0x1000);
        assert_eq!(seg.limit, 0xFFFF);
    }

    #[test]
    fn test_descriptor_table_snapshot() {
        let dt = DescriptorTableSnapshot::new(0x80000000, 0x1FF);
        assert_eq!(dt.base, 0x80000000);
        assert_eq!(dt.limit, 0x1FF);
    }

    #[test]
    fn test_memory_region_snapshot() {
        let region = MemoryRegionSnapshot::new(0x0, 0x100000)
            .with_offset(4096)
            .with_compression(CompressionType::Lz4, 0x80000);

        assert_eq!(region.gpa_start, 0x0);
        assert_eq!(region.size, 0x100000);
        assert_eq!(region.file_offset, 4096);
        assert_eq!(region.disk_size(), 0x80000);
        assert!(region.compression_ratio() > 1.0);
    }

    #[test]
    fn test_compression_type_display() {
        assert_eq!(format!("{}", CompressionType::None), "none");
        assert_eq!(format!("{}", CompressionType::Lz4), "lz4");
        assert_eq!(format!("{}", CompressionType::Zstd), "zstd");
    }

    #[test]
    fn test_device_snapshot() {
        let mut device = DeviceSnapshot::new("virtio-blk", "disk0")
            .with_state(vec![1, 2, 3, 4])
            .with_version(2);

        device.calculate_checksum();
        assert!(device.verify_checksum());
        assert_eq!(device.size_bytes(), 4);
    }

    #[test]
    fn test_vm_snapshot() {
        let id = SnapshotId::new(1);
        let info = SnapshotInfo::new(id, "test");
        let mut snapshot = VmSnapshot::new(info);

        snapshot.add_cpu(CpuSnapshot::new(0));
        snapshot.add_cpu(CpuSnapshot::new(1));
        snapshot.add_memory_region(MemoryRegionSnapshot::new(0, 0x100000));
        snapshot.add_device(DeviceSnapshot::new("serial", "com1").with_state(vec![0; 64]));

        assert_eq!(snapshot.info.vcpu_count, 2);
        assert!(snapshot.info.memory_size > 0);
        assert!(snapshot.get_cpu(0).is_some());
        assert!(snapshot.get_device("com1").is_some());
    }

    #[test]
    fn test_vm_snapshot_complete() {
        let id = SnapshotId::new(1);
        let info = SnapshotInfo::new(id, "test");
        let mut snapshot = VmSnapshot::new(info);

        std::thread::sleep(std::time::Duration::from_millis(1));
        snapshot.complete();

        assert_eq!(snapshot.info.state, SnapshotState::Valid);
        assert!(snapshot.capture_duration().as_micros() > 0);
    }

    #[test]
    fn test_snapshot_stats() {
        let mut stats = SnapshotStats::default();

        stats.record_creation(1000, Duration::from_millis(10));
        stats.record_creation(2000, Duration::from_millis(20));
        stats.record_restore(1500, Duration::from_millis(15));
        stats.record_failure();

        assert_eq!(stats.snapshots_created, 2);
        assert_eq!(stats.snapshots_restored, 1);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.bytes_saved, 3000);
        assert!(stats.avg_creation_time().as_millis() > 0);
        assert!(stats.success_rate() < 1.0);
    }

    #[test]
    fn test_crc32_checksum() {
        let data = b"Hello, World!";
        let crc1 = crc32_checksum(data);
        let crc2 = crc32_checksum(data);
        assert_eq!(crc1, crc2);

        let other = b"Hello, World?";
        let crc3 = crc32_checksum(other);
        assert_ne!(crc1, crc3);
    }

    #[test]
    fn test_snapshot_info_age() {
        let id = SnapshotId::new(1);
        let info = SnapshotInfo::new(id, "test");
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(info.age().as_micros() > 0);
    }
}
