//! Dirty page tracking for live migration
//!
//! This module provides mechanisms to track which memory pages have been
//! modified (dirtied) during VM execution. This is essential for live
//! migration to efficiently transfer only changed pages.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Page size for dirty tracking (4KB)
pub const PAGE_SIZE: u64 = 4096;

/// Number of pages per bitmap word (64 pages per u64)
const PAGES_PER_WORD: u64 = 64;

/// Dirty page bitmap for tracking modified pages
#[derive(Debug)]
pub struct DirtyBitmap {
    /// Base guest physical address
    base_gpa: u64,
    /// Size of the tracked region in bytes
    size: u64,
    /// Number of pages
    num_pages: u64,
    /// Bitmap data (1 bit per page)
    bitmap: Vec<AtomicU64>,
    /// Generation counter for tracking scan cycles
    generation: AtomicU64,
}

impl DirtyBitmap {
    /// Create a new dirty bitmap for a memory region
    pub fn new(base_gpa: u64, size: u64) -> Self {
        let num_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        let num_words = ((num_pages + PAGES_PER_WORD - 1) / PAGES_PER_WORD) as usize;

        let mut bitmap = Vec::with_capacity(num_words);
        for _ in 0..num_words {
            bitmap.push(AtomicU64::new(0));
        }

        Self {
            base_gpa,
            size,
            num_pages,
            bitmap,
            generation: AtomicU64::new(0),
        }
    }

    /// Get the base GPA of the tracked region
    pub fn base_gpa(&self) -> u64 {
        self.base_gpa
    }

    /// Get the size of the tracked region
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Get the number of pages
    pub fn num_pages(&self) -> u64 {
        self.num_pages
    }

    /// Get the current generation
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Mark a page as dirty
    pub fn mark_dirty(&self, gpa: u64) -> bool {
        if gpa < self.base_gpa || gpa >= self.base_gpa + self.size {
            return false;
        }

        let page_num = (gpa - self.base_gpa) / PAGE_SIZE;
        let word_idx = (page_num / PAGES_PER_WORD) as usize;
        let bit_idx = page_num % PAGES_PER_WORD;

        if word_idx < self.bitmap.len() {
            self.bitmap[word_idx].fetch_or(1u64 << bit_idx, Ordering::Release);
            true
        } else {
            false
        }
    }

    /// Mark a range of pages as dirty
    pub fn mark_range_dirty(&self, gpa: u64, size: u64) {
        let start_page = gpa.saturating_sub(self.base_gpa) / PAGE_SIZE;
        let end_gpa = gpa.saturating_add(size);
        let end_page = end_gpa
            .saturating_sub(self.base_gpa)
            .saturating_add(PAGE_SIZE - 1)
            / PAGE_SIZE;

        for page in start_page..end_page.min(self.num_pages) {
            let word_idx = (page / PAGES_PER_WORD) as usize;
            let bit_idx = page % PAGES_PER_WORD;
            if word_idx < self.bitmap.len() {
                self.bitmap[word_idx].fetch_or(1u64 << bit_idx, Ordering::Release);
            }
        }
    }

    /// Check if a page is dirty
    pub fn is_dirty(&self, gpa: u64) -> bool {
        if gpa < self.base_gpa || gpa >= self.base_gpa + self.size {
            return false;
        }

        let page_num = (gpa - self.base_gpa) / PAGE_SIZE;
        let word_idx = (page_num / PAGES_PER_WORD) as usize;
        let bit_idx = page_num % PAGES_PER_WORD;

        if word_idx < self.bitmap.len() {
            (self.bitmap[word_idx].load(Ordering::Acquire) & (1u64 << bit_idx)) != 0
        } else {
            false
        }
    }

