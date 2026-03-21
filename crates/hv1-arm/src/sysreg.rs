//! ARM64 system register trapping and emulation
//!
//! When HCR_EL2 is configured to trap certain system register accesses
//! from EL1, the CPU raises a synchronous exception to EL2 with
//! EC = 0x18 (MSR/MRS trap).  This module decodes the trapped register
//! from the ISS field and provides emulation stubs.
//!
//! # ISS encoding for system register traps (EC 0x18)
//!
//! ```text
//! ISS [24:20] = Op0
//! ISS [19:16] = Op1
//! ISS [15:12] = CRn
//! ISS [11:8]  = CRm
//! ISS [7:5]   = Op2
//! ISS [4:1]   = Rt  (source/dest register index)
//! ISS [0]     = Direction: 0 = write (MSR), 1 = read (MRS)
//! ```

use crate::vcpu::Vcpu;
use crate::{Error, Result};

/// Decoded system register ID (Op0, Op1, CRn, CRm, Op2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SysregId {
    pub op0: u8,
    pub op1: u8,
    pub crn: u8,
    pub crm: u8,
    pub op2: u8,
}

impl SysregId {
    /// Construct from individual fields.
    pub const fn new(op0: u8, op1: u8, crn: u8, crm: u8, op2: u8) -> Self {
        Self {
            op0,
            op1,
            crn,
            crm,
            op2,
        }
    }

    /// Decode from the ISS bits of ESR_EL2.
    pub fn from_iss(iss: u32) -> Self {
        Self {
            op0: ((iss >> 20) & 0x3) as u8,
            op1: ((iss >> 14) & 0x7) as u8,
            crn: ((iss >> 10) & 0xF) as u8,
            crm: ((iss >> 1) & 0xF) as u8,
            op2: ((iss >> 17) & 0x7) as u8,
        }
    }

    /// Pack back to a 32-bit key for fast comparison.
    pub fn as_u32(&self) -> u32 {
        ((self.op0 as u32) << 20)
            | ((self.op2 as u32) << 17)
            | ((self.op1 as u32) << 14)
            | ((self.crn as u32) << 10)
            | ((self.crm as u32) << 1)
    }
}

// Well-known system register IDs
// S<op0>_<op1>_<CRn>_<CRm>_<op2>

/// SCTLR_EL1 — System Control Register
pub const SCTLR_EL1: SysregId = SysregId::new(3, 0, 1, 0, 0);
/// TTBR0_EL1 — Translation Table Base Register 0
pub const TTBR0_EL1: SysregId = SysregId::new(3, 0, 2, 0, 0);
/// TTBR1_EL1 — Translation Table Base Register 1
pub const TTBR1_EL1: SysregId = SysregId::new(3, 0, 2, 0, 1);
/// TCR_EL1 — Translation Control Register
pub const TCR_EL1: SysregId = SysregId::new(3, 0, 2, 0, 2);
/// MAIR_EL1 — Memory Attribute Indirection Register
pub const MAIR_EL1: SysregId = SysregId::new(3, 0, 10, 2, 0);
/// VBAR_EL1 — Vector Base Address Register
pub const VBAR_EL1: SysregId = SysregId::new(3, 0, 12, 0, 0);
/// CONTEXTIDR_EL1 — Context ID Register
pub const CONTEXTIDR_EL1: SysregId = SysregId::new(3, 0, 13, 0, 1);
/// CNTV_CTL_EL0 — Counter-timer Virtual Timer Control
pub const CNTV_CTL_EL0: SysregId = SysregId::new(3, 3, 14, 3, 1);
/// CNTV_CVAL_EL0 — Counter-timer Virtual Timer Compare Value
pub const CNTV_CVAL_EL0: SysregId = SysregId::new(3, 3, 14, 3, 2);
/// CNTP_CTL_EL0 — Counter-timer Physical Timer Control
pub const CNTP_CTL_EL0: SysregId = SysregId::new(3, 3, 14, 2, 1);
/// CNTP_CVAL_EL0 — Counter-timer Physical Timer Compare Value
pub const CNTP_CVAL_EL0: SysregId = SysregId::new(3, 3, 14, 2, 2);
/// MIDR_EL1 — Main ID Register (read-only)
pub const MIDR_EL1: SysregId = SysregId::new(3, 0, 0, 0, 0);
/// MPIDR_EL1 — Multiprocessor Affinity Register (read-only)
pub const MPIDR_EL1: SysregId = SysregId::new(3, 0, 0, 0, 5);

