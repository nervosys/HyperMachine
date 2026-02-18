//! VM Introspection API
//!
//! This module provides tools for inspecting VM state including memory,
//! CPU registers, and various system structures.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

/// Memory region type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Regular RAM
    Ram,
    /// ROM (read-only)
    Rom,
    /// Memory-mapped I/O
    Mmio,
    /// Reserved region
    Reserved,
    /// Device memory
    Device,
    /// ACPI tables
    Acpi,
    /// ACPI NVS
    AcpiNvs,
    /// Unusable memory
    Unusable,
}

impl MemoryRegionType {
    /// Check if region is readable
    pub fn is_readable(&self) -> bool {
        !matches!(self, Self::Unusable)
    }

    /// Check if region is writable
    pub fn is_writable(&self) -> bool {
        matches!(self, Self::Ram | Self::Mmio | Self::Device)
    }

    /// Check if region is executable
    pub fn is_executable(&self) -> bool {
        matches!(self, Self::Ram | Self::Rom)
    }
}

/// Memory region descriptor
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Base address
    pub base: u64,
    /// Size in bytes
    pub size: u64,
    /// Region type
    pub region_type: MemoryRegionType,
    /// Region name
    pub name: String,
    /// Attributes
    pub attributes: MemoryAttributes,
}

impl MemoryRegion {
    /// Create new memory region
    pub fn new(base: u64, size: u64, region_type: MemoryRegionType, name: &str) -> Self {
        Self {
            base,
            size,
            region_type,
            name: name.to_string(),
            attributes: MemoryAttributes::default(),
        }
    }

    /// Check if address is in this region
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.size
    }

    /// Get end address
    pub fn end(&self) -> u64 {
        self.base + self.size
    }
}

/// Memory attributes
#[derive(Debug, Clone, Default)]
pub struct MemoryAttributes {
    /// Cacheable
    pub cacheable: bool,
    /// Write-through
    pub write_through: bool,
    /// Write-back
    pub write_back: bool,
    /// Uncacheable
    pub uncacheable: bool,
    /// Write-combining
    pub write_combining: bool,
    /// Write-protected
    pub write_protected: bool,
}

/// CPU execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    /// Real mode (16-bit)
    Real,
    /// Protected mode (32-bit)
    Protected,
    /// Long mode (64-bit)
    Long,
    /// Virtual 8086 mode
    Virtual8086,
    /// System management mode
    Smm,
}

impl CpuMode {
    /// Get address size for this mode
    pub fn address_size(&self) -> usize {
        match self {
            Self::Real | Self::Virtual8086 => 16,
            Self::Protected | Self::Smm => 32,
            Self::Long => 64,
        }
    }

    /// Get default operand size for this mode
    pub fn operand_size(&self) -> usize {
        match self {
            Self::Real | Self::Virtual8086 => 16,
            Self::Protected | Self::Smm => 32,
            Self::Long => 64,
        }
    }
}

/// CPU state snapshot
#[derive(Debug, Clone, Default)]
pub struct CpuState {
    /// General purpose registers
    pub gprs: [u64; 16],
    /// Instruction pointer
    pub rip: u64,
    /// Flags register
    pub rflags: u64,
    /// Control registers
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    /// Extended feature enable register
    pub efer: u64,
    /// Segment selectors
    pub cs: SegmentState,
    pub ds: SegmentState,
    pub es: SegmentState,
    pub fs: SegmentState,
    pub gs: SegmentState,
    pub ss: SegmentState,
    /// Descriptor tables
    pub gdtr: TableState,
    pub idtr: TableState,
    pub ldtr: SegmentState,
    pub tr: SegmentState,
    /// Debug registers
    pub dr0: u64,
    pub dr1: u64,
    pub dr2: u64,
    pub dr3: u64,
    pub dr6: u64,
    pub dr7: u64,
}

impl CpuState {
    /// Get current CPU mode
    pub fn mode(&self) -> CpuMode {
        // Check for long mode
        if self.efer & 0x400 != 0 && self.cr0 & 0x80000000 != 0 {
            if self.cs.attributes & 0x2000 != 0 {
                return CpuMode::Long;
            }
        }

        // Check for protected mode
        if self.cr0 & 0x1 != 0 {
            if self.rflags & 0x20000 != 0 {
                return CpuMode::Virtual8086;
            }
            return CpuMode::Protected;
        }

        CpuMode::Real
    }

    /// Check if paging is enabled
    pub fn paging_enabled(&self) -> bool {
        self.cr0 & 0x80000000 != 0
    }

    /// Check if PAE is enabled
    pub fn pae_enabled(&self) -> bool {
        self.cr4 & 0x20 != 0
    }