    /// Clear dirty bit for a page
    pub fn clear_dirty(&self, gpa: u64) -> bool {
        if gpa < self.base_gpa || gpa >= self.base_gpa + self.size {
            return false;
        }

        let page_num = (gpa - self.base_gpa) / PAGE_SIZE;
        let word_idx = (page_num / PAGES_PER_WORD) as usize;
        let bit_idx = page_num % PAGES_PER_WORD;

        if word_idx < self.bitmap.len() {
            let old = self.bitmap[word_idx].fetch_and(!(1u64 << bit_idx), Ordering::AcqRel);
            (old & (1u64 << bit_idx)) != 0
        } else {
            false
        }
    }

    /// Clear all dirty bits and increment generation
    pub fn clear_all(&self) {
        for word in &self.bitmap {
            word.store(0, Ordering::Release);
        }
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Get and clear all dirty bits (atomic swap)
    pub fn collect_and_clear(&self) -> Vec<u64> {
        let mut result = Vec::with_capacity(self.bitmap.len());
        for word in &self.bitmap {
            result.push(word.swap(0, Ordering::AcqRel));
        }
        self.generation.fetch_add(1, Ordering::Release);
        result
    }

    /// Count the number of dirty pages
    pub fn count_dirty(&self) -> u64 {
        self.bitmap
            .iter()
            .map(|word| word.load(Ordering::Acquire).count_ones() as u64)
            .sum()
    }

    /// Iterate over dirty page GPAs
    pub fn dirty_pages(&self) -> DirtyPageIterator<'_> {
        DirtyPageIterator {
            bitmap: self,
            word_idx: 0,
            current_word: if self.bitmap.is_empty() {
                0
            } else {
                self.bitmap[0].load(Ordering::Acquire)
            },
        }
    }

    /// Get dirty pages as a list of GPAs
    pub fn dirty_page_list(&self) -> Vec<u64> {
        self.dirty_pages().collect()
    }

    /// Mark all pages as dirty (for initial transfer)
    pub fn mark_all_dirty(&self) {
        let full_words = (self.num_pages / PAGES_PER_WORD) as usize;
        let remaining_bits = self.num_pages % PAGES_PER_WORD;

        // Set all full words to MAX
        for i in 0..full_words {
            self.bitmap[i].store(u64::MAX, Ordering::Release);
        }

        // Set only valid bits in the last partial word
        if remaining_bits > 0 && full_words < self.bitmap.len() {
            let mask = (1u64 << remaining_bits) - 1;
            self.bitmap[full_words].store(mask, Ordering::Release);
        }
    }
}

/// Iterator over dirty pages
pub struct DirtyPageIterator<'a> {
    bitmap: &'a DirtyBitmap,
    word_idx: usize,
    current_word: u64,
}

impl Iterator for DirtyPageIterator<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.current_word != 0 {
                let bit_idx = self.current_word.trailing_zeros() as u64;
                self.current_word &= self.current_word - 1; // Clear lowest set bit
                let page_num = (self.word_idx as u64) * PAGES_PER_WORD + bit_idx;
                if page_num < self.bitmap.num_pages {
                    return Some(self.bitmap.base_gpa + page_num * PAGE_SIZE);
                }
            }

            self.word_idx += 1;
            if self.word_idx >= self.bitmap.bitmap.len() {
                return None;
            }
            self.current_word = self.bitmap.bitmap[self.word_idx].load(Ordering::Acquire);
        }
    }
}

/// Dirty tracking statistics
#[derive(Debug, Clone, Default)]
pub struct DirtyStats {
    /// Total pages tracked
    pub total_pages: u64,
    /// Currently dirty pages
    pub dirty_pages: u64,
    /// Pages dirtied since last clear
    pub pages_dirtied: u64,
    /// Number of clear operations
    pub clear_count: u64,
    /// Dirty rate (pages per second)
    pub dirty_rate: f64,
}

/// Dirty page tracker for multiple memory regions
#[derive(Debug)]
pub struct DirtyTracker {
    /// Bitmaps for each tracked region
    regions: Vec<DirtyBitmap>,
    /// Whether tracking is enabled
    enabled: bool,
    /// Statistics
    stats: RwLock<DirtyStats>,
    /// Last sample time for rate calculation
    last_sample_time: AtomicU64,
    /// Dirty count at last sample
    last_dirty_count: AtomicU64,
}

