//! Intel VT-d IOMMU Support
//!
//! This module provides Intel VT-d (Virtualization Technology for Directed I/O)
//! support including DMAR table generation, DMA remapping, and interrupt remapping.

use super::types::{
    AddressWidth, DeviceId, DeviceScope, DomainId, FaultReason, FaultRecord, IommuStats,
    PageTableEntry, PageTableFlags, TranslationType, PAGE_SIZE_4K,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// VT-d register offsets
pub mod registers {
    /// Version register
    pub const VER: u32 = 0x00;
    /// Capability register
    pub const CAP: u32 = 0x08;
    /// Extended capability register
    pub const ECAP: u32 = 0x10;
    /// Global command register
    pub const GCMD: u32 = 0x18;
    /// Global status register
    pub const GSTS: u32 = 0x1C;
    /// Root table address register
    pub const RTADDR: u32 = 0x20;
    /// Context command register
    pub const CCMD: u32 = 0x28;
    /// Fault status register
    pub const FSTS: u32 = 0x34;
    /// Fault event control register
    pub const FECTL: u32 = 0x38;
    /// Fault event data register
    pub const FEDATA: u32 = 0x3C;
    /// Fault event address register
    pub const FEADDR: u32 = 0x40;
    /// Fault event upper address register
    pub const FEUADDR: u32 = 0x44;
    /// Advanced fault log register
    pub const AFLOG: u32 = 0x58;
    /// Protected memory enable register
    pub const PMEN: u32 = 0x64;
    /// Protected low memory base register
    pub const PLMBASE: u32 = 0x68;
    /// Protected low memory limit register
    pub const PLMLIMIT: u32 = 0x6C;
    /// Protected high memory base register
    pub const PHMBASE: u32 = 0x70;
    /// Protected high memory limit register
    pub const PHMLIMIT: u32 = 0x78;
    /// Invalidation queue head register
    pub const IQH: u32 = 0x80;
    /// Invalidation queue tail register
    pub const IQT: u32 = 0x88;
    /// Invalidation queue address register
    pub const IQA: u32 = 0x90;
    /// Invalidation completion status register
    pub const ICS: u32 = 0x9C;
    /// Invalidation event control register
    pub const IECTL: u32 = 0xA0;
    /// Invalidation event data register
    pub const IEDATA: u32 = 0xA4;
    /// Invalidation event address register
    pub const IEADDR: u32 = 0xA8;
    /// Invalidation event upper address register
    pub const IEUADDR: u32 = 0xAC;
    /// Interrupt remapping table address register
    pub const IRTA: u32 = 0xB8;
    /// Page request queue head register
    pub const PQH: u32 = 0xC0;
    /// Page request queue tail register
    pub const PQT: u32 = 0xC8;
    /// Page request queue address register
    pub const PQA: u32 = 0xD0;
    /// Page request status register
    pub const PRS: u32 = 0xDC;
    /// Page request event control register
    pub const PECTL: u32 = 0xE0;
    /// Page request event data register
    pub const PEDATA: u32 = 0xE4;
    /// Page request event address register
    pub const PEADDR: u32 = 0xE8;
    /// Page request event upper address register
    pub const PEUADDR: u32 = 0xEC;
}

/// Global command register bits
pub mod gcmd {
    /// Translation enable
    pub const TE: u32 = 1 << 31;
    /// Set root table pointer
    pub const SRTP: u32 = 1 << 30;
    /// Set fault log
    pub const SFL: u32 = 1 << 29;
    /// Enable advanced fault logging
    pub const EAFL: u32 = 1 << 28;
    /// Write buffer flush
    pub const WBF: u32 = 1 << 27;
    /// Queued invalidation enable
    pub const QIE: u32 = 1 << 26;
    /// Interrupt remapping enable
    pub const IRE: u32 = 1 << 25;
    /// Set interrupt remapping table pointer
    pub const SIRTP: u32 = 1 << 24;
    /// Compatibility format interrupt
    pub const CFI: u32 = 1 << 23;
}

/// Global status register bits
pub mod gsts {
    /// Translation enable status
    pub const TES: u32 = 1 << 31;
    /// Root table pointer status
    pub const RTPS: u32 = 1 << 30;
    /// Fault log status
    pub const FLS: u32 = 1 << 29;
    /// Advanced fault logging status
    pub const AFLS: u32 = 1 << 28;
    /// Write buffer flush status
    pub const WBFS: u32 = 1 << 27;
    /// Queued invalidation enable status
    pub const QIES: u32 = 1 << 26;
    /// Interrupt remapping enable status
    pub const IRES: u32 = 1 << 25;
    /// Interrupt remapping table pointer status
    pub const IRTPS: u32 = 1 << 24;
    /// Compatibility format interrupt status
    pub const CFIS: u32 = 1 << 23;
}

/// Capability register bits
pub mod cap {
    /// Number of domains supported
    pub const ND_MASK: u64 = 0x07;
    /// Advanced fault logging support
    pub const AFL: u64 = 1 << 3;
    /// Required write buffer flushing
    pub const RWBF: u64 = 1 << 4;
    /// Protected low memory region support
    pub const PLMR: u64 = 1 << 5;
    /// Protected high memory region support
    pub const PHMR: u64 = 1 << 6;
    /// Caching mode
    pub const CM: u64 = 1 << 7;
    /// Supported adjusted guest address widths
    pub const SAGAW_MASK: u64 = 0x1F << 8;
    pub const SAGAW_SHIFT: u64 = 8;
    /// Maximum guest address width
    pub const MGAW_MASK: u64 = 0x3F << 16;
    pub const MGAW_SHIFT: u64 = 16;
    /// Zero length read support
    pub const ZLR: u64 = 1 << 22;
    /// Deprecated (was isoch)
    pub const DEPRECATED: u64 = 1 << 23;
    /// Fault recording register offset
    pub const FRO_MASK: u64 = 0x3FF << 24;
    pub const FRO_SHIFT: u64 = 24;
    /// Super page support
    pub const SLLPS_MASK: u64 = 0x0F << 34;
    pub const SLLPS_SHIFT: u64 = 34;
    /// Page selective invalidation
    pub const PSI: u64 = 1 << 39;
    /// Number of fault recording registers
    pub const NFR_MASK: u64 = 0xFF << 40;
    pub const NFR_SHIFT: u64 = 40;
    /// Maximum address mask value
    pub const MAMV_MASK: u64 = 0x3F << 48;
    pub const MAMV_SHIFT: u64 = 48;
    /// DMA write draining
    pub const DWD: u64 = 1 << 54;
    /// DMA read draining
    pub const DRD: u64 = 1 << 55;
    /// First level 1GB page support
    pub const FL1GP: u64 = 1 << 56;
    /// Posted interrupts support
    pub const PI: u64 = 1 << 59;
    /// First level 5-level paging support
    pub const FL5LP: u64 = 1 << 60;
    /// Enhanced set interrupt remap table pointer support
    pub const ESIRTPS: u64 = 1 << 61;
    /// Enhanced set root table pointer support
    pub const ESRTPS: u64 = 1 << 62;
}

/// Extended capability register bits
pub mod ecap {
    /// Page walk coherency
    pub const C: u64 = 1 << 0;
    /// Queued invalidation support
    pub const QI: u64 = 1 << 1;
    /// Device TLB support
    pub const DT: u64 = 1 << 2;
    /// Interrupt remapping support
    pub const IR: u64 = 1 << 3;
    /// Extended interrupt mode
    pub const EIM: u64 = 1 << 4;
    /// Deprecated
    pub const DEPRECATED1: u64 = 1 << 5;
    /// Pass through support
    pub const PT: u64 = 1 << 6;
    /// Snoop control
    pub const SC: u64 = 1 << 7;
    /// IOTLB register offset
    pub const IRO_MASK: u64 = 0x3FF << 8;
    pub const IRO_SHIFT: u64 = 8;
    /// Deprecated
    pub const DEPRECATED2: u64 = 1 << 18;
    /// Maximum handle mask value
    pub const MHMV_MASK: u64 = 0x0F << 20;
    pub const MHMV_SHIFT: u64 = 20;
    /// Deprecated
    pub const DEPRECATED3: u64 = 1 << 24;
    /// Memory type support
    pub const MTS: u64 = 1 << 25;
    /// Nested translation support
    pub const NEST: u64 = 1 << 26;
    /// Deferred invalidate
    pub const DIS: u64 = 1 << 27;
    /// Page request support
    pub const PRS: u64 = 1 << 29;
    /// Execute request support
    pub const ERS: u64 = 1 << 30;
    /// Supervisor request support
    pub const SRS: u64 = 1 << 31;
    /// No write flag support
    pub const NWFS: u64 = 1 << 33;
    /// Extended accessed flag support
    pub const EAFS: u64 = 1 << 34;
    /// Process address space ID size
    pub const PSS_MASK: u64 = 0x1F << 35;
    pub const PSS_SHIFT: u64 = 35;
    /// Page request drain support
    pub const PASID: u64 = 1 << 40;
    /// Device TLB invalidation throttle
    pub const DIT: u64 = 1 << 41;
    /// Page walk drain support
    pub const PDS: u64 = 1 << 42;
    /// Scalable mode translation support
    pub const SMTS: u64 = 1 << 43;
    /// Virtual command support
    pub const VCS: u64 = 1 << 44;
    /// Second level accessed/dirty support
    pub const SLADS: u64 = 1 << 45;
    /// Scalable mode page walk coherency
    pub const SMPWCS: u64 = 1 << 46;
    /// RID-PASID support
    pub const RPS: u64 = 1 << 47;
}

/// Root table entry
#[derive(Debug, Clone, Copy)]
pub struct RootEntry {
    /// Lower 64 bits
    lo: u64,
    /// Upper 64 bits (extended root entry)
    hi: u64,
}

impl RootEntry {
    /// Entry is present
    const PRESENT: u64 = 1 << 0;
    /// Context table pointer mask
    const CTP_MASK: u64 = 0xFFFF_FFFF_FFFF_F000;

    /// Create empty entry
    pub const fn empty() -> Self {
        Self { lo: 0, hi: 0 }
    }

    /// Create entry pointing to context table
    pub fn new(context_table_addr: u64) -> Self {
        Self {
            lo: (context_table_addr & Self::CTP_MASK) | Self::PRESENT,
            hi: 0,
        }
    }

    /// Check if present
    pub const fn is_present(&self) -> bool {
        (self.lo & Self::PRESENT) != 0
    }

    /// Get context table address
    pub const fn context_table_addr(&self) -> u64 {
        self.lo & Self::CTP_MASK
    }

    /// Get raw lower bits
    pub const fn lo(&self) -> u64 {
        self.lo
    }

    /// Get raw upper bits
    pub const fn hi(&self) -> u64 {
        self.hi
    }
}

/// Context entry (Type 0 - legacy)
#[derive(Debug, Clone, Copy)]
pub struct ContextEntry {
    /// Lower 64 bits
    lo: u64,
    /// Upper 64 bits
    hi: u64,
}

impl ContextEntry {
    /// Entry is present
    const PRESENT: u64 = 1 << 0;
    /// Fault processing disable
    const FPD: u64 = 1 << 1;
    /// Translation type shift
    const TT_SHIFT: u64 = 2;
    /// Translation type mask
    const TT_MASK: u64 = 0x03 << 2;
    /// Second level page table pointer mask
    const SLPTPTR_MASK: u64 = 0xFFFF_FFFF_FFFF_F000;
    /// Address width shift
    const AW_SHIFT: u64 = 0;
    /// Address width mask
    const AW_MASK: u64 = 0x07;
    /// Domain ID shift
    const DID_SHIFT: u64 = 8;
    /// Domain ID mask
    const DID_MASK: u64 = 0xFFFF << 8;

    /// Create empty entry
    pub const fn empty() -> Self {
        Self { lo: 0, hi: 0 }
    }

    /// Create identity mapping entry
    pub fn identity(domain_id: DomainId, address_width: AddressWidth) -> Self {
        Self {
            lo: Self::PRESENT | (0x02 << Self::TT_SHIFT), // Pass-through
            hi: ((domain_id.0 as u64) << Self::DID_SHIFT) | (address_width as u64),
        }
    }

    /// Create translated mapping entry
    pub fn translated(
        page_table_addr: u64,
        domain_id: DomainId,
        address_width: AddressWidth,
    ) -> Self {
        Self {
            lo: (page_table_addr & Self::SLPTPTR_MASK) | Self::PRESENT,
            hi: ((domain_id.0 as u64) << Self::DID_SHIFT) | (address_width as u64),
        }
    }

    /// Check if present
    pub const fn is_present(&self) -> bool {
        (self.lo & Self::PRESENT) != 0
    }

    /// Check if fault processing disabled
    pub const fn is_fpd(&self) -> bool {
        (self.lo & Self::FPD) != 0
    }

    /// Get translation type
    pub fn translation_type(&self) -> TranslationType {
        match (self.lo & Self::TT_MASK) >> Self::TT_SHIFT {
            0 => TranslationType::Translated,
            1 => TranslationType::Translated,
            2 => TranslationType::Identity,
            _ => TranslationType::Reserved,
        }
    }

    /// Get page table address
    pub const fn page_table_addr(&self) -> u64 {
        self.lo & Self::SLPTPTR_MASK
    }

    /// Get domain ID
    pub fn domain_id(&self) -> DomainId {
        DomainId::new(((self.hi & Self::DID_MASK) >> Self::DID_SHIFT) as u16)
    }

    /// Get address width
    pub fn address_width(&self) -> Option<AddressWidth> {
        AddressWidth::from_agaw((self.hi & Self::AW_MASK) as u8)
    }

    /// Get raw lower bits
    pub const fn lo(&self) -> u64 {
        self.lo
    }

    /// Get raw upper bits
    pub const fn hi(&self) -> u64 {
        self.hi
    }
}

/// IOTLB entry
#[derive(Debug, Clone)]
pub struct IotlbEntry {
    /// Device ID
    pub device: DeviceId,
    /// Domain ID
    pub domain: DomainId,
    /// Input address (page aligned)
    pub iova: u64,
    /// Output address (page aligned)
    pub hpa: u64,
    /// Page size (4K, 2M, 1G)
    pub page_size: u64,
    /// Permissions
    pub flags: PageTableFlags,
    /// Access timestamp
    pub timestamp: u64,
}

impl IotlbEntry {
    /// Create new IOTLB entry
    pub fn new(
        device: DeviceId,
        domain: DomainId,
        iova: u64,
        hpa: u64,
        page_size: u64,
        flags: PageTableFlags,
    ) -> Self {
        Self {
            device,
            domain,
            iova: iova & !(page_size - 1),
            hpa: hpa & !(page_size - 1),
            page_size,
            flags,
            timestamp: 0,
        }
    }

    /// Check if this entry matches a lookup
    pub fn matches(&self, device: &DeviceId, domain: DomainId, iova: u64) -> bool {
        self.device == *device
            && self.domain == domain
            && iova >= self.iova
            && iova < self.iova + self.page_size
    }

    /// Translate IOVA to HPA
    pub fn translate(&self, iova: u64) -> u64 {
        self.hpa + (iova - self.iova)
    }
}

/// IOTLB cache
pub struct Iotlb {
    /// Entries indexed by (device source_id, domain, iova >> 12)
    entries: HashMap<(u16, u16, u64), IotlbEntry>,
    /// Maximum entries
    max_entries: usize,
    /// Current timestamp
    timestamp: AtomicU64,
}

impl Default for Iotlb {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl Iotlb {
    /// Create new IOTLB
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries),
            max_entries,
            timestamp: AtomicU64::new(0),
        }
    }

    /// Lookup entry
    pub fn lookup(&self, device: &DeviceId, domain: DomainId, iova: u64) -> Option<&IotlbEntry> {
        let key = (device.source_id(), domain.0, iova >> 12);
        self.entries
            .get(&key)
            .filter(|e| e.matches(device, domain, iova))
    }

    /// Insert entry
    pub fn insert(&mut self, entry: IotlbEntry) {
        // Evict if full
        if self.entries.len() >= self.max_entries {
            self.evict_oldest();
        }

        let key = (entry.device.source_id(), entry.domain.0, entry.iova >> 12);
        self.entries.insert(key, entry);
    }

    /// Invalidate entries for a device
    pub fn invalidate_device(&mut self, device: &DeviceId) {
        let source_id = device.source_id();
        self.entries.retain(|k, _| k.0 != source_id);
    }

    /// Invalidate entries for a domain
    pub fn invalidate_domain(&mut self, domain: DomainId) {
        self.entries.retain(|k, _| k.1 != domain.0);
    }

    /// Invalidate specific page
    pub fn invalidate_page(&mut self, device: &DeviceId, domain: DomainId, iova: u64) {
        let key = (device.source_id(), domain.0, iova >> 12);
        self.entries.remove(&key);
    }

    /// Invalidate all entries
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }

    /// Evict oldest entry (simple LRU approximation)
    fn evict_oldest(&mut self) {
        if let Some(key) = self.entries.keys().next().cloned() {
            self.entries.remove(&key);
        }
    }

    /// Get entry count
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// VT-d hardware unit
pub struct VtdUnit {
    /// Base MMIO address
    base_address: u64,
    /// Segment
    segment: u16,
    /// Capabilities
    capabilities: u64,
    /// Extended capabilities
    extended_capabilities: u64,
    /// Global status
    global_status: u32,
    /// Root table address
    root_table_addr: u64,
    /// Root table (256 entries, one per bus)
    root_table: Vec<RootEntry>,
    /// Context tables (per bus)
    context_tables: HashMap<u8, Vec<ContextEntry>>,
    /// Page tables (per domain)
    page_tables: HashMap<DomainId, Vec<PageTableEntry>>,
    /// Translation enabled
    translation_enabled: AtomicBool,
    /// IOTLB
    iotlb: Iotlb,
    /// Fault records
    fault_records: Vec<FaultRecord>,
    /// Statistics
    stats: IommuStats,
    /// Device scopes
    device_scopes: Vec<DeviceScope>,
    /// Address width
    address_width: AddressWidth,
}

