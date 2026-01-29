//! CPUID Emulation
//!
//! This module provides CPUID instruction emulation for virtual CPUs.
//! It handles standard and extended CPUID leaves, providing information
//! about processor capabilities, vendor identification, and features.
//!
//! # CPUID Leaves
//!
//! | Leaf (EAX) | Description |
//! |------------|-------------|
//! | 0x00 | Maximum standard leaf + Vendor ID |
//! | 0x01 | Processor info + Feature flags |
//! | 0x02 | Cache and TLB descriptors |
//! | 0x04 | Deterministic cache parameters |
//! | 0x06 | Thermal and power management |
//! | 0x07 | Structured extended feature flags |
//! | 0x0A | Architectural Performance Monitoring |
//! | 0x0D | Processor extended state enumeration |
//! | 0x80000000 | Maximum extended leaf |
//! | 0x80000001 | Extended processor info |
//! | 0x80000002-4 | Processor brand string |
//! | 0x80000008 | Address sizes |

use serde::{Deserialize, Serialize};

/// CPUID result containing EAX, EBX, ECX, EDX values
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuidResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

impl CpuidResult {
    /// Create a new CPUID result
    pub const fn new(eax: u32, ebx: u32, ecx: u32, edx: u32) -> Self {
        Self { eax, ebx, ecx, edx }
    }

    /// Create an empty result (all zeros)
    pub const fn zero() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

/// CPUID feature flags for leaf 0x01 EDX
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuidFeatures1Edx(u32);

impl CpuidFeatures1Edx {
    /// FPU on-chip
    pub const FPU: u32 = 1 << 0;
    /// Virtual 8086 mode enhancements
    pub const VME: u32 = 1 << 1;
    /// Debugging extensions
    pub const DE: u32 = 1 << 2;
    /// Page size extension
    pub const PSE: u32 = 1 << 3;
    /// Time stamp counter
    pub const TSC: u32 = 1 << 4;
    /// Model-specific registers
    pub const MSR: u32 = 1 << 5;
    /// Physical address extension
    pub const PAE: u32 = 1 << 6;
    /// Machine check exception
    pub const MCE: u32 = 1 << 7;
    /// CMPXCHG8 instruction
    pub const CX8: u32 = 1 << 8;
    /// APIC on-chip
    pub const APIC: u32 = 1 << 9;
    /// SYSENTER/SYSEXIT
    pub const SEP: u32 = 1 << 11;
    /// Memory type range registers
    pub const MTRR: u32 = 1 << 12;
    /// Page global enable
    pub const PGE: u32 = 1 << 13;
    /// Machine check architecture
    pub const MCA: u32 = 1 << 14;
    /// CMOV instructions
    pub const CMOV: u32 = 1 << 15;
    /// Page attribute table
    pub const PAT: u32 = 1 << 16;
    /// 36-bit page size extension
    pub const PSE36: u32 = 1 << 17;
    /// Processor serial number
    pub const PSN: u32 = 1 << 18;
    /// CLFLUSH instruction
    pub const CLFSH: u32 = 1 << 19;
    /// Debug store
    pub const DS: u32 = 1 << 21;
    /// ACPI thermal monitor
    pub const ACPI: u32 = 1 << 22;
    /// MMX technology
    pub const MMX: u32 = 1 << 23;
    /// FXSAVE/FXRSTOR
    pub const FXSR: u32 = 1 << 24;
    /// SSE instructions
    pub const SSE: u32 = 1 << 25;
    /// SSE2 instructions
    pub const SSE2: u32 = 1 << 26;
    /// Self snoop
    pub const SS: u32 = 1 << 27;
    /// Hyper-threading technology
    pub const HTT: u32 = 1 << 28;
    /// Thermal monitor
    pub const TM: u32 = 1 << 29;
    /// Pending break enable
    pub const PBE: u32 = 1 << 31;

