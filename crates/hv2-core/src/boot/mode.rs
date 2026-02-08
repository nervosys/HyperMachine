//! CPU Mode Transition Support
//!
//! This module provides utilities for transitioning the virtual CPU between
//! different operating modes: Real Mode → Protected Mode → Long Mode (64-bit).
//!
//! # x86 CPU Modes
//!
//! - **Real Mode**: 16-bit mode, 1MB address space, no protection
//! - **Protected Mode**: 32-bit mode, segmentation, 4GB address space
//! - **Long Mode**: 64-bit mode, flat memory model, 256TB virtual address space
//!
//! # Transition Requirements
//!
//! ## Real Mode → Protected Mode
//! 1. Disable interrupts (CLI)
//! 2. Enable A20 line
//! 3. Load GDT with valid descriptors
//! 4. Set CR0.PE (Protection Enable)
//! 5. Far jump to reload CS
//! 6. Load data segment registers
//!
//! ## Protected Mode → Long Mode
//! 1. Disable paging (CR0.PG = 0)
//! 2. Enable PAE (CR4.PAE = 1)
//! 3. Load CR3 with PML4 table address
//! 4. Enable long mode (IA32_EFER.LME = 1)
//! 5. Enable paging (CR0.PG = 1)
//! 6. Far jump to 64-bit code segment

use crate::{Error, Result};

/// Control Register 0 bits
pub mod cr0 {
    /// Protection Enable - Enables protected mode
    pub const PE: u64 = 1 << 0;
    /// Monitor Coprocessor
    pub const MP: u64 = 1 << 1;
    /// Emulation - x87 FPU emulation
    pub const EM: u64 = 1 << 2;
    /// Task Switched
    pub const TS: u64 = 1 << 3;
    /// Extension Type - x87 FPU type
    pub const ET: u64 = 1 << 4;
    /// Numeric Error - x87 FPU error reporting
    pub const NE: u64 = 1 << 5;
    /// Write Protect - Write protection for user pages
    pub const WP: u64 = 1 << 16;
    /// Alignment Mask - Alignment checking
    pub const AM: u64 = 1 << 18;
    /// Not Write-through - Cache write policy
    pub const NW: u64 = 1 << 29;
    /// Cache Disable
    pub const CD: u64 = 1 << 30;
    /// Paging Enable
    pub const PG: u64 = 1 << 31;
}

/// Control Register 4 bits
pub mod cr4 {
    /// Virtual-8086 Mode Extensions
    pub const VME: u64 = 1 << 0;
    /// Protected-Mode Virtual Interrupts
    pub const PVI: u64 = 1 << 1;
    /// Time Stamp Disable
    pub const TSD: u64 = 1 << 2;
    /// Debugging Extensions
    pub const DE: u64 = 1 << 3;
    /// Page Size Extensions - Enable 4MB pages
    pub const PSE: u64 = 1 << 4;
    /// Physical Address Extension - Enable PAE paging
    pub const PAE: u64 = 1 << 5;
    /// Machine Check Enable
    pub const MCE: u64 = 1 << 6;
    /// Page Global Enable
    pub const PGE: u64 = 1 << 7;
    /// Performance-Monitoring Counter Enable
    pub const PCE: u64 = 1 << 8;
    /// OS support for FXSAVE/FXRSTOR
    pub const OSFXSR: u64 = 1 << 9;
    /// OS support for SIMD exceptions
    pub const OSXMMEXCPT: u64 = 1 << 10;
    /// User-Mode Instruction Prevention
    pub const UMIP: u64 = 1 << 11;
    /// 57-bit linear addresses (LA57)
    pub const LA57: u64 = 1 << 12;
    /// VMX Enable
    pub const VMXE: u64 = 1 << 13;
    /// SMX Enable
    pub const SMXE: u64 = 1 << 14;
    /// FSGSBASE Enable
    pub const FSGSBASE: u64 = 1 << 16;
    /// PCID Enable
    pub const PCIDE: u64 = 1 << 17;
    /// XSAVE Enable
    pub const OSXSAVE: u64 = 1 << 18;
    /// Key Locker Enable
    pub const KL: u64 = 1 << 19;
    /// SMEP Enable
    pub const SMEP: u64 = 1 << 20;
    /// SMAP Enable
    pub const SMAP: u64 = 1 << 21;
    /// Protection Key Enable
    pub const PKE: u64 = 1 << 22;
    /// Control-flow Enforcement
    pub const CET: u64 = 1 << 23;
    /// Protection Keys for Supervisor
    pub const PKS: u64 = 1 << 24;
}