impl Default for VtdUnit {
    fn default() -> Self {
        Self::new(0xFED90000, 0)
    }
}

impl VtdUnit {
    /// Create new VT-d unit
    pub fn new(base_address: u64, segment: u16) -> Self {
        // Standard capabilities for emulated VT-d
        let capabilities = cap::ND_MASK // 16 domains
            | (0x1E << cap::SAGAW_SHIFT) // 39, 48, 57-bit AGAW
            | (57 << cap::MGAW_SHIFT) // 57-bit max guest address width
            | cap::CM // Caching mode
            | cap::PSI // Page selective invalidation
            | (1 << cap::NFR_SHIFT) // 2 fault recording registers
            | cap::DWD // Write draining
            | cap::DRD; // Read draining

        let extended_capabilities = ecap::C // Page walk coherency
            | ecap::QI // Queued invalidation
            | ecap::IR // Interrupt remapping
            | ecap::PT // Pass through
            | ecap::SC // Snoop control
            | (0x10 << ecap::IRO_SHIFT); // IOTLB register offset

        Self {
            base_address,
            segment,
            capabilities,
            extended_capabilities,
            global_status: 0,
            root_table_addr: 0,
            root_table: vec![RootEntry::empty(); 256],
            context_tables: HashMap::new(),
            page_tables: HashMap::new(),
            translation_enabled: AtomicBool::new(false),
            iotlb: Iotlb::default(),
            fault_records: Vec::new(),
            stats: IommuStats::default(),
            device_scopes: Vec::new(),
            address_width: AddressWidth::Bits48,
        }
    }