    /// Create with flags
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Get raw bits
    pub const fn bits(&self) -> u32 {
        self.0
    }
}

/// CPUID feature flags for leaf 0x01 ECX
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuidFeatures1Ecx(u32);

impl CpuidFeatures1Ecx {
    /// SSE3 instructions
    pub const SSE3: u32 = 1 << 0;
    /// PCLMULQDQ instruction
    pub const PCLMULQDQ: u32 = 1 << 1;
    /// 64-bit DS area
    pub const DTES64: u32 = 1 << 2;
    /// MONITOR/MWAIT
    pub const MONITOR: u32 = 1 << 3;
    /// CPL qualified debug store
    pub const DSCPL: u32 = 1 << 4;
    /// Virtual machine extensions
    pub const VMX: u32 = 1 << 5;
    /// Safer mode extensions
    pub const SMX: u32 = 1 << 6;
    /// Enhanced SpeedStep
    pub const EIST: u32 = 1 << 7;
    /// Thermal monitor 2
    pub const TM2: u32 = 1 << 8;
    /// SSSE3 instructions
    pub const SSSE3: u32 = 1 << 9;
    /// L1 context ID
    pub const CNXTID: u32 = 1 << 10;
    /// FMA instructions
    pub const FMA: u32 = 1 << 12;
    /// CMPXCHG16B instruction
    pub const CX16: u32 = 1 << 13;
    /// xTPR update control
    pub const XTPR: u32 = 1 << 14;
    /// Perfmon and debug capability
    pub const PDCM: u32 = 1 << 15;
    /// Process-context identifiers
    pub const PCID: u32 = 1 << 17;
    /// Direct cache access
    pub const DCA: u32 = 1 << 18;
    /// SSE4.1 instructions
    pub const SSE41: u32 = 1 << 19;
    /// SSE4.2 instructions
    pub const SSE42: u32 = 1 << 20;
    /// x2APIC
    pub const X2APIC: u32 = 1 << 21;
    /// MOVBE instruction
    pub const MOVBE: u32 = 1 << 22;
    /// POPCNT instruction
    pub const POPCNT: u32 = 1 << 23;
    /// TSC deadline
    pub const TSCDEADLINE: u32 = 1 << 24;
    /// AESNI instructions
    pub const AESNI: u32 = 1 << 25;
    /// XSAVE/XRSTOR
    pub const XSAVE: u32 = 1 << 26;
    /// OS has enabled XSAVE
    pub const OSXSAVE: u32 = 1 << 27;
    /// AVX instructions
    pub const AVX: u32 = 1 << 28;
    /// 16-bit FP conversion
    pub const F16C: u32 = 1 << 29;
    /// RDRAND instruction
    pub const RDRAND: u32 = 1 << 30;
    /// Hypervisor present
    pub const HYPERVISOR: u32 = 1 << 31;

    /// Create with flags
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Get raw bits
    pub const fn bits(&self) -> u32 {
        self.0
    }
}

/// Extended feature flags for leaf 0x80000001 EDX
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuidExtFeatures1Edx(u32);

impl CpuidExtFeatures1Edx {
    /// SYSCALL/SYSRET
    pub const SYSCALL: u32 = 1 << 11;
    /// No-execute page protection
    pub const NX: u32 = 1 << 20;
    /// 1GB pages
    pub const PDPE1GB: u32 = 1 << 26;
    /// RDTSCP instruction
    pub const RDTSCP: u32 = 1 << 27;
    /// Long mode (64-bit)
    pub const LM: u32 = 1 << 29;