    /// Get page table base
    pub fn page_table_base(&self) -> u64 {
        self.cr3 & 0xFFFFFFFFFF000
    }

    /// Check if interrupts are enabled
    pub fn interrupts_enabled(&self) -> bool {
        self.rflags & 0x200 != 0
    }

    /// Get current privilege level
    pub fn cpl(&self) -> u8 {
        (self.cs.selector & 0x3) as u8
    }
}

/// Segment state
#[derive(Debug, Clone, Default)]
pub struct SegmentState {
    /// Selector
    pub selector: u16,
    /// Base address
    pub base: u64,
    /// Limit
    pub limit: u32,
    /// Attributes
    pub attributes: u16,
}

impl SegmentState {
    /// Check if segment is present
    pub fn is_present(&self) -> bool {
        self.attributes & 0x80 != 0
    }

    /// Check if segment is code segment
    pub fn is_code(&self) -> bool {
        self.attributes & 0x8 != 0
    }

    /// Get descriptor privilege level
    pub fn dpl(&self) -> u8 {
        ((self.attributes >> 5) & 0x3) as u8
    }
}

/// Table state (GDT/IDT)
#[derive(Debug, Clone, Default)]
pub struct TableState {
    /// Base address
    pub base: u64,
    /// Limit
    pub limit: u16,
}

/// Page table entry
#[derive(Debug, Clone, Copy)]
pub struct PageTableEntry {
    /// Raw entry value
    pub value: u64,
    /// Entry level (4=PML4, 3=PDPT, 2=PD, 1=PT)
    pub level: u8,
}

impl PageTableEntry {
    /// Check if entry is present
    pub fn is_present(&self) -> bool {
        self.value & 0x1 != 0
    }

    /// Check if entry is writable
    pub fn is_writable(&self) -> bool {
        self.value & 0x2 != 0
    }

    /// Check if entry is user-accessible
    pub fn is_user(&self) -> bool {
        self.value & 0x4 != 0
    }

    /// Check if entry is a large page
    pub fn is_large_page(&self) -> bool {
        self.value & 0x80 != 0 && self.level > 1
    }

    /// Check if execute disabled
    pub fn is_nx(&self) -> bool {
        self.value & (1 << 63) != 0
    }

    /// Get physical address from entry
    pub fn physical_address(&self) -> u64 {
        if self.is_large_page() {
            match self.level {
                3 => self.value & 0xFFFFFC0000000, // 1GB page
                2 => self.value & 0xFFFFFFFE00000, // 2MB page
                _ => self.value & 0xFFFFFFFFFF000,
            }
        } else {
            self.value & 0xFFFFFFFFFF000
        }
    }

    /// Get page size for this entry
    pub fn page_size(&self) -> u64 {
        if self.is_large_page() {
            match self.level {
                3 => 1024 * 1024 * 1024, // 1GB
                2 => 2 * 1024 * 1024,    // 2MB
                _ => 4096,
            }
        } else {
            4096
        }
    }
}

/// Page walk result
#[derive(Debug, Clone)]
pub struct PageWalkResult {
    /// Virtual address
    pub virtual_address: u64,
    /// Physical address
    pub physical_address: Option<u64>,
    /// Page size
    pub page_size: u64,
    /// Is writable
    pub writable: bool,
    /// Is user accessible
    pub user: bool,
    /// Is executable
    pub executable: bool,
    /// Page table entries traversed
    pub entries: Vec<PageTableEntry>,
    /// Error if translation failed
    pub error: Option<PageWalkError>,
}

/// Page walk error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageWalkError {
    /// Entry not present
    NotPresent(u8),
    /// Reserved bits set
    ReservedBits,
    /// Access violation
    AccessViolation,
    /// Memory read error
    MemoryError,
}

/// Interrupt descriptor
#[derive(Debug, Clone)]
pub struct InterruptDescriptor {
    /// Vector number
    pub vector: u8,
    /// Handler address
    pub handler: u64,
    /// Segment selector
    pub selector: u16,
    /// Type (interrupt gate, trap gate, etc.)
    pub gate_type: GateType,
    /// Descriptor privilege level
    pub dpl: u8,
    /// Present flag
    pub present: bool,
    /// IST index (for 64-bit)
    pub ist: u8,
}

/// Gate type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateType {
    /// Interrupt gate (clears IF)
    Interrupt,
    /// Trap gate (preserves IF)
    Trap,
    /// Task gate
    Task,
    /// Unknown type
    Unknown(u8),
}

impl GateType {
    /// Parse from type field
    pub fn from_type(type_field: u8) -> Self {
        match type_field {
            0xE => Self::Interrupt,
            0xF => Self::Trap,
            0x5 => Self::Task,
            t => Self::Unknown(t),
        }
    }
}

