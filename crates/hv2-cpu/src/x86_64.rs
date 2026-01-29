//! x86-64 CPU emulation

use crate::{CpuError, Result};

/// x86-64 general-purpose registers
#[derive(Debug, Clone, Copy, Default)]
pub struct X86Registers {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

/// x86-64 CPU flags
#[allow(dead_code)]
pub mod flags {
    pub const CF: u64 = 1 << 0; // Carry Flag
    pub const PF: u64 = 1 << 2; // Parity Flag
    pub const AF: u64 = 1 << 4; // Auxiliary Carry Flag
    pub const ZF: u64 = 1 << 6; // Zero Flag
    pub const SF: u64 = 1 << 7; // Sign Flag
    pub const TF: u64 = 1 << 8; // Trap Flag
    pub const IF: u64 = 1 << 9; // Interrupt Enable Flag
    pub const DF: u64 = 1 << 10; // Direction Flag
    pub const OF: u64 = 1 << 11; // Overflow Flag
}

/// x86-64 CPU emulator
pub struct X86_64Cpu {
    regs: X86Registers,
    halted: bool,
}

impl X86_64Cpu {
    pub fn new() -> Self {
        Self {
            regs: X86Registers::default(),
            halted: false,
        }
    }

    /// Get registers
    pub fn registers(&self) -> &X86Registers {
        &self.regs
    }

    /// Get mutable registers
    pub fn registers_mut(&mut self) -> &mut X86Registers {
        &mut self.regs
    }

    /// Check if CPU is halted
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Reset the CPU
    pub fn reset(&mut self) {
        self.regs = X86Registers::default();
        self.regs.rip = 0xFFF0; // x86 reset vector
        self.halted = false;
    }

    /// Execute instruction with memory access
    pub fn execute_with_memory(&mut self, memory: &mut [u8]) -> Result<()> {
        if self.halted {
            return Ok(());
        }

        let rip = self.regs.rip as usize;
        if rip >= memory.len() {
            return Err(CpuError::InvalidMemoryAccess.into());
        }

        let opcode = memory[rip];
        self.execute_instruction_detailed(opcode, &memory[rip..])
    }