    /// Create with flags
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Get raw bits
    pub const fn bits(&self) -> u32 {
        self.0
    }
}

/// CPUID configuration for a virtual CPU
#[derive(Debug, Clone)]
pub struct CpuidConfig {
    /// Vendor ID string (12 bytes, typically "GenuineIntel" or "AuthenticAMD")
    pub vendor_id: [u8; 12],
    /// Processor brand string (48 bytes)
    pub brand_string: [u8; 48],
    /// Family (4 bits + extended family)
    pub family: u8,
    /// Model (4 bits + extended model)
    pub model: u8,
    /// Stepping (4 bits)
    pub stepping: u8,
    /// Number of logical processors per package
    pub logical_processors: u8,
    /// Initial APIC ID
    pub apic_id: u8,
    /// Feature flags (leaf 1 EDX)
    pub features_edx: u32,
    /// Feature flags (leaf 1 ECX)
    pub features_ecx: u32,
    /// Extended feature flags (leaf 0x80000001 EDX)
    pub ext_features_edx: u32,
    /// Extended feature flags (leaf 0x80000001 ECX)
    pub ext_features_ecx: u32,
    /// Maximum physical address bits
    pub physical_address_bits: u8,
    /// Maximum virtual address bits
    pub virtual_address_bits: u8,
}

impl Default for CpuidConfig {
    fn default() -> Self {
        // Default configuration: a reasonable modern x86-64 processor
        Self {
            vendor_id: *b"AetherVMCPU\0",
            brand_string: {
                let mut brand = [0u8; 48];
                let s = b"AetherVM Virtual CPU @ 2.00GHz";
                brand[..s.len()].copy_from_slice(s);
                brand
            },
            family: 0x06,
            model: 0x3A, // Ivy Bridge-like
            stepping: 0x01,
            logical_processors: 1,
            apic_id: 0,
            // Standard features
            features_edx: CpuidFeatures1Edx::FPU
                | CpuidFeatures1Edx::VME
                | CpuidFeatures1Edx::DE
                | CpuidFeatures1Edx::PSE
                | CpuidFeatures1Edx::TSC
                | CpuidFeatures1Edx::MSR
                | CpuidFeatures1Edx::PAE
                | CpuidFeatures1Edx::MCE
                | CpuidFeatures1Edx::CX8
                | CpuidFeatures1Edx::APIC
                | CpuidFeatures1Edx::SEP
                | CpuidFeatures1Edx::MTRR
                | CpuidFeatures1Edx::PGE
                | CpuidFeatures1Edx::MCA
                | CpuidFeatures1Edx::CMOV
                | CpuidFeatures1Edx::PAT
                | CpuidFeatures1Edx::PSE36
                | CpuidFeatures1Edx::CLFSH
                | CpuidFeatures1Edx::MMX
                | CpuidFeatures1Edx::FXSR
                | CpuidFeatures1Edx::SSE
                | CpuidFeatures1Edx::SSE2,
            features_ecx: CpuidFeatures1Ecx::SSE3
                | CpuidFeatures1Ecx::PCLMULQDQ
                | CpuidFeatures1Ecx::SSSE3
                | CpuidFeatures1Ecx::CX16
                | CpuidFeatures1Ecx::SSE41
                | CpuidFeatures1Ecx::SSE42
                | CpuidFeatures1Ecx::POPCNT
                | CpuidFeatures1Ecx::XSAVE
                | CpuidFeatures1Ecx::HYPERVISOR,
            // Extended features
            ext_features_edx: CpuidExtFeatures1Edx::SYSCALL
                | CpuidExtFeatures1Edx::NX
                | CpuidExtFeatures1Edx::RDTSCP
                | CpuidExtFeatures1Edx::LM,
            ext_features_ecx: 0,
            physical_address_bits: 48,
            virtual_address_bits: 48,
        }
    }
}

impl CpuidConfig {
    /// Create a new CPUID configuration with custom vendor ID
    pub fn with_vendor(mut self, vendor: &[u8; 12]) -> Self {
        self.vendor_id = *vendor;
        self
    }

    /// Set the brand string
    pub fn with_brand(mut self, brand: &str) -> Self {
        self.brand_string = [0u8; 48];
        let bytes = brand.as_bytes();
        let len = bytes.len().min(48);
        self.brand_string[..len].copy_from_slice(&bytes[..len]);
        self
    }

    /// Set logical processor count
    pub fn with_logical_processors(mut self, count: u8) -> Self {
        self.logical_processors = count;
        self
    }

    /// Set APIC ID
    pub fn with_apic_id(mut self, id: u8) -> Self {
        self.apic_id = id;
        self
    }

    /// Enable a feature (leaf 1 EDX)
    pub fn enable_feature_edx(mut self, feature: u32) -> Self {
        self.features_edx |= feature;
        self
    }

    /// Disable a feature (leaf 1 EDX)
    pub fn disable_feature_edx(mut self, feature: u32) -> Self {
        self.features_edx &= !feature;
        self
    }

