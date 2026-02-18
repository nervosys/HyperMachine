//! Nested EPT (Extended Page Tables) management
//!
//! This module provides nested EPT support for L2 guest address translation
//! when running with nested virtualization.

use std::collections::HashMap;

/// Page sizes for EPT
pub const PAGE_SIZE_4K: u64 = 4096;
pub const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
pub const PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;

/// EPT memory types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EptMemoryType {
    /// Uncacheable
    Uncacheable = 0,
    /// Write Combining
    WriteCombining = 1,
    /// Write Through
    WriteThrough = 4,
    /// Write Protected
    WriteProtected = 5,
    /// Write Back
    #[default]
    WriteBack = 6,
}

/// EPT entry flags
pub mod ept_flags {
    pub const READ: u64 = 1 << 0;
    pub const WRITE: u64 = 1 << 1;
    pub const EXECUTE: u64 = 1 << 2;
    pub const MEMORY_TYPE_MASK: u64 = 0x7 << 3;
    pub const MEMORY_TYPE_SHIFT: u64 = 3;
    pub const IGNORE_PAT: u64 = 1 << 6;
    pub const LARGE_PAGE: u64 = 1 << 7;
    pub const ACCESSED: u64 = 1 << 8;
    pub const DIRTY: u64 = 1 << 9;
    pub const EXECUTE_USER: u64 = 1 << 10;
    pub const VERIFY_GUEST_PAGING: u64 = 1 << 57;
    pub const PAGING_WRITE_ACCESS: u64 = 1 << 58;
    pub const SUPPRESS_VE: u64 = 1 << 63;

    pub const RWX: u64 = READ | WRITE | EXECUTE;
    pub const RW: u64 = READ | WRITE;
    pub const RX: u64 = READ | EXECUTE;
}

/// EPT pointer configuration
#[derive(Debug, Clone, Copy, Default)]
pub struct EptPointer {
    /// Raw EPTP value
    raw: u64,
}

impl EptPointer {
    /// Memory type shift
    const MEMORY_TYPE_SHIFT: u64 = 0;
    /// Page walk length shift
    const PAGE_WALK_LENGTH_SHIFT: u64 = 3;
    /// Access/Dirty enable bit
    const AD_ENABLE: u64 = 1 << 6;
    /// PML4 address mask
    const PML4_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

    /// Create a new EPT pointer
    pub fn new(pml4_addr: u64, memory_type: EptMemoryType, page_walk_length: u8) -> Self {
        let raw = (pml4_addr & Self::PML4_ADDR_MASK)
            | ((memory_type as u64) << Self::MEMORY_TYPE_SHIFT)
            | (((page_walk_length as u64 - 1) & 0x7) << Self::PAGE_WALK_LENGTH_SHIFT);
        Self { raw }
    }

    /// Create with default settings (WB, 4-level)
    pub fn with_defaults(pml4_addr: u64) -> Self {
        Self::new(pml4_addr, EptMemoryType::WriteBack, 4)
    }

    /// Enable access/dirty bits
    pub fn with_ad_enabled(mut self) -> Self {
        self.raw |= Self::AD_ENABLE;
        self
    }

    /// Get raw value
    pub fn raw(&self) -> u64 {
        self.raw
    }

    /// Get PML4 physical address
    pub fn pml4_addr(&self) -> u64 {
        self.raw & Self::PML4_ADDR_MASK
    }

    /// Get memory type
    pub fn memory_type(&self) -> EptMemoryType {
        match (self.raw >> Self::MEMORY_TYPE_SHIFT) & 0x7 {
            0 => EptMemoryType::Uncacheable,
            1 => EptMemoryType::WriteCombining,
            4 => EptMemoryType::WriteThrough,
            5 => EptMemoryType::WriteProtected,
            6 => EptMemoryType::WriteBack,
            _ => EptMemoryType::Uncacheable,
        }
    }

    /// Get page walk length
    pub fn page_walk_length(&self) -> u8 {
        (((self.raw >> Self::PAGE_WALK_LENGTH_SHIFT) & 0x7) + 1) as u8
    }