/// Introspection event type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntrospectionEvent {
    /// Breakpoint hit
    Breakpoint(u64),
    /// Watchpoint triggered
    Watchpoint { address: u64, is_write: bool },
    /// Page fault
    PageFault { address: u64, error_code: u32 },
    /// System call
    Syscall { number: u64, args: [u64; 6] },
    /// Exception
    Exception { vector: u8, error_code: Option<u32> },
    /// Control register write
    CrWrite {
        cr: u8,
        old_value: u64,
        new_value: u64,
    },
    /// MSR write
    MsrWrite {
        msr: u32,
        old_value: u64,
        new_value: u64,
    },
    /// I/O port access
    IoPort { port: u16, is_write: bool, size: u8 },
}

/// Memory inspector
pub struct MemoryInspector {
    /// Memory regions
    regions: RwLock<Vec<MemoryRegion>>,
    /// Read callback
    read_fn: Option<Box<dyn Fn(u64, usize) -> Option<Vec<u8>> + Send + Sync>>,
}

impl MemoryInspector {
    /// Create new memory inspector
    pub fn new() -> Self {
        Self {
            regions: RwLock::new(Vec::new()),
            read_fn: None,
        }
    }

    /// Set read callback
    pub fn set_read_fn<F>(&mut self, f: F)
    where
        F: Fn(u64, usize) -> Option<Vec<u8>> + Send + Sync + 'static,
    {
        self.read_fn = Some(Box::new(f));
    }

    /// Add memory region
    pub fn add_region(&self, region: MemoryRegion) {
        self.regions.write().unwrap_or_else(|e| e.into_inner()).push(region);
    }

    /// Find region containing address
    pub fn find_region(&self, addr: u64) -> Option<MemoryRegion> {
        self.regions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|r| r.contains(addr))
            .cloned()
    }

    /// Get all regions
    pub fn regions(&self) -> Vec<MemoryRegion> {
        self.regions.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Read memory
    pub fn read(&self, addr: u64, size: usize) -> Option<Vec<u8>> {
        if let Some(ref read_fn) = self.read_fn {
            read_fn(addr, size)
        } else {
            None
        }
    }

    /// Read u8
    pub fn read_u8(&self, addr: u64) -> Option<u8> {
        self.read(addr, 1).map(|v| v[0])
    }

    /// Read u16
    pub fn read_u16(&self, addr: u64) -> Option<u16> {
        self.read(addr, 2).map(|v| u16::from_le_bytes([v[0], v[1]]))
    }

    /// Read u32
    pub fn read_u32(&self, addr: u64) -> Option<u32> {
        self.read(addr, 4)
            .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }

    /// Read u64
    pub fn read_u64(&self, addr: u64) -> Option<u64> {
        self.read(addr, 8)
            .map(|v| u64::from_le_bytes([v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]]))
    }

    /// Dump memory region as hex
    pub fn dump_hex(&self, addr: u64, size: usize) -> Option<String> {
        let data = self.read(addr, size)?;
        let mut result = String::new();

        for (i, chunk) in data.chunks(16).enumerate() {
            let offset = addr + (i * 16) as u64;
            result.push_str(&format!("{:016x}  ", offset));

            // Hex bytes
            for (j, byte) in chunk.iter().enumerate() {
                if j == 8 {
                    result.push(' ');
                }
                result.push_str(&format!("{:02x} ", byte));
            }

            // Padding for incomplete lines
            for j in chunk.len()..16 {
                if j == 8 {
                    result.push(' ');
                }
                result.push_str("   ");
            }

            result.push(' ');

            // ASCII
            for byte in chunk {
                if *byte >= 0x20 && *byte < 0x7f {
                    result.push(*byte as char);
                } else {
                    result.push('.');
                }
            }

            result.push('\n');
        }

        Some(result)
    }

    /// Search for pattern in memory
    pub fn search(&self, start: u64, size: usize, pattern: &[u8]) -> Vec<u64> {
        let mut results = Vec::new();

        if pattern.is_empty() {
            return results;
        }

        if let Some(data) = self.read(start, size) {
            for i in 0..=data.len().saturating_sub(pattern.len()) {
                if &data[i..i + pattern.len()] == pattern {
                    results.push(start + i as u64);
                }
            }
        }

        results
    }
}

impl Default for MemoryInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MemoryInspector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryInspector")
            .field("regions", &self.regions)
            .finish()
    }
}

/// CPU inspector
#[derive(Debug, Default)]
pub struct CpuInspector {
    /// CPU states (per vCPU)
    states: RwLock<HashMap<u32, CpuState>>,
    /// Event log
    events: RwLock<Vec<(u64, IntrospectionEvent)>>,
    /// Event counter
    event_counter: AtomicU64,
    /// Event logging enabled
    logging_enabled: AtomicBool,
    /// Maximum events to keep
    max_events: usize,
}