/// EFER (Extended Feature Enable Register) bits - MSR 0xC0000080
pub mod efer {
    /// MSR address for EFER
    pub const MSR_EFER: u32 = 0xC0000080;
    /// System Call Enable
    pub const SCE: u64 = 1 << 0;
    /// Long Mode Enable
    pub const LME: u64 = 1 << 8;
    /// Long Mode Active (read-only)
    pub const LMA: u64 = 1 << 10;
    /// No-Execute Enable
    pub const NXE: u64 = 1 << 11;
    /// Secure Virtual Machine Enable
    pub const SVME: u64 = 1 << 12;
    /// Long Mode Segment Limit Enable
    pub const LMSLE: u64 = 1 << 13;
    /// Fast FXSAVE/FXRSTOR
    pub const FFXSR: u64 = 1 << 14;
    /// Translation Cache Extension
    pub const TCE: u64 = 1 << 15;
}

/// RFLAGS register bits
pub mod rflags {
    /// Carry Flag
    pub const CF: u64 = 1 << 0;
    /// Parity Flag
    pub const PF: u64 = 1 << 2;
    /// Auxiliary Carry Flag
    pub const AF: u64 = 1 << 4;
    /// Zero Flag
    pub const ZF: u64 = 1 << 6;
    /// Sign Flag
    pub const SF: u64 = 1 << 7;
    /// Trap Flag
    pub const TF: u64 = 1 << 8;
    /// Interrupt Enable Flag
    pub const IF: u64 = 1 << 9;
    /// Direction Flag
    pub const DF: u64 = 1 << 10;
    /// Overflow Flag
    pub const OF: u64 = 1 << 11;
    /// I/O Privilege Level (bits 12-13)
    pub const IOPL_MASK: u64 = 3 << 12;
    /// Nested Task
    pub const NT: u64 = 1 << 14;
    /// Resume Flag
    pub const RF: u64 = 1 << 16;
    /// Virtual-8086 Mode
    pub const VM: u64 = 1 << 17;
    /// Alignment Check
    pub const AC: u64 = 1 << 18;
    /// Virtual Interrupt Flag
    pub const VIF: u64 = 1 << 19;
    /// Virtual Interrupt Pending
    pub const VIP: u64 = 1 << 20;
    /// ID Flag - CPUID supported
    pub const ID: u64 = 1 << 21;
    /// Reserved bit 1 (always set)
    pub const RESERVED_1: u64 = 1 << 1;
}

/// GDT segment selector indices
pub mod selectors {
    /// Null selector (index 0)
    pub const NULL: u16 = 0x00;
    /// Kernel code segment (index 1)
    pub const KERNEL_CODE_32: u16 = 0x08;
    /// Kernel data segment (index 2)
    pub const KERNEL_DATA_32: u16 = 0x10;
    /// Kernel code segment 64-bit (index 3)
    pub const KERNEL_CODE_64: u16 = 0x18;
    /// Kernel data segment 64-bit (index 4)
    pub const KERNEL_DATA_64: u16 = 0x20;
    /// User code segment 32-bit (index 5)
    pub const USER_CODE_32: u16 = 0x28 | 3;
    /// User data segment 32-bit (index 6)
    pub const USER_DATA_32: u16 = 0x30 | 3;
    /// User code segment 64-bit (index 7)
    pub const USER_CODE_64: u16 = 0x38 | 3;
    /// User data segment 64-bit (index 8)
    pub const USER_DATA_64: u16 = 0x40 | 3;
    /// TSS segment (index 9)
    pub const TSS: u16 = 0x48;
}

