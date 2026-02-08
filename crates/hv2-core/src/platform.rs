//! Platform Integration and Cross-Platform Abstractions
//!
//! This module provides platform-independent abstractions for hypervisor
//! functionality, allowing the same code to run on Windows (WHPX), Linux (KVM),
//! and macOS (HVF).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │              Application Layer                   │
//! ├─────────────────────────────────────────────────┤
//! │           Platform Abstraction                   │
//! │  ┌──────────────────────────────────────────┐   │
//! │  │  PlatformVm / PlatformVcpu               │   │
//! │  │  Unified API for all platforms           │   │
//! │  └──────────────────────────────────────────┘   │
//! ├─────────────────────────────────────────────────┤
//! │         Platform-Specific Backends              │
//! │  ┌─────────┐  ┌─────────┐  ┌─────────┐        │
//! │  │  WHPX   │  │   KVM   │  │   HVF   │        │
//! │  │ Windows │  │  Linux  │  │  macOS  │        │
//! │  └─────────┘  └─────────┘  └─────────┘        │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! # Hyper-V Enlightenments
//!
//! When running on Windows with Hyper-V, enlightenments can significantly
//! improve guest performance by allowing the guest to use paravirtualized
//! interfaces instead of emulated hardware.

use crate::hypervisor::HypervisorPlatform;
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Platform feature flags
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlatformFeatures {
    /// Hardware virtualization available (VT-x/AMD-V/ARM VHE)
    pub hardware_virt: bool,
    /// Nested virtualization supported
    pub nested_virt: bool,
    /// Extended Page Tables (EPT/NPT/Stage-2)
    pub extended_page_tables: bool,
    /// APIC virtualization (APICv/AVIC)
    pub apic_virtualization: bool,
    /// Posted interrupts support
    pub posted_interrupts: bool,
    /// VMCS shadowing (for nested virt)
    pub vmcs_shadowing: bool,
    /// Virtual interrupt delivery
    pub virtual_interrupt_delivery: bool,
    /// PML (Page Modification Logging)
    pub page_modification_logging: bool,
}

impl PlatformFeatures {
    /// Create features for a typical modern Intel CPU with VT-x
    pub fn intel_vtx() -> Self {
        Self {
            hardware_virt: true,
            nested_virt: true,
            extended_page_tables: true,
            apic_virtualization: true,
            posted_interrupts: true,
            vmcs_shadowing: true,
            virtual_interrupt_delivery: true,
            page_modification_logging: true,
        }
    }

    /// Create features for a typical modern AMD CPU with AMD-V
    pub fn amd_v() -> Self {
        Self {
            hardware_virt: true,
            nested_virt: true,
            extended_page_tables: true, // NPT
            apic_virtualization: true,  // AVIC
            posted_interrupts: false,
            vmcs_shadowing: false,
            virtual_interrupt_delivery: true,
            page_modification_logging: true,
        }
    }

    /// Create features for software emulation (no hardware support)
    pub fn software_only() -> Self {
        Self::default()
    }
}

/// Hyper-V enlightenment flags
#[derive(Debug, Clone, Copy, Default)]
pub struct HyperVEnlightenments {
    /// Use synthetic timers instead of emulated PIT/HPET
    pub synthetic_timers: bool,
    /// Use synthetic interrupt controller
    pub synthetic_interrupt_controller: bool,
    /// Enable VP assist page for faster hypercalls
    pub vp_assist_page: bool,
    /// Use reference TSC page for fast timekeeping
    pub reference_tsc: bool,
    /// Enable relaxed timing (allows timer coalescing)
    pub relaxed_timing: bool,
    /// Enable vapic (virtual APIC acceleration)
    pub vapic: bool,
    /// Enable crash MSRs for guest crash notification
    pub crash_msrs: bool,
    /// Enable frequency MSRs for accurate guest TSC
    pub frequency_msrs: bool,
    /// Enable reenlightenment notification
    pub reenlightenment: bool,
    /// Enable TLB flush hypercalls
    pub tlb_flush: bool,
    /// Enable IPI hypercalls
    pub ipi_hypercalls: bool,
    /// Enable APIC access relaxation
    pub apic_access_relaxation: bool,
}

