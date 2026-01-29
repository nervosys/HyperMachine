//! IOMMU Core Types
//!
//! This module provides common types used by both Intel VT-d and AMD-Vi IOMMUs.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// IOMMU page sizes
pub const PAGE_SIZE_4K: u64 = 4096;
pub const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
pub const PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;

/// Address width for IOMMU
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AddressWidth {
    /// 30-bit AGAW (3-level page table)
    Bits30 = 0,
    /// 39-bit AGAW (4-level page table)
    Bits39 = 1,
    /// 48-bit AGAW (5-level page table)
    Bits48 = 2,
    /// 57-bit AGAW (6-level page table)
    Bits57 = 3,
}

impl AddressWidth {
    /// Get the number of address bits
    pub fn bits(&self) -> u8 {
        match self {
            AddressWidth::Bits30 => 30,
            AddressWidth::Bits39 => 39,
            AddressWidth::Bits48 => 48,
            AddressWidth::Bits57 => 57,
        }
    }

    /// Get the number of page table levels
    pub fn levels(&self) -> u8 {
        match self {
            AddressWidth::Bits30 => 3,
            AddressWidth::Bits39 => 4,
            AddressWidth::Bits48 => 5,
            AddressWidth::Bits57 => 6,
        }
    }

    /// Convert from AGAW value (Intel VT-d)
    pub fn from_agaw(agaw: u8) -> Option<Self> {
        match agaw {
            0 => Some(AddressWidth::Bits30),
            1 => Some(AddressWidth::Bits39),
            2 => Some(AddressWidth::Bits48),
            3 => Some(AddressWidth::Bits57),
            _ => None,
        }
    }
}

/// Device identifier (BDF - Bus:Device:Function)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId {
    /// Segment group (for multi-segment systems)
    pub segment: u16,
    /// Bus number
    pub bus: u8,
    /// Device and function combined
    pub devfn: u8,
}

impl DeviceId {
    /// Create new device ID
    pub fn new(segment: u16, bus: u8, device: u8, function: u8) -> Self {
        Self {
            segment,
            bus,
            devfn: (device << 3) | (function & 0x07),
        }
    }

    /// Create from source ID (16-bit BDF)
    pub fn from_source_id(segment: u16, source_id: u16) -> Self {
        Self {
            segment,
            bus: (source_id >> 8) as u8,
            devfn: source_id as u8,
        }
    }

    /// Get device number
    pub fn device(&self) -> u8 {
        self.devfn >> 3
    }

    /// Get function number
    pub fn function(&self) -> u8 {
        self.devfn & 0x07
    }

    /// Get source ID (16-bit BDF)
    pub fn source_id(&self) -> u16 {
        ((self.bus as u16) << 8) | (self.devfn as u16)
    }

    /// Get requester ID (alias for source_id)
    pub fn requester_id(&self) -> u16 {
        self.source_id()
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04x}:{:02x}:{:02x}.{:x}",
            self.segment,
            self.bus,
            self.device(),
            self.function()
        )
    }
}

/// Device scope type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceScopeType {
    /// PCI Endpoint Device
    PciEndpoint = 0x01,
    /// PCI Sub-hierarchy
    PciSubHierarchy = 0x02,
    /// IOAPIC
    IoApic = 0x03,
    /// MSI Capable HPET
    MsiCapableHpet = 0x04,
    /// ACPI Namespace Device
    AcpiNamespace = 0x05,
}

impl DeviceScopeType {
    /// Convert from raw value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(DeviceScopeType::PciEndpoint),
            0x02 => Some(DeviceScopeType::PciSubHierarchy),
            0x03 => Some(DeviceScopeType::IoApic),
            0x04 => Some(DeviceScopeType::MsiCapableHpet),
            0x05 => Some(DeviceScopeType::AcpiNamespace),
            _ => None,
        }
    }
}

/// Device scope entry
#[derive(Debug, Clone)]
pub struct DeviceScope {
    /// Scope type
    pub scope_type: DeviceScopeType,
    /// Enumeration ID (for IOAPIC/HPET)
    pub enumeration_id: u8,
    /// Start bus number
    pub start_bus: u8,
    /// Path (device:function pairs)
    pub path: Vec<(u8, u8)>,
}