impl DirtyTracker {
    /// Create a new dirty tracker
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            enabled: false,
            stats: RwLock::new(DirtyStats::default()),
            last_sample_time: AtomicU64::new(0),
            last_dirty_count: AtomicU64::new(0),
        }
    }

    /// Add a memory region to track
    pub fn add_region(&mut self, base_gpa: u64, size: u64) {
        self.regions.push(DirtyBitmap::new(base_gpa, size));
    }

    /// Remove a memory region
    pub fn remove_region(&mut self, base_gpa: u64) -> bool {
        if let Some(idx) = self.regions.iter().position(|r| r.base_gpa == base_gpa) {
            self.regions.remove(idx);
            true
        } else {
            false
        }
    }

    /// Enable dirty tracking
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable dirty tracking
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Check if tracking is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Mark a page as dirty
    pub fn mark_dirty(&self, gpa: u64) {
        if !self.enabled {
            return;
        }
        for region in &self.regions {
            if region.mark_dirty(gpa) {
                return;
            }
        }
    }

    /// Mark a range as dirty
    pub fn mark_range_dirty(&self, gpa: u64, size: u64) {
        if !self.enabled {
            return;
        }
        for region in &self.regions {
            region.mark_range_dirty(gpa, size);
        }
    }

    /// Check if a page is dirty
    pub fn is_dirty(&self, gpa: u64) -> bool {
        for region in &self.regions {
            if region.is_dirty(gpa) {
                return true;
            }
        }
        false
    }

    /// Get total dirty page count
    pub fn count_dirty(&self) -> u64 {
        self.regions.iter().map(|r| r.count_dirty()).sum()
    }

    /// Get all dirty pages
    pub fn dirty_pages(&self) -> Vec<u64> {
        let mut pages = Vec::new();
        for region in &self.regions {
            pages.extend(region.dirty_pages());
        }
        pages
    }

    /// Clear all dirty bits
    pub fn clear_all(&self) {
        for region in &self.regions {
            region.clear_all();
        }
        if let Ok(mut stats) = self.stats.write() {
            stats.clear_count += 1;
        }
    }

    /// Collect and clear dirty pages
    pub fn collect_dirty(&self) -> Vec<(u64, Vec<u64>)> {
        self.regions
            .iter()
            .map(|r| (r.base_gpa, r.dirty_page_list()))
            .filter(|(_, pages)| !pages.is_empty())
            .collect()
    }

    /// Get statistics
    pub fn stats(&self) -> DirtyStats {
        let dirty = self.count_dirty();
        let total: u64 = self.regions.iter().map(|r| r.num_pages).sum();

        DirtyStats {
            total_pages: total,
            dirty_pages: dirty,
            pages_dirtied: dirty,
            clear_count: self.stats.read().map(|s| s.clear_count).unwrap_or(0),
            dirty_rate: 0.0, // Calculated externally
        }
    }

    /// Update dirty rate calculation
    pub fn update_rate(&self, current_time_ns: u64) {
        let last_time = self
            .last_sample_time
            .swap(current_time_ns, Ordering::AcqRel);
        let current_count = self.count_dirty();
        let last_count = self.last_dirty_count.swap(current_count, Ordering::AcqRel);

        if last_time > 0 && current_time_ns > last_time {
            let elapsed_secs = (current_time_ns - last_time) as f64 / 1_000_000_000.0;
            let pages_diff = current_count.saturating_sub(last_count);
            let rate = pages_diff as f64 / elapsed_secs;

            if let Ok(mut stats) = self.stats.write() {
                stats.dirty_rate = rate;
            }
        }
    }

    /// Get the number of tracked regions
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Get region info
    pub fn regions(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.regions.iter().map(|r| (r.base_gpa, r.size))
    }
}

impl Default for DirtyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared dirty tracker
pub type SharedDirtyTracker = Arc<RwLock<DirtyTracker>>;