    /// Check if access/dirty bits are enabled
    pub fn ad_enabled(&self) -> bool {
        self.raw & Self::AD_ENABLE != 0
    }
}

impl From<u64> for EptPointer {
    fn from(value: u64) -> Self {
        Self { raw: value }
    }
}

/// EPT violation qualification
#[derive(Debug, Clone, Copy, Default)]
pub struct EptViolationQualification {
    raw: u64,
}

impl EptViolationQualification {
    /// Create from exit qualification
    pub fn new(qualification: u64) -> Self {
        Self { raw: qualification }
    }

    /// Was it a read access?
    pub fn is_read(&self) -> bool {
        self.raw & (1 << 0) != 0
    }

    /// Was it a write access?
    pub fn is_write(&self) -> bool {
        self.raw & (1 << 1) != 0
    }

    /// Was it an instruction fetch?
    pub fn is_fetch(&self) -> bool {
        self.raw & (1 << 2) != 0
    }

    /// Was the page readable?
    pub fn page_readable(&self) -> bool {
        self.raw & (1 << 3) != 0
    }

    /// Was the page writable?
    pub fn page_writable(&self) -> bool {
        self.raw & (1 << 4) != 0
    }

    /// Was the page executable?
    pub fn page_executable(&self) -> bool {
        self.raw & (1 << 5) != 0
    }

    /// Was it a user-mode access?
    pub fn is_user_mode(&self) -> bool {
        self.raw & (1 << 6) != 0
    }

    /// Was the GPA valid?
    pub fn gpa_valid(&self) -> bool {
        self.raw & (1 << 7) != 0
    }

    /// Was it caused by page walk?
    pub fn is_page_walk(&self) -> bool {
        self.raw & (1 << 8) != 0
    }

    /// Was it caused by guest paging?
    pub fn is_guest_paging(&self) -> bool {
        self.raw & (1 << 9) != 0
    }

    /// NMI unblocking due to IRET
    pub fn nmi_unblocking(&self) -> bool {
        self.raw & (1 << 12) != 0
    }

    /// Get raw value
    pub fn raw(&self) -> u64 {
        self.raw
    }
}

/// EPT entry (used for all levels)
#[derive(Debug, Clone, Copy, Default)]
pub struct EptEntry {
    raw: u64,
}

impl EptEntry {
    /// Create an empty entry
    pub fn empty() -> Self {
        Self { raw: 0 }
    }

    /// Create a PML4/PDPT/PD entry pointing to next level
    pub fn table_entry(next_level_addr: u64, flags: u64) -> Self {
        Self {
            raw: (next_level_addr & 0x000F_FFFF_FFFF_F000) | flags,
        }
    }

    /// Create a page entry (4KB, 2MB, or 1GB)
    pub fn page_entry(
        phys_addr: u64,
        memory_type: EptMemoryType,
        flags: u64,
        large_page: bool,
    ) -> Self {
        let mut raw = (phys_addr & 0x000F_FFFF_FFFF_F000)
            | ((memory_type as u64) << ept_flags::MEMORY_TYPE_SHIFT)
            | flags;
        if large_page {
            raw |= ept_flags::LARGE_PAGE;
        }
        Self { raw }
    }

    /// Get raw value
    pub fn raw(&self) -> u64 {
        self.raw
    }

    /// Check if entry is present (has any access bits)
    pub fn is_present(&self) -> bool {
        self.raw & ept_flags::RWX != 0
    }

    /// Check if this is a large page
    pub fn is_large_page(&self) -> bool {
        self.raw & ept_flags::LARGE_PAGE != 0
    }

    /// Get physical address
    pub fn phys_addr(&self) -> u64 {
        self.raw & 0x000F_FFFF_FFFF_F000
    }