impl DeviceScope {
    /// Create new device scope for PCI endpoint
    pub fn pci_endpoint(bus: u8, device: u8, function: u8) -> Self {
        Self {
            scope_type: DeviceScopeType::PciEndpoint,
            enumeration_id: 0,
            start_bus: bus,
            path: vec![(device, function)],
        }
    }

    /// Create new device scope for PCI sub-hierarchy (bridge)
    pub fn pci_bridge(bus: u8, device: u8, function: u8) -> Self {
        Self {
            scope_type: DeviceScopeType::PciSubHierarchy,
            enumeration_id: 0,
            start_bus: bus,
            path: vec![(device, function)],
        }
    }

    /// Create new device scope for IOAPIC
    pub fn ioapic(ioapic_id: u8, bus: u8, device: u8, function: u8) -> Self {
        Self {
            scope_type: DeviceScopeType::IoApic,
            enumeration_id: ioapic_id,
            start_bus: bus,
            path: vec![(device, function)],
        }
    }

    /// Create new device scope for HPET
    pub fn hpet(hpet_id: u8, bus: u8, device: u8, function: u8) -> Self {
        Self {
            scope_type: DeviceScopeType::MsiCapableHpet,
            enumeration_id: hpet_id,
            start_bus: bus,
            path: vec![(device, function)],
        }
    }

    /// Get the device ID from the scope
    pub fn device_id(&self, segment: u16) -> DeviceId {
        if let Some(&(device, function)) = self.path.last() {
            DeviceId::new(segment, self.start_bus, device, function)
        } else {
            DeviceId::new(segment, self.start_bus, 0, 0)
        }
    }

    /// Get encoded size in bytes
    pub fn encoded_size(&self) -> usize {
        6 + self.path.len() * 2
    }

    /// Encode to bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.encoded_size());
        bytes.push(self.scope_type as u8);
        bytes.push(self.encoded_size() as u8);
        bytes.push(0); // Reserved
        bytes.push(self.enumeration_id);
        bytes.push(self.start_bus);
        bytes.push(0); // Reserved

        for (device, function) in &self.path {
            bytes.push(*device);
            bytes.push(*function);
        }

        bytes
    }
}

/// IOMMU page table entry flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTableFlags(u64);

impl PageTableFlags {
    /// Entry is present/valid
    pub const PRESENT: Self = Self(1 << 0);
    /// Read permission
    pub const READ: Self = Self(1 << 0);
    /// Write permission
    pub const WRITE: Self = Self(1 << 1);
    /// Execute permission (AMD-Vi)
    pub const EXECUTE: Self = Self(1 << 62);
    /// Accessed flag
    pub const ACCESSED: Self = Self(1 << 5);
    /// Dirty flag
    pub const DIRTY: Self = Self(1 << 6);
    /// Page size (large page)
    pub const PAGE_SIZE: Self = Self(1 << 7);
    /// Snoop behavior (Intel VT-d)
    pub const SNOOP: Self = Self(1 << 11);
    /// Transient mapping
    pub const TRANSIENT: Self = Self(1 << 62);

    /// Create empty flags
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create flags from raw value
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Get raw bits
    pub const fn bits(&self) -> u64 {
        self.0
    }

    /// Check if flag is set
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Insert flags
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Remove flags
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// Check if entry is present
    pub const fn is_present(&self) -> bool {
        self.contains(Self::PRESENT)
    }

    /// Check if writable
    pub const fn is_writable(&self) -> bool {
        self.contains(Self::WRITE)
    }

    /// Standard read-write mapping
    pub const fn read_write() -> Self {
        Self(Self::READ.0 | Self::WRITE.0)
    }

    /// Read-only mapping
    pub const fn read_only() -> Self {
        Self::READ
    }
}

impl std::ops::BitOr for PageTableFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for PageTableFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

