//! ARM64 EL2 (Exception Level 2) hypervisor initialization and trap handling
//!
//! This module manages:
//! - EL2 entry and configuration
//! - HCR_EL2 (Hypervisor Configuration Register) setup
//! - Exception vector table for EL2
//! - VM entry/exit (ERET to EL1, trap back to EL2)

use crate::{Error, Result};
use core::sync::atomic::{AtomicBool, Ordering};

use bitflags::bitflags;

/// Whether EL2 has been initialized
static EL2_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// HCR_EL2 — Hypervisor Configuration Register
// ---------------------------------------------------------------------------

bitflags! {
    /// HCR_EL2 bit definitions (ARMv8-A Reference Manual D13.2.46)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct HcrEl2: u64 {
        /// Trap WFI from EL1
        const TWI   = 1 << 13;
        /// Trap WFE from EL1
        const TWE   = 1 << 14;
        /// Trap ID-group registers
        const TID3  = 1 << 18;
        /// Trap SMC from EL1
        const TSC   = 1 << 19;
        /// Trap access to implementation-defined functionality
        const TACR  = 1 << 21;
        /// Trap auxiliary control registers
        const TIDCP = 1 << 20;
        /// Route physical SError to EL2
        const AMO   = 1 << 5;
        /// Route physical IRQ to EL2
        const IMO   = 1 << 4;
        /// Route physical FIQ to EL2
        const FMO   = 1 << 3;
        /// Second stage translation enable
        const VM    = 1 << 0;
        /// Set/Way invalidation override
        const SWIO  = 1 << 1;
        /// Protected Table Walk
        const PTW   = 1 << 2;
        /// Route synchronous external aborts to EL2
        const RW    = 1 << 31;
        /// Trap general exceptions from EL1
        const TGE   = 1 << 27;
        /// Trap DC ZVA instruction from EL1
        const TDZ   = 1 << 28;
        /// EL1 AArch64 execution state (when set, EL1 is AArch64)
        const E2H   = 1 << 34;
    }
}

impl HcrEl2 {
    /// Default hypervisor configuration:
    /// - Stage-2 translation enabled (VM)
    /// - EL1 runs AArch64 (RW)
    /// - Route physical interrupts to EL2 (IMO, FMO, AMO)
    /// - Trap SMC to EL2 (TSC)
    pub fn hypervisor_default() -> Self {
        Self::VM | Self::RW | Self::IMO | Self::FMO | Self::AMO | Self::TSC | Self::SWIO
    }
}

// ---------------------------------------------------------------------------
// VTTBR_EL2 — Virtualization Translation Table Base Register
// ---------------------------------------------------------------------------

/// VTTBR_EL2 value combining a VMID with the stage-2 table base address.
///
/// Layout (IPA size ≤ 48 bits):
///   - Bits [47:1]  — BADDR: stage-2 translation table base (page-aligned)
///   - Bits [63:48] — VMID (up to 16 bits when VMID16EL2 is supported)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VttbrEl2(u64);

impl VttbrEl2 {
    /// Construct a VTTBR from a VMID and page-table base physical address.
    ///
    /// `base_addr` must be page-aligned (bits [11:0] == 0).
    pub fn new(vmid: u16, base_addr: u64) -> Result<Self> {
        if base_addr & 0xFFF != 0 {
            return Err(Error::InvalidParameter);
        }
        let val = ((vmid as u64) << 48) | (base_addr & 0x0000_FFFF_FFFF_F000);
        Ok(Self(val))
    }

    /// Raw 64-bit value for writing to the register.
    pub fn raw(&self) -> u64 {
        self.0
    }

    /// Extract the VMID field.
    pub fn vmid(&self) -> u16 {
        (self.0 >> 48) as u16
    }

    /// Extract the base address field.
    pub fn base_addr(&self) -> u64 {
        self.0 & 0x0000_FFFF_FFFF_F000
    }
}

// ---------------------------------------------------------------------------
// Exception class (ESR_EL2.EC) — reason the guest trapped to EL2
// ---------------------------------------------------------------------------