    /// Get memory type
    pub fn memory_type(&self) -> EptMemoryType {
        match (self.raw >> ept_flags::MEMORY_TYPE_SHIFT) & 0x7 {
            0 => EptMemoryType::Uncacheable,
            1 => EptMemoryType::WriteCombining,
            4 => EptMemoryType::WriteThrough,
            5 => EptMemoryType::WriteProtected,
            6 => EptMemoryType::WriteBack,
            _ => EptMemoryType::Uncacheable,
        }
    }

    /// Check read permission
    pub fn is_readable(&self) -> bool {
        self.raw & ept_flags::READ != 0
    }

    /// Check write permission
    pub fn is_writable(&self) -> bool {
        self.raw & ept_flags::WRITE != 0
    }

    /// Check execute permission
    pub fn is_executable(&self) -> bool {
        self.raw & ept_flags::EXECUTE != 0
    }

    /// Check if accessed
    pub fn is_accessed(&self) -> bool {
        self.raw & ept_flags::ACCESSED != 0
    }

    /// Check if dirty
    pub fn is_dirty(&self) -> bool {
        self.raw & ept_flags::DIRTY != 0
    }

    /// Set accessed bit
    pub fn set_accessed(&mut self) {
        self.raw |= ept_flags::ACCESSED;
    }

    /// Set dirty bit
    pub fn set_dirty(&mut self) {
        self.raw |= ept_flags::DIRTY;
    }

    /// Clear accessed bit
    pub fn clear_accessed(&mut self) {
        self.raw &= !ept_flags::ACCESSED;
    }

    /// Clear dirty bit
    pub fn clear_dirty(&mut self) {
        self.raw &= !ept_flags::DIRTY;
    }
}

impl From<u64> for EptEntry {
    fn from(value: u64) -> Self {
        Self { raw: value }
    }
}

/// Nested EPT translation result
#[derive(Debug, Clone)]
pub enum EptTranslationResult {
    /// Translation succeeded
    Success {
        hpa: u64,
        page_size: u64,
        permissions: u64,
    },
    /// EPT violation
    Violation { gpa: u64, qualification: u64 },
    /// EPT misconfiguration
    Misconfiguration { gpa: u64 },
}

/// Nested EPT manager
#[derive(Debug, Default)]
pub struct NestedEptManager {
    /// L1's EPT pointer (points to L1's EPT for L2)
    l1_eptp: Option<EptPointer>,
    /// L0's EPT pointer (our actual EPT)
    l0_eptp: Option<EptPointer>,
    /// Cached translations (L2 GPA -> HPA)
    translation_cache: HashMap<u64, (u64, u64)>,
    /// Statistics
    stats: NestedEptStats,
}

impl NestedEptManager {
    /// Create a new nested EPT manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Set L0 EPT pointer
    pub fn set_l0_eptp(&mut self, eptp: EptPointer) {
        self.l0_eptp = Some(eptp);
        self.translation_cache.clear();
    }

    /// Set L1 EPT pointer (L1's EPT for L2)
    pub fn set_l1_eptp(&mut self, eptp: EptPointer) {
        self.l1_eptp = Some(eptp);
        self.translation_cache.clear();
    }

    /// Get L1 EPT pointer
    pub fn l1_eptp(&self) -> Option<EptPointer> {
        self.l1_eptp
    }

    /// Get L0 EPT pointer
    pub fn l0_eptp(&self) -> Option<EptPointer> {
        self.l0_eptp
    }

    /// Clear translation cache
    pub fn flush_cache(&mut self) {
        self.translation_cache.clear();
        self.stats.cache_flushes += 1;
    }

    /// Invalidate a single address
    pub fn invalidate(&mut self, gpa: u64) {
        let page_gpa = gpa & !0xFFF;
        self.translation_cache.remove(&page_gpa);
        self.stats.invalidations += 1;
    }

    /// Check cache for translation
    pub fn lookup_cache(&self, gpa: u64) -> Option<(u64, u64)> {
        let page_gpa = gpa & !0xFFF;
        self.translation_cache.get(&page_gpa).copied()
    }