impl CpuInspector {
    /// Create new CPU inspector
    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
            events: RwLock::new(Vec::new()),
            event_counter: AtomicU64::new(0),
            logging_enabled: AtomicBool::new(true),
            max_events: 10000,
        }
    }

    /// Update CPU state
    pub fn update_state(&self, vcpu_id: u32, state: CpuState) {
        self.states.write().unwrap_or_else(|e| e.into_inner()).insert(vcpu_id, state);
    }

    /// Get CPU state
    pub fn get_state(&self, vcpu_id: u32) -> Option<CpuState> {
        self.states.read().unwrap_or_else(|e| e.into_inner()).get(&vcpu_id).cloned()
    }

    /// Get all CPU states
    pub fn all_states(&self) -> HashMap<u32, CpuState> {
        self.states.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Log event
    pub fn log_event(&self, event: IntrospectionEvent) {
        if !self.logging_enabled.load(Ordering::Acquire) {
            return;
        }

        let id = self.event_counter.fetch_add(1, Ordering::SeqCst);
        let mut events = self.events.write().unwrap_or_else(|e| e.into_inner());

        // Trim if over limit
        if events.len() >= self.max_events {
            events.drain(0..self.max_events / 2);
        }

        events.push((id, event));
    }

    /// Get events
    pub fn get_events(&self, start: u64, count: usize) -> Vec<(u64, IntrospectionEvent)> {
        let events = self.events.read().unwrap_or_else(|e| e.into_inner());
        events
            .iter()
            .filter(|(id, _)| *id >= start)
            .take(count)
            .cloned()
            .collect()
    }

    /// Clear events
    pub fn clear_events(&self) {
        self.events.write().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Enable/disable logging
    pub fn set_logging(&self, enabled: bool) {
        self.logging_enabled.store(enabled, Ordering::Release);
    }

    /// Check if logging is enabled
    pub fn is_logging(&self) -> bool {
        self.logging_enabled.load(Ordering::Acquire)
    }

    /// Get event count
    pub fn event_count(&self) -> u64 {
        self.event_counter.load(Ordering::Acquire)
    }
}

/// Page table walker
pub struct PageTableWalker {
    /// Memory reader
    read_fn: Option<Box<dyn Fn(u64) -> Option<u64> + Send + Sync>>,
}

impl PageTableWalker {
    /// Create new page table walker
    pub fn new() -> Self {
        Self { read_fn: None }
    }

    /// Set memory read function
    pub fn set_read_fn<F>(&mut self, f: F)
    where
        F: Fn(u64) -> Option<u64> + Send + Sync + 'static,
    {
        self.read_fn = Some(Box::new(f));
    }

    /// Walk page tables for address
    pub fn walk(&self, cr3: u64, virtual_addr: u64, is_long_mode: bool) -> PageWalkResult {
        if is_long_mode {
            self.walk_long_mode(cr3, virtual_addr)
        } else {
            self.walk_legacy(cr3, virtual_addr)
        }
    }

    /// Walk 4-level page tables (long mode)
    fn walk_long_mode(&self, cr3: u64, virtual_addr: u64) -> PageWalkResult {
        let read_fn = match &self.read_fn {
            Some(f) => f,
            None => {
                return PageWalkResult {
                    virtual_address: virtual_addr,
                    physical_address: None,
                    page_size: 4096,
                    writable: false,
                    user: false,
                    executable: false,
                    entries: Vec::new(),
                    error: Some(PageWalkError::MemoryError),
                }
            }
        };

        let mut result = PageWalkResult {
            virtual_address: virtual_addr,
            physical_address: None,
            page_size: 4096,
            writable: true,
            user: true,
            executable: true,
            entries: Vec::new(),
            error: None,
        };

        // Extract indices from virtual address
        let pml4_idx = ((virtual_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virtual_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virtual_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virtual_addr >> 12) & 0x1FF) as usize;

        // PML4
        let pml4_base = cr3 & 0xFFFFFFFFFF000;
        let pml4_entry_addr = pml4_base + (pml4_idx * 8) as u64;
        let pml4_entry = match read_fn(pml4_entry_addr) {
            Some(v) => PageTableEntry { value: v, level: 4 },
            None => {
                result.error = Some(PageWalkError::MemoryError);
                return result;
            }
        };
        result.entries.push(pml4_entry);

        if !pml4_entry.is_present() {
            result.error = Some(PageWalkError::NotPresent(4));
            return result;
        }

        result.writable &= pml4_entry.is_writable();
        result.user &= pml4_entry.is_user();
        result.executable &= !pml4_entry.is_nx();

        // PDPT
        let pdpt_base = pml4_entry.physical_address();
        let pdpt_entry_addr = pdpt_base + (pdpt_idx * 8) as u64;
        let pdpt_entry = match read_fn(pdpt_entry_addr) {
            Some(v) => PageTableEntry { value: v, level: 3 },
            None => {
                result.error = Some(PageWalkError::MemoryError);
                return result;
            }
        };
        result.entries.push(pdpt_entry);

        if !pdpt_entry.is_present() {
            result.error = Some(PageWalkError::NotPresent(3));
            return result;
        }

        result.writable &= pdpt_entry.is_writable();
        result.user &= pdpt_entry.is_user();
        result.executable &= !pdpt_entry.is_nx();

        // Check for 1GB page
        if pdpt_entry.is_large_page() {
            result.page_size = 1024 * 1024 * 1024;
            let offset = virtual_addr & 0x3FFFFFFF;
            result.physical_address = Some(pdpt_entry.physical_address() + offset);
            return result;
        }

        // PD
        let pd_base = pdpt_entry.physical_address();
        let pd_entry_addr = pd_base + (pd_idx * 8) as u64;
        let pd_entry = match read_fn(pd_entry_addr) {
            Some(v) => PageTableEntry { value: v, level: 2 },
            None => {
                result.error = Some(PageWalkError::MemoryError);
                return result;
            }
        };
        result.entries.push(pd_entry);

        if !pd_entry.is_present() {
            result.error = Some(PageWalkError::NotPresent(2));
            return result;
        }

        result.writable &= pd_entry.is_writable();
        result.user &= pd_entry.is_user();
        result.executable &= !pd_entry.is_nx();

        // Check for 2MB page
        if pd_entry.is_large_page() {
            result.page_size = 2 * 1024 * 1024;
            let offset = virtual_addr & 0x1FFFFF;
            result.physical_address = Some(pd_entry.physical_address() + offset);
            return result;
        }

        // PT
        let pt_base = pd_entry.physical_address();
        let pt_entry_addr = pt_base + (pt_idx * 8) as u64;
        let pt_entry = match read_fn(pt_entry_addr) {
            Some(v) => PageTableEntry { value: v, level: 1 },
            None => {
                result.error = Some(PageWalkError::MemoryError);
                return result;
            }
        };
        result.entries.push(pt_entry);

        if !pt_entry.is_present() {
            result.error = Some(PageWalkError::NotPresent(1));
            return result;
        }

        result.writable &= pt_entry.is_writable();
        result.user &= pt_entry.is_user();
        result.executable &= !pt_entry.is_nx();

        let offset = virtual_addr & 0xFFF;
        result.physical_address = Some(pt_entry.physical_address() + offset);

        result
    }

    /// Walk 2-level page tables (legacy 32-bit)
    fn walk_legacy(&self, cr3: u64, virtual_addr: u64) -> PageWalkResult {
        let read_fn = match &self.read_fn {
            Some(f) => f,
            None => {
                return PageWalkResult {
                    virtual_address: virtual_addr,
                    physical_address: None,
                    page_size: 4096,
                    writable: false,
                    user: false,
                    executable: true,
                    entries: Vec::new(),
                    error: Some(PageWalkError::MemoryError),
                }
            }
        };

        let mut result = PageWalkResult {
            virtual_address: virtual_addr,
            physical_address: None,
            page_size: 4096,
            writable: true,
            user: true,
            executable: true,
            entries: Vec::new(),
            error: None,
        };

        // Extract indices (32-bit paging)
        let pd_idx = ((virtual_addr >> 22) & 0x3FF) as usize;
        let pt_idx = ((virtual_addr >> 12) & 0x3FF) as usize;

        // PD
        let pd_base = cr3 & 0xFFFFF000;
        let pd_entry_addr = pd_base + (pd_idx * 4) as u64;
        let pd_entry_val = match read_fn(pd_entry_addr) {
            Some(v) => v as u32,
            None => {
                result.error = Some(PageWalkError::MemoryError);
                return result;
            }
        };
        let pd_entry = PageTableEntry {
            value: pd_entry_val as u64,
            level: 2,
        };
        result.entries.push(pd_entry);

        if !pd_entry.is_present() {
            result.error = Some(PageWalkError::NotPresent(2));
            return result;
        }

        result.writable &= pd_entry.is_writable();
        result.user &= pd_entry.is_user();

        // Check for 4MB page (PSE)
        if pd_entry.value & 0x80 != 0 {
            result.page_size = 4 * 1024 * 1024;
            let phys_base = (pd_entry.value & 0xFFC00000) | ((pd_entry.value & 0x1FE000) << 19);
            let offset = virtual_addr & 0x3FFFFF;
            result.physical_address = Some(phys_base + offset);
            return result;
        }

        // PT
        let pt_base = (pd_entry.value & 0xFFFFF000) as u64;
        let pt_entry_addr = pt_base + (pt_idx * 4) as u64;
        let pt_entry_val = match read_fn(pt_entry_addr) {
            Some(v) => v as u32,
            None => {
                result.error = Some(PageWalkError::MemoryError);
                return result;
            }
        };
        let pt_entry = PageTableEntry {
            value: pt_entry_val as u64,
            level: 1,
        };
        result.entries.push(pt_entry);

        if !pt_entry.is_present() {
            result.error = Some(PageWalkError::NotPresent(1));
            return result;
        }

        result.writable &= pt_entry.is_writable();
        result.user &= pt_entry.is_user();

        let offset = virtual_addr & 0xFFF;
        result.physical_address = Some((pt_entry.value & 0xFFFFF000) + offset);

        result
    }
}

impl Default for PageTableWalker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PageTableWalker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageTableWalker").finish()
    }
}