/// IOMMU page table entry
#[derive(Debug, Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// Physical address mask (bits 12-51)
    const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

    /// Create empty (not present) entry
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create from raw value
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get raw value
    pub const fn raw(&self) -> u64 {
        self.0
    }

    /// Create entry pointing to next level table
    pub fn table(phys_addr: u64) -> Self {
        Self((phys_addr & Self::ADDR_MASK) | PageTableFlags::read_write().bits())
    }

    /// Create entry mapping to physical page
    pub fn page(phys_addr: u64, flags: PageTableFlags) -> Self {
        Self((phys_addr & Self::ADDR_MASK) | flags.bits())
    }

    /// Create large page entry (2MB or 1GB)
    pub fn large_page(phys_addr: u64, flags: PageTableFlags) -> Self {
        Self((phys_addr & Self::ADDR_MASK) | flags.bits() | PageTableFlags::PAGE_SIZE.bits())
    }

    /// Check if entry is present
    pub const fn is_present(&self) -> bool {
        (self.0 & PageTableFlags::PRESENT.bits()) != 0
    }

    /// Check if this is a large page
    pub const fn is_large_page(&self) -> bool {
        (self.0 & PageTableFlags::PAGE_SIZE.bits()) != 0
    }

    /// Get physical address
    pub const fn phys_addr(&self) -> u64 {
        self.0 & Self::ADDR_MASK
    }

    /// Get flags
    pub const fn flags(&self) -> PageTableFlags {
        PageTableFlags::from_bits(self.0 & !Self::ADDR_MASK)
    }

    /// Set physical address
    pub fn set_phys_addr(&mut self, addr: u64) {
        self.0 = (self.0 & !Self::ADDR_MASK) | (addr & Self::ADDR_MASK);
    }

    /// Set flags
    pub fn set_flags(&mut self, flags: PageTableFlags) {
        self.0 = (self.0 & Self::ADDR_MASK) | flags.bits();
    }
}

/// IOMMU fault reason
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FaultReason {
    /// No fault
    None = 0,
    /// Present bit clear in root entry
    RootNotPresent = 1,
    /// Present bit clear in context entry
    ContextNotPresent = 2,
    /// Invalid context entry
    InvalidContextEntry = 3,
    /// Reserved field non-zero
    ReservedFieldFault = 4,
    /// Address translation fault
    AddressTranslationFault = 5,
    /// Write request blocked
    WriteBlocked = 6,
    /// Read request blocked
    ReadBlocked = 7,
    /// Invalid request
    InvalidRequest = 8,
    /// Page table entry not present
    PageNotPresent = 9,
    /// Invalid page table entry
    InvalidPageEntry = 10,
    /// Access flag fault
    AccessFlagFault = 11,
    /// Address size fault
    AddressSizeFault = 12,
}

impl FaultReason {
    /// Convert from raw value
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => FaultReason::None,
            1 => FaultReason::RootNotPresent,
            2 => FaultReason::ContextNotPresent,
            3 => FaultReason::InvalidContextEntry,
            4 => FaultReason::ReservedFieldFault,
            5 => FaultReason::AddressTranslationFault,
            6 => FaultReason::WriteBlocked,
            7 => FaultReason::ReadBlocked,
            8 => FaultReason::InvalidRequest,
            9 => FaultReason::PageNotPresent,
            10 => FaultReason::InvalidPageEntry,
            11 => FaultReason::AccessFlagFault,
            12 => FaultReason::AddressSizeFault,
            _ => FaultReason::InvalidRequest,
        }
    }

    /// Check if this is a real fault
    pub fn is_fault(&self) -> bool {
        *self != FaultReason::None
    }
}

/// IOMMU fault record
#[derive(Debug, Clone)]
pub struct FaultRecord {
    /// Faulting device
    pub device: DeviceId,
    /// Faulting address
    pub address: u64,
    /// Fault reason
    pub reason: FaultReason,
    /// Was this a write?
    pub is_write: bool,
    /// Page request (vs. translation fault)
    pub is_page_request: bool,
    /// Timestamp
    pub timestamp: u64,
}

impl FaultRecord {
    /// Create new fault record
    pub fn new(device: DeviceId, address: u64, reason: FaultReason, is_write: bool) -> Self {
        Self {
            device,
            address,
            reason,
            is_write,
            is_page_request: false,
            timestamp: 0,
        }
    }
}