    /// Add translation to cache
    pub fn cache_translation(&mut self, gpa: u64, hpa: u64, page_size: u64) {
        let page_gpa = gpa & !(page_size - 1);
        self.translation_cache.insert(page_gpa, (hpa, page_size));
    }

    /// Get statistics
    pub fn stats(&self) -> &NestedEptStats {
        &self.stats
    }

    /// Record a translation
    pub fn record_translation(&mut self) {
        self.stats.translations += 1;
    }

    /// Record a cache hit
    pub fn record_cache_hit(&mut self) {
        self.stats.cache_hits += 1;
    }

    /// Record an EPT violation
    pub fn record_violation(&mut self) {
        self.stats.violations += 1;
    }

    /// Record an EPT misconfiguration
    pub fn record_misconfiguration(&mut self) {
        self.stats.misconfigurations += 1;
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.translation_cache.len()
    }
}

/// Nested EPT statistics
#[derive(Debug, Clone, Default)]
pub struct NestedEptStats {
    /// Total translations performed
    pub translations: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Cache flushes
    pub cache_flushes: u64,
    /// Single invalidations
    pub invalidations: u64,
    /// EPT violations
    pub violations: u64,
    /// EPT misconfigurations
    pub misconfigurations: u64,
}

/// INVEPT types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum InvEptType {
    /// Invalidate single context
    SingleContext = 1,
    /// Invalidate all contexts
    Global = 2,
}

/// INVVPID types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum InvVpidType {
    /// Invalidate individual address
    IndividualAddress = 0,
    /// Invalidate single context
    SingleContext = 1,
    /// Invalidate all contexts
    AllContexts = 2,
    /// Invalidate single context retaining globals
    SingleContextRetainingGlobals = 3,
}

/// VPID (Virtual Processor ID) for TLB management
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Vpid(pub u16);

impl Vpid {
    /// Invalid VPID (0 is reserved)
    pub const INVALID: Self = Self(0);

    /// Create a new VPID
    pub fn new(id: u16) -> Self {
        Self(id)
    }

    /// Check if valid
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ept_pointer_creation() {
        let eptp = EptPointer::new(0x1000, EptMemoryType::WriteBack, 4);
        assert_eq!(eptp.pml4_addr(), 0x1000);
        assert_eq!(eptp.memory_type(), EptMemoryType::WriteBack);
        assert_eq!(eptp.page_walk_length(), 4);
    }

    #[test]
    fn test_ept_pointer_with_defaults() {
        let eptp = EptPointer::with_defaults(0x2000);
        assert_eq!(eptp.pml4_addr(), 0x2000);
        assert_eq!(eptp.memory_type(), EptMemoryType::WriteBack);
        assert_eq!(eptp.page_walk_length(), 4);
    }

    #[test]
    fn test_ept_pointer_ad_enabled() {
        let eptp = EptPointer::with_defaults(0x1000).with_ad_enabled();
        assert!(eptp.ad_enabled());
    }

    #[test]
    fn test_ept_pointer_from_raw() {
        let raw = 0x1_0000_001E_u64;
        let eptp = EptPointer::from(raw);
        assert_eq!(eptp.raw(), raw);
    }

    #[test]
    fn test_ept_violation_qualification() {
        let qual = EptViolationQualification::new(0b1001_0111);

        assert!(qual.is_read());
        assert!(qual.is_write());
        assert!(qual.is_fetch());
        assert!(!qual.page_readable());
        assert!(qual.page_writable());
    }

    #[test]
    fn test_ept_entry_empty() {
        let entry = EptEntry::empty();
        assert!(!entry.is_present());
    }

    #[test]
    fn test_ept_entry_table() {
        let entry = EptEntry::table_entry(0x1000, ept_flags::RWX);
        assert!(entry.is_present());
        assert_eq!(entry.phys_addr(), 0x1000);
        assert!(!entry.is_large_page());
    }

    #[test]
    fn test_ept_entry_page() {
        let entry = EptEntry::page_entry(0x2000, EptMemoryType::WriteBack, ept_flags::RWX, false);
        assert!(entry.is_present());
        assert_eq!(entry.phys_addr(), 0x2000);
        assert_eq!(entry.memory_type(), EptMemoryType::WriteBack);
        assert!(entry.is_readable());
        assert!(entry.is_writable());
        assert!(entry.is_executable());
    }