/// Create a shared dirty tracker
pub fn shared_dirty_tracker() -> SharedDirtyTracker {
    Arc::new(RwLock::new(DirtyTracker::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dirty_bitmap_creation() {
        let bitmap = DirtyBitmap::new(0x1000, 0x10000);
        assert_eq!(bitmap.base_gpa(), 0x1000);
        assert_eq!(bitmap.size(), 0x10000);
        assert_eq!(bitmap.num_pages(), 16); // 64KB / 4KB
    }

    #[test]
    fn test_mark_dirty() {
        let bitmap = DirtyBitmap::new(0, 0x10000);

        assert!(!bitmap.is_dirty(0));
        assert!(bitmap.mark_dirty(0));
        assert!(bitmap.is_dirty(0));

        assert!(bitmap.mark_dirty(0x1000));
        assert!(bitmap.is_dirty(0x1000));
    }

    #[test]
    fn test_mark_dirty_out_of_range() {
        let bitmap = DirtyBitmap::new(0x1000, 0x1000);

        assert!(!bitmap.mark_dirty(0)); // Before region
        assert!(!bitmap.mark_dirty(0x3000)); // After region
        assert!(bitmap.mark_dirty(0x1000)); // In region
    }

    #[test]
    fn test_mark_range_dirty() {
        let bitmap = DirtyBitmap::new(0, 0x10000);

        bitmap.mark_range_dirty(0x1000, 0x3000);

        assert!(!bitmap.is_dirty(0));
        assert!(bitmap.is_dirty(0x1000));
        assert!(bitmap.is_dirty(0x2000));
        assert!(bitmap.is_dirty(0x3000));
        assert!(!bitmap.is_dirty(0x4000));
    }

    #[test]
    fn test_clear_dirty() {
        let bitmap = DirtyBitmap::new(0, 0x10000);

        bitmap.mark_dirty(0x1000);
        assert!(bitmap.is_dirty(0x1000));

        assert!(bitmap.clear_dirty(0x1000));
        assert!(!bitmap.is_dirty(0x1000));

        // Clearing already clean page returns false
        assert!(!bitmap.clear_dirty(0x1000));
    }

    #[test]
    fn test_clear_all() {
        let bitmap = DirtyBitmap::new(0, 0x10000);

        bitmap.mark_dirty(0);
        bitmap.mark_dirty(0x1000);
        bitmap.mark_dirty(0x2000);

        assert_eq!(bitmap.count_dirty(), 3);
        assert_eq!(bitmap.generation(), 0);

        bitmap.clear_all();

        assert_eq!(bitmap.count_dirty(), 0);
        assert_eq!(bitmap.generation(), 1);
    }

    #[test]
    fn test_collect_and_clear() {
        let bitmap = DirtyBitmap::new(0, 0x10000);

        bitmap.mark_dirty(0);
        bitmap.mark_dirty(0x1000);

        let collected = bitmap.collect_and_clear();

        assert!(!collected.is_empty());
        assert_eq!(bitmap.count_dirty(), 0);
        assert_eq!(bitmap.generation(), 1);
    }

    #[test]
    fn test_count_dirty() {
        let bitmap = DirtyBitmap::new(0, 0x100000); // 1MB = 256 pages

        assert_eq!(bitmap.count_dirty(), 0);

        for i in 0..10 {
            bitmap.mark_dirty(i * PAGE_SIZE);
        }

        assert_eq!(bitmap.count_dirty(), 10);
    }

    #[test]
    fn test_dirty_page_iterator() {
        let bitmap = DirtyBitmap::new(0, 0x10000);

        bitmap.mark_dirty(0);
        bitmap.mark_dirty(0x2000);
        bitmap.mark_dirty(0x3000);

        let pages: Vec<u64> = bitmap.dirty_pages().collect();

        assert_eq!(pages.len(), 3);
        assert!(pages.contains(&0));
        assert!(pages.contains(&0x2000));
        assert!(pages.contains(&0x3000));
    }

    #[test]
    fn test_mark_all_dirty() {
        let bitmap = DirtyBitmap::new(0, 0x10000);

        bitmap.mark_all_dirty();

        assert_eq!(bitmap.count_dirty(), 16); // All 16 pages dirty
    }

    #[test]
    fn test_dirty_tracker_creation() {
        let tracker = DirtyTracker::new();
        assert!(!tracker.is_enabled());
        assert_eq!(tracker.region_count(), 0);
    }

    #[test]
    fn test_dirty_tracker_regions() {
        let mut tracker = DirtyTracker::new();

        tracker.add_region(0, 0x10000);
        tracker.add_region(0x100000, 0x20000);

        assert_eq!(tracker.region_count(), 2);

        tracker.remove_region(0);
        assert_eq!(tracker.region_count(), 1);
    }

    #[test]
    fn test_dirty_tracker_enable_disable() {
        let mut tracker = DirtyTracker::new();
        tracker.add_region(0, 0x10000);

        // Disabled by default
        tracker.mark_dirty(0x1000);
        assert!(!tracker.is_dirty(0x1000));

        // Enable tracking
        tracker.enable();
        tracker.mark_dirty(0x1000);
        assert!(tracker.is_dirty(0x1000));

        // Disable tracking
        tracker.disable();
        tracker.mark_dirty(0x2000);
        // Note: is_dirty still works even when disabled
    }

    #[test]
    fn test_dirty_tracker_multi_region() {
        let mut tracker = DirtyTracker::new();
        tracker.add_region(0, 0x10000);
        tracker.add_region(0x100000, 0x10000);
        tracker.enable();

        tracker.mark_dirty(0x1000);
        tracker.mark_dirty(0x101000);

        assert!(tracker.is_dirty(0x1000));
        assert!(tracker.is_dirty(0x101000));
        assert_eq!(tracker.count_dirty(), 2);
    }

    #[test]
    fn test_dirty_tracker_collect() {
        let mut tracker = DirtyTracker::new();
        tracker.add_region(0, 0x10000);
        tracker.enable();

        tracker.mark_dirty(0x1000);
        tracker.mark_dirty(0x2000);

        let collected = tracker.collect_dirty();

        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].0, 0); // Base GPA
        assert_eq!(collected[0].1.len(), 2); // Two dirty pages
    }

    #[test]
    fn test_dirty_tracker_stats() {
        let mut tracker = DirtyTracker::new();
        tracker.add_region(0, 0x10000);
        tracker.enable();

        tracker.mark_dirty(0x1000);
        tracker.mark_dirty(0x2000);

        let stats = tracker.stats();
        assert_eq!(stats.total_pages, 16);
        assert_eq!(stats.dirty_pages, 2);
    }

    #[test]
    fn test_dirty_tracker_clear_all() {
        let mut tracker = DirtyTracker::new();
        tracker.add_region(0, 0x10000);
        tracker.enable();

        tracker.mark_dirty(0x1000);
        tracker.clear_all();

        assert_eq!(tracker.count_dirty(), 0);
        assert_eq!(tracker.stats().clear_count, 1);
    }

    #[test]
    fn test_shared_dirty_tracker() {
        let tracker = shared_dirty_tracker();

        {
            let mut t = tracker.write().unwrap();
            t.add_region(0, 0x10000);
            t.enable();
        }

        {
            let t = tracker.read().unwrap();
            assert!(t.is_enabled());
            assert_eq!(t.region_count(), 1);
        }
    }

    #[test]
    fn test_large_region() {
        // Test with 1GB region (262144 pages)
        let bitmap = DirtyBitmap::new(0, 1 << 30);

        assert_eq!(bitmap.num_pages(), 262144);

        // Mark some pages dirty
        bitmap.mark_dirty(0);
        bitmap.mark_dirty((1 << 29) - PAGE_SIZE); // Middle
        bitmap.mark_dirty((1 << 30) - PAGE_SIZE); // Last page

        assert_eq!(bitmap.count_dirty(), 3);
    }
}
