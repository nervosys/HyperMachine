//! Memory snapshot management
//!
//! This module provides memory capture, storage, and restoration functionality
//! for VM snapshots, including dirty page tracking and compression support.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use super::types::{CompressionType, MemoryRegionSnapshot};

/// Memory snapshot result type
pub type MemoryResult<T> = Result<T, MemorySnapshotError>;

/// Memory snapshot error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemorySnapshotError {
    /// Memory region not found
    RegionNotFound(u64),
    /// Invalid address
    InvalidAddress { gpa: u64, size: u64 },
    /// Compression error
    CompressionError(String),
    /// Decompression error
    DecompressionError(String),
    /// I/O error
    IoError(String),
    /// Checksum mismatch
    ChecksumMismatch { expected: u32, actual: u32 },
    /// Buffer too small
    BufferTooSmall { required: usize, provided: usize },
    /// Page not dirty
    PageNotDirty(u64),
}

impl std::fmt::Display for MemorySnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemorySnapshotError::RegionNotFound(gpa) => {
                write!(f, "Memory region not found at GPA {:#x}", gpa)
            }
            MemorySnapshotError::InvalidAddress { gpa, size } => {
                write!(f, "Invalid memory address: GPA {:#x}, size {}", gpa, size)
            }
            MemorySnapshotError::CompressionError(msg) => {
                write!(f, "Compression error: {}", msg)
            }
            MemorySnapshotError::DecompressionError(msg) => {
                write!(f, "Decompression error: {}", msg)
            }
            MemorySnapshotError::IoError(msg) => write!(f, "I/O error: {}", msg),
            MemorySnapshotError::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Checksum mismatch: expected {:#x}, got {:#x}",
                    expected, actual
                )
            }
            MemorySnapshotError::BufferTooSmall { required, provided } => {
                write!(
                    f,
                    "Buffer too small: required {}, provided {}",
                    required, provided
                )
            }
            MemorySnapshotError::PageNotDirty(gpa) => {
                write!(f, "Page at GPA {:#x} is not dirty", gpa)
            }
        }
    }
}

impl std::error::Error for MemorySnapshotError {}

/// Page size constant
pub const PAGE_SIZE: u64 = 4096;
pub const LARGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;
pub const HUGE_PAGE_SIZE: u64 = 1024 * 1024 * 1024;

/// Memory page state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageState {
    /// Page is clean (not modified)
    #[default]
    Clean,
    /// Page is dirty (modified)
    Dirty,
    /// Page is zero-filled
    Zero,
    /// Page is not present
    NotPresent,
}

/// Dirty page tracker for incremental snapshots
#[derive(Debug)]
pub struct DirtyPageTracker {
    /// Dirty page bitmap (one bit per page)
    bitmap: Vec<u64>,
    /// Base GPA
    base_gpa: u64,
    /// Total tracked pages
    page_count: u64,
    /// Number of dirty pages
    dirty_count: u64,
    /// Page size
    page_size: u64,
    /// Generation counter
    generation: u64,
}

impl DirtyPageTracker {
    /// Create a new dirty page tracker
    pub fn new(base_gpa: u64, size: u64) -> Self {
        let page_count = size.div_ceil(PAGE_SIZE);
        let bitmap_size = (page_count.div_ceil(64)) as usize;

        Self {
            bitmap: vec![0; bitmap_size],
            base_gpa,
            page_count,
            dirty_count: 0,
            page_size: PAGE_SIZE,
            generation: 0,
        }
    }

    /// Create with custom page size
    pub fn with_page_size(base_gpa: u64, size: u64, page_size: u64) -> Self {
        let page_count = size.div_ceil(page_size);
        let bitmap_size = (page_count.div_ceil(64)) as usize;

        Self {
            bitmap: vec![0; bitmap_size],
            base_gpa,
            page_count,
            dirty_count: 0,
            page_size,
            generation: 0,
        }
    }

    /// Mark a page as dirty
    pub fn mark_dirty(&mut self, gpa: u64) -> bool {
        if let Some((idx, bit)) = self.gpa_to_index(gpa) {
            if self.bitmap[idx] & (1 << bit) == 0 {
                self.bitmap[idx] |= 1 << bit;
                self.dirty_count += 1;
                return true;
            }
        }
        false
    }

