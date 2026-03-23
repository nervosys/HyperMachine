//! ARM64 vCPU (Virtual CPU) management
//!
//! This module handles:
//! - AArch64 guest register state save/restore
//! - vCPU creation and lifecycle
//! - EL1 context switching
//! - Floating-point / SIMD state management

use crate::{Error, Result};
use core::sync::atomic::{AtomicU32, Ordering};

/// vCPU ID counter
static VCPU_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Maximum number of vCPUs per VM
pub const MAX_VCPUS_PER_VM: usize = 256;

/// vCPU state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcpuState {
    /// vCPU is not initialized
    Uninitialized,
    /// vCPU is ready to run
    Ready,
    /// vCPU is currently running (in EL1/EL0)
    Running,
    /// vCPU executed WFI and is halted
    Halted,
    /// vCPU is waiting for an interrupt
    WaitingForInterrupt,
    /// vCPU has encountered an error
    Error,
}

/// AArch64 general-purpose registers (X0-X30 + SP + PC + PSTATE)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GeneralRegisters {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
    pub x4: u64,
    pub x5: u64,
    pub x6: u64,
    pub x7: u64,
    pub x8: u64,
    pub x9: u64,
    pub x10: u64,
    pub x11: u64,
    pub x12: u64,
    pub x13: u64,
    pub x14: u64,
    pub x15: u64,
    pub x16: u64,
    pub x17: u64,
    pub x18: u64,
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
    pub x29: u64, // Frame pointer
    pub x30: u64, // Link register
}

/// Guest EL1 system register context
///
/// These registers are saved/restored on every VM entry/exit to preserve
/// the guest kernel's view of the system.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRegisterContext {
    /// EL1 Stack Pointer
    pub sp_el1: u64,
    /// Exception Link Register (return address from exception)
    pub elr_el1: u64,
    /// Saved Program Status Register
    pub spsr_el1: u64,
    /// System Control Register
    pub sctlr_el1: u64,
    /// Translation Control Register
    pub tcr_el1: u64,
    /// Translation Table Base Register 0
    pub ttbr0_el1: u64,
    /// Translation Table Base Register 1
    pub ttbr1_el1: u64,
    /// Memory Attribute Indirection Register
    pub mair_el1: u64,
    /// Vector Base Address Register
    pub vbar_el1: u64,
    /// Context ID Register
    pub contextidr_el1: u64,
    /// Auxiliary Memory Attribute Indirection Register
    pub amair_el1: u64,
    /// Counter-timer Physical Timer Control Register
    pub cntp_ctl_el0: u64,
    /// Counter-timer Physical Timer Compare Value
    pub cntp_cval_el0: u64,
    /// Counter-timer Virtual Timer Control Register
    pub cntv_ctl_el0: u64,
    /// Counter-timer Virtual Timer Compare Value
    pub cntv_cval_el0: u64,
    /// Counter-timer Virtual Offset Register (set by hypervisor)
    pub cntvoff_el2: u64,
    /// Auxiliary Control Register
    pub actlr_el1: u64,
    /// Current Program Status Register (EL2 view: ELR_EL2 on trap)
    pub elr_el2: u64,
    /// Saved PSTATE on trap to EL2
    pub spsr_el2: u64,
}

/// SIMD/FP register state (128-bit NEON Q-registers)
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub struct SimdFpState {
    /// Q0-Q31 (128-bit each, stored as pairs of u64)
    pub q: [[u64; 2]; 32],
    /// FPCR — Floating-Point Control Register
    pub fpcr: u32,
    /// FPSR — Floating-Point Status Register
    pub fpsr: u32,
}


impl core::fmt::Debug for SimdFpState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SimdFpState")
            .field("fpcr", &self.fpcr)
            .field("fpsr", &self.fpsr)
            .finish()
    }
}

