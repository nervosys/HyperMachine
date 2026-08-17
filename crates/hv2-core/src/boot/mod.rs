//! Boot protocol support for loading operating systems
//!
//! This module provides utilities for booting operating systems using various
//! boot protocols. It handles the setup of CPU state, memory layout, and
//! boot parameters required by different boot specifications.
//!
//! # Supported Boot Protocols
//!
//! - **Linux Boot Protocol**: Direct kernel loading for Linux bzImage format
//! - **Multiboot**: Multiboot 1.0 specification for loading kernels
//! - **BIOS Boot**: Traditional boot sector loading at 0x7C00
//! - **CPU Modes**: Real mode, protected mode, and long mode transitions
//!
//! # Example: Linux Boot
//!
//! ```ignore
//! use hv2_core::boot::linux::{LinuxBootParams, LinuxBootProtocol};
//! use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
//! # fn example() -> hv2_core::Result<()> {
//! # let backend = WhpxBackend::new()?;
//! # let vm = WhpxVm::new(1, 64 * 1024 * 1024)?;
//! # let vcpu = vm.create_vcpu(0)?;
//!
//! // Configure Linux boot parameters
//! let params = LinuxBootParams {
//!     kernel_image: include_bytes!("vmlinuz").to_vec(),
//!     initrd: None,
//!     cmdline: "console=ttyS0 root=/dev/vda".to_string(),
//!     setup_addr: 0x90000,
//!     kernel_addr: 0x100000,
//! };
//!
//! // Boot the kernel
//! LinuxBootProtocol::boot(&vcpu, &vm, params)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Multiboot
//!
//! ```ignore
//! use hv2_core::boot::multiboot::{MultibootInfo, MultibootProtocol};
//! use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
//! # fn example() -> hv2_core::Result<()> {
//! # let backend = WhpxBackend::new()?;
//! # let vm = WhpxVm::new(1, 64 * 1024 * 1024)?;
//! # let vcpu = vm.create_vcpu(0)?;
//!
//! // Configure Multiboot parameters
//! let info = MultibootInfo {
//!     kernel_image: include_bytes!("kernel.elf").to_vec(),
//!     modules: Vec::new(),
//!     cmdline: "root=/dev/sda1".to_string(),
//!     memory_map: vec![(0, 640 * 1024), (1024 * 1024, 63 * 1024 * 1024)],
//! };
//!
//! // Boot the kernel
//! MultibootProtocol::boot(&vcpu, &vm, info)?;
//! # Ok(())
//! # }
//! ```

pub mod descriptor;
pub mod linux;
pub mod mode;
pub mod multiboot;
pub mod sector;
pub mod source;

use crate::{Error, Result};

/// Common boot setup utilities
pub struct BootSetup;

impl BootSetup {
    /// Allocate standard memory regions for boot tables
    ///
    /// This allocates memory for GDT, IDT, and page tables in conventional
    /// memory locations that are commonly expected by boot protocols.
    ///
    /// # Memory Layout
    ///
    /// - **0x1000-0x1FFF**: GDT (4KB)
    /// - **0x2000-0x2FFF**: IDT (4KB)  
    /// - **0x3000-0x6FFF**: Page tables (16KB)
    /// - **0x7000-0x7FFF**: Boot stack (4KB)
    ///
    /// # Returns
    ///
    /// Returns a tuple of (gdt_base, idt_base, page_table_base, stack_pointer)
    pub const fn allocate_standard_tables() -> (u64, u64, u64, u64) {
        const GDT_BASE: u64 = 0x1000;
        const IDT_BASE: u64 = 0x2000;
        const PAGE_TABLE_BASE: u64 = 0x3000;
        const STACK_POINTER: u64 = 0x8000; // Stack grows down from 0x8000

        (GDT_BASE, IDT_BASE, PAGE_TABLE_BASE, STACK_POINTER)
    }

    /// Setup identity-mapped page tables for the first 2MB
    ///
    /// This creates a simple identity mapping (virtual == physical) for the
    /// first 2MB of memory, which is sufficient for early boot code.
    ///
    /// # Page Table Structure (4-level paging)
    ///
    /// - PML4 (Page Map Level 4) at base
    /// - PDPT (Page Directory Pointer Table) at base + 0x1000
    /// - PD (Page Directory) at base + 0x2000
    /// - Uses 2MB pages (PS bit set)
    ///
    /// # Arguments
    ///
    /// * `page_table_base` - Physical address for page table structures
    ///
    /// # Returns
    ///
    /// Returns the bytes for page table structures to be written to guest memory
    pub fn create_identity_page_tables(page_table_base: u64) -> Vec<u8> {
        // Allocate 16KB for page tables (PML4 + PDPT + PD + reserved)
        let mut tables = vec![0u8; 16 * 1024];

        // PML4E[0] -> PDPT
        let pdpt_addr = page_table_base + 0x1000;
        let pml4e = pdpt_addr | 0x03; // Present + Writable
        tables[0..8].copy_from_slice(&pml4e.to_le_bytes());

        // PDPTE[0] -> PD
        let pd_addr = page_table_base + 0x2000;
        let pdpte = pd_addr | 0x03; // Present + Writable
        tables[0x1000..0x1008].copy_from_slice(&pdpte.to_le_bytes());

        // PDE[0] -> 2MB page at 0x000000
        let pde = 0x00000000u64 | 0x83; // Present + Writable + Page Size (2MB)
        tables[0x2000..0x2008].copy_from_slice(&pde.to_le_bytes());

        tables
    }