    /// Mark a range as dirty
    pub fn mark_dirty_range(&mut self, gpa: u64, size: u64) {
        let end_gpa = gpa + size;
        let mut current = gpa & !(self.page_size - 1);
        while current < end_gpa {
            self.mark_dirty(current);
            current += self.page_size;
        }
    }

    /// Check if a page is dirty
    pub fn is_dirty(&self, gpa: u64) -> bool {
        if let Some((idx, bit)) = self.gpa_to_index(gpa) {
            return self.bitmap[idx] & (1 << bit) != 0;
        }
        false
    }

    /// Clear dirty status for a page
    pub fn clear_dirty(&mut self, gpa: u64) -> bool {
        if let Some((idx, bit)) = self.gpa_to_index(gpa) {
            if self.bitmap[idx] & (1 << bit) != 0 {
                self.bitmap[idx] &= !(1 << bit);
                self.dirty_count = self.dirty_count.saturating_sub(1);
                return true;
            }
        }
        false
    }

    /// Clear all dirty pages and increment generation
    pub fn clear_all(&mut self) {
        self.bitmap.fill(0);
        self.dirty_count = 0;
        self.generation += 1;
    }

    /// Get number of dirty pages
    pub fn dirty_count(&self) -> u64 {
        self.dirty_count
    }

    /// Get total page count
    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    /// Get dirty ratio
    pub fn dirty_ratio(&self) -> f64 {
        if self.page_count == 0 {
            return 0.0;
        }
        self.dirty_count as f64 / self.page_count as f64
    }

    /// Get generation counter
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Iterate over dirty page GPAs
    pub fn dirty_pages(&self) -> impl Iterator<Item = u64> + '_ {
        DirtyPageIterator {
            tracker: self,
            word_idx: 0,
            bit_idx: 0,
        }
    }

    /// Convert GPA to bitmap index and bit
    fn gpa_to_index(&self, gpa: u64) -> Option<(usize, usize)> {
        if gpa < self.base_gpa {
            return None;
        }
        let offset = gpa - self.base_gpa;
        let page_idx = offset / self.page_size;
        if page_idx >= self.page_count {
            return None;
        }
        let word_idx = (page_idx / 64) as usize;
        let bit_idx = (page_idx % 64) as usize;
        if word_idx >= self.bitmap.len() {
            return None;
        }
        Some((word_idx, bit_idx))
    }
}

/// Iterator over dirty pages
pub struct DirtyPageIterator<'a> {
    tracker: &'a DirtyPageTracker,
    word_idx: usize,
    bit_idx: usize,
}

impl<'a> Iterator for DirtyPageIterator<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        while self.word_idx < self.tracker.bitmap.len() {
            let word = self.tracker.bitmap[self.word_idx];
            while self.bit_idx < 64 {
                let bit = self.bit_idx;
                self.bit_idx += 1;

                if word & (1 << bit) != 0 {
                    let page_idx = self.word_idx as u64 * 64 + bit as u64;
                    if page_idx < self.tracker.page_count {
                        return Some(self.tracker.base_gpa + page_idx * self.tracker.page_size);
                    }
                }
            }
            self.word_idx += 1;
            self.bit_idx = 0;
        }
        None
    }
}

/// Memory snapshot configuration
#[derive(Debug, Clone)]
pub struct MemorySnapshotConfig {
    /// Compression algorithm
    pub compression: CompressionType,
    /// Compression level (0-9, higher = better compression)
    pub compression_level: u8,
    /// Deduplicate zero pages
    pub dedupe_zeros: bool,
    /// Parallel compression threads
    pub threads: usize,
    /// Chunk size for parallel processing
    pub chunk_size: u64,
    /// Verify checksums on restore
    pub verify_checksums: bool,
}

impl Default for MemorySnapshotConfig {
    fn default() -> Self {
        Self {
            compression: CompressionType::None,
            compression_level: 3,
            dedupe_zeros: true,
            threads: 1,
            chunk_size: 1024 * 1024, // 1MB chunks
            verify_checksums: true,
        }
    }
}