/// Exception class from ESR_EL2 bits [31:26]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionClass {
    /// Unknown or uncategorised
    Unknown = 0x00,
    /// Trapped WFI/WFE
    WfxTrap = 0x01,
    /// Trapped MCR/MRC to CP15
    Cp15Trap = 0x03,
    /// Trapped MCRR/MRRC to CP15
    Cp15RrTrap = 0x04,
    /// Trapped MCR/MRC to CP14
    Cp14Trap = 0x05,
    /// Trapped LDC/STC to CP14
    Cp14LsTrap = 0x06,
    /// Trapped SIMD/FP access
    SimdFpTrap = 0x07,
    /// Trapped MSR/MRS to system register (AArch64)
    SysregTrap = 0x18,
    /// Instruction abort from lower EL
    InstrAbortLowerEl = 0x20,
    /// Instruction abort from current EL
    InstrAbortCurrentEl = 0x21,
    /// Data abort from lower EL
    DataAbortLowerEl = 0x24,
    /// Data abort from current EL
    DataAbortCurrentEl = 0x25,
    /// HVC (Hypervisor Call) from AArch64
    Hvc64 = 0x16,
    /// SMC (Secure Monitor Call) from AArch64
    Smc64 = 0x17,
    /// SError interrupt
    SError = 0x2F,
    /// Breakpoint from lower EL
    BreakpointLowerEl = 0x30,
    /// Software step from lower EL
    SoftwareStepLowerEl = 0x32,
}