impl HyperVEnlightenments {
    /// Create minimal enlightenments for basic functionality
    pub fn minimal() -> Self {
        Self {
            synthetic_timers: true,
            reference_tsc: true,
            relaxed_timing: true,
            ..Default::default()
        }
    }

    /// Create full enlightenments for maximum performance
    pub fn full() -> Self {
        Self {
            synthetic_timers: true,
            synthetic_interrupt_controller: true,
            vp_assist_page: true,
            reference_tsc: true,
            relaxed_timing: true,
            vapic: true,
            crash_msrs: true,
            frequency_msrs: true,
            reenlightenment: true,
            tlb_flush: true,
            ipi_hypercalls: true,
            apic_access_relaxation: true,
        }
    }

    /// Create a CPUID feature flags value for enlightenment detection
    pub fn to_cpuid_features(&self) -> u32 {
        let mut flags = 0u32;
        if self.vp_assist_page {
            flags |= 1 << 0;
        }
        if self.synthetic_timers {
            flags |= 1 << 3;
        }
        if self.reference_tsc {
            flags |= 1 << 9;
        }
        if self.crash_msrs {
            flags |= 1 << 10;
        }
        if self.frequency_msrs {
            flags |= 1 << 11;
        }
        flags
    }

    /// Create a CPUID recommendations value
    pub fn to_cpuid_recommendations(&self) -> u32 {
        let mut flags = 0u32;
        if self.relaxed_timing {
            flags |= 1 << 5;
        }
        if self.tlb_flush {
            flags |= 1 << 6;
        }
        if self.ipi_hypercalls {
            flags |= 1 << 10;
        }
        if self.apic_access_relaxation {
            flags |= 1 << 13;
        }
        flags
    }
}

/// Hyper-V partition privilege flags
#[derive(Debug, Clone, Copy, Default)]
pub struct HyperVPrivileges {
    /// Access to virtual MSRs
    pub access_vp_runtime_msr: bool,
    /// Access to partition reference counter
    pub access_partition_reference_counter: bool,
    /// Access to synthetic timers
    pub access_synic_msrs: bool,
    /// Access to APIC frequency MSR
    pub access_apic_frequency_msr: bool,
    /// Access to hypercall MSRs
    pub access_hypercall_msrs: bool,
    /// Access to VP index
    pub access_vp_index: bool,
    /// Access to partition reference TSC
    pub access_partition_reference_tsc: bool,
    /// Access to guest idle MSR
    pub access_guest_idle_msr: bool,
    /// Access to frequency MSRs
    pub access_frequency_msrs: bool,
}

impl HyperVPrivileges {
    /// Create standard privileges for a Windows guest
    pub fn standard() -> Self {
        Self {
            access_vp_runtime_msr: true,
            access_partition_reference_counter: true,
            access_synic_msrs: true,
            access_apic_frequency_msr: true,
            access_hypercall_msrs: true,
            access_vp_index: true,
            access_partition_reference_tsc: true,
            access_guest_idle_msr: true,
            access_frequency_msrs: true,
        }
    }

    /// Convert to CPUID EAX value for leaf 0x40000003
    pub fn to_cpuid_eax(&self) -> u32 {
        let mut flags = 0u32;
        if self.access_vp_runtime_msr {
            flags |= 1 << 0;
        }
        if self.access_partition_reference_counter {
            flags |= 1 << 1;
        }
        if self.access_synic_msrs {
            flags |= 1 << 2;
        }
        if self.access_apic_frequency_msr {
            flags |= 1 << 4;
        }
        if self.access_hypercall_msrs {
            flags |= 1 << 5;
        }
        if self.access_vp_index {
            flags |= 1 << 6;
        }
        if self.access_partition_reference_tsc {
            flags |= 1 << 9;
        }
        if self.access_guest_idle_msr {
            flags |= 1 << 10;
        }
        if self.access_frequency_msrs {
            flags |= 1 << 11;
        }
        flags
    }
}