/// Result of emulating a system register access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationResult {
    /// Handled; advance guest PC.
    Handled,
    /// The register was not recognised; caller should inject an undefined
    /// exception into the guest.
    Unhandled,
}

/// Decoded system register trap from ESR_EL2 ISS.
#[derive(Debug, Clone, Copy)]
pub struct SysregTrap {
    /// The trapped register
    pub reg: SysregId,
    /// General-purpose register index (Xt) — 0..30 or 31 (XZR)
    pub rt: u8,
    /// True for MSR (write), false for MRS (read)
    pub is_write: bool,
}

impl SysregTrap {
    /// Decode from the raw ISS value (25 bits from ESR_EL2).
    pub fn from_iss(iss: u32) -> Self {
        let direction = iss & 1;
        let rt = ((iss >> 5) & 0x1F) as u8;
        Self {
            reg: SysregId::from_iss(iss),
            rt,
            is_write: direction == 0,
        }
    }
}

/// Emulate a trapped system register access against a vCPU.
///
/// For reads (MRS), the emulated value is written to `vcpu.gpr[trap.rt]`.
/// For writes (MSR), the value is read from `vcpu.gpr[trap.rt]` and stored
/// in the vCPU's system register context.
pub fn emulate_sysreg(vcpu: &mut Vcpu, trap: &SysregTrap) -> Result<EmulationResult> {
    let reg_key = trap.reg.as_u32();

    // XZR (register 31) reads as zero, writes are discarded
    let read_rt = |vcpu: &Vcpu, rt: u8| -> Result<u64> {
        if rt == 31 {
            Ok(0)
        } else {
            vcpu.read_gpr(rt)
        }
    };

    let write_rt = |vcpu: &mut Vcpu, rt: u8, val: u64| -> Result<()> {
        if rt == 31 {
            Ok(()) // writes to XZR are discarded
        } else {
            vcpu.write_gpr(rt, val)
        }
    };

    if trap.is_write {
        // MSR — guest writes to system register
        let value = read_rt(vcpu, trap.rt)?;
        match_sysreg_write(vcpu, &trap.reg, value)
    } else {
        // MRS — guest reads from system register
        match match_sysreg_read(vcpu, &trap.reg)? {
            Some(value) => {
                write_rt(vcpu, trap.rt, value)?;
                Ok(EmulationResult::Handled)
            }
            None => Ok(EmulationResult::Unhandled),
        }
    }
}

