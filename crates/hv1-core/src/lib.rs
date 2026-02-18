//! HyperMachine Type-1 (Bare-Metal) Hypervisor Core
//!
//! This crate implements a Type-1 hypervisor that runs directly on hardware
//! without a host operating system. It supports both Intel VT-x (VMX) and
//! AMD-V (SVM) hardware virtualization extensions.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    AI Agent Interface                               │
//! │              (Scriptable, Safe, Observable)                         │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                     Remote API Layer                                │
//! │                   (Network Stack, RPC)                              │
//! ├──────────────┬──────────────┬──────────────┬────────────────────────┤
//! │   vCPU       │  Guest       │   Device     │   Interrupt            │
//! │   Manager    │  Memory      │   Emulation  │   Controller           │
//! ├──────────────┴──────────────┴──────────────┴────────────────────────┤
//! │                      HV1 Core Engine                                │
//! │              (Type 1 Hypervisor - VMX/SVM)                          │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                    Hardware (x86-64/ARM64)                          │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Features
//!
//! - **Intel VT-x**: Full VMX support with VMCS management
//! - **AMD-V**: Full SVM support with VMCB management  
//! - **EPT/NPT**: Hardware-assisted nested page tables
//! - **Interrupt Virtualization**: APIC virtualization, posted interrupts
//! - **Device Passthrough**: IOMMU/VT-d support for PCIe devices
//!
//! # Boot Process
//!
//! 1. UEFI/BIOS bootloader loads hypervisor
//! 2. BSP (Bootstrap Processor) initializes VMX/SVM
//! 3. Memory manager sets up physical memory map
//! 4. Each CPU enables virtualization and enters hypervisor mode
//! 5. Guest VMs are created and scheduled

#![no_std]
#![cfg_attr(feature = "bootloader_api", no_main)]
#![allow(dead_code, unused_variables, unused_imports)]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]

extern crate alloc;

pub mod arch;
pub mod boot;
pub mod cpu;
pub mod device;
pub mod error;
pub mod interrupt;
pub mod memory;
pub mod serial;
pub mod svm;
pub mod vcpu;
pub mod vm;
pub mod vmx;

pub use error::{Error, Result};

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// Global hypervisor state
static HYPERVISOR_INITIALIZED: AtomicBool = AtomicBool::new(false);
static HYPERVISOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// CPU vendor detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown,
}

impl CpuVendor {
    /// Detect CPU vendor from CPUID
    pub fn detect() -> Self {
        let cpuid = raw_cpuid::CpuId::new();
        if let Some(vendor) = cpuid.get_vendor_info() {
            match vendor.as_str() {
                "GenuineIntel" => CpuVendor::Intel,
                "AuthenticAMD" => CpuVendor::Amd,
                _ => CpuVendor::Unknown,
            }
        } else {
            CpuVendor::Unknown
        }
    }
}

/// Hypervisor capabilities
#[derive(Debug, Clone, Copy)]
pub struct HypervisorCapabilities {
    /// CPU vendor
    pub vendor: CpuVendor,
    /// VMX supported (Intel)
    pub vmx_supported: bool,
    /// SVM supported (AMD)
    pub svm_supported: bool,
    /// EPT supported (Intel nested paging)
    pub ept_supported: bool,
    /// NPT supported (AMD nested paging)
    pub npt_supported: bool,
    /// Unrestricted guest mode
    pub unrestricted_guest: bool,
    /// VPID supported (TLB tagging)
    pub vpid_supported: bool,
    /// Posted interrupts supported
    pub posted_interrupts: bool,
    /// Number of physical CPUs
    pub cpu_count: u32,
    /// Total physical memory (bytes)
    pub total_memory: u64,
}

impl HypervisorCapabilities {
    /// Detect hypervisor capabilities from hardware
    pub fn detect() -> Self {
        let vendor = CpuVendor::detect();
        let cpuid = raw_cpuid::CpuId::new();

        let mut caps = Self {
            vendor,
            vmx_supported: false,
            svm_supported: false,
            ept_supported: false,
            npt_supported: false,
            unrestricted_guest: false,
            vpid_supported: false,
            posted_interrupts: false,
            cpu_count: 1,
            total_memory: 0,
        };

        // Check virtualization support
        if let Some(features) = cpuid.get_feature_info() {
            caps.vmx_supported = features.has_vmx();
        }

        if let Some(ext_features) = cpuid.get_extended_processor_and_feature_identifiers() {
            caps.svm_supported = ext_features.has_svm();
        }

        caps
    }

    /// Check if hardware virtualization is available
    pub fn is_virtualization_supported(&self) -> bool {
        self.vmx_supported || self.svm_supported
    }
}

/// Initialize the Type-1 hypervisor
///
/// This is called early in boot before any guests are created.
/// It sets up VMX/SVM, memory management, and interrupt handling.
pub fn initialize() -> Result<()> {
    if HYPERVISOR_INITIALIZED.load(Ordering::SeqCst) {
        return Err(Error::AlreadyInitialized);
    }

    // Detect capabilities
    let caps = HypervisorCapabilities::detect();

    if !caps.is_virtualization_supported() {
        return Err(Error::NoHardwareSupport);
    }

    // Initialize based on CPU vendor
    match caps.vendor {
        CpuVendor::Intel => {
            #[cfg(feature = "intel")]
            vmx::initialize()?;
        }
        CpuVendor::Amd => {
            #[cfg(feature = "amd")]
            svm::initialize()?;
        }
        CpuVendor::Unknown => {
            return Err(Error::UnsupportedCpu);
        }
    }

    HYPERVISOR_INITIALIZED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Check if hypervisor is initialized
pub fn is_initialized() -> bool {
    HYPERVISOR_INITIALIZED.load(Ordering::SeqCst)
}

/// Get hypervisor version
pub fn version() -> &'static str {
    HYPERVISOR_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_vendor_detect() {
        let vendor = CpuVendor::detect();
        // Should be Intel or AMD on x86_64
        assert!(vendor == CpuVendor::Intel || vendor == CpuVendor::Amd);
    }

    #[test]
    fn test_capabilities_detect() {
        let caps = HypervisorCapabilities::detect();
        // Most modern CPUs support virtualization
        // This test may fail on older hardware
    }
}