    /// Enable a feature (leaf 1 ECX)
    pub fn enable_feature_ecx(mut self, feature: u32) -> Self {
        self.features_ecx |= feature;
        self
    }

    /// Disable a feature (leaf 1 ECX)
    pub fn disable_feature_ecx(mut self, feature: u32) -> Self {
        self.features_ecx &= !feature;
        self
    }
}

/// CPUID emulator
#[derive(Debug, Clone)]
pub struct CpuidEmulator {
    config: CpuidConfig,
}

impl CpuidEmulator {
    /// Create a new CPUID emulator with default configuration
    pub fn new() -> Self {
        Self {
            config: CpuidConfig::default(),
        }
    }

    /// Create a new CPUID emulator with custom configuration
    pub fn with_config(config: CpuidConfig) -> Self {
        Self { config }
    }

    /// Get the configuration
    pub fn config(&self) -> &CpuidConfig {
        &self.config
    }

    /// Get mutable configuration
    pub fn config_mut(&mut self) -> &mut CpuidConfig {
        &mut self.config
    }

    /// Execute CPUID instruction
    ///
    /// # Arguments
    /// * `leaf` - EAX input (function number)
    /// * `subleaf` - ECX input (sub-function number, for leaves that use it)
    ///
    /// # Returns
    /// CPUID result with EAX, EBX, ECX, EDX values
    pub fn execute(&self, leaf: u32, subleaf: u32) -> CpuidResult {
        match leaf {
            // Standard leaves
            0x0000_0000 => self.leaf_0(),
            0x0000_0001 => self.leaf_1(),
            0x0000_0002 => self.leaf_2(),
            0x0000_0004 => self.leaf_4(subleaf),
            0x0000_0006 => self.leaf_6(),
            0x0000_0007 => self.leaf_7(subleaf),
            0x0000_000A => self.leaf_a(),
            0x0000_000D => self.leaf_d(subleaf),

            // Hypervisor leaves (0x40000000 - 0x4FFFFFFF)
            0x4000_0000 => self.hypervisor_leaf_0(),
            0x4000_0001 => self.hypervisor_leaf_1(),

            // Extended leaves
            0x8000_0000 => self.ext_leaf_0(),
            0x8000_0001 => self.ext_leaf_1(),
            0x8000_0002 => self.ext_leaf_2(),
            0x8000_0003 => self.ext_leaf_3(),
            0x8000_0004 => self.ext_leaf_4(),
            0x8000_0008 => self.ext_leaf_8(),

            // Unknown leaf - return zeros
            _ => CpuidResult::zero(),
        }
    }

    /// Leaf 0: Maximum standard function + Vendor ID
    fn leaf_0(&self) -> CpuidResult {
        // Vendor ID is split across EBX, EDX, ECX (in that order)
        let ebx = u32::from_le_bytes([
            self.config.vendor_id[0],
            self.config.vendor_id[1],
            self.config.vendor_id[2],
            self.config.vendor_id[3],
        ]);
        let edx = u32::from_le_bytes([
            self.config.vendor_id[4],
            self.config.vendor_id[5],
            self.config.vendor_id[6],
            self.config.vendor_id[7],
        ]);
        let ecx = u32::from_le_bytes([
            self.config.vendor_id[8],
            self.config.vendor_id[9],
            self.config.vendor_id[10],
            self.config.vendor_id[11],
        ]);

        CpuidResult::new(
            0x0D, // Maximum standard leaf supported
            ebx, ecx, edx,
        )
    }