    /// Get base address
    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    /// Get segment
    pub fn segment(&self) -> u16 {
        self.segment
    }

    /// Get capabilities
    pub fn capabilities(&self) -> u64 {
        self.capabilities
    }

    /// Get extended capabilities
    pub fn extended_capabilities(&self) -> u64 {
        self.extended_capabilities
    }

    /// Add device scope
    pub fn add_device_scope(&mut self, scope: DeviceScope) {
        self.device_scopes.push(scope);
    }

    /// Get device scopes
    pub fn device_scopes(&self) -> &[DeviceScope] {
        &self.device_scopes
    }

    /// Get statistics
    pub fn stats(&self) -> &IommuStats {
        &self.stats
    }

    /// Read register
    pub fn read_register(&self, offset: u32) -> u64 {
        match offset {
            registers::VER => 0x10, // Version 1.0
            registers::CAP => self.capabilities,
            registers::ECAP => self.extended_capabilities,
            registers::GSTS => self.global_status as u64,
            registers::RTADDR => self.root_table_addr,
            registers::FSTS => {
                if self.fault_records.is_empty() {
                    0
                } else {
                    1 // PPF - Primary Pending Fault
                }
            }
            _ => 0,
        }
    }

    /// Write register
    pub fn write_register(&mut self, offset: u32, value: u64) {
        match offset {
            registers::GCMD => {
                let cmd = value as u32;
                if cmd & gcmd::TE != 0 {
                    self.enable_translation();
                } else {
                    self.disable_translation();
                }
                if cmd & gcmd::SRTP != 0 {
                    self.global_status |= gsts::RTPS;
                }
            }
            registers::RTADDR => {
                self.root_table_addr = value & 0xFFFF_FFFF_FFFF_F000;
            }
            registers::CCMD => {
                // Context command - handle invalidation
                self.handle_context_command(value);
            }
            _ => {}
        }
    }