    #[test]
    fn test_ept_entry_large_page() {
        let entry = EptEntry::page_entry(0x20_0000, EptMemoryType::WriteBack, ept_flags::RWX, true);
        assert!(entry.is_large_page());
    }

    #[test]
    fn test_ept_entry_accessed_dirty() {
        let mut entry =
            EptEntry::page_entry(0x1000, EptMemoryType::WriteBack, ept_flags::RWX, false);

        assert!(!entry.is_accessed());
        assert!(!entry.is_dirty());

        entry.set_accessed();
        entry.set_dirty();

        assert!(entry.is_accessed());
        assert!(entry.is_dirty());

        entry.clear_accessed();
        entry.clear_dirty();

        assert!(!entry.is_accessed());
        assert!(!entry.is_dirty());
    }

    #[test]
    fn test_nested_ept_manager_creation() {
        let manager = NestedEptManager::new();
        assert!(manager.l0_eptp().is_none());
        assert!(manager.l1_eptp().is_none());
    }

    #[test]
    fn test_nested_ept_manager_set_eptp() {
        let mut manager = NestedEptManager::new();

        manager.set_l0_eptp(EptPointer::with_defaults(0x1000));
        manager.set_l1_eptp(EptPointer::with_defaults(0x2000));

        assert!(manager.l0_eptp().is_some());
        assert!(manager.l1_eptp().is_some());
    }

    #[test]
    fn test_nested_ept_manager_cache() {
        let mut manager = NestedEptManager::new();

        manager.cache_translation(0x1000, 0x2000, PAGE_SIZE_4K);
        assert!(manager.lookup_cache(0x1000).is_some());
        assert_eq!(manager.cache_size(), 1);

        manager.flush_cache();
        assert!(manager.lookup_cache(0x1000).is_none());
        assert_eq!(manager.cache_size(), 0);
    }

    #[test]
    fn test_nested_ept_manager_invalidate() {
        let mut manager = NestedEptManager::new();

        manager.cache_translation(0x1000, 0x2000, PAGE_SIZE_4K);
        manager.cache_translation(0x2000, 0x3000, PAGE_SIZE_4K);

        manager.invalidate(0x1000);
        assert!(manager.lookup_cache(0x1000).is_none());
        assert!(manager.lookup_cache(0x2000).is_some());
    }

    #[test]
    fn test_nested_ept_stats() {
        let mut manager = NestedEptManager::new();

        manager.record_translation();
        manager.record_cache_hit();
        manager.record_violation();
        manager.record_misconfiguration();

        let stats = manager.stats();
        assert_eq!(stats.translations, 1);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.violations, 1);
        assert_eq!(stats.misconfigurations, 1);
    }

    #[test]
    fn test_invept_types() {
        assert_eq!(InvEptType::SingleContext as u64, 1);
        assert_eq!(InvEptType::Global as u64, 2);
    }

    #[test]
    fn test_invvpid_types() {
        assert_eq!(InvVpidType::IndividualAddress as u64, 0);
        assert_eq!(InvVpidType::SingleContext as u64, 1);
        assert_eq!(InvVpidType::AllContexts as u64, 2);
    }

    #[test]
    fn test_vpid() {
        let vpid = Vpid::new(1);
        assert!(vpid.is_valid());

        let invalid = Vpid::INVALID;
        assert!(!invalid.is_valid());
    }

    #[test]
    fn test_ept_memory_types() {
        assert_eq!(EptMemoryType::Uncacheable as u8, 0);
        assert_eq!(EptMemoryType::WriteCombining as u8, 1);
        assert_eq!(EptMemoryType::WriteThrough as u8, 4);
        assert_eq!(EptMemoryType::WriteProtected as u8, 5);
        assert_eq!(EptMemoryType::WriteBack as u8, 6);
    }
}
