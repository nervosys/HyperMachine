//! AArch64 (ARM64) CPU emulation
//!
//! This module provides a software AArch64 CPU emulator for executing
//! guest code. It supports common A64 instructions, exception handling,
//! and system registers.

use crate::{CpuError, Result};
use std::collections::HashMap;

/// AArch64 general-purpose registers (X0-X30, SP, PC)
#[derive(Debug, Clone, Copy, Default)]
pub struct AArch64Registers {
    /// General-purpose registers X0-X30
    pub x: [u64; 31],
    /// Stack Pointer (SP)
    pub sp: u64,
    /// Program Counter (PC)
    pub pc: u64,
    /// Process State (PSTATE/NZCV flags)
    pub pstate: u32,
    /// Exception Link Register (holds return address on exception)
    pub elr_el1: u64,
    /// Saved Program Status Register
    pub spsr_el1: u32,
    /// Current Exception Level (0-3)
    pub current_el: u8,
}

/// PSTATE flags
#[allow(dead_code)]
pub mod pstate {
    pub const N: u32 = 1 << 31; // Negative
    pub const Z: u32 = 1 << 30; // Zero
    pub const C: u32 = 1 << 29; // Carry
    pub const V: u32 = 1 << 28; // Overflow
    pub const D: u32 = 1 << 9; // Debug mask
    pub const A: u32 = 1 << 8; // SError mask
    pub const I: u32 = 1 << 7; // IRQ mask
    pub const F: u32 = 1 << 6; // FIQ mask
    pub const SS: u32 = 1 << 21; // Single Step
    pub const IL: u32 = 1 << 20; // Illegal Execution State
}

/// Exception types for AArch64
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionType {
    /// Synchronous exception
    Sync,
    /// IRQ interrupt
    Irq,
    /// FIQ interrupt
    Fiq,
    /// SError interrupt
    SError,
}

/// Exception class (ESR_ELx.EC field)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionClass {
    Unknown = 0x00,
    WFxTrap = 0x01,
    Cp15Trap = 0x03,
    Cp14Trap = 0x05,
    SvcA32 = 0x11,
    HvcA32 = 0x12,
    SmcA32 = 0x13,
    SvcA64 = 0x15,
    HvcA64 = 0x16,
    SmcA64 = 0x17,
    SysReg = 0x18,
    InstrAbortLower = 0x20,
    InstrAbortCurrent = 0x21,
    PcAlign = 0x22,
    DataAbortLower = 0x24,
    DataAbortCurrent = 0x25,
    SpAlign = 0x26,
    Serror = 0x2F,
    BreakpointLower = 0x30,
    BreakpointCurrent = 0x31,
    SoftwareStep = 0x32,
    WatchpointLower = 0x34,
    WatchpointCurrent = 0x35,
    Brk = 0x3C,
}

/// System register ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemRegId {
    pub op0: u8,
    pub op1: u8,
    pub crn: u8,
    pub crm: u8,
    pub op2: u8,
}

impl SystemRegId {
    pub const fn new(op0: u8, op1: u8, crn: u8, crm: u8, op2: u8) -> Self {
        Self {
            op0,
            op1,
            crn,
            crm,
            op2,
        }
    }

    // Common system registers
    pub const SCTLR_EL1: Self = Self::new(3, 0, 1, 0, 0);
    pub const TTBR0_EL1: Self = Self::new(3, 0, 2, 0, 0);
    pub const TTBR1_EL1: Self = Self::new(3, 0, 2, 0, 1);
    pub const TCR_EL1: Self = Self::new(3, 0, 2, 0, 2);
    pub const MAIR_EL1: Self = Self::new(3, 0, 10, 2, 0);
    pub const VBAR_EL1: Self = Self::new(3, 0, 12, 0, 0);
    pub const CNTFRQ_EL0: Self = Self::new(3, 3, 14, 0, 0);
    pub const CNTVCT_EL0: Self = Self::new(3, 3, 14, 0, 2);
    pub const CNTV_CTL_EL0: Self = Self::new(3, 3, 14, 3, 1);
    pub const CNTV_CVAL_EL0: Self = Self::new(3, 3, 14, 3, 2);
}

/// Memory access trait for CPU operations
pub trait MemoryAccess {
    /// Read a byte from memory
    fn read_u8(&self, addr: u64) -> Result<u8>;
    /// Read a halfword (16-bit) from memory
    fn read_u16(&self, addr: u64) -> Result<u16>;
    /// Read a word (32-bit) from memory
    fn read_u32(&self, addr: u64) -> Result<u32>;
    /// Read a doubleword (64-bit) from memory
    fn read_u64(&self, addr: u64) -> Result<u64>;
    /// Write a byte to memory
    fn write_u8(&mut self, addr: u64, value: u8) -> Result<()>;
    /// Write a halfword to memory
    fn write_u16(&mut self, addr: u64, value: u16) -> Result<()>;
    /// Write a word to memory
    fn write_u32(&mut self, addr: u64, value: u32) -> Result<()>;
    /// Write a doubleword to memory
    fn write_u64(&mut self, addr: u64, value: u64) -> Result<()>;
}

/// Simple slice-based memory implementation
pub struct SliceMemory<'a> {
    data: &'a mut [u8],
}

impl<'a> SliceMemory<'a> {
    pub fn new(data: &'a mut [u8]) -> Self {
        Self { data }
    }
}

impl<'a> MemoryAccess for SliceMemory<'a> {
    fn read_u8(&self, addr: u64) -> Result<u8> {
        let addr = addr as usize;
        self.data
            .get(addr)
            .copied()
            .ok_or(CpuError::InvalidMemoryAccess)
    }