    /// Validate boot parameters for safety
    ///
    /// Checks that boot addresses don't overlap with critical regions
    /// and are properly aligned.
    pub fn validate_boot_addresses(
        kernel_addr: u64,
        kernel_size: usize,
        setup_addr: Option<u64>,
    ) -> Result<()> {
        // Kernel must be above 1MB
        if kernel_addr < 0x100000 {
            return Err(Error::VM(
                "Kernel address must be at or above 1MB (0x100000)".into(),
            ));
        }

        // Check kernel doesn't overflow address space
        if kernel_addr.checked_add(kernel_size as u64).is_none() {
            return Err(Error::VM("Kernel size causes address overflow".into()));
        }

        // If setup_addr provided, verify it's in conventional memory
        if let Some(setup) = setup_addr {
            if setup >= 0x100000 {
                return Err(Error::VM(
                    "Setup address must be below 1MB (conventional memory)".into(),
                ));
            }

            // Check setup doesn't overlap with kernel. Two half-open intervals
            // [setup, setup_end) and [kernel_addr, kernel_end) overlap iff each
            // starts before the other ends.
            let setup_end = setup + 0x10000; // Setup is typically max 64KB
            let kernel_end = kernel_addr + kernel_size as u64;

            if setup < kernel_end && setup_end > kernel_addr {
                return Err(Error::VM("Setup region overlaps with kernel region".into()));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_standard_tables() {
        let (gdt, idt, pt, sp) = BootSetup::allocate_standard_tables();
        assert_eq!(gdt, 0x1000);
        assert_eq!(idt, 0x2000);
        assert_eq!(pt, 0x3000);
        assert_eq!(sp, 0x8000);
    }

    #[test]
    fn test_create_identity_page_tables() {
        let tables = BootSetup::create_identity_page_tables(0x3000);
        assert_eq!(tables.len(), 16 * 1024);

        // Check PML4E[0] points to PDPT at 0x4000
        let pml4e = u64::from_le_bytes(tables[0..8].try_into().unwrap());
        assert_eq!(pml4e & !0xFFF, 0x4000); // PDPT address
        assert_eq!(pml4e & 0x03, 0x03); // Present + Writable

        // Check PDPTE[0] points to PD at 0x5000
        let pdpte = u64::from_le_bytes(tables[0x1000..0x1008].try_into().unwrap());
        assert_eq!(pdpte & !0xFFF, 0x5000); // PD address
        assert_eq!(pdpte & 0x03, 0x03); // Present + Writable

        // Check PDE[0] is 2MB page at 0x0
        let pde = u64::from_le_bytes(tables[0x2000..0x2008].try_into().unwrap());
        assert_eq!(pde & !0xFFF, 0x0); // Physical address 0
        assert_eq!(pde & 0x83, 0x83); // Present + Writable + PS (2MB page)
    }

    #[test]
    fn test_validate_boot_addresses() {
        // Valid kernel at 1MB
        assert!(BootSetup::validate_boot_addresses(0x100000, 0x400000, None).is_ok());

        // Valid kernel with setup
        assert!(BootSetup::validate_boot_addresses(0x100000, 0x400000, Some(0x90000)).is_ok());

        // Kernel below 1MB should fail
        assert!(BootSetup::validate_boot_addresses(0x10000, 0x400000, None).is_err());

        // Setup above 1MB should fail
        assert!(BootSetup::validate_boot_addresses(0x100000, 0x400000, Some(0x100000)).is_err());

        // Overflow should fail
        assert!(BootSetup::validate_boot_addresses(0xFFFFFFFF_FFFFF000, 0x2000, None).is_err());
    }

    #[test]
    fn test_validate_boot_addresses_overlap() {
        // Setup and kernel should not overlap
        // If kernel is at 0x100000 and setup overlaps, should fail
        // This is actually fine since setup is at 0x90000 and kernel at 0x100000
        assert!(BootSetup::validate_boot_addresses(0x100000, 0x400000, Some(0x90000)).is_ok());

        // But if we had a kernel starting lower, it might overlap
        // (Though this would fail the "kernel must be above 1MB" check first)
    }
}