    /// Enable translation
    pub fn enable_translation(&mut self) {
        self.translation_enabled.store(true, Ordering::SeqCst);
        self.global_status |= gsts::TES;
    }

    /// Disable translation
    pub fn disable_translation(&mut self) {
        self.translation_enabled.store(false, Ordering::SeqCst);
        self.global_status &= !gsts::TES;
    }

    /// Check if translation is enabled
    pub fn is_translation_enabled(&self) -> bool {
        self.translation_enabled.load(Ordering::SeqCst)
    }

    /// Handle context command
    fn handle_context_command(&mut self, cmd: u64) {
        let granularity = (cmd >> 61) & 0x03;
        match granularity {
            0 => {
                // Global invalidation
                self.iotlb.invalidate_all();
                self.stats.record_context_invalidation();
            }
            1 => {
                // Domain-selective
                let domain = DomainId::new((cmd & 0xFFFF) as u16);
                self.iotlb.invalidate_domain(domain);
                self.stats.record_context_invalidation();
            }
            2 => {
                // Device-selective
                let source_id = ((cmd >> 16) & 0xFFFF) as u16;
                let device = DeviceId::from_source_id(self.segment, source_id);
                self.iotlb.invalidate_device(&device);
                self.stats.record_context_invalidation();
            }
            _ => {}
        }
    }