/// Handle a system register write (MSR).
fn match_sysreg_write(vcpu: &mut Vcpu, reg: &SysregId, value: u64) -> Result<EmulationResult> {
    if *reg == SCTLR_EL1 {
        vcpu.sysregs.sctlr_el1 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == TTBR0_EL1 {
        vcpu.sysregs.ttbr0_el1 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == TTBR1_EL1 {
        vcpu.sysregs.ttbr1_el1 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == TCR_EL1 {
        vcpu.sysregs.tcr_el1 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == MAIR_EL1 {
        vcpu.sysregs.mair_el1 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == VBAR_EL1 {
        vcpu.sysregs.vbar_el1 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == CONTEXTIDR_EL1 {
        vcpu.sysregs.contextidr_el1 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == CNTV_CTL_EL0 {
        vcpu.sysregs.cntv_ctl_el0 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == CNTV_CVAL_EL0 {
        vcpu.sysregs.cntv_cval_el0 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == CNTP_CTL_EL0 {
        vcpu.sysregs.cntp_ctl_el0 = value;
        Ok(EmulationResult::Handled)
    } else if *reg == CNTP_CVAL_EL0 {
        vcpu.sysregs.cntp_cval_el0 = value;
        Ok(EmulationResult::Handled)
    } else {
        Ok(EmulationResult::Unhandled)
    }
}

/// Handle a system register read (MRS).  Returns `Some(value)` if handled.
fn match_sysreg_read(vcpu: &Vcpu, reg: &SysregId) -> Result<Option<u64>> {
    if *reg == SCTLR_EL1 {
        Ok(Some(vcpu.sysregs.sctlr_el1))
    } else if *reg == TTBR0_EL1 {
        Ok(Some(vcpu.sysregs.ttbr0_el1))
    } else if *reg == TTBR1_EL1 {
        Ok(Some(vcpu.sysregs.ttbr1_el1))
    } else if *reg == TCR_EL1 {
        Ok(Some(vcpu.sysregs.tcr_el1))
    } else if *reg == MAIR_EL1 {
        Ok(Some(vcpu.sysregs.mair_el1))
    } else if *reg == VBAR_EL1 {
        Ok(Some(vcpu.sysregs.vbar_el1))
    } else if *reg == CONTEXTIDR_EL1 {
        Ok(Some(vcpu.sysregs.contextidr_el1))
    } else if *reg == CNTV_CTL_EL0 {
        Ok(Some(vcpu.sysregs.cntv_ctl_el0))
    } else if *reg == CNTV_CVAL_EL0 {
        Ok(Some(vcpu.sysregs.cntv_cval_el0))
    } else if *reg == CNTP_CTL_EL0 {
        Ok(Some(vcpu.sysregs.cntp_ctl_el0))
    } else if *reg == CNTP_CVAL_EL0 {
        Ok(Some(vcpu.sysregs.cntp_cval_el0))
    } else if *reg == MIDR_EL1 {
        // Return a synthetic MIDR: implementer 0x41 (ARM), variant 0, part 0xD08 (Cortex-A72)
        Ok(Some(0x410F_D080))
    } else if *reg == MPIDR_EL1 {
        // Return affinity based on vCPU ID
        Ok(Some(vcpu.id() as u64 | (1u64 << 31))) // Bit 31 = RES1
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcpu::Vcpu;

    fn make_vcpu() -> Vcpu {
        let mut vcpu = Vcpu::new(1);
        vcpu.initialize(0x8_0000, 0x10_0000).unwrap();
        vcpu
    }

    // -- SysregId tests ---

    #[test]
    fn sysreg_id_round_trip() {
        let id = SysregId::new(3, 0, 1, 0, 0);
        let packed = id.as_u32();
        let decoded = SysregId::from_iss(packed);
        assert_eq!(decoded.op0, id.op0);
        assert_eq!(decoded.crn, id.crn);
    }

    #[test]
    fn sysreg_well_known_sctlr() {
        assert_eq!(SCTLR_EL1.op0, 3);
        assert_eq!(SCTLR_EL1.crn, 1);
        assert_eq!(SCTLR_EL1.crm, 0);
        assert_eq!(SCTLR_EL1.op1, 0);
        assert_eq!(SCTLR_EL1.op2, 0);
    }

    // -- SysregTrap decode tests ---

    #[test]
    fn sysreg_trap_write_direction() {
        // ISS bit[0] = 0 → MSR (write)
        let trap = SysregTrap::from_iss(0x00);
        assert!(trap.is_write);
    }

    #[test]
    fn sysreg_trap_read_direction() {
        // ISS bit[0] = 1 → MRS (read)
        let trap = SysregTrap::from_iss(0x01);
        assert!(!trap.is_write);
    }

    #[test]
    fn sysreg_trap_rt_decoding() {
        // Rt is in ISS[9:5], set Rt = 7
        let iss = 7u32 << 5;
        let trap = SysregTrap::from_iss(iss);
        assert_eq!(trap.rt, 7);
    }

    // -- Emulation tests ---

    #[test]
    fn emulate_write_sctlr() {
        let mut vcpu = make_vcpu();
        vcpu.write_gpr(5, 0xDEAD_BEEF).unwrap();

        let trap = SysregTrap {
            reg: SCTLR_EL1,
            rt: 5,
            is_write: true,
        };
        let result = emulate_sysreg(&mut vcpu, &trap).unwrap();
        assert_eq!(result, EmulationResult::Handled);
        assert_eq!(vcpu.sysregs.sctlr_el1, 0xDEAD_BEEF);
    }

    #[test]
    fn emulate_read_sctlr() {
        let mut vcpu = make_vcpu();
        vcpu.sysregs.sctlr_el1 = 0xCAFE_BABE;

        let trap = SysregTrap {
            reg: SCTLR_EL1,
            rt: 10,
            is_write: false,
        };
        let result = emulate_sysreg(&mut vcpu, &trap).unwrap();
        assert_eq!(result, EmulationResult::Handled);
        assert_eq!(vcpu.read_gpr(10).unwrap(), 0xCAFE_BABE);
    }

    #[test]
    fn emulate_write_ttbr0() {
        let mut vcpu = make_vcpu();
        vcpu.write_gpr(0, 0x1234_5000).unwrap();

        let trap = SysregTrap {
            reg: TTBR0_EL1,
            rt: 0,
            is_write: true,
        };
        assert_eq!(
            emulate_sysreg(&mut vcpu, &trap).unwrap(),
            EmulationResult::Handled
        );
        assert_eq!(vcpu.sysregs.ttbr0_el1, 0x1234_5000);
    }

    #[test]
    fn emulate_read_midr() {
        let mut vcpu = make_vcpu();
        let trap = SysregTrap {
            reg: MIDR_EL1,
            rt: 3,
            is_write: false,
        };
        assert_eq!(
            emulate_sysreg(&mut vcpu, &trap).unwrap(),
            EmulationResult::Handled
        );
        assert_eq!(vcpu.read_gpr(3).unwrap(), 0x410F_D080);
    }

    #[test]
    fn emulate_read_mpidr() {
        let mut vcpu = make_vcpu();
        let trap = SysregTrap {
            reg: MPIDR_EL1,
            rt: 2,
            is_write: false,
        };
        assert_eq!(
            emulate_sysreg(&mut vcpu, &trap).unwrap(),
            EmulationResult::Handled
        );
        let val = vcpu.read_gpr(2).unwrap();
        // Bit 31 (RES1) should be set
        assert_ne!(val & (1 << 31), 0);
    }

    #[test]
    fn emulate_unknown_register_returns_unhandled() {
        let mut vcpu = make_vcpu();
        let unknown_reg = SysregId::new(3, 7, 15, 15, 7);
        let trap = SysregTrap {
            reg: unknown_reg,
            rt: 0,
            is_write: false,
        };
        assert_eq!(
            emulate_sysreg(&mut vcpu, &trap).unwrap(),
            EmulationResult::Unhandled
        );
    }

    #[test]
    fn emulate_write_to_xzr_discarded() {
        let mut vcpu = make_vcpu();
        vcpu.sysregs.sctlr_el1 = 0xABCD;

        // Read SCTLR into XZR (rt=31) — value should be discarded
        let trap = SysregTrap {
            reg: SCTLR_EL1,
            rt: 31,
            is_write: false,
        };
        assert_eq!(
            emulate_sysreg(&mut vcpu, &trap).unwrap(),
            EmulationResult::Handled
        );
    }

    #[test]
    fn emulate_msr_from_xzr_writes_zero() {
        let mut vcpu = make_vcpu();
        vcpu.sysregs.vbar_el1 = 0xFFFF;

        // Write from XZR (rt=31) — should write 0
        let trap = SysregTrap {
            reg: VBAR_EL1,
            rt: 31,
            is_write: true,
        };
        assert_eq!(
            emulate_sysreg(&mut vcpu, &trap).unwrap(),
            EmulationResult::Handled
        );
        assert_eq!(vcpu.sysregs.vbar_el1, 0);
    }

    #[test]
    fn emulate_timer_registers() {
        let mut vcpu = make_vcpu();

        // Write CNTV_CTL_EL0
        vcpu.write_gpr(1, 0x1).unwrap();
        let trap = SysregTrap {
            reg: CNTV_CTL_EL0,
            rt: 1,
            is_write: true,
        };
        emulate_sysreg(&mut vcpu, &trap).unwrap();
        assert_eq!(vcpu.sysregs.cntv_ctl_el0, 0x1);

        // Read it back
        let trap = SysregTrap {
            reg: CNTV_CTL_EL0,
            rt: 2,
            is_write: false,
        };
        emulate_sysreg(&mut vcpu, &trap).unwrap();
        assert_eq!(vcpu.read_gpr(2).unwrap(), 0x1);
    }
}