impl ExceptionClass {
    /// Decode from the raw EC field (6 bits).
    pub fn from_ec(ec: u8) -> Self {
        match ec {
            0x00 => Self::Unknown,
            0x01 => Self::WfxTrap,
            0x03 => Self::Cp15Trap,
            0x04 => Self::Cp15RrTrap,
            0x05 => Self::Cp14Trap,
            0x06 => Self::Cp14LsTrap,
            0x07 => Self::SimdFpTrap,
            0x18 => Self::SysregTrap,
            0x20 => Self::InstrAbortLowerEl,
            0x21 => Self::InstrAbortCurrentEl,
            0x24 => Self::DataAbortLowerEl,
            0x25 => Self::DataAbortCurrentEl,
            0x16 => Self::Hvc64,
            0x17 => Self::Smc64,
            0x2F => Self::SError,
            0x30 => Self::BreakpointLowerEl,
            0x32 => Self::SoftwareStepLowerEl,
            _ => Self::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// EL2 trap reason — high-level VM exit descriptor
// ---------------------------------------------------------------------------

/// Describes why the guest VM trapped to EL2.
#[derive(Debug, Clone, Copy)]
pub enum TrapReason {
    /// System register access (MSR/MRS)
    SystemRegisterAccess {
        /// ISS encoding for the trapped register
        iss: u32,
        /// True if this was a write (MSR), false for read (MRS)
        is_write: bool,
    },
    /// HVC (Hypervisor Call) from guest
    HypervisorCall {
        /// Immediate value from HVC instruction
        imm: u16,
    },
    /// SMC (Secure Monitor Call) — trapped because HCR_EL2.TSC == 1
    SecureMonitorCall {
        /// Immediate value from SMC instruction
        imm: u16,
    },
    /// WFI/WFE instruction trapped
    WaitForEvent {
        /// True for WFE, false for WFI
        is_wfe: bool,
    },
    /// Instruction abort from guest
    InstructionAbort {
        /// Faulting IPA
        ipa: u64,
        /// DFSC (Data Fault Status Code)
        fault_code: u8,
    },
    /// Data abort from guest (e.g. MMIO access)
    DataAbort {
        /// Faulting IPA
        ipa: u64,
        /// True if the access was a write
        is_write: bool,
        /// Access size in bytes (1, 2, 4, or 8)
        access_size: u8,
        /// Sign-extend the value
        sign_extend: bool,
        /// Destination/source register index (Xt)
        srt: u8,
        /// DFSC
        fault_code: u8,
    },
    /// IRQ routed to EL2 (IMO, FMO, AMO)
    Interrupt,
    /// SError routed to EL2
    SError {
        /// Implementation-defined syndrome
        iss: u32,
    },
    /// Unknown or unhandled exception class
    Unknown {
        /// Raw ESR_EL2 value
        esr: u64,
    },
}

/// Decode a raw ESR_EL2 value into a [`TrapReason`].
pub fn decode_trap(esr: u64) -> TrapReason {
    let ec = ((esr >> 26) & 0x3F) as u8;
    let iss = (esr & 0x01FF_FFFF) as u32;

    match ExceptionClass::from_ec(ec) {
        ExceptionClass::SysregTrap => TrapReason::SystemRegisterAccess {
            iss,
            is_write: (iss & 1) == 0, // Direction bit: 0 = write (MSR), 1 = read (MRS)
        },
        ExceptionClass::Hvc64 => TrapReason::HypervisorCall {
            imm: (iss & 0xFFFF) as u16,
        },
        ExceptionClass::Smc64 => TrapReason::SecureMonitorCall {
            imm: (iss & 0xFFFF) as u16,
        },
        ExceptionClass::WfxTrap => TrapReason::WaitForEvent {
            is_wfe: (iss & 1) != 0,
        },
        ExceptionClass::InstrAbortLowerEl => TrapReason::InstructionAbort {
            ipa: 0, // caller should read HPFAR_EL2 << 8
            fault_code: (iss & 0x3F) as u8,
        },
        ExceptionClass::DataAbortLowerEl => {
            let is_write = (iss & (1 << 6)) != 0;
            let sas = ((iss >> 22) & 0x3) as u8;
            let access_size = 1u8 << sas;
            let sign_extend = (iss & (1 << 21)) != 0;
            let srt = ((iss >> 16) & 0x1F) as u8;
            TrapReason::DataAbort {
                ipa: 0, // caller should read HPFAR_EL2 << 8
                is_write,
                access_size,
                sign_extend,
                srt,
                fault_code: (iss & 0x3F) as u8,
            }
        }
        ExceptionClass::SError => TrapReason::SError { iss },
        _ => TrapReason::Unknown { esr },
    }
}

// ---------------------------------------------------------------------------
// EL2 initialization state
// ---------------------------------------------------------------------------

/// Check whether EL2 has been initialized.
pub fn is_initialized() -> bool {
    EL2_INITIALIZED.load(Ordering::Acquire)
}

/// Configuration for EL2 initialization.
#[derive(Debug, Clone)]
pub struct El2Config {
    /// HCR_EL2 flags to program
    pub hcr: HcrEl2,
    /// Number of VMID bits supported (8 or 16)
    pub vmid_bits: u8,
}

impl Default for El2Config {
    fn default() -> Self {
        Self {
            hcr: HcrEl2::hypervisor_default(),
            vmid_bits: 8,
        }
    }
}

/// Initialize EL2 hypervisor mode.
///
/// This must be called once from the primary CPU before creating any VMs.
/// On real hardware this writes HCR_EL2, VTCR_EL2, and installs the EL2
/// exception vector table.  In the current portable skeleton it validates
/// the configuration and records initialization state.
pub fn initialize(config: &El2Config) -> Result<()> {
    if EL2_INITIALIZED.load(Ordering::Acquire) {
        return Err(Error::AlreadyInitialized);
    }

    if config.vmid_bits != 8 && config.vmid_bits != 16 {
        return Err(Error::InvalidParameter);
    }

    // On real hardware the following steps would occur here:
    // 1. Verify CurrentEL == EL2
    // 2. Write HCR_EL2 with config.hcr
    // 3. Configure VTCR_EL2 for stage-2 translation (4KB granule, 40-bit IPA)
    // 4. Write VBAR_EL2 to point at our exception vector table
    // 5. Set CPTR_EL2 to not trap SIMD/FP
    // 6. Barrier (isb) to synchronise context

    EL2_INITIALIZED.store(true, Ordering::Release);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    // -- HCR_EL2 tests ---

    #[test]
    fn hcr_default_enables_stage2() {
        let hcr = HcrEl2::hypervisor_default();
        assert!(hcr.contains(HcrEl2::VM));
    }

    #[test]
    fn hcr_default_routes_interrupts() {
        let hcr = HcrEl2::hypervisor_default();
        assert!(hcr.contains(HcrEl2::IMO));
        assert!(hcr.contains(HcrEl2::FMO));
        assert!(hcr.contains(HcrEl2::AMO));
    }

    #[test]
    fn hcr_default_traps_smc() {
        let hcr = HcrEl2::hypervisor_default();
        assert!(hcr.contains(HcrEl2::TSC));
    }

    #[test]
    fn hcr_default_sets_rw() {
        let hcr = HcrEl2::hypervisor_default();
        assert!(hcr.contains(HcrEl2::RW));
    }

    // -- VTTBR_EL2 tests ---

    #[test]
    fn vttbr_round_trip() {
        let vttbr = VttbrEl2::new(42, 0x0000_0001_0000_0000).unwrap();
        assert_eq!(vttbr.vmid(), 42);
        assert_eq!(vttbr.base_addr(), 0x0000_0001_0000_0000);
    }

    #[test]
    fn vttbr_rejects_unaligned_base() {
        let result = VttbrEl2::new(1, 0x1001);
        assert_eq!(result, Err(Error::InvalidParameter));
    }

    #[test]
    fn vttbr_zero_vmid() {
        let vttbr = VttbrEl2::new(0, 0x4000).unwrap();
        assert_eq!(vttbr.vmid(), 0);
        assert_eq!(vttbr.base_addr(), 0x4000);
    }

    // -- ExceptionClass decode tests ---

    #[test]
    fn exception_class_sysreg() {
        assert_eq!(ExceptionClass::from_ec(0x18), ExceptionClass::SysregTrap);
    }

    #[test]
    fn exception_class_data_abort() {
        assert_eq!(
            ExceptionClass::from_ec(0x24),
            ExceptionClass::DataAbortLowerEl
        );
    }

    #[test]
    fn exception_class_unknown_maps_fallback() {
        assert_eq!(ExceptionClass::from_ec(0xFF), ExceptionClass::Unknown);
    }

    // -- decode_trap tests ---

    #[test]
    fn decode_hvc_trap() {
        // EC = 0x16 (HVC64), ISS = 0x42
        let esr: u64 = (0x16u64 << 26) | 0x42;
        match decode_trap(esr) {
            TrapReason::HypervisorCall { imm } => assert_eq!(imm, 0x42),
            other => panic!("expected HypervisorCall, got {:?}", other),
        }
    }

    #[test]
    fn decode_smc_trap() {
        let esr: u64 = (0x17u64 << 26) | 0x01;
        match decode_trap(esr) {
            TrapReason::SecureMonitorCall { imm } => assert_eq!(imm, 1),
            other => panic!("expected SecureMonitorCall, got {:?}", other),
        }
    }

    #[test]
    fn decode_wfi_trap() {
        // EC = 0x01 (WFx), ISS bit[0] = 0 → WFI
        let esr: u64 = (0x01u64 << 26) | 0x00;
        match decode_trap(esr) {
            TrapReason::WaitForEvent { is_wfe } => assert!(!is_wfe),
            other => panic!("expected WaitForEvent, got {:?}", other),
        }
    }

    #[test]
    fn decode_wfe_trap() {
        let esr: u64 = (0x01u64 << 26) | 0x01;
        match decode_trap(esr) {
            TrapReason::WaitForEvent { is_wfe } => assert!(is_wfe),
            other => panic!("expected WaitForEvent, got {:?}", other),
        }
    }

    #[test]
    fn decode_sysreg_write_trap() {
        // EC = 0x18, ISS bit[0] = 0 → write (MSR)
        let esr: u64 = (0x18u64 << 26) | 0x100;
        match decode_trap(esr) {
            TrapReason::SystemRegisterAccess { is_write, .. } => assert!(is_write),
            other => panic!("expected SystemRegisterAccess, got {:?}", other),
        }
    }

    #[test]
    fn decode_data_abort_write() {
        // EC = 0x24, ISS bit[6] = 1 → write
        let esr: u64 = (0x24u64 << 26) | (1 << 6);
        match decode_trap(esr) {
            TrapReason::DataAbort { is_write, .. } => assert!(is_write),
            other => panic!("expected DataAbort, got {:?}", other),
        }
    }

    #[test]
    fn decode_unknown_ec() {
        let esr: u64 = (0x3Fu64 << 26) | 0xAB;
        match decode_trap(esr) {
            TrapReason::Unknown { esr: raw } => assert_eq!(raw, esr),
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    // -- El2Config + initialize tests ---

    #[test]
    fn el2_config_default() {
        let cfg = El2Config::default();
        assert_eq!(cfg.vmid_bits, 8);
        assert!(cfg.hcr.contains(HcrEl2::VM));
    }

    #[test]
    fn initialize_rejects_invalid_vmid_bits() {
        // Reset for test isolation
        EL2_INITIALIZED.store(false, Ordering::Release);
        let cfg = El2Config {
            vmid_bits: 4,
            ..Default::default()
        };
        assert_eq!(initialize(&cfg), Err(Error::InvalidParameter));
    }

    #[test]
    fn initialize_succeeds_once() {
        EL2_INITIALIZED.store(false, Ordering::Release);
        let cfg = El2Config::default();
        assert!(initialize(&cfg).is_ok());
        assert!(is_initialized());
        // second call fails
        assert_eq!(initialize(&cfg), Err(Error::AlreadyInitialized));
        // reset for other tests
        EL2_INITIALIZED.store(false, Ordering::Release);
    }
}
