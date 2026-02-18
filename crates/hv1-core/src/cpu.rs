//! CPU management for Type-1 hypervisor
//!
//! This module handles:
//! - Per-CPU data structures
//! - CPU enumeration and initialization
//! - CPUID virtualization
//! - MSR management

use crate::{CpuVendor, Error, Result};
use core::sync::atomic::{AtomicU32, Ordering};

/// Maximum number of CPUs supported
pub const MAX_CPUS: usize = 256;

/// Global CPU count
static CPU_COUNT: AtomicU32 = AtomicU32::new(0);

/// Per-CPU data structure
#[derive(Debug)]
pub struct CpuData {
    /// CPU ID (APIC ID)
    pub id: u32,
    /// CPU index (0-based)
    pub index: u32,
    /// CPU vendor
    pub vendor: CpuVendor,
    /// Whether this CPU is the BSP
    pub is_bsp: bool,
    /// Whether VMX/SVM is enabled on this CPU
    pub virtualization_enabled: bool,
}

impl CpuData {
    /// Create new CPU data for the current CPU
    pub fn new(index: u32) -> Self {
        let cpuid = raw_cpuid::CpuId::new();

        let vendor = detect_vendor(&cpuid);
        let id = get_apic_id(&cpuid);
        let is_bsp = index == 0;

        Self {
            id,
            index,
            vendor,
            is_bsp,
            virtualization_enabled: false,
        }
    }

    /// Initialize virtualization on this CPU
    pub fn enable_virtualization(&mut self) -> Result<()> {
        match self.vendor {
            CpuVendor::Intel => {
                #[cfg(feature = "intel")]
                {
                    crate::vmx::initialize()?;
                    self.virtualization_enabled = true;
                }
                #[cfg(not(feature = "intel"))]
                {
                    return Err(Error::NoHardwareSupport);
                }
            }
            CpuVendor::Amd => {
                #[cfg(feature = "amd")]
                {
                    crate::svm::initialize()?;
                    self.virtualization_enabled = true;
                }
                #[cfg(not(feature = "amd"))]
                {
                    return Err(Error::NoHardwareSupport);
                }
            }
            CpuVendor::Unknown => {
                return Err(Error::NoHardwareSupport);
            }
        }
        Ok(())
    }
}

/// Detect CPU vendor
fn detect_vendor<R: raw_cpuid::CpuIdReader>(cpuid: &raw_cpuid::CpuId<R>) -> CpuVendor {
    cpuid
        .get_vendor_info()
        .map(|v| {
            if v.as_str() == "GenuineIntel" {
                CpuVendor::Intel
            } else if v.as_str() == "AuthenticAMD" {
                CpuVendor::Amd
            } else {
                CpuVendor::Unknown
            }
        })
        .unwrap_or(CpuVendor::Unknown)
}

/// Get the APIC ID of the current CPU
fn get_apic_id<R: raw_cpuid::CpuIdReader>(cpuid: &raw_cpuid::CpuId<R>) -> u32 {
    cpuid
        .get_feature_info()
        .map(|f| f.initial_local_apic_id() as u32)
        .unwrap_or(0)
}

/// Register a new CPU
pub fn register_cpu() -> u32 {
    CPU_COUNT.fetch_add(1, Ordering::SeqCst)
}

/// Get the current CPU count
pub fn cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::SeqCst)
}

/// CPUID result
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuidResult {
    /// EAX register
    pub eax: u32,
    /// EBX register
    pub ebx: u32,
    /// ECX register
    pub ecx: u32,
    /// EDX register
    pub edx: u32,
}

/// Execute CPUID instruction
pub fn cpuid(leaf: u32, subleaf: u32) -> CpuidResult {
    let result = core::arch::x86_64::__cpuid_count(leaf, subleaf);
    CpuidResult {
        eax: result.eax,
        ebx: result.ebx,
        ecx: result.ecx,
        edx: result.edx,
    }
}