    /// Set root entry for a bus
    pub fn set_root_entry(&mut self, bus: u8, entry: RootEntry) {
        self.root_table[bus as usize] = entry;
    }

    /// Get root entry for a bus
    pub fn root_entry(&self, bus: u8) -> &RootEntry {
        &self.root_table[bus as usize]
    }

    /// Set context entry
    pub fn set_context_entry(&mut self, bus: u8, devfn: u8, entry: ContextEntry) {
        let table = self
            .context_tables
            .entry(bus)
            .or_insert_with(|| vec![ContextEntry::empty(); 256]);
        table[devfn as usize] = entry;
    }

    /// Get context entry
    pub fn context_entry(&self, bus: u8, devfn: u8) -> Option<&ContextEntry> {
        self.context_tables.get(&bus).map(|t| &t[devfn as usize])
    }

    /// Configure device for passthrough (identity mapping)
    pub fn configure_passthrough(&mut self, device: &DeviceId, domain: DomainId) {
        // Set up root entry if needed
        if !self.root_table[device.bus as usize].is_present() {
            // Generate a unique context table address per bus: page-aligned, non-zero
            let ctx_addr = Self::context_table_addr_for_bus(device.bus);
            self.root_table[device.bus as usize] = RootEntry::new(ctx_addr);
        }

        // Set up context entry for identity mapping
        let ctx = ContextEntry::identity(domain, self.address_width);
        self.set_context_entry(device.bus, device.devfn, ctx);
    }