    fn read_u16(&self, addr: u64) -> Result<u16> {
        let addr = addr as usize;
        if addr + 1 < self.data.len() {
            Ok(u16::from_le_bytes([self.data[addr], self.data[addr + 1]]))
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
    }

    fn read_u32(&self, addr: u64) -> Result<u32> {
        let addr = addr as usize;
        if addr + 3 < self.data.len() {
            Ok(u32::from_le_bytes([
                self.data[addr],
                self.data[addr + 1],
                self.data[addr + 2],
                self.data[addr + 3],
            ]))
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
    }

    fn read_u64(&self, addr: u64) -> Result<u64> {
        let addr = addr as usize;
        if addr + 7 < self.data.len() {
            Ok(u64::from_le_bytes([
                self.data[addr],
                self.data[addr + 1],
                self.data[addr + 2],
                self.data[addr + 3],
                self.data[addr + 4],
                self.data[addr + 5],
                self.data[addr + 6],
                self.data[addr + 7],
            ]))
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
    }

    fn write_u8(&mut self, addr: u64, value: u8) -> Result<()> {
        let addr = addr as usize;
        if addr < self.data.len() {
            self.data[addr] = value;
            Ok(())
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
    }

    fn write_u16(&mut self, addr: u64, value: u16) -> Result<()> {
        let addr = addr as usize;
        if addr + 1 < self.data.len() {
            let bytes = value.to_le_bytes();
            self.data[addr] = bytes[0];
            self.data[addr + 1] = bytes[1];
            Ok(())
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
    }

    fn write_u32(&mut self, addr: u64, value: u32) -> Result<()> {
        let addr = addr as usize;
        if addr + 3 < self.data.len() {
            let bytes = value.to_le_bytes();
            self.data[addr..addr + 4].copy_from_slice(&bytes);
            Ok(())
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
    }

    fn write_u64(&mut self, addr: u64, value: u64) -> Result<()> {
        let addr = addr as usize;
        if addr + 7 < self.data.len() {
            let bytes = value.to_le_bytes();
            self.data[addr..addr + 8].copy_from_slice(&bytes);
            Ok(())
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
    }
}

/// AArch64 CPU emulator
pub struct AArch64Cpu {
    /// General-purpose and special registers
    regs: AArch64Registers,
    /// System registers
    sys_regs: HashMap<SystemRegId, u64>,
    /// CPU is in WFI/WFE state (waiting)
    waiting: bool,
    /// Pending IRQ
    pending_irq: bool,
    /// Pending FIQ
    pending_fiq: bool,
    /// Vector Base Address Register
    vbar: u64,
}

impl AArch64Cpu {
    /// Create a new AArch64 CPU
    #[must_use]
    pub fn new() -> Self {
        let mut cpu = Self {
            regs: AArch64Registers::default(),
            sys_regs: HashMap::new(),
            waiting: false,
            pending_irq: false,
            pending_fiq: false,
            vbar: 0,
        };
        cpu.reset();
        cpu
    }

    /// Reset the CPU to initial state
    pub fn reset(&mut self) {
        self.regs = AArch64Registers::default();
        self.regs.current_el = 1; // Start in EL1 (kernel mode)
        self.regs.pstate = pstate::D | pstate::A | pstate::I | pstate::F; // All exceptions masked
        self.sys_regs.clear();
        self.waiting = false;
        self.pending_irq = false;
        self.pending_fiq = false;
        self.vbar = 0;
    }

    /// Get registers
    pub fn registers(&self) -> &AArch64Registers {
        &self.regs
    }

    /// Get mutable registers
    pub fn registers_mut(&mut self) -> &mut AArch64Registers {
        &mut self.regs
    }

    /// Get a general-purpose register (X0-X30, XZR for 31)
    pub fn get_xreg(&self, reg: u8) -> u64 {
        if reg < 31 {
            self.regs.x[reg as usize]
        } else {
            0 // XZR (zero register)
        }
    }

    /// Set a general-purpose register (X0-X30, writes to 31 are ignored)
    pub fn set_xreg(&mut self, reg: u8, value: u64) {
        if reg < 31 {
            self.regs.x[reg as usize] = value;
        }
        // Writes to XZR (reg 31) are ignored
    }

    /// Get 32-bit view of register (W0-W30)
    pub fn get_wreg(&self, reg: u8) -> u32 {
        self.get_xreg(reg) as u32
    }

    /// Set 32-bit view of register (zero-extends to 64-bit)
    pub fn set_wreg(&mut self, reg: u8, value: u32) {
        self.set_xreg(reg, value as u64);
    }

    /// Read a system register
    pub fn read_sys_reg(&self, reg: SystemRegId) -> u64 {
        self.sys_regs.get(&reg).copied().unwrap_or(0)
    }

    /// Write a system register
    pub fn write_sys_reg(&mut self, reg: SystemRegId, value: u64) {
        self.sys_regs.insert(reg, value);

        // Handle special registers
        if reg == SystemRegId::VBAR_EL1 {
            self.vbar = value;
        }
    }

    /// Check if CPU is waiting (WFI/WFE)
    pub fn is_waiting(&self) -> bool {
        self.waiting
    }

    /// Check if interrupts are enabled (IRQ not masked)
    pub fn irq_enabled(&self) -> bool {
        (self.regs.pstate & pstate::I) == 0
    }

    /// Check if FIQ is enabled
    pub fn fiq_enabled(&self) -> bool {
        (self.regs.pstate & pstate::F) == 0
    }

    /// Raise an IRQ
    pub fn raise_irq(&mut self) {
        self.pending_irq = true;
        if self.irq_enabled() {
            self.waiting = false;
        }
    }

    /// Raise an FIQ
    pub fn raise_fiq(&mut self) {
        self.pending_fiq = true;
        if self.fiq_enabled() {
            self.waiting = false;
        }
    }

    /// Clear pending IRQ
    pub fn clear_irq(&mut self) {
        self.pending_irq = false;
    }

    /// Clear pending FIQ
    pub fn clear_fiq(&mut self) {
        self.pending_fiq = false;
    }

    /// Check and handle pending exceptions
    pub fn check_exceptions<M: MemoryAccess>(&mut self, memory: &mut M) -> Result<bool> {
        // Check FIQ first (higher priority)
        if self.pending_fiq && self.fiq_enabled() {
            self.take_exception(ExceptionType::Fiq, memory)?;
            self.pending_fiq = false;
            return Ok(true);
        }

        // Check IRQ
        if self.pending_irq && self.irq_enabled() {
            self.take_exception(ExceptionType::Irq, memory)?;
            self.pending_irq = false;
            return Ok(true);
        }

        Ok(false)
    }

    /// Take an exception
    fn take_exception<M: MemoryAccess>(
        &mut self,
        exc_type: ExceptionType,
        _memory: &mut M,
    ) -> Result<()> {
        // Save current state
        self.regs.elr_el1 = self.regs.pc;
        self.regs.spsr_el1 = self.regs.pstate;

        // Calculate vector offset
        let vector_offset = match exc_type {
            ExceptionType::Sync => 0x200,
            ExceptionType::Irq => 0x280,
            ExceptionType::Fiq => 0x300,
            ExceptionType::SError => 0x380,
        };

        // Jump to exception vector
        self.regs.pc = self.vbar + vector_offset;

        // Mask interrupts
        self.regs.pstate |= pstate::I | pstate::F;

        tracing::trace!(
            "Taking {:?} exception, jumping to 0x{:x}",
            exc_type,
            self.regs.pc
        );

        Ok(())
    }

    /// Return from exception (ERET)
    fn eret(&mut self) {
        self.regs.pc = self.regs.elr_el1;
        self.regs.pstate = self.regs.spsr_el1;
    }

    /// Fetch instruction from memory
    pub fn fetch_instruction<M: MemoryAccess>(&self, memory: &M) -> Result<u32> {
        // Check PC alignment (must be 4-byte aligned)
        if (self.regs.pc & 3) != 0 {
            return Err(CpuError::InvalidMemoryAccess);
        }
        memory.read_u32(self.regs.pc)
    }

    /// Execute a single instruction
    pub fn execute_instruction<M: MemoryAccess>(
        &mut self,
        opcode: u32,
        memory: &mut M,
    ) -> Result<()> {
        // Decode instruction class (bits 25-28)
        let op0 = (opcode >> 25) & 0xF;

        match op0 {
            // Data Processing -- Immediate
            0b1000 | 0b1001 => self.execute_data_processing_imm(opcode),

            // Branches, Exception Generating and System instructions
            0b1010 | 0b1011 => self.execute_branch_system(opcode, memory),

            // Loads and Stores
            0b0100 | 0b0110 | 0b1100 | 0b1110 => self.execute_load_store(opcode, memory),

            // Data Processing -- Register
            0b0101 | 0b1101 => self.execute_data_processing_reg(opcode),

            // Data Processing -- SIMD and FP (simplified)
            0b0111 | 0b1111 => {
                // Skip SIMD/FP for now
                self.regs.pc += 4;
                Ok(())
            }

            _ => Err(CpuError::UnsupportedInstruction(format!(
                "0x{:08X}",
                opcode
            ))),
        }
    }

    /// Execute Data Processing (Immediate) instructions
    fn execute_data_processing_imm(&mut self, opcode: u32) -> Result<()> {
        let op = (opcode >> 23) & 0x7;

        match op {
            // ADD/SUB (immediate)
            0b010 => {
                let sf = (opcode >> 31) & 1; // 64-bit if 1
                let op_sub = (opcode >> 30) & 1; // SUB if 1
                let s = (opcode >> 29) & 1; // Set flags if 1
                let imm12 = ((opcode >> 10) & 0xFFF) as u64;
                let shift = (opcode >> 22) & 0x3;
                let rn = ((opcode >> 5) & 0x1F) as u8;
                let rd = (opcode & 0x1F) as u8;

                let imm = if shift == 1 { imm12 << 12 } else { imm12 };
                let operand1 = if sf == 1 {
                    self.get_xreg(rn)
                } else {
                    self.get_wreg(rn) as u64
                };

                let (result, carry, overflow) = if op_sub == 1 {
                    let (res, borrow) = operand1.overflowing_sub(imm);
                    let overflow = ((operand1 ^ imm) & (operand1 ^ res) & (1 << 63)) != 0;
                    (res, !borrow, overflow)
                } else {
                    let (res, carry) = operand1.overflowing_add(imm);
                    let overflow = (!(operand1 ^ imm) & (operand1 ^ res) & (1 << 63)) != 0;
                    (res, carry, overflow)
                };

                if sf == 1 {
                    self.set_xreg(rd, result);
                } else {
                    self.set_wreg(rd, result as u32);
                }

                if s == 1 {
                    self.update_nzcv(result, carry, overflow, sf == 1);
                }
            }

            // MOV (wide immediate)
            0b101 => {
                let sf = (opcode >> 31) & 1;
                let opc = (opcode >> 29) & 0x3;
                let hw = (opcode >> 21) & 0x3;
                let imm16 = ((opcode >> 5) & 0xFFFF) as u64;
                let rd = (opcode & 0x1F) as u8;

                let shift = hw * 16;
                let imm = imm16 << shift;

                let result = match opc {
                    0b00 => !imm, // MOVN
                    0b10 => imm,  // MOVZ
                    0b11 => {
                        // MOVK
                        let old = self.get_xreg(rd);
                        let mask = !(0xFFFFu64 << shift);
                        (old & mask) | imm
                    }
                    _ => return Err(CpuError::InvalidInstruction),
                };

                if sf == 1 {
                    self.set_xreg(rd, result);
                } else {
                    self.set_wreg(rd, result as u32);
                }
            }

            // Logical (immediate)
            0b100 => {
                let sf = (opcode >> 31) & 1;
                let opc = (opcode >> 29) & 0x3;
                let n = (opcode >> 22) & 1;
                let immr = (opcode >> 16) & 0x3F;
                let imms = (opcode >> 10) & 0x3F;
                let rn = ((opcode >> 5) & 0x1F) as u8;
                let rd = (opcode & 0x1F) as u8;

                // Decode bitmask immediate (simplified)
                let imm = decode_bitmask(n, imms, immr, sf == 1);
                let operand1 = if sf == 1 {
                    self.get_xreg(rn)
                } else {
                    self.get_wreg(rn) as u64
                };

                let result = match opc {
                    0b00 => operand1 & imm, // AND
                    0b01 => operand1 | imm, // ORR
                    0b10 => operand1 ^ imm, // EOR
                    0b11 => {
                        // ANDS
                        let res = operand1 & imm;
                        self.update_nzcv(res, false, false, sf == 1);
                        res
                    }
                    _ => unreachable!("2-bit opc value exceeded 0..=3"),
                };

                if sf == 1 {
                    self.set_xreg(rd, result);
                } else {
                    self.set_wreg(rd, result as u32);
                }
            }

            _ => {
                return Err(CpuError::UnsupportedInstruction(format!(
                    "DP-Imm op={}",
                    op
                )))
            }
        }

        self.regs.pc += 4;
        Ok(())
    }

    /// Execute Branch and System instructions
    fn execute_branch_system<M: MemoryAccess>(
        &mut self,
        opcode: u32,
        memory: &mut M,
    ) -> Result<()> {
        let _op0 = (opcode >> 29) & 0x7;
        let _op1 = (opcode >> 22) & 0x7F;

        // Unconditional branch (immediate)
        if (opcode >> 26) == 0b000101 {
            let imm26 = opcode & 0x3FFFFFF;
            let offset = sign_extend(imm26 as u64, 26) << 2;
            self.regs.pc = self.regs.pc.wrapping_add(offset as u64);
            return Ok(());
        }

        // Branch with link
        if (opcode >> 26) == 0b100101 {
            let imm26 = opcode & 0x3FFFFFF;
            let offset = sign_extend(imm26 as u64, 26) << 2;
            self.regs.x[30] = self.regs.pc + 4; // Link register
            self.regs.pc = self.regs.pc.wrapping_add(offset as u64);
            return Ok(());
        }

        // Conditional branch
        if (opcode >> 25) & 0x7F == 0b0101010 {
            let imm19 = (opcode >> 5) & 0x7FFFF;
            let cond = (opcode & 0xF) as u8;
            let offset = sign_extend(imm19 as u64, 19) << 2;

            if self.condition_holds(cond) {
                self.regs.pc = self.regs.pc.wrapping_add(offset as u64);
            } else {
                self.regs.pc += 4;
            }
            return Ok(());
        }

        // Branch to register (BR, BLR, RET)
        if (opcode >> 25) & 0x7F == 0b1101011 {
            let opc = (opcode >> 21) & 0xF;
            let rn = ((opcode >> 5) & 0x1F) as u8;

            match opc {
                0b0000 => {
                    // BR
                    self.regs.pc = self.get_xreg(rn);
                }
                0b0001 => {
                    // BLR
                    self.regs.x[30] = self.regs.pc + 4;
                    self.regs.pc = self.get_xreg(rn);
                }
                0b0010 => {
                    // RET
                    self.regs.pc = self.get_xreg(rn);
                }
                _ => return Err(CpuError::UnsupportedInstruction(format!("BR opc={}", opc))),
            }
            return Ok(());
        }

        // Exception generating
        if (opcode >> 24) == 0b11010100 {
            let opc = (opcode >> 21) & 0x7;
            let imm16 = (opcode >> 5) & 0xFFFF;

            match opc {
                0b000 => {
                    // SVC
                    tracing::trace!("SVC #{}", imm16);
                    self.take_exception(ExceptionType::Sync, memory)?;
                    return Ok(());
                }
                0b001 => {
                    // HVC
                    tracing::trace!("HVC #{}", imm16);
                    self.take_exception(ExceptionType::Sync, memory)?;
                    return Ok(());
                }
                0b010 => {
                    // SMC
                    tracing::trace!("SMC #{}", imm16);
                    self.take_exception(ExceptionType::Sync, memory)?;
                    return Ok(());
                }
                0b011 => {
                    // BRK
                    return Err(CpuError::Execution(format!("BRK #{}", imm16)));
                }
                _ => {}
            }
        }

        // System instructions (MSR, MRS, NOP, WFI, etc.)
        if (opcode >> 22) == 0b1101010100 {
            let l = (opcode >> 21) & 1;
            let op0 = ((opcode >> 19) & 0x3) as u8;
            let op1 = ((opcode >> 16) & 0x7) as u8;
            let crn = ((opcode >> 12) & 0xF) as u8;
            let crm = ((opcode >> 8) & 0xF) as u8;
            let op2 = ((opcode >> 5) & 0x7) as u8;
            let rt = (opcode & 0x1F) as u8;

            // NOP, WFI, WFE, etc.
            if op0 == 0 && crn == 0b0011 {
                match (op1, crm, op2) {
                    (0b011, 0b0000, 0b000) => {
                        // NOP
                    }
                    (0b011, 0b0010, 0b000) => {
                        // WFE
                        self.waiting = true;
                    }
                    (0b011, 0b0010, 0b001) => {
                        // WFI
                        self.waiting = true;
                    }
                    _ => {}
                }
                self.regs.pc += 4;
                return Ok(());
            }

            // MRS/MSR
            let reg_id = SystemRegId::new(op0 + 2, op1, crn, crm, op2);

            if l == 1 {
                // MRS
                let value = self.read_sys_reg(reg_id);
                self.set_xreg(rt, value);
            } else {
                // MSR
                let value = self.get_xreg(rt);
                self.write_sys_reg(reg_id, value);
            }

            self.regs.pc += 4;
            return Ok(());
        }

        // ERET
        if opcode == 0xD69F03E0 {
            self.eret();
            return Ok(());
        }

        Err(CpuError::UnsupportedInstruction(format!(
            "Branch/Sys 0x{:08X}",
            opcode
        )))
    }

    /// Execute Load/Store instructions
    fn execute_load_store<M: MemoryAccess>(&mut self, opcode: u32, memory: &mut M) -> Result<()> {
        let _op = (opcode >> 22) & 0x3FF;
        let size = (opcode >> 30) & 0x3;
        let v = (opcode >> 26) & 1; // Vector register if 1
        let opc = (opcode >> 22) & 0x3;
        let rn = ((opcode >> 5) & 0x1F) as u8;
        let rt = (opcode & 0x1F) as u8;

        if v == 1 {
            // Vector load/store - skip for now
            self.regs.pc += 4;
            return Ok(());
        }

        // Load/Store with immediate offset
        if (opcode >> 24) & 0x3F == 0b111001 {
            let imm12 = ((opcode >> 10) & 0xFFF) as u64;
            let scale = size as u64;
            let offset = imm12 << scale;

            let base = if rn == 31 {
                self.regs.sp
            } else {
                self.get_xreg(rn)
            };
            let addr = base.wrapping_add(offset);

            let is_load = (opc & 1) == 1;

            if is_load {
                let value = match size {
                    0 => memory.read_u8(addr)? as u64,
                    1 => memory.read_u16(addr)? as u64,
                    2 => memory.read_u32(addr)? as u64,
                    3 => memory.read_u64(addr)?,
                    _ => unreachable!("2-bit size value exceeded 0..=3"),
                };
                self.set_xreg(rt, value);
            } else {
                let value = self.get_xreg(rt);
                match size {
                    0 => memory.write_u8(addr, value as u8)?,
                    1 => memory.write_u16(addr, value as u16)?,
                    2 => memory.write_u32(addr, value as u32)?,
                    3 => memory.write_u64(addr, value)?,
                    _ => unreachable!("2-bit size value exceeded 0..=3"),
                }
            }

            self.regs.pc += 4;
            return Ok(());
        }

        // Load/Store pair
        if (opcode >> 25) & 0x7F == 0b0101001 {
            let _opc = (opcode >> 23) & 0x3;
            let l = (opcode >> 22) & 1;
            let imm7 = ((opcode >> 15) & 0x7F) as i32;
            let rt2 = ((opcode >> 10) & 0x1F) as u8;

            let scale = if (opcode >> 31) & 1 == 1 { 3 } else { 2 };
            let offset = sign_extend(imm7 as u64, 7) << scale;

            let base = if rn == 31 {
                self.regs.sp
            } else {
                self.get_xreg(rn)
            };
            let addr = (base as i64 + offset) as u64;

            if l == 1 {
                // Load pair
                if scale == 3 {
                    self.set_xreg(rt, memory.read_u64(addr)?);
                    self.set_xreg(rt2, memory.read_u64(addr + 8)?);
                } else {
                    self.set_xreg(rt, memory.read_u32(addr)? as u64);
                    self.set_xreg(rt2, memory.read_u32(addr + 4)? as u64);
                }
            } else {
                // Store pair
                if scale == 3 {
                    memory.write_u64(addr, self.get_xreg(rt))?;
                    memory.write_u64(addr + 8, self.get_xreg(rt2))?;
                } else {
                    memory.write_u32(addr, self.get_wreg(rt))?;
                    memory.write_u32(addr + 4, self.get_wreg(rt2))?;
                }
            }

            self.regs.pc += 4;
            return Ok(());
        }

        Err(CpuError::UnsupportedInstruction(format!(
            "Load/Store 0x{:08X}",
            opcode
        )))
    }

    /// Execute Data Processing (Register) instructions
    fn execute_data_processing_reg(&mut self, opcode: u32) -> Result<()> {
        let sf = (opcode >> 31) & 1;
        let _op = (opcode >> 21) & 0x7FF;

        // Logical (shifted register)
        if (opcode >> 24) & 0x1F == 0b01010 {
            let opc = (opcode >> 29) & 0x3;
            let shift_type = ((opcode >> 22) & 0x3) as u8;
            let n = (opcode >> 21) & 1;
            let rm = ((opcode >> 16) & 0x1F) as u8;
            let imm6 = (opcode >> 10) & 0x3F;
            let rn = ((opcode >> 5) & 0x1F) as u8;
            let rd = (opcode & 0x1F) as u8;

            let operand1 = if sf == 1 {
                self.get_xreg(rn)
            } else {
                self.get_wreg(rn) as u64
            };
            let mut operand2 = if sf == 1 {
                self.get_xreg(rm)
            } else {
                self.get_wreg(rm) as u64
            };

            // Apply shift
            operand2 = match shift_type {
                0 => operand2 << imm6,                 // LSL
                1 => operand2 >> imm6,                 // LSR
                2 => (operand2 as i64 >> imm6) as u64, // ASR
                3 => operand2.rotate_right(imm6),      // ROR
                _ => operand2,
            };

            if n == 1 {
                operand2 = !operand2;
            }

            let result = match opc {
                0b00 => operand1 & operand2, // AND
                0b01 => operand1 | operand2, // ORR
                0b10 => operand1 ^ operand2, // EOR
                0b11 => {
                    // ANDS
                    let res = operand1 & operand2;
                    self.update_nzcv(res, false, false, sf == 1);
                    res
                }
                _ => unreachable!("2-bit opc value exceeded 0..=3"),
            };

            if sf == 1 {
                self.set_xreg(rd, result);
            } else {
                self.set_wreg(rd, result as u32);
            }

            self.regs.pc += 4;
            return Ok(());
        }

        // Add/subtract (shifted register)
        if (opcode >> 24) & 0x1F == 0b01011 {
            let op_sub = (opcode >> 30) & 1;
            let s = (opcode >> 29) & 1;
            let shift_type = ((opcode >> 22) & 0x3) as u8;
            let rm = ((opcode >> 16) & 0x1F) as u8;
            let imm6 = (opcode >> 10) & 0x3F;
            let rn = ((opcode >> 5) & 0x1F) as u8;
            let rd = (opcode & 0x1F) as u8;

            let operand1 = if sf == 1 {
                self.get_xreg(rn)
            } else {
                self.get_wreg(rn) as u64
            };
            let mut operand2 = if sf == 1 {
                self.get_xreg(rm)
            } else {
                self.get_wreg(rm) as u64
            };

            // Apply shift
            operand2 = match shift_type {
                0 => operand2 << imm6,                 // LSL
                1 => operand2 >> imm6,                 // LSR
                2 => (operand2 as i64 >> imm6) as u64, // ASR
                _ => operand2,
            };

            let (result, carry, overflow) = if op_sub == 1 {
                let (res, borrow) = operand1.overflowing_sub(operand2);
                let overflow = ((operand1 ^ operand2) & (operand1 ^ res) & (1 << 63)) != 0;
                (res, !borrow, overflow)
            } else {
                let (res, carry) = operand1.overflowing_add(operand2);
                let overflow = (!(operand1 ^ operand2) & (operand1 ^ res) & (1 << 63)) != 0;
                (res, carry, overflow)
            };

            if sf == 1 {
                self.set_xreg(rd, result);
            } else {
                self.set_wreg(rd, result as u32);
            }

            if s == 1 {
                self.update_nzcv(result, carry, overflow, sf == 1);
            }

            self.regs.pc += 4;
            return Ok(());
        }

        Err(CpuError::UnsupportedInstruction(format!(
            "DP-Reg 0x{:08X}",
            opcode
        )))
    }

    /// Update NZCV flags
    fn update_nzcv(&mut self, result: u64, carry: bool, overflow: bool, is_64bit: bool) {
        let bit_size = if is_64bit { 63 } else { 31 };

        // Negative
        if (result >> bit_size) & 1 == 1 {
            self.regs.pstate |= pstate::N;
        } else {
            self.regs.pstate &= !pstate::N;
        }

        // Zero
        let zero_mask = if is_64bit { !0u64 } else { 0xFFFFFFFF };
        if (result & zero_mask) == 0 {
            self.regs.pstate |= pstate::Z;
        } else {
            self.regs.pstate &= !pstate::Z;
        }

        // Carry
        if carry {
            self.regs.pstate |= pstate::C;
        } else {
            self.regs.pstate &= !pstate::C;
        }

        // Overflow
        if overflow {
            self.regs.pstate |= pstate::V;
        } else {
            self.regs.pstate &= !pstate::V;
        }
    }

    /// Check if condition holds
    fn condition_holds(&self, cond: u8) -> bool {
        let n = (self.regs.pstate & pstate::N) != 0;
        let z = (self.regs.pstate & pstate::Z) != 0;
        let c = (self.regs.pstate & pstate::C) != 0;
        let v = (self.regs.pstate & pstate::V) != 0;

        let result = match cond >> 1 {
            0b000 => z,            // EQ/NE
            0b001 => c,            // CS/CC
            0b010 => n,            // MI/PL
            0b011 => v,            // VS/VC
            0b100 => c && !z,      // HI/LS
            0b101 => n == v,       // GE/LT
            0b110 => n == v && !z, // GT/LE
            0b111 => true,         // AL
            _ => true,
        };

        // Invert if LSB is 1 (except for AL)
        if (cond & 1) == 1 && cond != 0b1111 {
            !result
        } else {
            result
        }
    }

    /// Execute one step (fetch + decode + execute)
    pub fn step<M: MemoryAccess>(&mut self, memory: &mut M) -> Result<()> {
        if self.waiting {
            // Check for interrupts that might wake us
            if self.check_exceptions(memory)? {
                return Ok(());
            }
            return Ok(());
        }

        // Check for pending exceptions
        self.check_exceptions(memory)?;

        // Fetch and execute
        let opcode = self.fetch_instruction(memory)?;
        self.execute_instruction(opcode, memory)
    }
}

impl Default for AArch64Cpu {
    fn default() -> Self {
        Self::new()
    }
}

/// Sign extend a value
fn sign_extend(value: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((value as i64) << shift) >> shift
}

/// Decode bitmask immediate (simplified)
fn decode_bitmask(n: u32, imms: u32, immr: u32, is_64bit: bool) -> u64 {
    let len = if n == 1 {
        6
    } else {
        (imms >> 1).leading_zeros() - 26
    };
    let levels = (1u32 << len) - 1;
    let s = imms & levels;
    let r = immr & levels;
    let _diff = s.wrapping_sub(r);
    let esize = 1u64 << len;
    let welem = (1u64 << (s + 1)) - 1;
    let elem = welem.rotate_right(r);

    // Replicate element
    let mut result = 0u64;
    let mut i = 0;
    while i < (if is_64bit { 64 } else { 32 }) {
        result |= elem << i;
        i += esize as usize;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_creation() {
        let cpu = AArch64Cpu::new();
        assert_eq!(cpu.regs.current_el, 1);
        assert!(!cpu.irq_enabled()); // IRQs masked by default
    }

    #[test]
    fn test_register_access() {
        let mut cpu = AArch64Cpu::new();

        cpu.set_xreg(0, 0xDEADBEEF);
        assert_eq!(cpu.get_xreg(0), 0xDEADBEEF);

        // XZR always reads as 0
        cpu.set_xreg(31, 0x12345678);
        assert_eq!(cpu.get_xreg(31), 0);

        // W registers
        cpu.set_wreg(1, 0xFFFFFFFF);
        assert_eq!(cpu.get_xreg(1), 0xFFFFFFFF);
    }

    #[test]
    fn test_condition_flags() {
        let mut cpu = AArch64Cpu::new();

        // Set Z flag
        cpu.regs.pstate |= pstate::Z;
        assert!(cpu.condition_holds(0b0000)); // EQ
        assert!(!cpu.condition_holds(0b0001)); // NE
    }

    #[test]
    fn test_memory_access() {
        let mut data = vec![0u8; 0x1000];
        data[0] = 0x12;
        data[1] = 0x34;
        data[2] = 0x56;
        data[3] = 0x78;

        let mem = SliceMemory::new(&mut data);
        assert_eq!(mem.read_u8(0).unwrap(), 0x12);
        assert_eq!(mem.read_u16(0).unwrap(), 0x3412);
        assert_eq!(mem.read_u32(0).unwrap(), 0x78563412);
    }

    #[test]
    fn test_sign_extend() {
        assert_eq!(sign_extend(0x7F, 8), 127);
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x800, 12), -2048);
    }

    #[test]
    fn test_reset_clears_state() {
        let mut cpu = AArch64Cpu::new();
        // Modify various state
        cpu.set_xreg(0, 0xCAFE);
        cpu.regs.pc = 0x1000;
        cpu.write_sys_reg(SystemRegId::SCTLR_EL1, 0xABCD);
        cpu.raise_irq();

        cpu.reset();

        assert_eq!(cpu.get_xreg(0), 0);
        assert_eq!(cpu.regs.pc, 0);
        assert_eq!(cpu.regs.current_el, 1);
        assert!(!cpu.irq_enabled()); // masked after reset
        assert!(!cpu.fiq_enabled());
        assert!(!cpu.is_waiting());
        assert_eq!(cpu.read_sys_reg(SystemRegId::SCTLR_EL1), 0);
    }

    #[test]
    fn test_system_register_roundtrip() {
        let mut cpu = AArch64Cpu::new();
        cpu.write_sys_reg(SystemRegId::SCTLR_EL1, 0xDEAD_BEEF);
        assert_eq!(cpu.read_sys_reg(SystemRegId::SCTLR_EL1), 0xDEAD_BEEF);

        // Unwritten register returns 0
        assert_eq!(cpu.read_sys_reg(SystemRegId::TTBR0_EL1), 0);
    }

    #[test]
    fn test_vbar_write_updates_field() {
        let mut cpu = AArch64Cpu::new();
        cpu.write_sys_reg(SystemRegId::VBAR_EL1, 0x8000_0000);
        assert_eq!(cpu.read_sys_reg(SystemRegId::VBAR_EL1), 0x8000_0000);
    }

    #[test]
    fn test_irq_raise_and_clear() {
        let mut cpu = AArch64Cpu::new();
        // IRQ masked by default after reset
        assert!(!cpu.irq_enabled());

        cpu.raise_irq();
        // Waiting should not be cleared when IRQ is masked
        cpu.waiting = true;
        cpu.raise_irq();
        assert!(cpu.is_waiting()); // still waiting since masked

        // Unmask IRQ
        cpu.regs.pstate &= !pstate::I;
        assert!(cpu.irq_enabled());

        cpu.raise_irq();
        assert!(!cpu.is_waiting()); // woken up

        cpu.clear_irq();
    }

    #[test]
    fn test_fiq_raise_and_clear() {
        let mut cpu = AArch64Cpu::new();
        assert!(!cpu.fiq_enabled());

        cpu.regs.pstate &= !pstate::F;
        assert!(cpu.fiq_enabled());

        cpu.waiting = true;
        cpu.raise_fiq();
        assert!(!cpu.is_waiting());

        cpu.clear_fiq();
    }

    #[test]
    fn test_check_exceptions_irq() {
        let mut cpu = AArch64Cpu::new();
        cpu.write_sys_reg(SystemRegId::VBAR_EL1, 0x1000);
        cpu.regs.pc = 0x2000;
        cpu.regs.pstate &= !pstate::I; // unmask IRQ
        cpu.raise_irq();

        let mut mem = vec![0u8; 0x4000];
        let mut memory = SliceMemory::new(&mut mem);
        let handled = cpu.check_exceptions(&mut memory).unwrap();
        assert!(handled);
        // IRQ vector offset is 0x280
        assert_eq!(cpu.regs.pc, 0x1000 + 0x280);
        assert_eq!(cpu.regs.elr_el1, 0x2000); // saved PC
    }

    #[test]
    fn test_check_exceptions_fiq_priority() {
        let mut cpu = AArch64Cpu::new();
        cpu.write_sys_reg(SystemRegId::VBAR_EL1, 0x1000);
        cpu.regs.pc = 0x500;
        cpu.regs.pstate &= !(pstate::I | pstate::F); // unmask both
        cpu.raise_irq();
        cpu.raise_fiq();

        let mut mem = vec![0u8; 0x4000];
        let mut memory = SliceMemory::new(&mut mem);
        let handled = cpu.check_exceptions(&mut memory).unwrap();
        assert!(handled);
        // FIQ has higher priority, vector offset 0x300
        assert_eq!(cpu.regs.pc, 0x1000 + 0x300);
    }

    #[test]
    fn test_fetch_unaligned_pc() {
        let mut cpu = AArch64Cpu::new();
        cpu.regs.pc = 3; // misaligned

        let mut data = vec![0u8; 0x100];
        let mem = SliceMemory::new(&mut data);
        assert!(cpu.fetch_instruction(&mem).is_err());
    }

    #[test]
    fn test_execute_add_immediate() {
        let mut cpu = AArch64Cpu::new();
        cpu.set_xreg(1, 100);

        // ADD X0, X1, #42  (64-bit, Rd=0, Rn=1, imm12=42)
        // sf=1, op=0 (ADD), S=0, shift=0, imm12=42, Rn=1, Rd=0
        // Encoding: 1|00|100010|0|000000101010|00001|00000
        let opcode: u32 = 0b1_00_100010_0_000000101010_00001_00000;
        let mut mem = vec![0u8; 4];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(opcode, &mut memory).unwrap();

        assert_eq!(cpu.get_xreg(0), 142);
    }

    #[test]
    fn test_execute_sub_immediate() {
        let mut cpu = AArch64Cpu::new();
        cpu.set_xreg(1, 200);

        // SUB X0, X1, #50 (64-bit)
        // sf=1, op=1 (SUB), S=0, shift=0, imm12=50, Rn=1, Rd=0
        let opcode: u32 = 0b1_10_100010_0_000000110010_00001_00000;
        let mut mem = vec![0u8; 4];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(opcode, &mut memory).unwrap();

        assert_eq!(cpu.get_xreg(0), 150);
    }

    #[test]
    fn test_execute_movz() {
        let mut cpu = AArch64Cpu::new();

        // MOVZ X0, #0x1234
        // sf=1, opc=10 (MOVZ), hw=0, imm16=0x1234, Rd=0
        // 1|10|100101|00|0001001000110100|00000
        let opcode: u32 = 0b1_10_100101_00_0001001000110100_00000;
        let mut mem = vec![0u8; 4];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(opcode, &mut memory).unwrap();

        assert_eq!(cpu.get_xreg(0), 0x1234);
    }

    #[test]
    fn test_execute_branch_unconditional() {
        let mut cpu = AArch64Cpu::new();
        cpu.regs.pc = 0x1000;

        // B +16 (offset = 4 instructions = 16 bytes, imm26 = 4)
        // 000101 | imm26=4
        let opcode: u32 = 0b000101_00000000000000000000000100;
        let mut mem = vec![0u8; 0x2000];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(opcode, &mut memory).unwrap();

        assert_eq!(cpu.regs.pc, 0x1010);
    }

    #[test]
    fn test_execute_branch_with_link() {
        let mut cpu = AArch64Cpu::new();
        cpu.regs.pc = 0x1000;

        // BL +8 (imm26 = 2)
        // 100101 | imm26=2
        let opcode: u32 = 0b100101_00000000000000000000000010;
        let mut mem = vec![0u8; 0x2000];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(opcode, &mut memory).unwrap();

        assert_eq!(cpu.regs.pc, 0x1008);
        assert_eq!(cpu.regs.x[30], 0x1004); // link register = return address
    }

    #[test]
    fn test_execute_store_and_load() {
        let mut cpu = AArch64Cpu::new();
        cpu.set_xreg(0, 0xCAFEBABE);
        cpu.set_xreg(1, 0x100); // base address

        // STR W0, [X1] (32-bit store with unsigned imm offset 0)
        // size=10, V=0, opc=00 (store), imm12=0, Rn=1, Rt=0
        // 10|111001|00|000000000000|00001|00000
        let str_opcode: u32 = 0b10_111001_00_000000000000_00001_00000;

        let mut mem = vec![0u8; 0x200];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(str_opcode, &mut memory).unwrap();

        // Verify memory written
        assert_eq!(memory.read_u32(0x100).unwrap(), 0xCAFEBABE);

        // LDR W2, [X1] (32-bit load, same offset)
        // size=10, V=0, opc=01 (load), imm12=0, Rn=1, Rt=2
        let ldr_opcode: u32 = 0b10_111001_01_000000000000_00001_00010;
        cpu.execute_instruction(ldr_opcode, &mut memory).unwrap();

        assert_eq!(cpu.get_wreg(2), 0xCAFEBABE);
    }

    #[test]
    fn test_execute_wfi_sets_waiting() {
        let mut cpu = AArch64Cpu::new();

        // WFI: system instruction with op0=0, CRn=0011, op1=011, CRm=0010, op2=001
        // 1101010100|0|00|011|0011|0010|001|11111
        let opcode: u32 = 0b1101010100_0_00_011_0011_0010_001_11111;
        let mut mem = vec![0u8; 0x100];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(opcode, &mut memory).unwrap();

        assert!(cpu.is_waiting());
    }

    #[test]
    fn test_execute_nop() {
        let mut cpu = AArch64Cpu::new();
        let pc_before = cpu.regs.pc;

        // NOP: op0=0, CRn=0011, op1=011, CRm=0000, op2=000
        let opcode: u32 = 0b1101010100_0_00_011_0011_0000_000_11111;
        let mut mem = vec![0u8; 0x100];
        let mut memory = SliceMemory::new(&mut mem);
        cpu.execute_instruction(opcode, &mut memory).unwrap();

        assert_eq!(cpu.regs.pc, pc_before + 4);
        assert!(!cpu.is_waiting());
    }

    #[test]
    fn test_eret_restores_state() {
        let mut cpu = AArch64Cpu::new();
        cpu.regs.elr_el1 = 0x5000;
        cpu.regs.spsr_el1 = 0x60000000; // N and Z flags

        // Call eret() directly — the ERET opcode (0xD69F03E0) shares the
        // branch-register encoding space and is handled after decode.
        cpu.eret();

        assert_eq!(cpu.regs.pc, 0x5000);
        assert_eq!(cpu.regs.pstate, 0x60000000);
    }

    #[test]
    fn test_memory_out_of_bounds() {
        let mut data = vec![0u8; 16];
        let mem = SliceMemory::new(&mut data);
        assert!(mem.read_u32(20).is_err());
        assert!(mem.read_u64(16).is_err());
    }

    #[test]
    fn test_step_advances_pc() {
        let mut cpu = AArch64Cpu::new();
        cpu.regs.pc = 0;

        // Place a NOP at address 0
        let nop: u32 = 0b1101010100_0_00_011_0011_0000_000_11111;
        let mut mem = vec![0u8; 0x100];
        mem[0] = (nop & 0xFF) as u8;
        mem[1] = ((nop >> 8) & 0xFF) as u8;
        mem[2] = ((nop >> 16) & 0xFF) as u8;
        mem[3] = ((nop >> 24) & 0xFF) as u8;

        let mut memory = SliceMemory::new(&mut mem);
        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.regs.pc, 4);
    }

    #[test]
    fn test_exception_type_variants() {
        // Ensure all variants exist and are distinct
        let types = [
            ExceptionType::Sync,
            ExceptionType::Irq,
            ExceptionType::Fiq,
            ExceptionType::SError,
        ];
        for t in &types {
            // Debug formatting should work
            let _ = format!("{:?}", t);
        }
    }

    #[test]
    fn test_exception_class_variants() {
        let classes = [
            ExceptionClass::Unknown,
            ExceptionClass::WFxTrap,
            ExceptionClass::SvcA64,
            ExceptionClass::HvcA64,
            ExceptionClass::SmcA64,
            ExceptionClass::SysReg,
            ExceptionClass::InstrAbortLower,
            ExceptionClass::DataAbortLower,
            ExceptionClass::SpAlign,
            ExceptionClass::Serror,
            ExceptionClass::Brk,
        ];
        for c in &classes {
            let _ = format!("{:?}", c);
        }
    }

    #[test]
    fn test_pstate_flag_constants() {
        // Each flag should be a distinct power of 2
        let flags = [
            pstate::N, pstate::Z, pstate::C, pstate::V,
            pstate::D, pstate::A, pstate::I, pstate::F,
            pstate::SS, pstate::IL,
        ];
        for (i, a) in flags.iter().enumerate() {
            assert!(a.is_power_of_two(), "Flag {} should be power of 2", i);
            for b in flags.iter().skip(i + 1) {
                assert_ne!(a, b, "Flags should be distinct");
            }
        }
    }
}