impl MemorySnapshotConfig {
    /// Create config optimized for speed
    pub fn fast() -> Self {
        Self {
            compression: CompressionType::Lz4,
            compression_level: 1,
            dedupe_zeros: true,
            threads: 4,
            chunk_size: 4 * 1024 * 1024,
            verify_checksums: false,
        }
    }

    /// Create config optimized for size
    pub fn compact() -> Self {
        Self {
            compression: CompressionType::Zstd,
            compression_level: 9,
            dedupe_zeros: true,
            threads: 2,
            chunk_size: 1024 * 1024,
            verify_checksums: true,
        }
    }
}

/// Memory snapshot manager
#[derive(Debug)]
pub struct MemorySnapshotManager {
    /// Memory regions being tracked
    regions: HashMap<u64, MemoryRegion>,
    /// Dirty page tracker
    dirty_tracker: Option<DirtyPageTracker>,
    /// Configuration
    config: MemorySnapshotConfig,
    /// Statistics
    stats: MemorySnapshotStats,
}

/// In-memory representation of a region
#[derive(Debug)]
struct MemoryRegion {
    gpa_start: u64,
    size: u64,
    data: Vec<u8>,
    page_states: Vec<PageState>,
}

impl MemorySnapshotManager {
    /// Create a new memory snapshot manager
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
            dirty_tracker: None,
            config: MemorySnapshotConfig::default(),
            stats: MemorySnapshotStats::default(),
        }
    }

    /// Create with configuration
    pub fn with_config(config: MemorySnapshotConfig) -> Self {
        Self {
            regions: HashMap::new(),
            dirty_tracker: None,
            config,
            stats: MemorySnapshotStats::default(),
        }
    }

    /// Add a memory region to track
    pub fn add_region(&mut self, gpa_start: u64, size: u64) {
        let page_count = (size.div_ceil(PAGE_SIZE)) as usize;
        self.regions.insert(
            gpa_start,
            MemoryRegion {
                gpa_start,
                size,
                data: vec![0; size as usize],
                page_states: vec![PageState::Clean; page_count],
            },
        );
    }

    /// Enable dirty tracking
    pub fn enable_dirty_tracking(&mut self, base_gpa: u64, size: u64) {
        self.dirty_tracker = Some(DirtyPageTracker::new(base_gpa, size));
    }

    /// Mark address as dirty
    pub fn mark_dirty(&mut self, gpa: u64) {
        if let Some(tracker) = &mut self.dirty_tracker {
            tracker.mark_dirty(gpa);
        }
    }

    /// Write data to a region
    pub fn write(&mut self, gpa: u64, data: &[u8]) -> MemoryResult<()> {
        // Find the region containing this GPA
        let region = self
            .regions
            .values_mut()
            .find(|r| gpa >= r.gpa_start && gpa < r.gpa_start + r.size)
            .ok_or(MemorySnapshotError::RegionNotFound(gpa))?;

        let offset = (gpa - region.gpa_start) as usize;
        if offset + data.len() > region.data.len() {
            return Err(MemorySnapshotError::InvalidAddress {
                gpa,
                size: data.len() as u64,
            });
        }

        region.data[offset..offset + data.len()].copy_from_slice(data);

        // Mark pages as dirty
        let start_page = offset / PAGE_SIZE as usize;
        let end_page = (offset + data.len()).div_ceil(PAGE_SIZE as usize);
        for page in start_page..end_page.min(region.page_states.len()) {
            region.page_states[page] = PageState::Dirty;
        }

        // Update dirty tracker
        if let Some(tracker) = &mut self.dirty_tracker {
            tracker.mark_dirty_range(gpa, data.len() as u64);
        }

        Ok(())
    }

    /// Read data from a region
    pub fn read(&self, gpa: u64, size: usize) -> MemoryResult<Vec<u8>> {
        let region = self
            .regions
            .values()
            .find(|r| gpa >= r.gpa_start && gpa < r.gpa_start + r.size)
            .ok_or(MemorySnapshotError::RegionNotFound(gpa))?;

        let offset = (gpa - region.gpa_start) as usize;
        if offset + size > region.data.len() {
            return Err(MemorySnapshotError::InvalidAddress {
                gpa,
                size: size as u64,
            });
        }

        Ok(region.data[offset..offset + size].to_vec())
    }

    /// Capture all memory regions
    pub fn capture_all(&mut self) -> Vec<MemoryRegionSnapshot> {
        let start = Instant::now();
        let mut snapshots = Vec::new();
        let mut offset = 0u64;

        for region in self.regions.values() {
            let mut snapshot = MemoryRegionSnapshot::new(region.gpa_start, region.size);
            snapshot.file_offset = offset;
            snapshot.checksum = crc32_checksum(&region.data);

            // Check for zero pages
            let zero_pages = self.count_zero_pages(region);
            if self.config.dedupe_zeros && zero_pages > 0 {
                self.stats.zero_pages_deduped += zero_pages;
            }

            // Apply compression if enabled
            if self.config.compression != CompressionType::None {
                let compressed = self.compress_region(region);
                if compressed.len() < region.data.len() {
                    snapshot =
                        snapshot.with_compression(self.config.compression, compressed.len() as u64);
                    self.stats.compression_savings += region.size - compressed.len() as u64;
                }
            }

            offset += snapshot.disk_size();
            self.stats.bytes_captured += region.size;
            snapshots.push(snapshot);
        }

        self.stats.capture_time_us += start.elapsed().as_micros() as u64;
        self.stats.captures += 1;

        snapshots
    }

    /// Capture only dirty pages
    pub fn capture_dirty(&mut self) -> MemoryResult<Vec<MemoryRegionSnapshot>> {
        let tracker = self
            .dirty_tracker
            .as_ref()
            .ok_or(MemorySnapshotError::IoError(
                "Dirty tracking not enabled".to_string(),
            ))?;

        let start = Instant::now();
        let mut snapshots = Vec::new();
        let mut offset = 0u64;

        for gpa in tracker.dirty_pages() {
            if let Some(region) = self
                .regions
                .values()
                .find(|r| gpa >= r.gpa_start && gpa < r.gpa_start + r.size)
            {
                let page_offset = (gpa - region.gpa_start) as usize;
                let page_data = &region.data[page_offset..page_offset + PAGE_SIZE as usize];

                let mut snapshot = MemoryRegionSnapshot::new(gpa, PAGE_SIZE);
                snapshot.file_offset = offset;
                snapshot.checksum = crc32_checksum(page_data);
                snapshot.is_dirty = true;

                offset += PAGE_SIZE;
                snapshots.push(snapshot);
            }
        }

        self.stats.capture_time_us += start.elapsed().as_micros() as u64;
        self.stats.dirty_captures += 1;
        self.stats.dirty_pages_captured += snapshots.len() as u64;

        Ok(snapshots)
    }

    /// Get dirty page count
    pub fn dirty_page_count(&self) -> u64 {
        self.dirty_tracker
            .as_ref()
            .map(|t| t.dirty_count())
            .unwrap_or(0)
    }

    /// Clear dirty tracking
    pub fn clear_dirty(&mut self) {
        if let Some(tracker) = &mut self.dirty_tracker {
            tracker.clear_all();
        }
    }

    /// Get statistics
    pub fn stats(&self) -> &MemorySnapshotStats {
        &self.stats
    }

    /// Count zero pages in a region
    fn count_zero_pages(&self, region: &MemoryRegion) -> u64 {
        let mut count = 0;
        let mut offset = 0;
        while offset + PAGE_SIZE as usize <= region.data.len() {
            let page = &region.data[offset..offset + PAGE_SIZE as usize];
            if page.iter().all(|&b| b == 0) {
                count += 1;
            }
            offset += PAGE_SIZE as usize;
        }
        count
    }

    /// Compress a region (simplified - actual impl would use real compression)
    fn compress_region(&self, region: &MemoryRegion) -> Vec<u8> {
        // Simplified compression simulation
        // Real implementation would use LZ4, Zstd, etc.
        region.data.clone()
    }
}