    /// Leaf 1: Processor info and feature flags
    fn leaf_1(&self) -> CpuidResult {
        // EAX: Version information
        // Bits 3:0   - Stepping
        // Bits 7:4   - Model
        // Bits 11:8  - Family
        // Bits 13:12 - Processor Type (00 = OEM)
        // Bits 19:16 - Extended Model
        // Bits 27:20 - Extended Family
        let extended_model = (self.config.model >> 4) & 0xF;
        let base_model = self.config.model & 0xF;
        let extended_family = if self.config.family >= 0x0F {
            self.config.family - 0x0F
        } else {
            0
        };
        let base_family = if self.config.family >= 0x0F {
            0x0F
        } else {
            self.config.family
        };

        let eax = (self.config.stepping as u32)
            | ((base_model as u32) << 4)
            | ((base_family as u32) << 8)
            | ((extended_model as u32) << 16)
            | ((extended_family as u32) << 20);

        // EBX: Additional information
        // Bits 7:0   - Brand Index
        // Bits 15:8  - CLFLUSH line size (in 8-byte units)
        // Bits 23:16 - Maximum addressable IDs for logical processors
        // Bits 31:24 - Initial APIC ID
        let ebx = (8u32 << 8) // 64-byte cache line
            | ((self.config.logical_processors as u32) << 16)
            | ((self.config.apic_id as u32) << 24);

        CpuidResult::new(eax, ebx, self.config.features_ecx, self.config.features_edx)
    }

    /// Leaf 2: Cache and TLB descriptors (simplified)
    fn leaf_2(&self) -> CpuidResult {
        // Return a simplified cache descriptor
        // In reality, this is more complex with descriptor bytes
        CpuidResult::new(0x01, 0, 0, 0)
    }

    /// Leaf 4: Deterministic cache parameters
    fn leaf_4(&self, subleaf: u32) -> CpuidResult {
        match subleaf {
            0 => {
                // L1 Data Cache
                // Type = Data (1), Level = 1
                let eax = 1 | (1 << 5) | (0 << 14) | (0 << 26);
                let ebx = (63 << 0) | (0 << 12) | (7 << 22); // 64-byte line, 8-way
                let ecx = 63; // 64 sets
                CpuidResult::new(eax, ebx, ecx, 0)
            }
            1 => {
                // L1 Instruction Cache
                let eax = 2 | (1 << 5) | (0 << 14) | (0 << 26);
                let ebx = (63 << 0) | (0 << 12) | (7 << 22);
                let ecx = 63;
                CpuidResult::new(eax, ebx, ecx, 0)
            }
            2 => {
                // L2 Unified Cache
                let eax = 3 | (2 << 5) | (0 << 14) | (0 << 26);
                let ebx = (63 << 0) | (0 << 12) | (7 << 22);
                let ecx = 511;
                CpuidResult::new(eax, ebx, ecx, 0)
            }
            _ => CpuidResult::zero(),
        }
    }

    /// Leaf 6: Thermal and power management
    fn leaf_6(&self) -> CpuidResult {
        // Minimal support
        CpuidResult::zero()
    }

    /// Leaf 7: Structured extended feature flags
    fn leaf_7(&self, subleaf: u32) -> CpuidResult {
        if subleaf == 0 {
            // Subleaf 0: Feature flags
            // We report minimal extended features
            CpuidResult::new(
                0, // Max subleaf
                0, // EBX features (FSGSBASE, etc.)
                0, // ECX features
                0, // EDX features
            )
        } else {
            CpuidResult::zero()
        }
    }

    /// Leaf 0x0A: Architectural performance monitoring
    fn leaf_a(&self) -> CpuidResult {
        // No performance monitoring support
        CpuidResult::zero()
    }

    /// Leaf 0x0D: Processor extended state enumeration
    fn leaf_d(&self, subleaf: u32) -> CpuidResult {
        match subleaf {
            0 => {
                // Subleaf 0: Main XSAVE information
                // EAX: Supported XCR0 bits (low)
                // EBX: Maximum size of XSAVE area
                // ECX: Maximum size of XSAVE area (all features)
                // EDX: Supported XCR0 bits (high)
                CpuidResult::new(
                    0x7, // x87, SSE, AVX
                    576, // Size without AVX
                    576, // Size with all features
                    0,
                )
            }
            1 => {
                // Subleaf 1: XSAVE features
                CpuidResult::zero()
            }
            _ => CpuidResult::zero(),
        }
    }