    /// Configure device for full translation
    pub fn configure_translation(
        &mut self,
        device: &DeviceId,
        domain: DomainId,
        page_table_addr: u64,
    ) {
        // Set up root entry if needed
        if !self.root_table[device.bus as usize].is_present() {
            let ctx_addr = Self::context_table_addr_for_bus(device.bus);
            self.root_table[device.bus as usize] = RootEntry::new(ctx_addr);
        }

        // Set up context entry
        let ctx = ContextEntry::translated(page_table_addr, domain, self.address_width);
        self.set_context_entry(device.bus, device.devfn, ctx);
    }

    /// Map an IOVA page to a host physical address within a domain.
    ///
    /// Uses a flat page table indexed by page number (iova >> 12).
    /// The Vec is grown as needed so sparse mappings only cost one entry
    /// per mapped page.
    pub fn map_iova(&mut self, domain: DomainId, iova: u64, hpa: u64, flags: PageTableFlags) {
        let page_num = (iova >> 12) as usize;
        let table = self.page_tables.entry(domain).or_default();
        if page_num >= table.len() {
            table.resize(page_num + 1, PageTableEntry::empty());
        }
        table[page_num] = PageTableEntry::page(hpa, flags);

        // Invalidate any stale IOTLB entry for this page + domain
        self.iotlb.invalidate_domain(domain);
    }

    /// Unmap an IOVA page within a domain.
    pub fn unmap_iova(&mut self, domain: DomainId, iova: u64) {
        let page_num = (iova >> 12) as usize;
        if let Some(table) = self.page_tables.get_mut(&domain) {
            if page_num < table.len() {
                table[page_num] = PageTableEntry::empty();
            }
        }
        self.iotlb.invalidate_domain(domain);
    }

    /// Walk the flat page table for a domain, returning the translated HPA
    /// and the entry's flags, or `None` if the page is not present.
    fn walk_page_table(&self, domain: DomainId, iova: u64) -> Option<(u64, PageTableFlags)> {
        let page_num = (iova >> 12) as usize;
        let table = self.page_tables.get(&domain)?;
        let entry = table.get(page_num)?;
        if !entry.is_present() {
            return None;
        }
        let offset = iova & 0xFFF;
        Some((entry.phys_addr() | offset, entry.flags()))
    }

    /// Compute a synthetic page-aligned context table base address for a given
    /// PCI bus number.  Each bus gets a unique 4 KiB-aligned address in a
    /// reserved region starting at 0xFEE0_0000 (above the local APIC).
    fn context_table_addr_for_bus(bus: u8) -> u64 {
        // Base chosen above the local APIC MMIO range
        0xFEE0_0000u64 + (bus as u64 + 1) * 0x1000
    }

    /// Translate DMA address
    pub fn translate(
        &mut self,
        device: &DeviceId,
        iova: u64,
        is_write: bool,
    ) -> Result<u64, FaultRecord> {
        if !self.is_translation_enabled() {
            return Ok(iova); // Pass-through when disabled
        }

        // Check root entry
        let root = &self.root_table[device.bus as usize];
        if !root.is_present() {
            self.stats.record_fault();
            return Err(FaultRecord::new(
                *device,
                iova,
                FaultReason::RootNotPresent,
                is_write,
            ));
        }

        // Check context entry — extract values before mutable borrows below
        let (translation_type, domain) = match self.context_entry(device.bus, device.devfn) {
            Some(ctx) if ctx.is_present() => (ctx.translation_type(), ctx.domain_id()),
            _ => {
                self.stats.record_fault();
                return Err(FaultRecord::new(
                    *device,
                    iova,
                    FaultReason::ContextNotPresent,
                    is_write,
                ));
            }
        };

        // Check translation type
        match translation_type {
            TranslationType::Identity => {
                self.stats.record_translation(false);
                Ok(iova) // Pass-through
            }
            TranslationType::Translated => {
                // Check IOTLB first
                if let Some(entry) = self.iotlb.lookup(device, domain, iova) {
                    // Check permissions
                    if is_write && !entry.flags.is_writable() {
                        self.stats.record_fault();
                        return Err(FaultRecord::new(
                            *device,
                            iova,
                            FaultReason::WriteBlocked,
                            is_write,
                        ));
                    }
                    let hpa = entry.translate(iova);
                    self.stats.record_translation(true);
                    return Ok(hpa);
                }

                // Walk the page table
                self.stats.record_page_walk();

                match self.walk_page_table(domain, iova) {
                    Some((hpa, flags)) => {
                        // Check write permission
                        if is_write && !flags.is_writable() {
                            self.stats.record_fault();
                            return Err(FaultRecord::new(
                                *device,
                                iova,
                                FaultReason::WriteBlocked,
                                is_write,
                            ));
                        }

                        // Cache in IOTLB
                        let page_iova = iova & !0xFFF;
                        let page_hpa = hpa & !0xFFF;
                        self.iotlb.insert(IotlbEntry::new(
                            *device,
                            domain,
                            page_iova,
                            page_hpa,
                            PAGE_SIZE_4K,
                            flags,
                        ));

                        self.stats.record_translation(false);
                        Ok(hpa)
                    }
                    None => {
                        self.stats.record_fault();
                        Err(FaultRecord::new(
                            *device,
                            iova,
                            FaultReason::PageNotPresent,
                            is_write,
                        ))
                    }
                }
            }
            TranslationType::Reserved => {
                self.stats.record_fault();
                Err(FaultRecord::new(
                    *device,
                    iova,
                    FaultReason::InvalidContextEntry,
                    is_write,
                ))
            }
        }
    }