/// CPU operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    /// 16-bit real mode
    RealMode,
    /// 32-bit protected mode (no paging)
    ProtectedMode,
    /// 32-bit protected mode with paging
    ProtectedModePaging,
    /// Long mode compatibility sub-mode (32-bit code in 64-bit OS)
    CompatibilityMode,
    /// Long mode 64-bit sub-mode
    LongMode64,
}

impl CpuMode {
    /// Determine CPU mode from control register values
    pub fn from_control_registers(cr0: u64, cr4: u64, efer: u64) -> Self {
        let pe = cr0 & cr0::PE != 0;
        let pg = cr0 & cr0::PG != 0;
        let pae = cr4 & cr4::PAE != 0;
        let lme = efer & efer::LME != 0;
        let lma = efer & efer::LMA != 0;

        if !pe {
            CpuMode::RealMode
        } else if !pg {
            CpuMode::ProtectedMode
        } else if pae && lme && lma {
            // Could be CompatibilityMode or LongMode64 depending on CS.L
            // For simplicity, assume Long Mode 64-bit
            CpuMode::LongMode64
        } else {
            CpuMode::ProtectedModePaging
        }
    }
}

/// Initial vCPU state configuration
#[derive(Debug, Clone)]
pub struct InitialCpuState {
    /// Target CPU mode
    pub mode: CpuMode,
    /// Instruction pointer / entry point
    pub rip: u64,
    /// Stack pointer
    pub rsp: u64,
    /// CR0 value
    pub cr0: u64,
    /// CR3 value (page table base)
    pub cr3: u64,
    /// CR4 value
    pub cr4: u64,
    /// EFER MSR value
    pub efer: u64,
    /// RFLAGS value
    pub rflags: u64,
    /// GDT base address
    pub gdt_base: u64,
    /// GDT limit
    pub gdt_limit: u16,
    /// IDT base address
    pub idt_base: u64,
    /// IDT limit
    pub idt_limit: u16,
    /// Code segment selector
    pub cs: u16,
    /// Data segment selector
    pub ds: u16,
    /// Extra segment selector
    pub es: u16,
    /// Stack segment selector
    pub ss: u16,
    /// FS segment selector
    pub fs: u16,
    /// GS segment selector
    pub gs: u16,
}

impl InitialCpuState {
    /// Create initial state for real mode
    ///
    /// Real mode starts at 0xFFFF0 (reset vector) with minimal setup.
    pub fn real_mode() -> Self {
        Self {
            mode: CpuMode::RealMode,
            rip: 0xFFF0,
            rsp: 0x0000,
            cr0: 0,
            cr3: 0,
            cr4: 0,
            efer: 0,
            rflags: rflags::RESERVED_1,
            gdt_base: 0,
            gdt_limit: 0,
            idt_base: 0,
            idt_limit: 0x3FF, // Real mode IDT
            cs: 0xF000,       // CS:IP = FFFF:0000 = 0xFFFF0
            ds: 0,
            es: 0,
            ss: 0,
            fs: 0,
            gs: 0,
        }
    }

    /// Create initial state for 32-bit protected mode
    ///
    /// Sets up protected mode with a flat memory model and no paging.
    pub fn protected_mode_32(entry: u64, stack: u64, gdt_base: u64) -> Self {
        Self {
            mode: CpuMode::ProtectedMode,
            rip: entry,
            rsp: stack,
            cr0: cr0::PE | cr0::ET | cr0::NE,
            cr3: 0,
            cr4: 0,
            efer: 0,
            rflags: rflags::RESERVED_1,
            gdt_base,
            gdt_limit: 0x2F, // 6 entries * 8 - 1
            idt_base: 0,
            idt_limit: 0,
            cs: selectors::KERNEL_CODE_32,
            ds: selectors::KERNEL_DATA_32,
            es: selectors::KERNEL_DATA_32,
            ss: selectors::KERNEL_DATA_32,
            fs: selectors::KERNEL_DATA_32,
            gs: selectors::KERNEL_DATA_32,
        }
    }