/// Domain ID for isolation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainId(pub u16);

impl DomainId {
    /// Invalid domain ID
    pub const INVALID: Self = Self(0xFFFF);

    /// Create new domain ID
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    /// Check if valid
    pub const fn is_valid(&self) -> bool {
        self.0 != 0xFFFF
    }
}

/// Translation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationType {
    /// Identity mapping (passthrough)
    Identity,
    /// Full translation
    Translated,
    /// Reserved/blocked
    Reserved,
}

/// IOMMU statistics
#[derive(Debug, Default)]
pub struct IommuStats {
    /// Total translations
    pub translations: AtomicU64,
    /// Translation cache hits
    pub iotlb_hits: AtomicU64,
    /// Translation cache misses
    pub iotlb_misses: AtomicU64,
    /// Page walk count
    pub page_walks: AtomicU64,
    /// Fault count
    pub faults: AtomicU64,
    /// Context cache invalidations
    pub context_invalidations: AtomicU64,
    /// IOTLB invalidations
    pub iotlb_invalidations: AtomicU64,
}

impl IommuStats {
    /// Record a translation
    pub fn record_translation(&self, cache_hit: bool) {
        self.translations.fetch_add(1, Ordering::Relaxed);
        if cache_hit {
            self.iotlb_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.iotlb_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a page walk
    pub fn record_page_walk(&self) {
        self.page_walks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a fault
    pub fn record_fault(&self) {
        self.faults.fetch_add(1, Ordering::Relaxed);
    }

    /// Record context invalidation
    pub fn record_context_invalidation(&self) {
        self.context_invalidations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record IOTLB invalidation
    pub fn record_iotlb_invalidation(&self) {
        self.iotlb_invalidations.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> IommuStatsSnapshot {
        IommuStatsSnapshot {
            translations: self.translations.load(Ordering::Relaxed),
            iotlb_hits: self.iotlb_hits.load(Ordering::Relaxed),
            iotlb_misses: self.iotlb_misses.load(Ordering::Relaxed),
            page_walks: self.page_walks.load(Ordering::Relaxed),
            faults: self.faults.load(Ordering::Relaxed),
            context_invalidations: self.context_invalidations.load(Ordering::Relaxed),
            iotlb_invalidations: self.iotlb_invalidations.load(Ordering::Relaxed),
        }
    }
}

/// IOMMU statistics snapshot
#[derive(Debug, Clone, Default)]
pub struct IommuStatsSnapshot {
    /// Total translations
    pub translations: u64,
    /// Translation cache hits
    pub iotlb_hits: u64,
    /// Translation cache misses
    pub iotlb_misses: u64,
    /// Page walk count
    pub page_walks: u64,
    /// Fault count
    pub faults: u64,
    /// Context cache invalidations
    pub context_invalidations: u64,
    /// IOTLB invalidations
    pub iotlb_invalidations: u64,
}

impl IommuStatsSnapshot {
    /// Calculate IOTLB hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.iotlb_hits + self.iotlb_misses;
        if total == 0 {
            0.0
        } else {
            self.iotlb_hits as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_width() {
        assert_eq!(AddressWidth::Bits39.bits(), 39);
        assert_eq!(AddressWidth::Bits48.levels(), 5);
        assert_eq!(AddressWidth::from_agaw(1), Some(AddressWidth::Bits39));
        assert_eq!(AddressWidth::from_agaw(5), None);
    }

    #[test]
    fn test_device_id() {
        let dev = DeviceId::new(0, 3, 5, 2);
        assert_eq!(dev.bus, 3);
        assert_eq!(dev.device(), 5);
        assert_eq!(dev.function(), 2);
        assert_eq!(dev.source_id(), 0x032A); // (3 << 8) | (5 << 3) | 2
    }

    #[test]
    fn test_device_id_display() {
        let dev = DeviceId::new(0, 0x03, 0x05, 0x02);
        assert_eq!(format!("{}", dev), "0000:03:05.2");
    }

    #[test]
    fn test_device_id_from_source() {
        let dev = DeviceId::from_source_id(0, 0x1234);
        assert_eq!(dev.bus, 0x12);
        assert_eq!(dev.devfn, 0x34);
    }

    #[test]
    fn test_device_scope_endpoint() {
        let scope = DeviceScope::pci_endpoint(0, 3, 0);
        assert_eq!(scope.scope_type, DeviceScopeType::PciEndpoint);
        assert_eq!(scope.start_bus, 0);
        assert_eq!(scope.path.len(), 1);
    }

    #[test]
    fn test_device_scope_encode() {
        let scope = DeviceScope::pci_endpoint(0, 3, 0);
        let bytes = scope.encode();
        assert_eq!(bytes[0], DeviceScopeType::PciEndpoint as u8);
        assert_eq!(bytes[1], 8); // Length
        assert_eq!(bytes[4], 0); // Start bus
        assert_eq!(bytes[6], 3); // Device
        assert_eq!(bytes[7], 0); // Function
    }

    #[test]
    fn test_device_scope_ioapic() {
        let scope = DeviceScope::ioapic(0, 0, 31, 0);
        assert_eq!(scope.scope_type, DeviceScopeType::IoApic);
        assert_eq!(scope.enumeration_id, 0);
    }

    #[test]
    fn test_page_table_flags() {
        let flags = PageTableFlags::READ | PageTableFlags::WRITE;
        assert!(flags.contains(PageTableFlags::READ));
        assert!(flags.contains(PageTableFlags::WRITE));
        assert!(!flags.contains(PageTableFlags::PAGE_SIZE));
    }

    #[test]
    fn test_page_table_flags_operations() {
        let mut flags = PageTableFlags::empty();
        flags.insert(PageTableFlags::READ);
        assert!(flags.is_present());
        flags.insert(PageTableFlags::WRITE);
        assert!(flags.is_writable());
    }

    #[test]
    fn test_page_table_entry() {
        let entry = PageTableEntry::page(0x1000_0000, PageTableFlags::read_write());
        assert!(entry.is_present());
        assert!(!entry.is_large_page());
        assert_eq!(entry.phys_addr(), 0x1000_0000);
    }

    #[test]
    fn test_page_table_entry_large() {
        let entry = PageTableEntry::large_page(0x0020_0000, PageTableFlags::read_write());
        assert!(entry.is_present());
        assert!(entry.is_large_page());
    }

    #[test]
    fn test_page_table_entry_table() {
        let entry = PageTableEntry::table(0x2000_0000);
        assert!(entry.is_present());
        assert_eq!(entry.phys_addr(), 0x2000_0000);
    }

    #[test]
    fn test_fault_reason() {
        assert!(!FaultReason::None.is_fault());
        assert!(FaultReason::PageNotPresent.is_fault());
        assert_eq!(FaultReason::from_u8(9), FaultReason::PageNotPresent);
    }

    #[test]
    fn test_fault_record() {
        let dev = DeviceId::new(0, 1, 2, 0);
        let fault = FaultRecord::new(dev, 0xDEAD_BEEF, FaultReason::PageNotPresent, true);
        assert!(fault.is_write);
        assert_eq!(fault.reason, FaultReason::PageNotPresent);
    }

    #[test]
    fn test_domain_id() {
        let domain = DomainId::new(5);
        assert!(domain.is_valid());
        assert!(!DomainId::INVALID.is_valid());
    }

    #[test]
    fn test_iommu_stats() {
        let stats = IommuStats::default();
        stats.record_translation(true);
        stats.record_translation(false);
        stats.record_fault();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.translations, 2);
        assert_eq!(snapshot.iotlb_hits, 1);
        assert_eq!(snapshot.iotlb_misses, 1);
        assert_eq!(snapshot.faults, 1);
    }

    #[test]
    fn test_iommu_stats_hit_rate() {
        let stats = IommuStats::default();
        stats.record_translation(true);
        stats.record_translation(true);
        stats.record_translation(false);

        let snapshot = stats.snapshot();
        let hit_rate = snapshot.hit_rate();
        assert!((hit_rate - 0.666666).abs() < 0.001);
    }
}