/// Hyper-V MSR numbers
pub mod hyperv_msrs {
    /// Guest OS ID MSR
    pub const HV_X64_MSR_GUEST_OS_ID: u32 = 0x40000000;
    /// Hypercall MSR
    pub const HV_X64_MSR_HYPERCALL: u32 = 0x40000001;
    /// VP index MSR
    pub const HV_X64_MSR_VP_INDEX: u32 = 0x40000002;
    /// VP runtime MSR
    pub const HV_X64_MSR_VP_RUNTIME: u32 = 0x40000010;
    /// Time reference count MSR
    pub const HV_X64_MSR_TIME_REF_COUNT: u32 = 0x40000020;
    /// Reference TSC MSR
    pub const HV_X64_MSR_REFERENCE_TSC: u32 = 0x40000021;
    /// TSC frequency MSR
    pub const HV_X64_MSR_TSC_FREQUENCY: u32 = 0x40000022;
    /// APIC frequency MSR
    pub const HV_X64_MSR_APIC_FREQUENCY: u32 = 0x40000023;
    /// EOI MSR
    pub const HV_X64_MSR_EOI: u32 = 0x40000070;
    /// ICR MSR
    pub const HV_X64_MSR_ICR: u32 = 0x40000071;
    /// TPR MSR
    pub const HV_X64_MSR_TPR: u32 = 0x40000072;
    /// VP assist page MSR
    pub const HV_X64_MSR_VP_ASSIST_PAGE: u32 = 0x40000073;
    /// SynIC control MSR
    pub const HV_X64_MSR_SCONTROL: u32 = 0x40000080;
    /// SynIC version MSR
    pub const HV_X64_MSR_SVERSION: u32 = 0x40000081;
    /// SynIC SINT0-15 MSRs base
    pub const HV_X64_MSR_SINT_BASE: u32 = 0x40000090;
    /// SynIC message page MSR
    pub const HV_X64_MSR_SIMP: u32 = 0x40000083;
    /// SynIC event flags page MSR
    pub const HV_X64_MSR_SIEFP: u32 = 0x40000082;
    /// Synthetic timer 0 config MSR
    pub const HV_X64_MSR_STIMER0_CONFIG: u32 = 0x400000B0;
    /// Synthetic timer 0 count MSR
    pub const HV_X64_MSR_STIMER0_COUNT: u32 = 0x400000B1;
    /// Crash MSR (P0)
    pub const HV_X64_MSR_CRASH_P0: u32 = 0x40000100;
    /// Crash control MSR
    pub const HV_X64_MSR_CRASH_CTL: u32 = 0x40000105;
    /// Reenlightenment control MSR
    pub const HV_X64_MSR_REENLIGHTENMENT_CONTROL: u32 = 0x40000106;
}

/// Platform-specific VM configuration
#[derive(Debug, Clone)]
pub struct PlatformVmConfig {
    /// Number of vCPUs
    pub vcpu_count: u32,
    /// Memory size in bytes
    pub memory_size: u64,
    /// Platform features to enable
    pub features: PlatformFeatures,
    /// Hyper-V enlightenments (for Windows guests)
    pub hyperv_enlightenments: Option<HyperVEnlightenments>,
    /// Hyper-V privileges
    pub hyperv_privileges: Option<HyperVPrivileges>,
    /// Enable APIC virtualization if available
    pub enable_apicv: bool,
    /// Enable EPT/NPT if available
    pub enable_ept: bool,
    /// Enable unrestricted guest mode
    pub unrestricted_guest: bool,
}