    /// Create initial state for 64-bit long mode
    ///
    /// Sets up long mode with identity-mapped page tables.
    pub fn long_mode_64(entry: u64, stack: u64, gdt_base: u64, cr3: u64) -> Self {
        Self {
            mode: CpuMode::LongMode64,
            rip: entry,
            rsp: stack,
            cr0: cr0::PE | cr0::ET | cr0::NE | cr0::WP | cr0::PG,
            cr3,
            cr4: cr4::PAE | cr4::OSFXSR | cr4::OSXMMEXCPT,
            efer: efer::LME | efer::LMA | efer::NXE | efer::SCE,
            rflags: rflags::RESERVED_1,
            gdt_base,
            gdt_limit: 0x4F, // 10 entries * 8 - 1
            idt_base: 0,
            idt_limit: 0,
            cs: selectors::KERNEL_CODE_64,
            ds: selectors::KERNEL_DATA_64,
            es: selectors::KERNEL_DATA_64,
            ss: selectors::KERNEL_DATA_64,
            fs: selectors::KERNEL_DATA_64,
            gs: selectors::KERNEL_DATA_64,
        }
    }
}

/// GDT builder for boot setup
#[derive(Debug, Default)]
pub struct GdtBuilder {
    entries: Vec<u64>,
}

impl GdtBuilder {
    /// Create a new GDT builder with null descriptor
    pub fn new() -> Self {
        Self {
            entries: vec![0], // Null descriptor
        }
    }

    /// Add a null descriptor
    pub fn null(mut self) -> Self {
        self.entries.push(0);
        self
    }

    /// Add a 32-bit code segment descriptor
    pub fn code_32(mut self, dpl: u8) -> Self {
        // Base=0, Limit=0xFFFFF (4GB with granularity)
        // Type: Execute/Read
        // S=1 (code/data), DPL, P=1, D/B=1 (32-bit), G=1
        let desc = 0x00CF9A000000FFFFu64 | ((dpl as u64 & 3) << 45);
        self.entries.push(desc);
        self
    }

    /// Add a 32-bit data segment descriptor
    pub fn data_32(mut self, dpl: u8) -> Self {
        // Base=0, Limit=0xFFFFF (4GB with granularity)
        // Type: Read/Write
        // S=1 (code/data), DPL, P=1, D/B=1 (32-bit), G=1
        let desc = 0x00CF92000000FFFFu64 | ((dpl as u64 & 3) << 45);
        self.entries.push(desc);
        self
    }

    /// Add a 64-bit code segment descriptor
    pub fn code_64(mut self, dpl: u8) -> Self {
        // In 64-bit mode, base and limit are ignored
        // Type: Execute/Read
        // S=1, DPL, P=1, L=1 (long mode), D=0
        let desc = 0x00209A0000000000u64 | ((dpl as u64 & 3) << 45);
        self.entries.push(desc);
        self
    }

    /// Add a 64-bit data segment descriptor
    pub fn data_64(mut self, dpl: u8) -> Self {
        // In 64-bit mode, base and limit are ignored
        // Type: Read/Write
        // S=1, DPL, P=1
        let desc = 0x0000920000000000u64 | ((dpl as u64 & 3) << 45);
        self.entries.push(desc);
        self
    }

    /// Add a TSS descriptor (takes 2 entries in 64-bit mode)
    pub fn tss_64(mut self, base: u64, limit: u32) -> Self {
        // First 8 bytes: Type=9 (available TSS), P=1, limit[15:0], base[23:0]
        let low = (limit as u64 & 0xFFFF)
            | ((base & 0xFFFF) << 16)
            | ((base & 0xFF0000) << 16)
            | (0x89 << 40)  // Type=9, P=1
            | ((limit as u64 & 0xF0000) << 32);
        
        // Second 8 bytes: base[63:32]
        let high = base >> 32;

        self.entries.push(low);
        self.entries.push(high);
        self
    }