impl Default for MemorySnapshotManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory snapshot statistics
#[derive(Debug, Clone, Default)]
pub struct MemorySnapshotStats {
    /// Total captures performed
    pub captures: u64,
    /// Dirty-only captures performed
    pub dirty_captures: u64,
    /// Total bytes captured
    pub bytes_captured: u64,
    /// Total bytes restored
    pub bytes_restored: u64,
    /// Dirty pages captured
    pub dirty_pages_captured: u64,
    /// Zero pages deduplicated
    pub zero_pages_deduped: u64,
    /// Compression savings in bytes
    pub compression_savings: u64,
    /// Time spent capturing (microseconds)
    pub capture_time_us: u64,
    /// Time spent restoring (microseconds)
    pub restore_time_us: u64,
    /// Checksum verification failures
    pub checksum_failures: u64,
}

impl MemorySnapshotStats {
    /// Average bytes per capture
    pub fn avg_capture_size(&self) -> u64 {
        if self.captures == 0 {
            return 0;
        }
        self.bytes_captured / self.captures
    }

    /// Compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.bytes_captured == 0 {
            return 1.0;
        }
        let compressed = self.bytes_captured - self.compression_savings;
        self.bytes_captured as f64 / compressed as f64
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
    fn test_memory_snapshot_error_display() {
        let err = MemorySnapshotError::RegionNotFound(0x1000);
        assert!(format!("{}", err).contains("0x1000"));
    }

    #[test]
    fn test_dirty_page_tracker_creation() {
        let tracker = DirtyPageTracker::new(0, 1024 * 1024);
        assert_eq!(tracker.page_count(), 256);
        assert_eq!(tracker.dirty_count(), 0);
    }

    #[test]
    fn test_dirty_page_tracker_mark_dirty() {
        let mut tracker = DirtyPageTracker::new(0, 16 * PAGE_SIZE);

        assert!(!tracker.is_dirty(0));
        tracker.mark_dirty(0);
        assert!(tracker.is_dirty(0));
        assert_eq!(tracker.dirty_count(), 1);

        // Mark same page again - should not increase count
        tracker.mark_dirty(0);
        assert_eq!(tracker.dirty_count(), 1);
    }

    #[test]
    fn test_dirty_page_tracker_clear_dirty() {
        let mut tracker = DirtyPageTracker::new(0, 16 * PAGE_SIZE);

        tracker.mark_dirty(0);
        tracker.mark_dirty(PAGE_SIZE);
        assert_eq!(tracker.dirty_count(), 2);

        tracker.clear_dirty(0);
        assert_eq!(tracker.dirty_count(), 1);
        assert!(!tracker.is_dirty(0));
        assert!(tracker.is_dirty(PAGE_SIZE));
    }

    #[test]
    fn test_dirty_page_tracker_clear_all() {
        let mut tracker = DirtyPageTracker::new(0, 16 * PAGE_SIZE);

        tracker.mark_dirty(0);
        tracker.mark_dirty(PAGE_SIZE);
        tracker.mark_dirty(2 * PAGE_SIZE);

        let gen_before = tracker.generation();
        tracker.clear_all();

        assert_eq!(tracker.dirty_count(), 0);
        assert_eq!(tracker.generation(), gen_before + 1);
    }

    #[test]
    fn test_dirty_page_tracker_dirty_range() {
        let mut tracker = DirtyPageTracker::new(0, 16 * PAGE_SIZE);

        // Mark range spanning 3 pages
        tracker.mark_dirty_range(PAGE_SIZE / 2, PAGE_SIZE * 2);

        assert!(tracker.is_dirty(0));
        assert!(tracker.is_dirty(PAGE_SIZE));
        assert!(tracker.is_dirty(2 * PAGE_SIZE));
        assert!(!tracker.is_dirty(3 * PAGE_SIZE));
    }

    #[test]
    fn test_dirty_page_tracker_iterator() {
        let mut tracker = DirtyPageTracker::new(0, 16 * PAGE_SIZE);

        tracker.mark_dirty(0);
        tracker.mark_dirty(4 * PAGE_SIZE);
        tracker.mark_dirty(8 * PAGE_SIZE);

        let dirty: Vec<u64> = tracker.dirty_pages().collect();
        assert_eq!(dirty.len(), 3);
        assert!(dirty.contains(&0));
        assert!(dirty.contains(&(4 * PAGE_SIZE)));
        assert!(dirty.contains(&(8 * PAGE_SIZE)));
    }

    #[test]
    fn test_dirty_page_tracker_ratio() {
        let mut tracker = DirtyPageTracker::new(0, 100 * PAGE_SIZE);

        for i in 0..25 {
            tracker.mark_dirty(i * PAGE_SIZE);
        }

        let ratio = tracker.dirty_ratio();
        assert!((ratio - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_memory_snapshot_config_default() {
        let config = MemorySnapshotConfig::default();
        assert_eq!(config.compression, CompressionType::None);
        assert!(config.dedupe_zeros);
    }

    #[test]
    fn test_memory_snapshot_config_fast() {
        let config = MemorySnapshotConfig::fast();
        assert_eq!(config.compression, CompressionType::Lz4);
        assert!(!config.verify_checksums);
    }

    #[test]
    fn test_memory_snapshot_config_compact() {
        let config = MemorySnapshotConfig::compact();
        assert_eq!(config.compression, CompressionType::Zstd);
        assert_eq!(config.compression_level, 9);
    }

    #[test]
    fn test_memory_snapshot_manager_creation() {
        let manager = MemorySnapshotManager::new();
        assert_eq!(manager.dirty_page_count(), 0);
    }

    #[test]
    fn test_memory_snapshot_manager_add_region() {
        let mut manager = MemorySnapshotManager::new();
        manager.add_region(0, 0x100000);

        // Should be able to write to the region
        let result = manager.write(0x1000, &[1, 2, 3, 4]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_memory_snapshot_manager_write_read() {
        let mut manager = MemorySnapshotManager::new();
        manager.add_region(0, 0x10000);

        let data = vec![0xAB; 256];
        manager.write(0x1000, &data).unwrap();

        let read = manager.read(0x1000, 256).unwrap();
        assert_eq!(read, data);
    }

    #[test]
    fn test_memory_snapshot_manager_invalid_region() {
        let manager = MemorySnapshotManager::new();
        let result = manager.read(0x1000, 100);
        assert!(matches!(
            result,
            Err(MemorySnapshotError::RegionNotFound(_))
        ));
    }

    #[test]
    fn test_memory_snapshot_manager_dirty_tracking() {
        let mut manager = MemorySnapshotManager::new();
        manager.add_region(0, 0x10000);
        manager.enable_dirty_tracking(0, 0x10000);

        manager.write(0x1000, &[1, 2, 3, 4]).unwrap();
        assert!(manager.dirty_page_count() > 0);

        manager.clear_dirty();
        assert_eq!(manager.dirty_page_count(), 0);
    }

    #[test]
    fn test_memory_snapshot_manager_capture_all() {
        let mut manager = MemorySnapshotManager::new();
        manager.add_region(0, 0x10000);
        manager.add_region(0x100000, 0x10000);

        let snapshots = manager.capture_all();
        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn test_memory_snapshot_manager_capture_dirty() {
        let mut manager = MemorySnapshotManager::new();
        manager.add_region(0, 0x10000);
        manager.enable_dirty_tracking(0, 0x10000);

        manager.write(0x1000, &[1, 2, 3, 4]).unwrap();

        let snapshots = manager.capture_dirty().unwrap();
        assert!(!snapshots.is_empty());
        assert!(snapshots[0].is_dirty);
    }

    #[test]
    fn test_memory_snapshot_stats() {
        let mut stats = MemorySnapshotStats::default();
        stats.captures = 10;
        stats.bytes_captured = 1000000;
        stats.compression_savings = 200000;

        assert_eq!(stats.avg_capture_size(), 100000);
        assert!(stats.compression_ratio() > 1.0);
    }

    #[test]
    fn test_crc32_checksum() {
        let data = b"test data";
        let crc1 = crc32_checksum(data);
        let crc2 = crc32_checksum(data);
        assert_eq!(crc1, crc2);

        let different = b"different";
        let crc3 = crc32_checksum(different);
        assert_ne!(crc1, crc3);
    }
}