impl Default for PlatformVmConfig {
    fn default() -> Self {
        Self {
            vcpu_count: 1,
            memory_size: 128 * 1024 * 1024, // 128 MB
            features: PlatformFeatures::default(),
            hyperv_enlightenments: None,
            hyperv_privileges: None,
            enable_apicv: true,
            enable_ept: true,
            unrestricted_guest: true,
        }
    }
}

impl PlatformVmConfig {
    /// Create a configuration for a Windows guest
    pub fn windows_guest(vcpu_count: u32, memory_mb: u64) -> Self {
        Self {
            vcpu_count,
            memory_size: memory_mb * 1024 * 1024,
            features: PlatformFeatures::intel_vtx(),
            hyperv_enlightenments: Some(HyperVEnlightenments::full()),
            hyperv_privileges: Some(HyperVPrivileges::standard()),
            enable_apicv: true,
            enable_ept: true,
            unrestricted_guest: true,
        }
    }

    /// Create a configuration for a Linux guest
    pub fn linux_guest(vcpu_count: u32, memory_mb: u64) -> Self {
        Self {
            vcpu_count,
            memory_size: memory_mb * 1024 * 1024,
            features: PlatformFeatures::intel_vtx(),
            hyperv_enlightenments: None,
            hyperv_privileges: None,
            enable_apicv: true,
            enable_ept: true,
            unrestricted_guest: true,
        }
    }
}

/// Platform-independent VM builder
#[derive(Debug)]
pub struct PlatformVmBuilder {
    config: PlatformVmConfig,
    platform: Option<HypervisorPlatform>,
}

impl PlatformVmBuilder {
    /// Create a new VM builder
    pub fn new() -> Self {
        Self {
            config: PlatformVmConfig::default(),
            platform: None,
        }
    }

    /// Set the number of vCPUs
    pub fn vcpus(mut self, count: u32) -> Self {
        self.config.vcpu_count = count;
        self
    }

    /// Set memory size in megabytes
    pub fn memory_mb(mut self, mb: u64) -> Self {
        self.config.memory_size = mb * 1024 * 1024;
        self
    }

    /// Set memory size in gigabytes
    pub fn memory_gb(mut self, gb: u64) -> Self {
        self.config.memory_size = gb * 1024 * 1024 * 1024;
        self
    }

    /// Force a specific platform
    pub fn platform(mut self, platform: HypervisorPlatform) -> Self {
        self.platform = Some(platform);
        self
    }

    /// Enable Hyper-V enlightenments for Windows guests
    pub fn with_hyperv_enlightenments(mut self) -> Self {
        self.config.hyperv_enlightenments = Some(HyperVEnlightenments::full());
        self.config.hyperv_privileges = Some(HyperVPrivileges::standard());
        self
    }

    /// Use minimal Hyper-V enlightenments
    pub fn with_minimal_hyperv(mut self) -> Self {
        self.config.hyperv_enlightenments = Some(HyperVEnlightenments::minimal());
        self.config.hyperv_privileges = Some(HyperVPrivileges::standard());
        self
    }

    /// Disable Hyper-V enlightenments
    pub fn without_hyperv(mut self) -> Self {
        self.config.hyperv_enlightenments = None;
        self.config.hyperv_privileges = None;
        self
    }

    /// Enable or disable APIC virtualization
    pub fn apicv(mut self, enabled: bool) -> Self {
        self.config.enable_apicv = enabled;
        self
    }

    /// Enable or disable EPT/NPT
    pub fn ept(mut self, enabled: bool) -> Self {
        self.config.enable_ept = enabled;
        self
    }

    /// Set platform features
    pub fn features(mut self, features: PlatformFeatures) -> Self {
        self.config.features = features;
        self
    }

    /// Get the final configuration
    pub fn config(&self) -> &PlatformVmConfig {
        &self.config
    }

    /// Get the selected platform (or auto-detect)
    pub fn get_platform(&self) -> HypervisorPlatform {
        self.platform.unwrap_or_else(HypervisorPlatform::detect)
    }

    /// Build and return the configuration
    pub fn build(self) -> (HypervisorPlatform, PlatformVmConfig) {
        (self.get_platform(), self.config)
    }
}