    /// Hypervisor leaf 0: Hypervisor identification
    fn hypervisor_leaf_0(&self) -> CpuidResult {
        // Maximum hypervisor leaf + signature
        // "AetherVMHV" (12 chars)
        let sig = b"AetherVMHV\x00\x00";
        let ebx = u32::from_le_bytes([sig[0], sig[1], sig[2], sig[3]]);
        let ecx = u32::from_le_bytes([sig[4], sig[5], sig[6], sig[7]]);
        let edx = u32::from_le_bytes([sig[8], sig[9], sig[10], sig[11]]);

        CpuidResult::new(0x4000_0001, ebx, ecx, edx)
    }

    /// Hypervisor leaf 1: Hypervisor features
    fn hypervisor_leaf_1(&self) -> CpuidResult {
        // Custom hypervisor features
        CpuidResult::zero()
    }

    /// Extended leaf 0: Maximum extended function
    fn ext_leaf_0(&self) -> CpuidResult {
        CpuidResult::new(0x8000_0008, 0, 0, 0)
    }

    /// Extended leaf 1: Extended processor info and feature flags
    fn ext_leaf_1(&self) -> CpuidResult {
        CpuidResult::new(
            0,
            0,
            self.config.ext_features_ecx,
            self.config.ext_features_edx,
        )
    }

    /// Extended leaf 2: Processor brand string (part 1)
    fn ext_leaf_2(&self) -> CpuidResult {
        let bytes = &self.config.brand_string[0..16];
        CpuidResult::new(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        )
    }

    /// Extended leaf 3: Processor brand string (part 2)
    fn ext_leaf_3(&self) -> CpuidResult {
        let bytes = &self.config.brand_string[16..32];
        CpuidResult::new(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        )
    }

    /// Extended leaf 4: Processor brand string (part 3)
    fn ext_leaf_4(&self) -> CpuidResult {
        let bytes = &self.config.brand_string[32..48];
        CpuidResult::new(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        )
    }

    /// Extended leaf 8: Address sizes
    fn ext_leaf_8(&self) -> CpuidResult {
        // EAX: Physical and virtual address sizes
        // Bits 7:0   - Physical address bits
        // Bits 15:8  - Linear address bits
        let eax = (self.config.physical_address_bits as u32)
            | ((self.config.virtual_address_bits as u32) << 8);

        CpuidResult::new(eax, 0, 0, 0)
    }

    /// Get the vendor ID string
    pub fn vendor_string(&self) -> String {
        String::from_utf8_lossy(&self.config.vendor_id)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Get the brand string
    pub fn brand_string(&self) -> String {
        String::from_utf8_lossy(&self.config.brand_string)
            .trim_end_matches('\0')
            .to_string()
    }
}

impl Default for CpuidEmulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpuid_leaf_0() {
        let cpuid = CpuidEmulator::new();
        let result = cpuid.execute(0, 0);

        // Maximum leaf should be 0x0D
        assert_eq!(result.eax, 0x0D);

        // Vendor string should be "AetherVMCPU\0"
        let vendor = cpuid.vendor_string();
        assert!(vendor.starts_with("AetherVMCPU"));
    }

    #[test]
    fn test_cpuid_leaf_1() {
        let cpuid = CpuidEmulator::new();
        let result = cpuid.execute(1, 0);

        // Check that SSE and SSE2 are enabled
        assert!(result.edx & CpuidFeatures1Edx::SSE != 0);
        assert!(result.edx & CpuidFeatures1Edx::SSE2 != 0);

        // Check hypervisor flag
        assert!(result.ecx & CpuidFeatures1Ecx::HYPERVISOR != 0);
    }

    #[test]
    fn test_cpuid_extended_leaf_1() {
        let cpuid = CpuidEmulator::new();
        let result = cpuid.execute(0x8000_0001, 0);

        // Long mode should be enabled
        assert!(result.edx & CpuidExtFeatures1Edx::LM != 0);

        // NX should be enabled
        assert!(result.edx & CpuidExtFeatures1Edx::NX != 0);
    }