/// A virtual CPU representing one guest execution context.
#[derive(Debug)]
pub struct Vcpu {
    /// vCPU identifier
    id: u32,
    /// Current state
    state: VcpuState,
    /// General-purpose registers
    pub gpr: GeneralRegisters,
    /// Guest EL1 system registers
    pub sysregs: SystemRegisterContext,
    /// SIMD/FP register state
    pub simd: SimdFpState,
    /// VMID this vCPU belongs to
    vmid: u16,
    /// Pending virtual IRQ
    pending_virq: bool,
    /// Pending virtual FIQ
    pending_vfiq: bool,
}

impl Vcpu {
    /// Create a new vCPU for the given VMID.
    pub fn new(vmid: u16) -> Self {
        let id = VCPU_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            id,
            state: VcpuState::Uninitialized,
            gpr: GeneralRegisters::default(),
            sysregs: SystemRegisterContext::default(),
            simd: SimdFpState::default(),
            vmid,
            pending_virq: false,
            pending_vfiq: false,
        }
    }

    /// Get the vCPU id.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the current state.
    pub fn state(&self) -> VcpuState {
        self.state
    }

    /// Get the VMID this vCPU belongs to.
    pub fn vmid(&self) -> u16 {
        self.vmid
    }

    /// Initialize the vCPU with a starting PC and stack pointer.
    pub fn initialize(&mut self, entry_point: u64, stack_pointer: u64) -> Result<()> {
        if self.state != VcpuState::Uninitialized {
            return Err(Error::InvalidVcpuState);
        }

        // Set guest entry point (goes into ELR_EL2 on VM entry)
        self.sysregs.elr_el2 = entry_point;

        // Set guest stack pointer
        self.sysregs.sp_el1 = stack_pointer;

        // Set initial PSTATE: EL1h, interrupts masked, AArch64
        // SPSR_EL2[3:0] = 0b0101 → EL1h
        // SPSR_EL2[9]   = 1 → DAIF.D (Debug masked)
        // SPSR_EL2[8]   = 1 → DAIF.A (SError masked)
        // SPSR_EL2[7]   = 1 → DAIF.I (IRQ masked)
        // SPSR_EL2[6]   = 1 → DAIF.F (FIQ masked)
        self.sysregs.spsr_el2 = 0x3C5; // EL1h + DAIF masked

        // Default SCTLR_EL1: caches and MMU off (guest will enable later)
        self.sysregs.sctlr_el1 = 0x0000_0000_0000_0030; // RES1 bits

        self.state = VcpuState::Ready;
        Ok(())
    }

    /// Mark the vCPU as running (called just before ERET to guest).
    pub fn enter(&mut self) -> Result<()> {
        match self.state {
            VcpuState::Ready | VcpuState::Halted => {
                self.state = VcpuState::Running;
                Ok(())
            }
            VcpuState::Running => Err(Error::VcpuAlreadyRunning),
            _ => Err(Error::InvalidVcpuState),
        }
    }

    /// Mark the vCPU as ready after a trap back to EL2.
    pub fn exit(&mut self) {
        if self.state == VcpuState::Running {
            self.state = VcpuState::Ready;
        }
    }

    /// Halt the vCPU (WFI trapped).
    pub fn halt(&mut self) {
        if self.state == VcpuState::Running {
            self.state = VcpuState::Halted;
        }
    }

    /// Inject a virtual IRQ into this vCPU.
    pub fn inject_virq(&mut self) {
        self.pending_virq = true;
    }

    /// Inject a virtual FIQ into this vCPU.
    pub fn inject_vfiq(&mut self) {
        self.pending_vfiq = true;
    }

    /// Check and clear the pending virtual IRQ flag.
    pub fn take_pending_virq(&mut self) -> bool {
        let pending = self.pending_virq;
        self.pending_virq = false;
        pending
    }

    /// Check and clear the pending virtual FIQ flag.
    pub fn take_pending_vfiq(&mut self) -> bool {
        let pending = self.pending_vfiq;
        self.pending_vfiq = false;
        pending
    }

    /// Read a general-purpose register by index (0-30).
    pub fn read_gpr(&self, index: u8) -> Result<u64> {
        if index > 30 {
            return Err(Error::InvalidParameter);
        }
        let regs = &self.gpr;
        let val = match index {
            0 => regs.x0,
            1 => regs.x1,
            2 => regs.x2,
            3 => regs.x3,
            4 => regs.x4,
            5 => regs.x5,
            6 => regs.x6,
            7 => regs.x7,
            8 => regs.x8,
            9 => regs.x9,
            10 => regs.x10,
            11 => regs.x11,
            12 => regs.x12,
            13 => regs.x13,
            14 => regs.x14,
            15 => regs.x15,
            16 => regs.x16,
            17 => regs.x17,
            18 => regs.x18,
            19 => regs.x19,
            20 => regs.x20,
            21 => regs.x21,
            22 => regs.x22,
            23 => regs.x23,
            24 => regs.x24,
            25 => regs.x25,
            26 => regs.x26,
            27 => regs.x27,
            28 => regs.x28,
            29 => regs.x29,
            30 => regs.x30,
            _ => unreachable!(),
        };
        Ok(val)
    }

    /// Write a general-purpose register by index (0-30).
    pub fn write_gpr(&mut self, index: u8, value: u64) -> Result<()> {
        if index > 30 {
            return Err(Error::InvalidParameter);
        }
        let regs = &mut self.gpr;
        match index {
            0 => regs.x0 = value,
            1 => regs.x1 = value,
            2 => regs.x2 = value,
            3 => regs.x3 = value,
            4 => regs.x4 = value,
            5 => regs.x5 = value,
            6 => regs.x6 = value,
            7 => regs.x7 = value,
            8 => regs.x8 = value,
            9 => regs.x9 = value,
            10 => regs.x10 = value,
            11 => regs.x11 = value,
            12 => regs.x12 = value,
            13 => regs.x13 = value,
            14 => regs.x14 = value,
            15 => regs.x15 = value,
            16 => regs.x16 = value,
            17 => regs.x17 = value,
            18 => regs.x18 = value,
            19 => regs.x19 = value,
            20 => regs.x20 = value,
            21 => regs.x21 = value,
            22 => regs.x22 = value,
            23 => regs.x23 = value,
            24 => regs.x24 = value,
            25 => regs.x25 = value,
            26 => regs.x26 = value,
            27 => regs.x27 = value,
            28 => regs.x28 = value,
            29 => regs.x29 = value,
            30 => regs.x30 = value,
            _ => unreachable!(),
        }
        Ok(())
    }

    /// Advance the guest PC past a trapped instruction (typically +4 bytes).
    pub fn advance_pc(&mut self) {
        self.sysregs.elr_el2 = self.sysregs.elr_el2.wrapping_add(4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcpu_creation() {
        VCPU_ID_COUNTER.store(0, Ordering::Relaxed);
        let vcpu = Vcpu::new(1);
        assert_eq!(vcpu.id(), 0);
        assert_eq!(vcpu.vmid(), 1);
        assert_eq!(vcpu.state(), VcpuState::Uninitialized);
    }

    #[test]
    fn vcpu_ids_are_unique() {
        VCPU_ID_COUNTER.store(100, Ordering::Relaxed);
        let v1 = Vcpu::new(1);
        let v2 = Vcpu::new(1);
        assert_ne!(v1.id(), v2.id());
    }

    #[test]
    fn vcpu_initialize() {
        let mut vcpu = Vcpu::new(1);
        assert!(vcpu.initialize(0x8_0000, 0x10_0000).is_ok());
        assert_eq!(vcpu.state(), VcpuState::Ready);
        assert_eq!(vcpu.sysregs.elr_el2, 0x8_0000);
        assert_eq!(vcpu.sysregs.sp_el1, 0x10_0000);
        assert_eq!(vcpu.sysregs.spsr_el2, 0x3C5);
    }

    #[test]
    fn vcpu_double_initialize_fails() {
        let mut vcpu = Vcpu::new(1);
        vcpu.initialize(0x8_0000, 0x10_0000).unwrap();
        assert_eq!(
            vcpu.initialize(0x8_0000, 0x10_0000),
            Err(Error::InvalidVcpuState)
        );
    }

    #[test]
    fn vcpu_enter_exit_cycle() {
        let mut vcpu = Vcpu::new(1);
        vcpu.initialize(0x8_0000, 0x10_0000).unwrap();

        assert!(vcpu.enter().is_ok());
        assert_eq!(vcpu.state(), VcpuState::Running);

        vcpu.exit();
        assert_eq!(vcpu.state(), VcpuState::Ready);
    }

    #[test]
    fn vcpu_enter_when_running_fails() {
        let mut vcpu = Vcpu::new(1);
        vcpu.initialize(0x8_0000, 0x10_0000).unwrap();
        vcpu.enter().unwrap();
        assert_eq!(vcpu.enter(), Err(Error::VcpuAlreadyRunning));
    }

    #[test]
    fn vcpu_enter_from_halted() {
        let mut vcpu = Vcpu::new(1);
        vcpu.initialize(0x8_0000, 0x10_0000).unwrap();
        vcpu.enter().unwrap();
        vcpu.halt();
        assert_eq!(vcpu.state(), VcpuState::Halted);
        assert!(vcpu.enter().is_ok());
        assert_eq!(vcpu.state(), VcpuState::Running);
    }

    #[test]
    fn vcpu_enter_from_uninitialized_fails() {
        let mut vcpu = Vcpu::new(1);
        assert_eq!(vcpu.enter(), Err(Error::InvalidVcpuState));
    }

    #[test]
    fn vcpu_gpr_read_write() {
        let mut vcpu = Vcpu::new(1);
        for i in 0..=30u8 {
            vcpu.write_gpr(i, (i as u64) * 0x1000).unwrap();
        }
        for i in 0..=30u8 {
            assert_eq!(vcpu.read_gpr(i).unwrap(), (i as u64) * 0x1000);
        }
    }

    #[test]
    fn vcpu_gpr_out_of_range() {
        let vcpu = Vcpu::new(1);
        assert_eq!(vcpu.read_gpr(31), Err(Error::InvalidParameter));
    }

    #[test]
    fn vcpu_write_gpr_out_of_range() {
        let mut vcpu = Vcpu::new(1);
        assert_eq!(vcpu.write_gpr(31, 0), Err(Error::InvalidParameter));
    }

    #[test]
    fn vcpu_advance_pc() {
        let mut vcpu = Vcpu::new(1);
        vcpu.initialize(0x8_0000, 0).unwrap();
        assert_eq!(vcpu.sysregs.elr_el2, 0x8_0000);
        vcpu.advance_pc();
        assert_eq!(vcpu.sysregs.elr_el2, 0x8_0004);
    }

    #[test]
    fn vcpu_inject_virq() {
        let mut vcpu = Vcpu::new(1);
        assert!(!vcpu.take_pending_virq());
        vcpu.inject_virq();
        assert!(vcpu.take_pending_virq());
        // second take should return false
        assert!(!vcpu.take_pending_virq());
    }

    #[test]
    fn vcpu_inject_vfiq() {
        let mut vcpu = Vcpu::new(1);
        assert!(!vcpu.take_pending_vfiq());
        vcpu.inject_vfiq();
        assert!(vcpu.take_pending_vfiq());
        assert!(!vcpu.take_pending_vfiq());
    }

    #[test]
    fn simd_state_default_zeroed() {
        let s = SimdFpState::default();
        assert_eq!(s.fpcr, 0);
        assert_eq!(s.fpsr, 0);
        for q in &s.q {
            assert_eq!(q[0], 0);
            assert_eq!(q[1], 0);
        }
    }

    #[test]
    fn general_registers_default_zeroed() {
        let gpr = GeneralRegisters::default();
        assert_eq!(gpr.x0, 0);
        assert_eq!(gpr.x30, 0);
    }
}