impl Default for PlatformVmBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Cross-platform memory region
#[derive(Debug, Clone)]
pub struct PlatformMemoryRegion {
    /// Guest physical address
    pub guest_addr: u64,
    /// Size in bytes
    pub size: u64,
    /// Host virtual address
    pub host_addr: *mut u8,
    /// Region flags
    pub flags: PlatformMemoryFlags,
    /// Slot ID (platform-specific)
    pub slot: u32,
}

/// Memory region flags
#[derive(Debug, Clone, Copy, Default)]
pub struct PlatformMemoryFlags {
    /// Region is readable
    pub read: bool,
    /// Region is writable
    pub write: bool,
    /// Region is executable
    pub execute: bool,
    /// Dirty page logging enabled
    pub log_dirty: bool,
    /// Region is ROM (read-only memory)
    pub readonly: bool,
}

impl PlatformMemoryFlags {
    /// Standard RAM flags (read/write)
    pub fn ram() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
            log_dirty: false,
            readonly: false,
        }
    }

    /// ROM flags (read-only)
    pub fn rom() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
            log_dirty: false,
            readonly: true,
        }
    }

    /// Flags for memory with dirty page tracking
    pub fn ram_with_dirty_tracking() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
            log_dirty: true,
            readonly: false,
        }
    }
}

/// Platform statistics
#[derive(Debug, Default)]
pub struct PlatformStats {
    /// Total VM exits
    pub total_exits: AtomicU64,
    /// I/O port exits
    pub io_exits: AtomicU64,
    /// MMIO exits
    pub mmio_exits: AtomicU64,
    /// Interrupt injection count
    pub interrupts_injected: AtomicU64,
    /// Hypercall count
    pub hypercalls: AtomicU64,
    /// EPT violations
    pub ept_violations: AtomicU64,
    /// MSR exits
    pub msr_exits: AtomicU64,
    /// CPUID exits
    pub cpuid_exits: AtomicU64,
}

impl PlatformStats {
    /// Create new statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a VM exit
    pub fn record_exit(&self) {
        self.total_exits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an I/O exit
    pub fn record_io_exit(&self) {
        self.io_exits.fetch_add(1, Ordering::Relaxed);
        self.record_exit();
    }

    /// Record an MMIO exit
    pub fn record_mmio_exit(&self) {
        self.mmio_exits.fetch_add(1, Ordering::Relaxed);
        self.record_exit();
    }

    /// Record an interrupt injection
    pub fn record_interrupt(&self) {
        self.interrupts_injected.fetch_add(1, Ordering::Relaxed);
    }

    /// Get a snapshot of the statistics
    pub fn snapshot(&self) -> PlatformStatsSnapshot {
        PlatformStatsSnapshot {
            total_exits: self.total_exits.load(Ordering::Relaxed),
            io_exits: self.io_exits.load(Ordering::Relaxed),
            mmio_exits: self.mmio_exits.load(Ordering::Relaxed),
            interrupts_injected: self.interrupts_injected.load(Ordering::Relaxed),
            hypercalls: self.hypercalls.load(Ordering::Relaxed),
            ept_violations: self.ept_violations.load(Ordering::Relaxed),
            msr_exits: self.msr_exits.load(Ordering::Relaxed),
            cpuid_exits: self.cpuid_exits.load(Ordering::Relaxed),
        }
    }

    /// Reset all statistics
    pub fn reset(&self) {
        self.total_exits.store(0, Ordering::Relaxed);
        self.io_exits.store(0, Ordering::Relaxed);
        self.mmio_exits.store(0, Ordering::Relaxed);
        self.interrupts_injected.store(0, Ordering::Relaxed);
        self.hypercalls.store(0, Ordering::Relaxed);
        self.ept_violations.store(0, Ordering::Relaxed);
        self.msr_exits.store(0, Ordering::Relaxed);
        self.cpuid_exits.store(0, Ordering::Relaxed);
    }
}

/// Snapshot of platform statistics
#[derive(Debug, Clone, Copy)]
pub struct PlatformStatsSnapshot {
    pub total_exits: u64,
    pub io_exits: u64,
    pub mmio_exits: u64,
    pub interrupts_injected: u64,
    pub hypercalls: u64,
    pub ept_violations: u64,
    pub msr_exits: u64,
    pub cpuid_exits: u64,
}

/// Platform information
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    /// Detected platform
    pub platform: HypervisorPlatform,
    /// Platform version string
    pub version: String,
    /// Available features
    pub features: PlatformFeatures,
    /// Maximum supported vCPUs
    pub max_vcpus: u32,
    /// Maximum supported memory
    pub max_memory: u64,
    /// CPU vendor (Intel, AMD, etc.)
    pub cpu_vendor: CpuVendor,
}