    /// Record fault
    pub fn record_fault(&mut self, fault: FaultRecord) {
        self.fault_records.push(fault);
    }

    /// Get pending faults
    pub fn pending_faults(&self) -> &[FaultRecord] {
        &self.fault_records
    }

    /// Clear faults
    pub fn clear_faults(&mut self) {
        self.fault_records.clear();
    }

    /// Invalidate IOTLB
    pub fn invalidate_iotlb(&mut self) {
        self.iotlb.invalidate_all();
        self.stats.record_iotlb_invalidation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_entry() {
        let entry = RootEntry::new(0x1000_0000);
        assert!(entry.is_present());
        assert_eq!(entry.context_table_addr(), 0x1000_0000);
    }

    #[test]
    fn test_root_entry_empty() {
        let entry = RootEntry::empty();
        assert!(!entry.is_present());
    }

    #[test]
    fn test_context_entry_identity() {
        let ctx = ContextEntry::identity(DomainId::new(1), AddressWidth::Bits48);
        assert!(ctx.is_present());
        assert_eq!(ctx.translation_type(), TranslationType::Identity);
        assert_eq!(ctx.domain_id().0, 1);
    }

    #[test]
    fn test_context_entry_translated() {
        let ctx = ContextEntry::translated(0x2000_0000, DomainId::new(5), AddressWidth::Bits48);
        assert!(ctx.is_present());
        assert_eq!(ctx.translation_type(), TranslationType::Translated);
        assert_eq!(ctx.page_table_addr(), 0x2000_0000);
    }

    #[test]
    fn test_iotlb_entry() {
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);
        let entry = IotlbEntry::new(
            device,
            domain,
            0x1000,
            0x2000,
            PAGE_SIZE_4K,
            PageTableFlags::read_write(),
        );

        assert!(entry.matches(&device, domain, 0x1000));
        assert!(!entry.matches(&device, domain, 0x2000));
        assert_eq!(entry.translate(0x1500), 0x2500);
    }

    #[test]
    fn test_iotlb() {
        let mut iotlb = Iotlb::new(10);
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        let entry = IotlbEntry::new(
            device,
            domain,
            0x1000,
            0x5000,
            PAGE_SIZE_4K,
            PageTableFlags::read_write(),
        );

        iotlb.insert(entry);
        assert_eq!(iotlb.len(), 1);

        let result = iotlb.lookup(&device, domain, 0x1000);
        assert!(result.is_some());
    }

    #[test]
    fn test_iotlb_invalidate() {
        let mut iotlb = Iotlb::new(10);
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        iotlb.insert(IotlbEntry::new(
            device,
            domain,
            0x1000,
            0x5000,
            PAGE_SIZE_4K,
            PageTableFlags::read_write(),
        ));

        iotlb.invalidate_device(&device);
        assert!(iotlb.is_empty());
    }

    #[test]
    fn test_vtd_unit_creation() {
        let unit = VtdUnit::new(0xFED90000, 0);
        assert_eq!(unit.base_address(), 0xFED90000);
        assert_eq!(unit.segment(), 0);
    }

    #[test]
    fn test_vtd_unit_registers() {
        let unit = VtdUnit::default();
        assert_eq!(unit.read_register(registers::VER), 0x10);
        assert!(unit.read_register(registers::CAP) != 0);
        assert!(unit.read_register(registers::ECAP) != 0);
    }

    #[test]
    fn test_vtd_enable_disable() {
        let mut unit = VtdUnit::default();
        assert!(!unit.is_translation_enabled());

        unit.enable_translation();
        assert!(unit.is_translation_enabled());

        unit.disable_translation();
        assert!(!unit.is_translation_enabled());
    }

    #[test]
    fn test_vtd_configure_passthrough() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        unit.configure_passthrough(&device, domain);