    /// Execute a single instruction with immediate values
    fn execute_instruction_detailed(&mut self, opcode: u8, bytes: &[u8]) -> Result<()> {
        match opcode {
            // NOP (0x90)
            0x90 => {
                self.regs.rip += 1;
            }

            // HLT (0xF4)
            0xF4 => {
                self.halted = true;
                self.regs.rip += 1;
            }

            // MOV AL, imm8 (0xB0)
            0xB0 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                self.regs.rax = (self.regs.rax & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV CL, imm8 (0xB1)
            0xB1 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                self.regs.rcx = (self.regs.rcx & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV DL, imm8 (0xB2)
            0xB2 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                self.regs.rdx = (self.regs.rdx & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV BL, imm8 (0xB3)
            0xB3 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                self.regs.rbx = (self.regs.rbx & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV EAX, imm32 (0xB8)
            0xB8 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                self.regs.rax = imm as u64;
                self.regs.rip += 5;
            }

            // MOV ECX, imm32 (0xB9)
            0xB9 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                self.regs.rcx = imm as u64;
                self.regs.rip += 5;
            }

            // INC EAX (0x40)
            0x40 => {
                let old = self.regs.rax as u32;
                let new = old.wrapping_add(1);
                self.regs.rax = new as u64;
                self.update_flags_add(new as u64, old as u64, 1);
                self.regs.rip += 1;
            }

            // INC ECX (0x41)
            0x41 => {
                let old = self.regs.rcx as u32;
                let new = old.wrapping_add(1);
                self.regs.rcx = new as u64;
                self.update_flags_add(new as u64, old as u64, 1);
                self.regs.rip += 1;
            }

            // DEC EAX (0x48)
            0x48 => {
                let old = self.regs.rax as u32;
                let new = old.wrapping_sub(1);
                self.regs.rax = new as u64;
                self.update_flags_sub(new as u64, old as u64, 1);
                self.regs.rip += 1;
            }

            // DEC ECX (0x49)
            0x49 => {
                let old = self.regs.rcx as u32;
                let new = old.wrapping_sub(1);
                self.regs.rcx = new as u64;
                self.update_flags_sub(new as u64, old as u64, 1);
                self.regs.rip += 1;
            }

            // PUSH EAX (0x50)
            0x50 => {
                self.push_u64(self.regs.rax);
                self.regs.rip += 1;
            }

            // PUSH ECX (0x51)
            0x51 => {
                self.push_u64(self.regs.rcx);
                self.regs.rip += 1;
            }

            // POP EAX (0x58)
            0x58 => {
                self.regs.rax = self.pop_u64();
                self.regs.rip += 1;
            }

            // POP ECX (0x59)
            0x59 => {
                self.regs.rcx = self.pop_u64();
                self.regs.rip += 1;
            }

            // XOR RAX, RAX (0x31 0xC0)
            0x31 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                // Simple implementation - zero RAX
                self.regs.rax = 0;
                self.regs.rflags &= !(flags::CF | flags::OF);
                self.regs.rflags |= flags::ZF;
                self.regs.rip += 2;
            }

            // CMP AL, imm8 (0x3C)
            0x3C => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                let al = (self.regs.rax & 0xFF) as u8;
                let imm = bytes[1];
                let result = al.wrapping_sub(imm);
                self.update_flags_sub(result as u64, al as u64, imm as u64);
                self.regs.rip += 2;
            }

            // TEST AL, imm8 (0xA8)
            0xA8 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                let al = (self.regs.rax & 0xFF) as u8;
                let imm = bytes[1];
                let result = al & imm;
                self.update_flags_logic(result as u64);
                self.regs.rip += 2;
            }

            // RET (0xC3)
            0xC3 => {
                self.regs.rip = self.pop_u64();
            }

            // INT imm8 (0xCD)
            0xCD => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction.into());
                }
                let _int_num = bytes[1];
                // TODO: Handle interrupt
                self.regs.rip += 2;
            }

            // Unknown opcode
            _ => {
                return Err(CpuError::UnsupportedInstruction(format!("0x{:02X}", opcode)).into());
            }
        }

        Ok(())
    }

    /// Legacy execute instruction (for tests)
    pub fn execute_instruction(&mut self, opcode: u8) -> Result<()> {
        let bytes = [opcode, 0, 0, 0, 0, 0, 0, 0, 0];
        self.execute_instruction_detailed(opcode, &bytes)
    }

    /// Push value onto stack
    fn push_u64(&mut self, value: u64) {
        self.regs.rsp = self.regs.rsp.wrapping_sub(8);
        // TODO: Write to memory at RSP
    }

    /// Pop value from stack
    fn pop_u64(&mut self) -> u64 {
        let value = 0; // TODO: Read from memory at RSP
        self.regs.rsp = self.regs.rsp.wrapping_add(8);
        value
    }

    /// Update arithmetic flags after addition
    fn update_flags_add(&mut self, result: u64, op1: u64, op2: u64) {
        // Zero flag
        if result == 0 {
            self.regs.rflags |= flags::ZF;
        } else {
            self.regs.rflags &= !flags::ZF;
        }

        // Sign flag
        if (result & (1 << 63)) != 0 {
            self.regs.rflags |= flags::SF;
        } else {
            self.regs.rflags &= !flags::SF;
        }

        // Carry flag (unsigned overflow)
        if result < op1 {
            self.regs.rflags |= flags::CF;
        } else {
            self.regs.rflags &= !flags::CF;
        }

        // Overflow flag (signed overflow)
        let sign_op1 = (op1 & (1 << 63)) != 0;
        let sign_op2 = (op2 & (1 << 63)) != 0;
        let sign_result = (result & (1 << 63)) != 0;

        if sign_op1 == sign_op2 && sign_op1 != sign_result {
            self.regs.rflags |= flags::OF;
        } else {
            self.regs.rflags &= !flags::OF;
        }
    }

    /// Update arithmetic flags after subtraction
    fn update_flags_sub(&mut self, result: u64, op1: u64, op2: u64) {
        // Zero flag
        if result == 0 {
            self.regs.rflags |= flags::ZF;
        } else {
            self.regs.rflags &= !flags::ZF;
        }

        // Sign flag
        if (result & (1 << 63)) != 0 {
            self.regs.rflags |= flags::SF;
        } else {
            self.regs.rflags &= !flags::SF;
        }

        // Carry flag (borrow)
        if op1 < op2 {
            self.regs.rflags |= flags::CF;
        } else {
            self.regs.rflags &= !flags::CF;
        }

        // Overflow flag
        let sign_op1 = (op1 & (1 << 63)) != 0;
        let sign_op2 = (op2 & (1 << 63)) != 0;
        let sign_result = (result & (1 << 63)) != 0;

        if sign_op1 != sign_op2 && sign_op1 != sign_result {
            self.regs.rflags |= flags::OF;
        } else {
            self.regs.rflags &= !flags::OF;
        }
    }

    /// Update flags after logical operation
    fn update_flags_logic(&mut self, result: u64) {
        // Clear OF and CF for logical ops
        self.regs.rflags &= !(flags::OF | flags::CF);

        // Zero flag
        if result == 0 {
            self.regs.rflags |= flags::ZF;
        } else {
            self.regs.rflags &= !flags::ZF;
        }

        // Sign flag
        if (result & (1 << 63)) != 0 {
            self.regs.rflags |= flags::SF;
        } else {
            self.regs.rflags &= !flags::SF;
        }
    }

    /// Fetch and execute one instruction from memory
    pub fn step(&mut self, _memory: &[u8]) -> Result<()> {
        // TODO: Fetch instruction from memory at RIP
        // For now, just execute a NOP
        self.execute_instruction(0x90)
    }
}

impl Default for X86_64Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nop() {
        let mut cpu = X86_64Cpu::new();
        let rip = cpu.regs.rip;
        cpu.execute_instruction(0x90).unwrap();
        assert_eq!(cpu.regs.rip, rip + 1);
    }

    #[test]
    fn test_hlt() {
        let mut cpu = X86_64Cpu::new();
        cpu.execute_instruction(0xF4).unwrap();
        assert!(cpu.is_halted());
    }

    #[test]
    fn test_xor_zero() {
        let mut cpu = X86_64Cpu::new();
        cpu.regs.rax = 0x1234;
        cpu.execute_instruction(0x31).unwrap(); // XOR RAX, RAX
        assert_eq!(cpu.regs.rax, 0);
        assert!(cpu.regs.rflags & flags::ZF != 0);
    }
}