/// CPU vendor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Arm,
    Unknown,
}

impl CpuVendor {
    /// Detect CPU vendor from CPUID
    pub fn detect() -> Self {
        // In a real implementation, this would use CPUID
        // For now, return a default
        #[cfg(target_arch = "x86_64")]
        {
            // Would check CPUID leaf 0 for vendor string
            Self::Intel
        }

        #[cfg(target_arch = "aarch64")]
        {
            Self::Arm
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::Unknown
        }
    }
}

impl PlatformInfo {
    /// Detect platform information
    pub fn detect() -> Self {
        let platform = HypervisorPlatform::detect();
        let cpu_vendor = CpuVendor::detect();

        let features = match cpu_vendor {
            CpuVendor::Intel => PlatformFeatures::intel_vtx(),
            CpuVendor::Amd => PlatformFeatures::amd_v(),
            _ => PlatformFeatures::software_only(),
        };

        Self {
            platform,
            version: Self::get_version_string(platform),
            features,
            max_vcpus: 288,
            max_memory: 4 * 1024 * 1024 * 1024 * 1024, // 4TB
            cpu_vendor,
        }
    }

    fn get_version_string(platform: HypervisorPlatform) -> String {
        match platform {
            HypervisorPlatform::Kvm => "KVM (Linux)".to_string(),
            HypervisorPlatform::Whpx => "WHPX (Windows)".to_string(),
            HypervisorPlatform::Hvf => "HVF (macOS)".to_string(),
            HypervisorPlatform::Tcg => "TCG (Software)".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_features_default() {
        let features = PlatformFeatures::default();
        assert!(!features.hardware_virt);
        assert!(!features.nested_virt);
    }

    #[test]
    fn test_platform_features_intel() {
        let features = PlatformFeatures::intel_vtx();
        assert!(features.hardware_virt);
        assert!(features.extended_page_tables);
        assert!(features.apic_virtualization);
        assert!(features.posted_interrupts);
    }

    #[test]
    fn test_platform_features_amd() {
        let features = PlatformFeatures::amd_v();
        assert!(features.hardware_virt);
        assert!(features.extended_page_tables);
        assert!(features.apic_virtualization);
        assert!(!features.posted_interrupts);
    }

    #[test]
    fn test_hyperv_enlightenments_minimal() {
        let enlightenments = HyperVEnlightenments::minimal();
        assert!(enlightenments.synthetic_timers);
        assert!(enlightenments.reference_tsc);
        assert!(!enlightenments.vapic);
    }

    #[test]
    fn test_hyperv_enlightenments_full() {
        let enlightenments = HyperVEnlightenments::full();
        assert!(enlightenments.synthetic_timers);
        assert!(enlightenments.vapic);
        assert!(enlightenments.tlb_flush);
        assert!(enlightenments.ipi_hypercalls);
    }

    #[test]
    fn test_hyperv_enlightenments_cpuid() {
        let enlightenments = HyperVEnlightenments::full();
        let features = enlightenments.to_cpuid_features();
        let recommendations = enlightenments.to_cpuid_recommendations();

        // VP assist page bit
        assert!(features & (1 << 0) != 0);
        // Synthetic timers bit
        assert!(features & (1 << 3) != 0);
        // Relaxed timing bit
        assert!(recommendations & (1 << 5) != 0);
    }

    #[test]
    fn test_hyperv_privileges() {
        let privileges = HyperVPrivileges::standard();
        assert!(privileges.access_vp_runtime_msr);
        assert!(privileges.access_hypercall_msrs);

        let cpuid = privileges.to_cpuid_eax();
        assert!(cpuid & (1 << 0) != 0); // VP runtime
        assert!(cpuid & (1 << 5) != 0); // Hypercall MSRs
    }

    #[test]
    fn test_platform_vm_config_default() {
        let config = PlatformVmConfig::default();
        assert_eq!(config.vcpu_count, 1);
        assert_eq!(config.memory_size, 128 * 1024 * 1024);
        assert!(config.enable_apicv);
        assert!(config.enable_ept);
    }

    #[test]
    fn test_platform_vm_config_windows() {
        let config = PlatformVmConfig::windows_guest(4, 4096);
        assert_eq!(config.vcpu_count, 4);
        assert_eq!(config.memory_size, 4096 * 1024 * 1024);
        assert!(config.hyperv_enlightenments.is_some());
        assert!(config.hyperv_privileges.is_some());
    }

    #[test]
    fn test_platform_vm_config_linux() {
        let config = PlatformVmConfig::linux_guest(2, 2048);
        assert_eq!(config.vcpu_count, 2);
        assert_eq!(config.memory_size, 2048 * 1024 * 1024);
        assert!(config.hyperv_enlightenments.is_none());
    }

    #[test]
    fn test_platform_vm_builder() {
        let (platform, config) = PlatformVmBuilder::new()
            .vcpus(8)
            .memory_gb(16)
            .with_hyperv_enlightenments()
            .apicv(true)
            .build();

        assert_eq!(config.vcpu_count, 8);
        assert_eq!(config.memory_size, 16 * 1024 * 1024 * 1024);
        assert!(config.hyperv_enlightenments.is_some());
    }

    #[test]
    fn test_platform_vm_builder_without_hyperv() {
        let (_, config) = PlatformVmBuilder::new()
            .vcpus(4)
            .memory_mb(512)
            .without_hyperv()
            .build();

        assert!(config.hyperv_enlightenments.is_none());
        assert!(config.hyperv_privileges.is_none());
    }

    #[test]
    fn test_platform_memory_flags() {
        let ram = PlatformMemoryFlags::ram();
        assert!(ram.read);
        assert!(ram.write);
        assert!(!ram.readonly);

        let rom = PlatformMemoryFlags::rom();
        assert!(rom.read);
        assert!(!rom.write);
        assert!(rom.readonly);
    }

    #[test]
    fn test_platform_stats() {
        let stats = PlatformStats::new();

        stats.record_io_exit();
        stats.record_io_exit();
        stats.record_mmio_exit();
        stats.record_interrupt();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_exits, 3);
        assert_eq!(snapshot.io_exits, 2);
        assert_eq!(snapshot.mmio_exits, 1);
        assert_eq!(snapshot.interrupts_injected, 1);
    }

    #[test]
    fn test_platform_stats_reset() {
        let stats = PlatformStats::new();
        stats.record_io_exit();
        stats.reset();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_exits, 0);
    }

    #[test]
    fn test_cpu_vendor_detect() {
        let vendor = CpuVendor::detect();
        // Should return something valid
        assert!(matches!(
            vendor,
            CpuVendor::Intel | CpuVendor::Amd | CpuVendor::Arm | CpuVendor::Unknown
        ));
    }

    #[test]
    fn test_platform_info_detect() {
        let info = PlatformInfo::detect();
        assert!(!info.version.is_empty());
        assert!(info.max_vcpus > 0);
        assert!(info.max_memory > 0);
    }
}