/// IDT inspector
pub struct IdtInspector {
    /// Memory reader
    read_fn: Option<Box<dyn Fn(u64, usize) -> Option<Vec<u8>> + Send + Sync>>,
}

impl IdtInspector {
    /// Create new IDT inspector
    pub fn new() -> Self {
        Self { read_fn: None }
    }

    /// Set memory read function
    pub fn set_read_fn<F>(&mut self, f: F)
    where
        F: Fn(u64, usize) -> Option<Vec<u8>> + Send + Sync + 'static,
    {
        self.read_fn = Some(Box::new(f));
    }

    /// Read interrupt descriptor
    pub fn read_descriptor(
        &self,
        idtr: &TableState,
        vector: u8,
        is_long_mode: bool,
    ) -> Option<InterruptDescriptor> {
        let read_fn = self.read_fn.as_ref()?;

        let entry_size = if is_long_mode { 16 } else { 8 };
        let offset = vector as u64 * entry_size as u64;

        if offset + entry_size as u64 > idtr.limit as u64 + 1 {
            return None;
        }

        let entry_addr = idtr.base + offset;
        let data = read_fn(entry_addr, entry_size)?;

        if is_long_mode {
            // 64-bit IDT entry
            let offset_low = u16::from_le_bytes([data[0], data[1]]);
            let selector = u16::from_le_bytes([data[2], data[3]]);
            let ist = data[4] & 0x7;
            let type_attr = data[5];
            let offset_mid = u16::from_le_bytes([data[6], data[7]]);
            let offset_high = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

            let handler =
                (offset_high as u64) << 32 | (offset_mid as u64) << 16 | offset_low as u64;
            let gate_type = GateType::from_type(type_attr & 0xF);
            let dpl = (type_attr >> 5) & 0x3;
            let present = type_attr & 0x80 != 0;

            Some(InterruptDescriptor {
                vector,
                handler,
                selector,
                gate_type,
                dpl,
                present,
                ist,
            })
        } else {
            // 32-bit IDT entry
            let offset_low = u16::from_le_bytes([data[0], data[1]]);
            let selector = u16::from_le_bytes([data[2], data[3]]);
            let type_attr = data[5];
            let offset_high = u16::from_le_bytes([data[6], data[7]]);

            let handler = (offset_high as u64) << 16 | offset_low as u64;
            let gate_type = GateType::from_type(type_attr & 0xF);
            let dpl = (type_attr >> 5) & 0x3;
            let present = type_attr & 0x80 != 0;

            Some(InterruptDescriptor {
                vector,
                handler,
                selector,
                gate_type,
                dpl,
                present,
                ist: 0,
            })
        }
    }