    /// Build the GDT as bytes
    pub fn build(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.entries.len() * 8);
        for entry in self.entries {
            bytes.extend_from_slice(&entry.to_le_bytes());
        }
        bytes
    }

    /// Get the GDT limit (size - 1)
    pub fn limit(&self) -> u16 {
        (self.entries.len() * 8 - 1) as u16
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Create a standard boot GDT for 32-bit protected mode
pub fn create_boot_gdt_32() -> Vec<u8> {
    GdtBuilder::new()
        .code_32(0)  // Selector 0x08
        .data_32(0)  // Selector 0x10
        .build()
}

/// Create a standard boot GDT for 64-bit long mode
pub fn create_boot_gdt_64() -> Vec<u8> {
    GdtBuilder::new()
        .code_32(0)  // Selector 0x08 - 32-bit code (for transition)
        .data_32(0)  // Selector 0x10 - 32-bit data
        .code_64(0)  // Selector 0x18 - 64-bit code
        .data_64(0)  // Selector 0x20 - 64-bit data
        .build()
}

/// Page table entry flags
pub mod pte {
    /// Page is present
    pub const PRESENT: u64 = 1 << 0;
    /// Page is writable
    pub const WRITABLE: u64 = 1 << 1;
    /// Page is accessible from user mode
    pub const USER: u64 = 1 << 2;
    /// Write-through caching
    pub const WRITE_THROUGH: u64 = 1 << 3;
    /// Cache disabled
    pub const CACHE_DISABLE: u64 = 1 << 4;
    /// Page was accessed
    pub const ACCESSED: u64 = 1 << 5;
    /// Page was written (dirty)
    pub const DIRTY: u64 = 1 << 6;
    /// Page size (1 = large page: 2MB in PD, 1GB in PDPT)
    pub const PAGE_SIZE: u64 = 1 << 7;
    /// Global page
    pub const GLOBAL: u64 = 1 << 8;
    /// No execute (requires NXE in EFER)
    pub const NO_EXECUTE: u64 = 1 << 63;
}

/// Create identity-mapped page tables for long mode
///
/// Creates a 4-level page table structure that identity maps physical memory.
///
/// # Arguments
///
/// * `base` - Physical address where page tables will be placed
/// * `memory_size` - Amount of memory to identity map
///
/// # Returns
///
/// Returns the page table bytes to write to guest memory
pub fn create_identity_page_tables_64(base: u64, memory_size: u64) -> Vec<u8> {
    // Calculate how many 2MB pages we need
    let num_2mb_pages = memory_size.div_ceil(0x200000);
    
    // Calculate table structure
    // - PML4: 1 page (512 entries, each covers 512GB)
    // - PDPT: 1 page (512 entries, each covers 1GB)  
    // - PD: N pages (512 entries each, each entry covers 2MB)
    let num_pd_pages = num_2mb_pages.div_ceil(512);
    let total_pages = 2 + num_pd_pages; // PML4 + PDPT + PDs
    
    let mut tables = vec![0u8; total_pages as usize * 4096];
    
    // Page table addresses (PML4 at base)
    let _pml4_addr = base;
    let pdpt_addr = base + 0x1000;
    let pd_base = base + 0x2000;
    
    // PML4[0] -> PDPT
    let pml4e = pdpt_addr | pte::PRESENT | pte::WRITABLE;
    tables[0..8].copy_from_slice(&pml4e.to_le_bytes());
    
    // Fill PDPT entries pointing to PDs
    for i in 0..num_pd_pages.min(512) {
        let pd_addr = pd_base + i * 0x1000;
        let pdpte = pd_addr | pte::PRESENT | pte::WRITABLE;
        let offset = 0x1000 + (i as usize * 8);
        tables[offset..offset + 8].copy_from_slice(&pdpte.to_le_bytes());
    }
    
    // Fill PD entries with 2MB pages
    let mut page_idx = 0u64;
    for pd in 0..num_pd_pages {
        for entry in 0..512u64 {
            if page_idx >= num_2mb_pages {
                break;
            }
            let phys_addr = page_idx * 0x200000;
            let pde = phys_addr | pte::PRESENT | pte::WRITABLE | pte::PAGE_SIZE;
            let offset = 0x2000 + (pd as usize * 0x1000) + (entry as usize * 8);
            tables[offset..offset + 8].copy_from_slice(&pde.to_le_bytes());
            page_idx += 1;
        }
    }
    
    tables
}

/// Validate that control register values are consistent for the target mode
pub fn validate_mode_registers(state: &InitialCpuState) -> Result<()> {
    match state.mode {
        CpuMode::RealMode => {
            if state.cr0 & cr0::PE != 0 {
                return Err(Error::VM("PE must be clear for real mode".into()));
            }
        }
        CpuMode::ProtectedMode => {
            if state.cr0 & cr0::PE == 0 {
                return Err(Error::VM("PE must be set for protected mode".into()));
            }
            if state.cr0 & cr0::PG != 0 {
                return Err(Error::VM(
                    "PG must be clear for protected mode without paging".into(),
                ));
            }
        }
        CpuMode::ProtectedModePaging => {
            if state.cr0 & cr0::PE == 0 {
                return Err(Error::VM("PE must be set for protected mode".into()));
            }
            if state.cr0 & cr0::PG == 0 {
                return Err(Error::VM("PG must be set for paging mode".into()));
            }
            if state.cr3 == 0 {
                return Err(Error::VM("CR3 must be set for paging mode".into()));
            }
        }
        CpuMode::LongMode64 | CpuMode::CompatibilityMode => {
            if state.cr0 & cr0::PE == 0 {
                return Err(Error::VM("PE must be set for long mode".into()));
            }
            if state.cr0 & cr0::PG == 0 {
                return Err(Error::VM("PG must be set for long mode".into()));
            }
            if state.cr4 & cr4::PAE == 0 {
                return Err(Error::VM("PAE must be set for long mode".into()));
            }
            if state.efer & efer::LME == 0 {
                return Err(Error::VM("LME must be set for long mode".into()));
            }
            if state.cr3 == 0 {
                return Err(Error::VM("CR3 must be set for long mode".into()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_mode_from_registers() {
        // Real mode
        assert_eq!(
            CpuMode::from_control_registers(0, 0, 0),
            CpuMode::RealMode
        );

        // Protected mode (no paging)
        assert_eq!(
            CpuMode::from_control_registers(cr0::PE, 0, 0),
            CpuMode::ProtectedMode
        );

        // Protected mode with paging
        assert_eq!(
            CpuMode::from_control_registers(cr0::PE | cr0::PG, 0, 0),
            CpuMode::ProtectedModePaging
        );

        // Long mode
        assert_eq!(
            CpuMode::from_control_registers(
                cr0::PE | cr0::PG,
                cr4::PAE,
                efer::LME | efer::LMA
            ),
            CpuMode::LongMode64
        );
    }

    #[test]
    fn test_initial_state_real_mode() {
        let state = InitialCpuState::real_mode();
        assert_eq!(state.mode, CpuMode::RealMode);
        assert_eq!(state.cs, 0xF000);
        assert_eq!(state.rip, 0xFFF0);
        assert_eq!(state.cr0, 0);
    }

    #[test]
    fn test_initial_state_protected_mode() {
        let state = InitialCpuState::protected_mode_32(0x100000, 0x8000, 0x1000);
        assert_eq!(state.mode, CpuMode::ProtectedMode);
        assert_eq!(state.rip, 0x100000);
        assert_eq!(state.rsp, 0x8000);
        assert!(state.cr0 & cr0::PE != 0);
        assert!(state.cr0 & cr0::PG == 0);
    }

    #[test]
    fn test_initial_state_long_mode() {
        let state = InitialCpuState::long_mode_64(0x100000, 0x8000, 0x1000, 0x3000);
        assert_eq!(state.mode, CpuMode::LongMode64);
        assert!(state.cr0 & cr0::PE != 0);
        assert!(state.cr0 & cr0::PG != 0);
        assert!(state.cr4 & cr4::PAE != 0);
        assert!(state.efer & efer::LME != 0);
    }

    #[test]
    fn test_gdt_builder_32() {
        let gdt = GdtBuilder::new()
            .code_32(0)
            .data_32(0)
            .build();

        assert_eq!(gdt.len(), 24); // 3 entries * 8 bytes

        // Check null descriptor
        assert_eq!(&gdt[0..8], &[0; 8]);

        // Code segment should have execute bit
        let code = u64::from_le_bytes(gdt[8..16].try_into().unwrap());
        assert!(code & 0x0000_0800_0000_0000 != 0); // Execute bit

        // Data segment should have write bit
        let data = u64::from_le_bytes(gdt[16..24].try_into().unwrap());
        assert!(data & 0x0000_0200_0000_0000 != 0); // Write bit
    }

    #[test]
    fn test_gdt_builder_64() {
        let gdt = create_boot_gdt_64();
        
        // Should have: null, code32, data32, code64, data64 = 5 entries
        assert_eq!(gdt.len(), 40);
    }

    #[test]
    fn test_create_identity_page_tables_small() {
        let tables = create_identity_page_tables_64(0x3000, 2 * 1024 * 1024);
        
        // Should have PML4 + PDPT + 1 PD = 3 pages
        assert_eq!(tables.len(), 3 * 4096);

        // Check PML4[0] points to PDPT
        let pml4e = u64::from_le_bytes(tables[0..8].try_into().unwrap());
        assert_eq!(pml4e & !0xFFF, 0x4000); // PDPT at base + 0x1000
        assert!(pml4e & pte::PRESENT != 0);
        assert!(pml4e & pte::WRITABLE != 0);
    }

    #[test]
    fn test_create_identity_page_tables_large() {
        // 1GB = 512 * 2MB pages
        let tables = create_identity_page_tables_64(0x3000, 1024 * 1024 * 1024);
        
        // Should have PML4 + PDPT + 1 PD = 3 pages (512 entries cover 1GB)
        assert_eq!(tables.len(), 3 * 4096);

        // Check first PD entry is 2MB page at 0
        let pde = u64::from_le_bytes(tables[0x2000..0x2008].try_into().unwrap());
        assert_eq!(pde & !0xFFF, 0); // Physical address 0
        assert!(pde & pte::PAGE_SIZE != 0); // 2MB page
    }

    #[test]
    fn test_validate_mode_real() {
        let state = InitialCpuState::real_mode();
        assert!(validate_mode_registers(&state).is_ok());
    }

    #[test]
    fn test_validate_mode_protected() {
        let state = InitialCpuState::protected_mode_32(0x100000, 0x8000, 0x1000);
        assert!(validate_mode_registers(&state).is_ok());
    }

    #[test]
    fn test_validate_mode_long() {
        let state = InitialCpuState::long_mode_64(0x100000, 0x8000, 0x1000, 0x3000);
        assert!(validate_mode_registers(&state).is_ok());
    }

    #[test]
    fn test_validate_mode_invalid_real() {
        let mut state = InitialCpuState::real_mode();
        state.cr0 = cr0::PE; // Invalid: PE set in real mode
        assert!(validate_mode_registers(&state).is_err());
    }

    #[test]
    fn test_validate_mode_invalid_long() {
        let mut state = InitialCpuState::long_mode_64(0x100000, 0x8000, 0x1000, 0x3000);
        state.cr4 = 0; // Invalid: PAE must be set
        assert!(validate_mode_registers(&state).is_err());
    }

    #[test]
    fn test_selectors() {
        assert_eq!(selectors::NULL, 0x00);
        assert_eq!(selectors::KERNEL_CODE_32, 0x08);
        assert_eq!(selectors::KERNEL_DATA_32, 0x10);
        assert_eq!(selectors::KERNEL_CODE_64, 0x18);
        assert_eq!(selectors::KERNEL_DATA_64, 0x20);
    }

    #[test]
    fn test_cr0_flags() {
        assert_eq!(cr0::PE, 1);
        assert_eq!(cr0::PG, 0x80000000);
    }

    #[test]
    fn test_cr4_flags() {
        assert_eq!(cr4::PAE, 0x20);
        assert_eq!(cr4::OSFXSR, 0x200);
    }

    #[test]
    fn test_efer_flags() {
        assert_eq!(efer::LME, 0x100);
        assert_eq!(efer::LMA, 0x400);
        assert_eq!(efer::NXE, 0x800);
    }
}