    #[test]
    fn test_cpuid_brand_string() {
        let cpuid = CpuidEmulator::new();

        // Brand string is split across leaves 0x80000002-0x80000004
        let result2 = cpuid.execute(0x8000_0002, 0);
        let result3 = cpuid.execute(0x8000_0003, 0);
        let result4 = cpuid.execute(0x8000_0004, 0);

        // Reconstruct the brand string
        let mut brand = Vec::with_capacity(48);
        for result in [result2, result3, result4] {
            brand.extend_from_slice(&result.eax.to_le_bytes());
            brand.extend_from_slice(&result.ebx.to_le_bytes());
            brand.extend_from_slice(&result.ecx.to_le_bytes());
            brand.extend_from_slice(&result.edx.to_le_bytes());
        }

        let brand_str = String::from_utf8_lossy(&brand)
            .trim_end_matches('\0')
            .to_string();
        assert!(brand_str.contains("AetherVM"));
    }

    #[test]
    fn test_cpuid_hypervisor_leaf() {
        let cpuid = CpuidEmulator::new();
        let result = cpuid.execute(0x4000_0000, 0);

        // Maximum hypervisor leaf
        assert_eq!(result.eax, 0x4000_0001);

        // Signature should be "AetherVMHV"
        let mut sig = Vec::new();
        sig.extend_from_slice(&result.ebx.to_le_bytes());
        sig.extend_from_slice(&result.ecx.to_le_bytes());
        sig.extend_from_slice(&result.edx.to_le_bytes());
        let sig_str = String::from_utf8_lossy(&sig);
        assert!(sig_str.starts_with("AetherVMHV"));
    }

    #[test]
    fn test_cpuid_address_sizes() {
        let cpuid = CpuidEmulator::new();
        let result = cpuid.execute(0x8000_0008, 0);

        // Physical address bits
        let phys_bits = result.eax & 0xFF;
        assert_eq!(phys_bits, 48);

        // Virtual address bits
        let virt_bits = (result.eax >> 8) & 0xFF;
        assert_eq!(virt_bits, 48);
    }

    #[test]
    fn test_cpuid_unknown_leaf() {
        let cpuid = CpuidEmulator::new();
        let result = cpuid.execute(0xFFFF_FFFF, 0);

        // Unknown leaf should return zeros
        assert_eq!(result.eax, 0);
        assert_eq!(result.ebx, 0);
        assert_eq!(result.ecx, 0);
        assert_eq!(result.edx, 0);
    }

    #[test]
    fn test_cpuid_custom_config() {
        let config = CpuidConfig::default()
            .with_brand("Custom CPU @ 3.00GHz")
            .with_logical_processors(4)
            .with_apic_id(2);

        let cpuid = CpuidEmulator::with_config(config);

        // Check brand string
        assert!(cpuid.brand_string().contains("Custom CPU"));

        // Check logical processors (in leaf 1 EBX)
        let result = cpuid.execute(1, 0);
        let logical_cpus = ((result.ebx >> 16) & 0xFF) as u8;
        assert_eq!(logical_cpus, 4);

        // Check APIC ID
        let apic_id = ((result.ebx >> 24) & 0xFF) as u8;
        assert_eq!(apic_id, 2);
    }

    #[test]
    fn test_cpuid_feature_toggle() {
        let config = CpuidConfig::default()
            .disable_feature_edx(CpuidFeatures1Edx::SSE)
            .enable_feature_ecx(CpuidFeatures1Ecx::AVX);

        let cpuid = CpuidEmulator::with_config(config);
        let result = cpuid.execute(1, 0);

        // SSE should be disabled
        assert!(result.edx & CpuidFeatures1Edx::SSE == 0);

        // AVX should be enabled
        assert!(result.ecx & CpuidFeatures1Ecx::AVX != 0);
    }

    #[test]
    fn test_cpuid_cache_info() {
        let cpuid = CpuidEmulator::new();

        // L1 data cache
        let result = cpuid.execute(4, 0);
        assert_ne!(result.eax, 0); // Should report cache

        // L1 instruction cache
        let result = cpuid.execute(4, 1);
        assert_ne!(result.eax, 0);

        // L2 cache
        let result = cpuid.execute(4, 2);
        assert_ne!(result.eax, 0);

        // No L3 cache (subleaf 3)
        let result = cpuid.execute(4, 3);
        assert_eq!(result.eax, 0);
    }
}