/// CPUID leaves that may need virtualization
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuidLeaf {
    /// Basic CPUID information
    Basic = 0x0,
    /// Processor info and feature bits
    ProcessorInfo = 0x1,
    /// Cache and TLB descriptor information
    CacheTlb = 0x2,
    /// Processor serial number
    SerialNumber = 0x3,
    /// Cache parameters
    CacheParams = 0x4,
    /// MONITOR/MWAIT parameters
    MonitorMwait = 0x5,
    /// Thermal and power management
    ThermalPower = 0x6,
    /// Extended feature flags
    ExtendedFeatures = 0x7,
    /// Direct cache access parameters
    Dca = 0x9,
    /// Architectural performance monitoring
    PerfMon = 0xA,
    /// Extended topology enumeration
    ExtendedTopology = 0xB,
    /// Processor extended state enumeration
    ExtendedState = 0xD,
    /// Hypervisor info (when running under hypervisor)
    HypervisorInfo = 0x4000_0000,
    /// Extended function CPUID information
    ExtendedMax = 0x8000_0000,
    /// Extended processor info and feature bits
    ExtendedInfo = 0x8000_0001,
    /// Processor brand string (part 1)
    BrandString1 = 0x8000_0002,
    /// Processor brand string (part 2)
    BrandString2 = 0x8000_0003,
    /// Processor brand string (part 3)
    BrandString3 = 0x8000_0004,
    /// L1 cache and TLB identifiers
    L1CacheTlb = 0x8000_0005,
    /// L2 cache features
    L2Cache = 0x8000_0006,
    /// Advanced power management info
    AdvancedPower = 0x8000_0007,
    /// Virtual and physical address sizes
    AddressSizes = 0x8000_0008,
    /// SVM features (AMD)
    SvmFeatures = 0x8000_000A,
}

/// Virtualize CPUID for guest
///
/// This function filters and modifies CPUID results for the guest,
/// hiding hypervisor presence or adjusting feature flags as needed.
pub fn virtualize_cpuid(leaf: u32, subleaf: u32, hide_hypervisor: bool) -> CpuidResult {
    let mut result = cpuid(leaf, subleaf);

    if hide_hypervisor {
        match leaf {
            // Hide VMX/SVM feature bits and hypervisor present bit
            0x1 => {
                result.ecx &= !(1 << 5); // Clear VMX bit
                result.ecx &= !(1 << 31); // Clear hypervisor present bit
            }
            // Return 0 for hypervisor-specific leaves
            0x4000_0000..=0x4FFF_FFFF => {
                result = CpuidResult::default();
            }
            // Hide SVM feature bits
            0x8000_0001 => {
                result.ecx &= !(1 << 2); // Clear SVM bit
            }
            _ => {}
        }
    }

    result
}

/// MSR access helpers
pub mod msr {
    /// Read an MSR
    ///
    /// # Safety
    /// Reading certain MSRs can cause undefined behavior or faults.
    #[inline]
    pub unsafe fn read(msr: u32) -> u64 {
        x86::msr::rdmsr(msr)
    }

    /// Write to an MSR
    ///
    /// # Safety
    /// Writing to certain MSRs can cause undefined behavior or system instability.
    #[inline]
    pub unsafe fn write(msr: u32, value: u64) {
        x86::msr::wrmsr(msr, value);
    }

    /// Common MSR addresses
    pub mod addr {
        /// IA32_APIC_BASE
        pub const IA32_APIC_BASE: u32 = 0x1B;
        /// IA32_FEATURE_CONTROL
        pub const IA32_FEATURE_CONTROL: u32 = 0x3A;
        /// IA32_SYSENTER_CS
        pub const IA32_SYSENTER_CS: u32 = 0x174;
        /// IA32_SYSENTER_ESP
        pub const IA32_SYSENTER_ESP: u32 = 0x175;
        /// IA32_SYSENTER_EIP
        pub const IA32_SYSENTER_EIP: u32 = 0x176;
        /// IA32_PAT
        pub const IA32_PAT: u32 = 0x277;
        /// IA32_EFER
        pub const IA32_EFER: u32 = 0xC0000080;
        /// IA32_STAR
        pub const IA32_STAR: u32 = 0xC0000081;
        /// IA32_LSTAR
        pub const IA32_LSTAR: u32 = 0xC0000082;
        /// IA32_CSTAR
        pub const IA32_CSTAR: u32 = 0xC0000083;
        /// IA32_FMASK
        pub const IA32_FMASK: u32 = 0xC0000084;
        /// IA32_FS_BASE
        pub const IA32_FS_BASE: u32 = 0xC0000100;
        /// IA32_GS_BASE
        pub const IA32_GS_BASE: u32 = 0xC0000101;
        /// IA32_KERNEL_GS_BASE
        pub const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
        /// IA32_TSC_AUX
        pub const IA32_TSC_AUX: u32 = 0xC0000103;
    }
}