    /// Dump entire IDT
    pub fn dump_idt(&self, idtr: &TableState, is_long_mode: bool) -> Vec<InterruptDescriptor> {
        let mut descriptors = Vec::new();
        let entry_size = if is_long_mode { 16 } else { 8 };
        let count = (idtr.limit as usize + 1) / entry_size;

        for i in 0..count.min(256) {
            if let Some(desc) = self.read_descriptor(idtr, i as u8, is_long_mode) {
                descriptors.push(desc);
            }
        }

        descriptors
    }
}

impl Default for IdtInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for IdtInspector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdtInspector").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_region_type_properties() {
        assert!(MemoryRegionType::Ram.is_readable());
        assert!(MemoryRegionType::Ram.is_writable());
        assert!(MemoryRegionType::Ram.is_executable());

        assert!(MemoryRegionType::Rom.is_readable());
        assert!(!MemoryRegionType::Rom.is_writable());
        assert!(MemoryRegionType::Rom.is_executable());

        assert!(!MemoryRegionType::Unusable.is_readable());
    }

    #[test]
    fn test_memory_region_contains() {
        let region = MemoryRegion::new(0x1000, 0x1000, MemoryRegionType::Ram, "test");
        assert!(region.contains(0x1000));
        assert!(region.contains(0x1FFF));
        assert!(!region.contains(0x2000));
        assert!(!region.contains(0x0FFF));
    }

    #[test]
    fn test_cpu_mode_address_size() {
        assert_eq!(CpuMode::Real.address_size(), 16);
        assert_eq!(CpuMode::Protected.address_size(), 32);
        assert_eq!(CpuMode::Long.address_size(), 64);
    }

    #[test]
    fn test_cpu_state_mode_detection() {
        let mut state = CpuState::default();

        // Real mode
        assert_eq!(state.mode(), CpuMode::Real);

        // Protected mode
        state.cr0 = 0x1;
        assert_eq!(state.mode(), CpuMode::Protected);

        // Long mode
        state.efer = 0x400;
        state.cr0 = 0x80000001;
        state.cs.attributes = 0x2000;
        assert_eq!(state.mode(), CpuMode::Long);
    }

    #[test]
    fn test_cpu_state_paging() {
        let mut state = CpuState::default();
        assert!(!state.paging_enabled());

        state.cr0 = 0x80000000;
        assert!(state.paging_enabled());
    }

    #[test]
    fn test_segment_state() {
        let mut seg = SegmentState::default();
        seg.attributes = 0x88; // Present + code

        assert!(seg.is_present());
        assert!(seg.is_code());
    }

    #[test]
    fn test_page_table_entry() {
        // Present, writable, user, 4KB page
        let entry = PageTableEntry {
            value: 0x1007,
            level: 1,
        };
        assert!(entry.is_present());
        assert!(entry.is_writable());
        assert!(entry.is_user());
        assert!(!entry.is_large_page());

        // 2MB large page
        let large = PageTableEntry {
            value: 0x87,
            level: 2,
        };
        assert!(large.is_large_page());
        assert_eq!(large.page_size(), 2 * 1024 * 1024);
    }

    #[test]
    fn test_gate_type_from_type() {
        assert_eq!(GateType::from_type(0xE), GateType::Interrupt);
        assert_eq!(GateType::from_type(0xF), GateType::Trap);
        assert_eq!(GateType::from_type(0x5), GateType::Task);
        assert!(matches!(GateType::from_type(0x0), GateType::Unknown(0)));
    }

    #[test]
    fn test_memory_inspector_regions() {
        let inspector = MemoryInspector::new();

        inspector.add_region(MemoryRegion::new(
            0x0,
            0x1000,
            MemoryRegionType::Ram,
            "low_mem",
        ));
        inspector.add_region(MemoryRegion::new(
            0x1000,
            0x1000,
            MemoryRegionType::Rom,
            "rom",
        ));

        let region = inspector.find_region(0x500);
        assert!(region.is_some());
        assert_eq!(region.unwrap().name, "low_mem");

        let region = inspector.find_region(0x1500);
        assert!(region.is_some());
        assert_eq!(region.unwrap().name, "rom");

        assert!(inspector.find_region(0x3000).is_none());
    }

    #[test]
    fn test_memory_inspector_with_read() {
        let mut inspector = MemoryInspector::new();

        inspector.set_read_fn(|addr, size| {
            let mut data = vec![0u8; size];
            for (i, byte) in data.iter_mut().enumerate() {
                *byte = ((addr + i as u64) & 0xFF) as u8;
            }
            Some(data)
        });

        assert_eq!(inspector.read_u8(0x10), Some(0x10));
        assert_eq!(inspector.read_u16(0x00), Some(0x0100));
        assert_eq!(inspector.read_u32(0x00), Some(0x03020100));
    }

    #[test]
    fn test_memory_inspector_search() {
        let mut inspector = MemoryInspector::new();

        inspector.set_read_fn(|_addr, size| {
            // Return "Hello World" pattern
            let data = b"Hello World Hello";
            Some(data[..size.min(data.len())].to_vec())
        });

        let results = inspector.search(0, 17, b"Hello");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], 0);
        assert_eq!(results[1], 12);
    }

    #[test]
    fn test_cpu_inspector_state() {
        let inspector = CpuInspector::new();

        let mut state = CpuState::default();
        state.rip = 0x1000;
        inspector.update_state(0, state.clone());

        let retrieved = inspector.get_state(0).unwrap();
        assert_eq!(retrieved.rip, 0x1000);
    }

    #[test]
    fn test_cpu_inspector_events() {
        let inspector = CpuInspector::new();

        inspector.log_event(IntrospectionEvent::Breakpoint(0x1000));
        inspector.log_event(IntrospectionEvent::Syscall {
            number: 1,
            args: [0; 6],
        });

        let events = inspector.get_events(0, 10);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].1,
            IntrospectionEvent::Breakpoint(0x1000)
        ));
    }

    #[test]
    fn test_cpu_inspector_logging_toggle() {
        let inspector = CpuInspector::new();

        assert!(inspector.is_logging());
        inspector.set_logging(false);
        assert!(!inspector.is_logging());

        inspector.log_event(IntrospectionEvent::Breakpoint(0x1000));
        assert_eq!(inspector.get_events(0, 10).len(), 0);
    }

    #[test]
    fn test_page_table_walker_long_mode() {
        let mut walker = PageTableWalker::new();

        // Set up fake page tables
        walker.set_read_fn(|addr| {
            // PML4, PDPT, PD, PT entries all point to next level
            // and final PT entry maps to physical 0x12345000
            match addr {
                0x1000 => Some(0x2003),     // PML4 -> PDPT at 0x2000
                0x2000 => Some(0x3003),     // PDPT -> PD at 0x3000
                0x3000 => Some(0x4003),     // PD -> PT at 0x4000
                0x4000 => Some(0x12345003), // PT -> page at 0x12345000
                _ => None,
            }
        });

        let result = walker.walk(0x1000, 0x0, true);
        assert!(result.error.is_none());
        assert_eq!(result.physical_address, Some(0x12345000));
        assert_eq!(result.page_size, 4096);
    }

    #[test]
    fn test_page_table_walker_not_present() {
        let mut walker = PageTableWalker::new();

        walker.set_read_fn(|_addr| Some(0)); // Not present

        let result = walker.walk(0x1000, 0x0, true);
        assert!(matches!(result.error, Some(PageWalkError::NotPresent(4))));
    }

    #[test]
    fn test_page_table_walker_2mb_page() {
        let mut walker = PageTableWalker::new();

        walker.set_read_fn(|addr| {
            match addr {
                0x1000 => Some(0x2003),   // PML4
                0x2000 => Some(0x3003),   // PDPT
                0x3000 => Some(0x200083), // PD: 2MB page at 0x200000
                _ => None,
            }
        });

        let result = walker.walk(0x1000, 0x1234, true);
        assert!(result.error.is_none());
        assert_eq!(result.page_size, 2 * 1024 * 1024);
        assert_eq!(result.physical_address, Some(0x200000 + 0x1234));
    }

    #[test]
    fn test_idt_inspector_64bit() {
        let mut inspector = IdtInspector::new();

        inspector.set_read_fn(|_addr, _size| {
            // 64-bit IDT entry: handler at 0xFFFF80001234
            let mut data = vec![0u8; 16];
            data[0] = 0x34;
            data[1] = 0x12; // offset_low
            data[2] = 0x08;
            data[3] = 0x00; // selector
            data[4] = 0x01; // IST
            data[5] = 0x8E; // type (interrupt gate, DPL=0, present)
            data[6] = 0x00;
            data[7] = 0x80; // offset_mid
            data[8] = 0xFF;
            data[9] = 0xFF; // offset_high
            Some(data)
        });

        let idtr = TableState {
            base: 0x1000,
            limit: 0xFFF,
        };
        let desc = inspector.read_descriptor(&idtr, 0, true).unwrap();

        assert_eq!(desc.vector, 0);
        assert_eq!(desc.selector, 0x08);
        assert_eq!(desc.gate_type, GateType::Interrupt);
        assert!(desc.present);
        assert_eq!(desc.ist, 1);
    }

    #[test]
    fn test_introspection_event() {
        let event = IntrospectionEvent::PageFault {
            address: 0xDEADBEEF,
            error_code: 0x2,
        };

        if let IntrospectionEvent::PageFault {
            address,
            error_code,
        } = event
        {
            assert_eq!(address, 0xDEADBEEF);
            assert_eq!(error_code, 0x2);
        } else {
            panic!("Wrong event type");
        }
    }

    #[test]
    fn test_cpu_state_cpl() {
        let mut state = CpuState::default();
        state.cs.selector = 0x08; // Ring 0
        assert_eq!(state.cpl(), 0);

        state.cs.selector = 0x1B; // Ring 3
        assert_eq!(state.cpl(), 3);
    }

    #[test]
    fn test_memory_region_end() {
        let region = MemoryRegion::new(0x1000, 0x2000, MemoryRegionType::Ram, "test");
        assert_eq!(region.end(), 0x3000);
    }
}