        let root = unit.root_entry(1);
        assert!(root.is_present());
        // Root entry should have a non-zero, page-aligned context table address
        let addr = root.context_table_addr();
        assert_ne!(addr, 0, "context table address must not be zero");
        assert_eq!(
            addr & 0xFFF,
            0,
            "context table address must be page-aligned"
        );
        let ctx = unit.context_entry(1, device.devfn).unwrap();
        assert!(ctx.is_present());
        assert_eq!(ctx.translation_type(), TranslationType::Identity);
    }

    #[test]
    fn test_vtd_context_table_addrs_unique_per_bus() {
        let mut unit = VtdUnit::default();
        let dev_bus0 = DeviceId::new(0, 0, 1, 0);
        let dev_bus5 = DeviceId::new(0, 5, 1, 0);
        let domain = DomainId::new(1);

        unit.configure_passthrough(&dev_bus0, domain);
        unit.configure_passthrough(&dev_bus5, domain);

        let addr0 = unit.root_entry(0).context_table_addr();
        let addr5 = unit.root_entry(5).context_table_addr();
        assert_ne!(
            addr0, addr5,
            "different buses must have different ctx addrs"
        );
        assert_ne!(addr0, 0);
        assert_ne!(addr5, 0);
    }

    #[test]
    fn test_vtd_translate_passthrough() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        unit.configure_passthrough(&device, domain);
        unit.enable_translation();

        let result = unit.translate(&device, 0x1000, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0x1000); // Identity mapping
    }

    #[test]
    fn test_vtd_translate_disabled() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 1, 2, 0);

        // Translation disabled = pass-through
        let result = unit.translate(&device, 0xDEAD_BEEF, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn test_vtd_translate_no_root() {
        let mut unit = VtdUnit::default();
        unit.enable_translation();

        let device = DeviceId::new(0, 5, 0, 0);
        let result = unit.translate(&device, 0x1000, false);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().reason, FaultReason::RootNotPresent);
    }

    #[test]
    fn test_vtd_fault_recording() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 1, 0, 0);

        let fault = FaultRecord::new(device, 0x1000, FaultReason::PageNotPresent, true);
        unit.record_fault(fault);

        assert_eq!(unit.pending_faults().len(), 1);
        unit.clear_faults();
        assert!(unit.pending_faults().is_empty());
    }

    #[test]
    fn test_vtd_statistics() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        unit.configure_passthrough(&device, domain);
        unit.enable_translation();
        let _ = unit.translate(&device, 0x1000, false);

        let stats = unit.stats().snapshot();
        assert!(stats.translations > 0);
    }

    #[test]
    fn test_vtd_translate_mapped_page() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 2, 3, 0);
        let domain = DomainId::new(7);

        // Set up translated context
        unit.configure_translation(&device, domain, 0x100_0000);
        // Map IOVA 0x5000 → HPA 0xA000
        unit.map_iova(domain, 0x5000, 0xA000, PageTableFlags::read_write());
        unit.enable_translation();

        // Should translate with page-offset preserved
        let result = unit.translate(&device, 0x5100, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0xA100);
    }

    #[test]
    fn test_vtd_translate_unmapped_fault() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 2, 3, 0);
        let domain = DomainId::new(7);

        unit.configure_translation(&device, domain, 0x100_0000);
        // Map one page but translate a different one
        unit.map_iova(domain, 0x5000, 0xA000, PageTableFlags::read_write());
        unit.enable_translation();

        let result = unit.translate(&device, 0x9000, false);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().reason, FaultReason::PageNotPresent);
    }

    #[test]
    fn test_vtd_translate_write_blocked() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 2, 3, 0);
        let domain = DomainId::new(7);

        unit.configure_translation(&device, domain, 0x100_0000);
        // Map with read-only permissions
        unit.map_iova(domain, 0x5000, 0xA000, PageTableFlags::read_only());
        unit.enable_translation();

        // Read should succeed
        let result = unit.translate(&device, 0x5000, false);
        assert!(result.is_ok());

        // Write should be blocked
        let result = unit.translate(&device, 0x5000, true);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().reason, FaultReason::WriteBlocked);
    }

    #[test]
    fn test_vtd_iotlb_caching() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 2, 3, 0);
        let domain = DomainId::new(7);

        unit.configure_translation(&device, domain, 0x100_0000);
        unit.map_iova(domain, 0x5000, 0xA000, PageTableFlags::read_write());
        unit.enable_translation();

        // First translation populates IOTLB
        let result = unit.translate(&device, 0x5000, false);
        assert!(result.is_ok());

        // Second translation should hit IOTLB cache
        let result = unit.translate(&device, 0x5080, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0xA080);

        let stats = unit.stats().snapshot();
        assert!(stats.iotlb_hits > 0, "second lookup should be an IOTLB hit");
    }

    #[test]
    fn test_vtd_unmap_iova() {
        let mut unit = VtdUnit::default();
        let device = DeviceId::new(0, 2, 3, 0);
        let domain = DomainId::new(7);

        unit.configure_translation(&device, domain, 0x100_0000);
        unit.map_iova(domain, 0x5000, 0xA000, PageTableFlags::read_write());
        unit.enable_translation();

        // Should translate successfully
        assert!(unit.translate(&device, 0x5000, false).is_ok());

        // Unmap and retry
        unit.unmap_iova(domain, 0x5000);
        let result = unit.translate(&device, 0x5000, false);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().reason, FaultReason::PageNotPresent);
    }
}
