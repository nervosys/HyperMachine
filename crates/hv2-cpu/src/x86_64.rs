//! x86-64 CPU emulation
//!
//! This module provides a software x86-64 CPU emulator for executing
//! guest code. It supports common instructions, interrupt handling,
//! and memory access.

use crate::{CpuError, Result};

// ============================================================================
// x86 architecture constants (Intel SDM Vol 3, §9.1.4)
// ============================================================================

/// Reset vector: IP value after processor RESET (real mode entry point).
const X86_RESET_VECTOR: u64 = 0xFFF0;
/// RFLAGS reserved bit 1 — always set per Intel specification.
const RFLAGS_RESERVED_BIT1: u64 = 0x2;
/// CS selector after processor RESET.
const RESET_CS_SELECTOR: u16 = 0xF000;
/// CS base address after processor RESET.
const RESET_CS_BASE: u64 = 0xFFFF_0000;
/// Real-mode IDT limit (256 vectors × 4 bytes − 1).
const REAL_MODE_IDT_LIMIT: u16 = 0x3FF;

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

/// x86-64 segment register
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentRegister {
    pub selector: u16,
    pub base: u64,
    pub limit: u32,
    pub flags: u16,
}

/// x86-64 segment registers
#[derive(Debug, Clone, Copy, Default)]
pub struct X86Segments {
    pub cs: SegmentRegister,
    pub ds: SegmentRegister,
    pub es: SegmentRegister,
    pub fs: SegmentRegister,
    pub gs: SegmentRegister,
    pub ss: SegmentRegister,
}

/// x86-64 control registers
#[derive(Debug, Clone, Copy, Default)]
pub struct X86ControlRegs {
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,
}

/// Interrupt Descriptor Table entry
#[derive(Debug, Clone, Copy, Default)]
pub struct IdtEntry {
    /// Offset bits 0-15
    pub offset_low: u16,
    /// Code segment selector
    pub selector: u16,
    /// IST (Interrupt Stack Table) index
    pub ist: u8,
    /// Type and attributes
    pub type_attr: u8,
    /// Offset bits 16-31
    pub offset_mid: u16,
    /// Offset bits 32-63
    pub offset_high: u32,
    /// Reserved
    pub reserved: u32,
}

impl IdtEntry {
    /// Get the full handler address
    #[inline]
    pub const fn handler_address(&self) -> u64 {
        (self.offset_low as u64)
            | ((self.offset_mid as u64) << 16)
            | ((self.offset_high as u64) << 32)
    }

    /// Check if entry is present
    #[inline]
    pub const fn is_present(&self) -> bool {
        (self.type_attr & 0x80) != 0
    }

    /// Get the gate type
    #[inline]
    pub const fn gate_type(&self) -> u8 {
        self.type_attr & 0x0F
    }

    /// Check if it's a trap gate (doesn't clear IF)
    #[inline]
    pub const fn is_trap_gate(&self) -> bool {
        self.gate_type() == 0x0F
    }

    /// Get the DPL (Descriptor Privilege Level)
    #[inline]
    pub const fn dpl(&self) -> u8 {
        (self.type_attr >> 5) & 0x03
    }
}

/// IDT Register
#[derive(Debug, Clone, Copy, Default)]
pub struct IdtRegister {
    pub limit: u16,
    pub base: u64,
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
    pub const IOPL_MASK: u64 = 3 << 12; // I/O Privilege Level
    pub const NT: u64 = 1 << 14; // Nested Task Flag
    pub const RF: u64 = 1 << 16; // Resume Flag
    pub const VM: u64 = 1 << 17; // Virtual-8086 Mode
    pub const AC: u64 = 1 << 18; // Alignment Check
    pub const VIF: u64 = 1 << 19; // Virtual Interrupt Flag
    pub const VIP: u64 = 1 << 20; // Virtual Interrupt Pending
    pub const ID: u64 = 1 << 21; // ID Flag
}

/// Memory access trait for CPU operations
pub trait MemoryAccess {
    /// Read a byte from memory
    fn read_u8(&self, addr: u64) -> Result<u8>;
    /// Read a word from memory
    fn read_u16(&self, addr: u64) -> Result<u16>;
    /// Read a dword from memory
    fn read_u32(&self, addr: u64) -> Result<u32>;
    /// Read a qword from memory
    fn read_u64(&self, addr: u64) -> Result<u64>;
    /// Write a byte to memory
    fn write_u8(&mut self, addr: u64, value: u8) -> Result<()>;
    /// Write a word to memory
    fn write_u16(&mut self, addr: u64, value: u16) -> Result<()>;
    /// Write a dword to memory
    fn write_u32(&mut self, addr: u64, value: u32) -> Result<()>;
    /// Write a qword to memory
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
        if addr < self.data.len() {
            Ok(self.data[addr])
        } else {
            Err(CpuError::InvalidMemoryAccess)
        }
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

/// Pending interrupt
#[derive(Debug, Clone, Copy)]
pub struct PendingInterrupt {
    /// Interrupt vector (0-255)
    pub vector: u8,
    /// Error code (for some exceptions)
    pub error_code: Option<u32>,
    /// Is NMI (Non-Maskable Interrupt)
    pub is_nmi: bool,
}

/// x86-64 CPU emulator
pub struct X86_64Cpu {
    /// General-purpose registers
    regs: X86Registers,
    /// Segment registers
    segments: X86Segments,
    /// Control registers
    control: X86ControlRegs,
    /// IDT register
    idtr: IdtRegister,
    /// CPU is halted
    halted: bool,
    /// Pending interrupts queue
    pending_interrupts: Vec<PendingInterrupt>,
    /// Current privilege level
    cpl: u8,
    /// In long mode (64-bit)
    long_mode: bool,
}

impl X86_64Cpu {
    #[must_use]
    pub fn new() -> Self {
        let mut cpu = Self {
            regs: X86Registers::default(),
            segments: X86Segments::default(),
            control: X86ControlRegs::default(),
            idtr: IdtRegister::default(),
            halted: false,
            pending_interrupts: Vec::new(),
            cpl: 0,
            long_mode: false,
        };
        cpu.reset();
        cpu
    }

    /// Get registers
    pub fn registers(&self) -> &X86Registers {
        &self.regs
    }

    /// Get mutable registers
    pub fn registers_mut(&mut self) -> &mut X86Registers {
        &mut self.regs
    }

    /// Get segment registers
    pub fn segments(&self) -> &X86Segments {
        &self.segments
    }

    /// Get control registers
    pub fn control_registers(&self) -> &X86ControlRegs {
        &self.control
    }

    /// Check if CPU is halted
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Check if interrupts are enabled
    pub fn interrupts_enabled(&self) -> bool {
        (self.regs.rflags & flags::IF) != 0
    }

    /// Reset the CPU to initial state
    pub fn reset(&mut self) {
        self.regs = X86Registers::default();
        self.regs.rip = X86_RESET_VECTOR;
        self.regs.rflags = RFLAGS_RESERVED_BIT1;

        // Initialize segment registers (real mode defaults)
        self.segments = X86Segments::default();
        self.segments.cs.selector = RESET_CS_SELECTOR;
        self.segments.cs.base = RESET_CS_BASE;

        // Control registers
        self.control = X86ControlRegs::default();

        // IDT at 0
        self.idtr = IdtRegister {
            limit: REAL_MODE_IDT_LIMIT,
            base: 0,
        };

        self.halted = false;
        self.pending_interrupts.clear();
        self.cpl = 0;
        self.long_mode = false;
    }

    /// Queue an interrupt
    pub fn queue_interrupt(&mut self, vector: u8, error_code: Option<u32>) {
        self.pending_interrupts.push(PendingInterrupt {
            vector,
            error_code,
            is_nmi: vector == 2,
        });
    }

    /// Queue an NMI
    pub fn queue_nmi(&mut self) {
        self.pending_interrupts.push(PendingInterrupt {
            vector: 2,
            error_code: None,
            is_nmi: true,
        });
    }

    /// Check if there are pending interrupts
    pub fn has_pending_interrupt(&self) -> bool {
        if self.pending_interrupts.is_empty() {
            return false;
        }

        // NMIs are always deliverable
        if self.pending_interrupts.iter().any(|i| i.is_nmi) {
            return true;
        }

        // Regular interrupts require IF to be set
        self.interrupts_enabled()
    }

    /// Handle pending interrupt
    pub fn handle_interrupt<M: MemoryAccess>(&mut self, memory: &mut M) -> Result<bool> {
        if !self.has_pending_interrupt() {
            return Ok(false);
        }

        // Find the highest priority interrupt (NMIs first)
        let idx = self
            .pending_interrupts
            .iter()
            .position(|i| i.is_nmi)
            .or_else(|| {
                if self.interrupts_enabled() {
                    Some(0)
                } else {
                    None
                }
            });

        let Some(idx) = idx else {
            return Ok(false);
        };

        let interrupt = self.pending_interrupts.remove(idx);
        self.deliver_interrupt(memory, interrupt)?;

        // Wake from halt on interrupt
        if self.halted {
            self.halted = false;
            self.regs.rip = self.regs.rip.wrapping_add(1); // Skip HLT instruction
        }

        Ok(true)
    }

    /// Deliver an interrupt to the CPU
    fn deliver_interrupt<M: MemoryAccess>(
        &mut self,
        memory: &mut M,
        interrupt: PendingInterrupt,
    ) -> Result<()> {
        tracing::trace!("Delivering interrupt {}", interrupt.vector);

        if self.long_mode {
            self.deliver_interrupt_long_mode(memory, interrupt)
        } else {
            self.deliver_interrupt_real_mode(memory, interrupt)
        }
    }

    /// Deliver interrupt in real mode
    fn deliver_interrupt_real_mode<M: MemoryAccess>(
        &mut self,
        memory: &mut M,
        interrupt: PendingInterrupt,
    ) -> Result<()> {
        // Read IVT entry (4 bytes per entry: offset:segment)
        let ivt_addr = (interrupt.vector as u64) * 4;
        let offset = memory.read_u16(ivt_addr)?;
        let segment = memory.read_u16(ivt_addr + 2)?;

        // Push flags, CS, IP
        self.push_with_memory(memory, self.regs.rflags as u16)?;
        self.push_with_memory(memory, self.segments.cs.selector)?;
        self.push_with_memory(memory, self.regs.rip as u16)?;

        // Clear IF and TF
        self.regs.rflags &= !(flags::IF | flags::TF);

        // Jump to handler
        self.segments.cs.selector = segment;
        self.segments.cs.base = (segment as u64) << 4;
        self.regs.rip = offset as u64;

        Ok(())
    }

    /// Deliver interrupt in long mode (64-bit)
    fn deliver_interrupt_long_mode<M: MemoryAccess>(
        &mut self,
        memory: &mut M,
        interrupt: PendingInterrupt,
    ) -> Result<()> {
        // Check IDT bounds
        let entry_offset = (interrupt.vector as u64) * 16;
        if entry_offset + 15 > self.idtr.limit as u64 {
            return Err(CpuError::InvalidInterrupt(interrupt.vector));
        }

        // Read IDT entry
        let entry_addr = self.idtr.base + entry_offset;
        let offset_low = memory.read_u16(entry_addr)?;
        let selector = memory.read_u16(entry_addr + 2)?;
        let ist_type = memory.read_u16(entry_addr + 4)?;
        let offset_mid = memory.read_u16(entry_addr + 6)?;
        let offset_high = memory.read_u32(entry_addr + 8)?;

        let entry = IdtEntry {
            offset_low,
            selector,
            ist: (ist_type & 0x07) as u8,
            type_attr: (ist_type >> 8) as u8,
            offset_mid,
            offset_high,
            reserved: 0,
        };

        if !entry.is_present() {
            return Err(CpuError::InvalidInterrupt(interrupt.vector));
        }

        let handler = entry.handler_address();

        // Save state on stack
        // In 64-bit mode, push: SS, RSP, RFLAGS, CS, RIP, (error code if present)
        self.push_u64_with_memory(memory, self.segments.ss.selector as u64)?;
        self.push_u64_with_memory(memory, self.regs.rsp)?;
        self.push_u64_with_memory(memory, self.regs.rflags)?;
        self.push_u64_with_memory(memory, self.segments.cs.selector as u64)?;
        self.push_u64_with_memory(memory, self.regs.rip)?;

        if let Some(error_code) = interrupt.error_code {
            self.push_u64_with_memory(memory, error_code as u64)?;
        }

        // Update CS
        self.segments.cs.selector = selector;

        // Clear IF if not a trap gate
        if !entry.is_trap_gate() {
            self.regs.rflags &= !flags::IF;
        }

        // Clear TF, NT, VM, RF
        self.regs.rflags &= !(flags::TF | flags::NT | flags::VM | flags::RF);

        // Jump to handler
        self.regs.rip = handler;

        Ok(())
    }

    /// Push 16-bit value with memory access
    fn push_with_memory<M: MemoryAccess>(&mut self, memory: &mut M, value: u16) -> Result<()> {
        self.regs.rsp = self.regs.rsp.wrapping_sub(2);
        memory.write_u16(self.regs.rsp, value)
    }

    /// Push 64-bit value with memory access
    fn push_u64_with_memory<M: MemoryAccess>(&mut self, memory: &mut M, value: u64) -> Result<()> {
        self.regs.rsp = self.regs.rsp.wrapping_sub(8);
        memory.write_u64(self.regs.rsp, value)
    }

    /// Pop 16-bit value with memory access
    fn pop_with_memory<M: MemoryAccess>(&mut self, memory: &M) -> Result<u16> {
        let value = memory.read_u16(self.regs.rsp)?;
        self.regs.rsp = self.regs.rsp.wrapping_add(2);
        Ok(value)
    }

    /// Pop 64-bit value with memory access
    fn pop_u64_with_memory<M: MemoryAccess>(&mut self, memory: &M) -> Result<u64> {
        let value = memory.read_u64(self.regs.rsp)?;
        self.regs.rsp = self.regs.rsp.wrapping_add(8);
        Ok(value)
    }

    /// Fetch instruction bytes from memory
    pub fn fetch_instruction<M: MemoryAccess>(
        &self,
        memory: &M,
        max_len: usize,
    ) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(max_len);
        let rip = self.regs.rip;

        for i in 0..max_len {
            match memory.read_u8(rip + i as u64) {
                Ok(b) => bytes.push(b),
                Err(_) => break, // Stop at memory boundary
            }
        }

        if bytes.is_empty() {
            return Err(CpuError::InvalidMemoryAccess);
        }

        Ok(bytes)
    }

    /// Execute instruction with memory access
    pub fn execute_with_memory(&mut self, memory: &mut [u8]) -> Result<()> {
        if self.halted {
            return Ok(());
        }

        let rip = self.regs.rip as usize;
        if rip >= memory.len() {
            return Err(CpuError::InvalidMemoryAccess);
        }

        let opcode = memory[rip];
        // Copy instruction bytes before borrowing memory mutably
        let max_instr_len = std::cmp::min(16, memory.len() - rip);
        let mut instr_bytes = [0u8; 16];
        instr_bytes[..max_instr_len].copy_from_slice(&memory[rip..rip + max_instr_len]);
        let mut slice_mem = SliceMemory::new(memory);
        self.execute_instruction_with_memory(opcode, &instr_bytes[..max_instr_len], &mut slice_mem)
    }

    /// Execute a single instruction with memory support
    fn execute_instruction_with_memory<M: MemoryAccess>(
        &mut self,
        opcode: u8,
        bytes: &[u8],
        memory: &mut M,
    ) -> Result<()> {
        match opcode {
            // ================================================================
            // ADD family
            // ================================================================

            // ADD r/m64, r64 (0x01) — ModRM reg→r/m
            0x01 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let src = self.get_reg64(reg);
                    let dst = self.get_reg64(rm);
                    let result = dst.wrapping_add(src);
                    self.set_reg64(rm, result);
                    self.update_flags_add(result, dst, src);
                    self.regs.rip += 2;
                } else {
                    let (addr, consumed, _is_reg) = self.modrm_effective_addr(bytes, 1)?;
                    let dst = memory.read_u64(addr)?;
                    let src = self.get_reg64(reg);
                    let result = dst.wrapping_add(src);
                    memory.write_u64(addr, result)?;
                    self.update_flags_add(result, dst, src);
                    self.regs.rip += 1 + consumed as u64;
                }
            }

            // ADD r64, r/m64 (0x03) — ModRM r/m→reg
            0x03 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let (src, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let dst = self.get_reg64(reg);
                let result = dst.wrapping_add(src);
                self.set_reg64(reg, result);
                self.update_flags_add(result, dst, src);
                self.regs.rip += 1 + consumed as u64;
            }

            // ADD EAX, imm32 (0x05)
            0x05 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
                let dst = self.regs.rax;
                let result = dst.wrapping_add(imm);
                self.regs.rax = result;
                self.update_flags_add(result, dst, imm);
                self.regs.rip += 5;
            }

            // ADD AL, imm8 (0x04)
            0x04 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let al = (self.regs.rax & 0xFF) as u8;
                let imm = bytes[1];
                let result = al.wrapping_add(imm);
                self.regs.rax = (self.regs.rax & !0xFF) | result as u64;
                self.update_flags_add(result as u64, al as u64, imm as u64);
                self.regs.rip += 2;
            }

            // ================================================================
            // OR family
            // ================================================================

            // OR r/m64, r64 (0x09)
            0x09 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let result = self.get_reg64(rm) | self.get_reg64(reg);
                    self.set_reg64(rm, result);
                    self.update_flags_logic(result);
                    self.regs.rip += 2;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let dst = memory.read_u64(addr)?;
                    let result = dst | self.get_reg64(reg);
                    memory.write_u64(addr, result)?;
                    self.update_flags_logic(result);
                    self.regs.rip += 1 + consumed as u64;
                }
            }

            // OR r64, r/m64 (0x0B)
            0x0B => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg = (bytes[1] >> 3) & 0x07;
                let (src, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let result = self.get_reg64(reg) | src;
                self.set_reg64(reg, result);
                self.update_flags_logic(result);
                self.regs.rip += 1 + consumed as u64;
            }

            // OR EAX, imm32 (0x0D)
            0x0D => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
                let result = self.regs.rax | imm;
                self.regs.rax = result;
                self.update_flags_logic(result);
                self.regs.rip += 5;
            }

            // OR AL, imm8 (0x0C)
            0x0C => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let result = ((self.regs.rax & 0xFF) as u8) | bytes[1];
                self.regs.rax = (self.regs.rax & !0xFF) | result as u64;
                self.update_flags_logic(result as u64);
                self.regs.rip += 2;
            }

            // ================================================================
            // AND family
            // ================================================================

            // AND r/m64, r64 (0x21)
            0x21 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let result = self.get_reg64(rm) & self.get_reg64(reg);
                    self.set_reg64(rm, result);
                    self.update_flags_logic(result);
                    self.regs.rip += 2;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let dst = memory.read_u64(addr)?;
                    let result = dst & self.get_reg64(reg);
                    memory.write_u64(addr, result)?;
                    self.update_flags_logic(result);
                    self.regs.rip += 1 + consumed as u64;
                }
            }

            // AND r64, r/m64 (0x23)
            0x23 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg = (bytes[1] >> 3) & 0x07;
                let (src, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let result = self.get_reg64(reg) & src;
                self.set_reg64(reg, result);
                self.update_flags_logic(result);
                self.regs.rip += 1 + consumed as u64;
            }

            // AND EAX, imm32 (0x25)
            0x25 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
                let result = self.regs.rax & imm;
                self.regs.rax = result;
                self.update_flags_logic(result);
                self.regs.rip += 5;
            }

            // AND AL, imm8 (0x24)
            0x24 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let result = ((self.regs.rax & 0xFF) as u8) & bytes[1];
                self.regs.rax = (self.regs.rax & !0xFF) | result as u64;
                self.update_flags_logic(result as u64);
                self.regs.rip += 2;
            }

            // ================================================================
            // SUB family
            // ================================================================

            // SUB r/m64, r64 (0x29)
            0x29 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let src = self.get_reg64(reg);
                    let dst = self.get_reg64(rm);
                    let result = dst.wrapping_sub(src);
                    self.set_reg64(rm, result);
                    self.update_flags_sub(result, dst, src);
                    self.regs.rip += 2;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let dst = memory.read_u64(addr)?;
                    let src = self.get_reg64(reg);
                    let result = dst.wrapping_sub(src);
                    memory.write_u64(addr, result)?;
                    self.update_flags_sub(result, dst, src);
                    self.regs.rip += 1 + consumed as u64;
                }
            }

            // SUB r64, r/m64 (0x2B)
            0x2B => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg = (bytes[1] >> 3) & 0x07;
                let (src, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let dst = self.get_reg64(reg);
                let result = dst.wrapping_sub(src);
                self.set_reg64(reg, result);
                self.update_flags_sub(result, dst, src);
                self.regs.rip += 1 + consumed as u64;
            }

            // SUB EAX, imm32 (0x2D)
            0x2D => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
                let dst = self.regs.rax;
                let result = dst.wrapping_sub(imm);
                self.regs.rax = result;
                self.update_flags_sub(result, dst, imm);
                self.regs.rip += 5;
            }

            // SUB AL, imm8 (0x2C)
            0x2C => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let al = (self.regs.rax & 0xFF) as u8;
                let imm = bytes[1];
                let result = al.wrapping_sub(imm);
                self.regs.rax = (self.regs.rax & !0xFF) | result as u64;
                self.update_flags_sub(result as u64, al as u64, imm as u64);
                self.regs.rip += 2;
            }

            // ================================================================
            // XOR family
            // ================================================================

            // XOR r/m64, r64 (0x31)
            0x31 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let result = self.get_reg64(rm) ^ self.get_reg64(reg);
                    self.set_reg64(rm, result);
                    self.update_flags_logic(result);
                    self.regs.rip += 2;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let dst = memory.read_u64(addr)?;
                    let result = dst ^ self.get_reg64(reg);
                    memory.write_u64(addr, result)?;
                    self.update_flags_logic(result);
                    self.regs.rip += 1 + consumed as u64;
                }
            }

            // XOR r64, r/m64 (0x33)
            0x33 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg = (bytes[1] >> 3) & 0x07;
                let (src, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let result = self.get_reg64(reg) ^ src;
                self.set_reg64(reg, result);
                self.update_flags_logic(result);
                self.regs.rip += 1 + consumed as u64;
            }

            // XOR EAX, imm32 (0x35)
            0x35 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
                let result = self.regs.rax ^ imm;
                self.regs.rax = result;
                self.update_flags_logic(result);
                self.regs.rip += 5;
            }

            // XOR AL, imm8 (0x34)
            0x34 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let result = ((self.regs.rax & 0xFF) as u8) ^ bytes[1];
                self.regs.rax = (self.regs.rax & !0xFF) | result as u64;
                self.update_flags_logic(result as u64);
                self.regs.rip += 2;
            }

            // ================================================================
            // CMP family
            // ================================================================

            // CMP r/m64, r64 (0x39)
            0x39 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let (rm_val, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let src = self.get_reg64(reg);
                let result = rm_val.wrapping_sub(src);
                self.update_flags_sub(result, rm_val, src);
                self.regs.rip += 1 + consumed as u64;
            }

            // CMP r64, r/m64 (0x3B)
            0x3B => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg = (bytes[1] >> 3) & 0x07;
                let (src, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let dst = self.get_reg64(reg);
                let result = dst.wrapping_sub(src);
                self.update_flags_sub(result, dst, src);
                self.regs.rip += 1 + consumed as u64;
            }

            // CMP AL, imm8 (0x3C)
            0x3C => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let al = (self.regs.rax & 0xFF) as u8;
                let imm = bytes[1];
                let result = al.wrapping_sub(imm);
                self.update_flags_sub(result as u64, al as u64, imm as u64);
                self.regs.rip += 2;
            }

            // CMP EAX, imm32 (0x3D)
            0x3D => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
                let dst = self.regs.rax;
                let result = dst.wrapping_sub(imm);
                self.update_flags_sub(result, dst, imm);
                self.regs.rip += 5;
            }

            // ================================================================
            // INC/DEC single-byte (legacy, non-long-mode only)
            // In long mode 0x40-0x4F are REX prefixes
            // ================================================================

            // INC r32 (0x40-0x47) / REX prefix in long mode
            0x40..=0x47 => {
                if !self.long_mode {
                    let reg_idx = opcode - 0x40;
                    let old = self.get_reg32(reg_idx);
                    let new = old.wrapping_add(1);
                    self.set_reg32(reg_idx, new);
                    // INC preserves CF
                    let cf = self.regs.rflags & flags::CF;
                    self.update_flags_add(new as u64, old as u64, 1);
                    self.regs.rflags = (self.regs.rflags & !flags::CF) | cf;
                    self.regs.rip += 1;
                } else {
                    // REX prefix — in a full decoder we'd parse the next
                    // opcode with the prefix applied. For now skip it.
                    self.regs.rip += 1;
                }
            }

            // DEC r32 (0x48-0x4F) / REX prefix in long mode
            0x48..=0x4F => {
                if !self.long_mode {
                    let reg_idx = opcode - 0x48;
                    let old = self.get_reg32(reg_idx);
                    let new = old.wrapping_sub(1);
                    self.set_reg32(reg_idx, new);
                    let cf = self.regs.rflags & flags::CF;
                    self.update_flags_sub(new as u64, old as u64, 1);
                    self.regs.rflags = (self.regs.rflags & !flags::CF) | cf;
                    self.regs.rip += 1;
                } else {
                    self.regs.rip += 1;
                }
            }

            // ================================================================
            // PUSH r64 (0x50-0x57)
            // ================================================================
            0x50..=0x57 => {
                let reg_idx = opcode - 0x50;
                let val = self.get_reg64(reg_idx);
                self.push_u64_with_memory(memory, val)?;
                self.regs.rip += 1;
            }

            // ================================================================
            // POP r64 (0x58-0x5F)
            // ================================================================
            0x58..=0x5F => {
                let reg_idx = opcode - 0x58;
                let val = self.pop_u64_with_memory(memory)?;
                self.set_reg64(reg_idx, val);
                self.regs.rip += 1;
            }

            // ================================================================
            // PUSH imm32 (sign-extended to 64) (0x68)
            // ================================================================
            0x68 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                self.push_u64_with_memory(memory, imm as i64 as u64)?;
                self.regs.rip += 5;
            }

            // PUSH imm8 (sign-extended to 64) (0x6A)
            0x6A => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = bytes[1] as i8 as i64 as u64;
                self.push_u64_with_memory(memory, imm)?;
                self.regs.rip += 2;
            }

            // ================================================================
            // Jcc short (0x70-0x7F) — conditional jumps with rel8
            // ================================================================
            0x70..=0x7F => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let cc = opcode & 0x0F;
                let disp = bytes[1] as i8 as i64;
                self.regs.rip += 2;
                if self.check_condition(cc) {
                    self.regs.rip = self.regs.rip.wrapping_add(disp as u64);
                }
            }

            // ================================================================
            // Group 1 (ALU r/m, imm) — 0x80, 0x81, 0x83
            // ================================================================

            // Group 1: r/m8, imm8 (0x80)
            0x80 => {
                if bytes.len() < 3 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                let imm = bytes[2];
                if (modrm >> 6) == 0b11 {
                    let dst = self.get_reg8(rm);
                    let (result, update) = Self::alu_op8(op, dst, imm);
                    if op != 7 {
                        // CMP doesn't store
                        self.set_reg8(rm, result);
                    }
                    self.apply_alu_flags8(op, result, dst, imm, update);
                    self.regs.rip += 3;
                } else {
                    return Err(CpuError::UnsupportedInstruction("0x80 mem".into()));
                }
            }

            // Group 1: r/m64, imm32 (0x81)
            0x81 => {
                if bytes.len() < 6 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                let imm =
                    i32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as i64 as u64;
                if (modrm >> 6) == 0b11 {
                    let dst = self.get_reg64(rm);
                    let (result, update) = Self::alu_op64(op, dst, imm);
                    if op != 7 {
                        self.set_reg64(rm, result);
                    }
                    self.apply_alu_flags64(op, result, dst, imm, update);
                    self.regs.rip += 6;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let dst = memory.read_u64(addr)?;
                    let (result, update) = Self::alu_op64(op, dst, imm);
                    if op != 7 {
                        memory.write_u64(addr, result)?;
                    }
                    self.apply_alu_flags64(op, result, dst, imm, update);
                    self.regs.rip += 1 + consumed as u64 + 4; // modrm bytes + imm32
                }
            }

            // Group 1: r/m64, imm8 sign-extended (0x83)
            0x83 => {
                if bytes.len() < 3 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                let imm = bytes[2] as i8 as i64 as u64;
                if (modrm >> 6) == 0b11 {
                    let dst = self.get_reg64(rm);
                    let (result, update) = Self::alu_op64(op, dst, imm);
                    if op != 7 {
                        self.set_reg64(rm, result);
                    }
                    self.apply_alu_flags64(op, result, dst, imm, update);
                    self.regs.rip += 3;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let dst = memory.read_u64(addr)?;
                    let (result, update) = Self::alu_op64(op, dst, imm);
                    if op != 7 {
                        memory.write_u64(addr, result)?;
                    }
                    self.apply_alu_flags64(op, result, dst, imm, update);
                    self.regs.rip += 1 + consumed as u64 + 1; // modrm bytes + imm8
                }
            }

            // ================================================================
            // TEST r/m64, r64 (0x85)
            // ================================================================
            0x85 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let (rm_val, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                let result = rm_val & self.get_reg64(reg);
                self.update_flags_logic(result);
                self.regs.rip += 1 + consumed as u64;
            }

            // ================================================================
            // XCHG r64, r/m64 (0x87)
            // ================================================================
            0x87 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let a = self.get_reg64(reg);
                    let b = self.get_reg64(rm);
                    self.set_reg64(reg, b);
                    self.set_reg64(rm, a);
                    self.regs.rip += 2;
                } else {
                    return Err(CpuError::UnsupportedInstruction("0x87 mem".into()));
                }
            }

            // ================================================================
            // MOV family
            // ================================================================

            // MOV r/m64, r64 (0x89)
            0x89 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let src = self.get_reg64(reg);
                let consumed = self.write_modrm_rm64(bytes, 1, src, memory)?;
                self.regs.rip += 1 + consumed as u64;
            }

            // MOV r64, r/m64 (0x8B)
            0x8B => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg = (bytes[1] >> 3) & 0x07;
                let (val, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                self.set_reg64(reg, val);
                self.regs.rip += 1 + consumed as u64;
            }

            // LEA r64, m (0x8D)
            0x8D => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg = (bytes[1] >> 3) & 0x07;
                let (addr, consumed, is_reg) = self.modrm_effective_addr(bytes, 1)?;
                if is_reg {
                    // LEA with register source is undefined; treat as NOP
                    self.regs.rip += 2;
                } else {
                    self.set_reg64(reg, addr);
                    self.regs.rip += 1 + consumed as u64;
                }
            }

            // ================================================================
            // NOP (0x90) / XCHG eAX, r32 (0x91-0x97)
            // ================================================================
            0x90 => {
                self.regs.rip += 1;
            }
            0x91..=0x97 => {
                let reg_idx = opcode - 0x90;
                let a = self.regs.rax;
                let b = self.get_reg64(reg_idx);
                self.regs.rax = b;
                self.set_reg64(reg_idx, a);
                self.regs.rip += 1;
            }

            // ================================================================
            // CBW/CWDE/CDQE (0x98) — sign-extend AL→AX / EAX→RAX
            // ================================================================
            0x98 => {
                // In 64-bit with no REX.W this is CWDE (EAX sign-extend to RAX lower 32)
                // Simplified: sign-extend EAX to RAX
                self.regs.rax = self.regs.rax as i32 as i64 as u64;
                self.regs.rip += 1;
            }

            // CWD/CDQ/CQO (0x99) — sign-extend EAX into EDX:EAX
            0x99 => {
                if (self.regs.rax as i32) < 0 {
                    self.regs.rdx = 0xFFFF_FFFF_FFFF_FFFF;
                } else {
                    self.regs.rdx = 0;
                }
                self.regs.rip += 1;
            }

            // ================================================================
            // TEST AL, imm8 (0xA8)
            // ================================================================
            0xA8 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let al = (self.regs.rax & 0xFF) as u8;
                let imm = bytes[1];
                let result = al & imm;
                self.update_flags_logic(result as u64);
                self.regs.rip += 2;
            }

            // TEST EAX, imm32 (0xA9)
            0xA9 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
                let result = self.regs.rax & imm;
                self.update_flags_logic(result);
                self.regs.rip += 5;
            }

            // ================================================================
            // MOV r8, imm8 (0xB0-0xB7)
            // ================================================================
            0xB0..=0xB7 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg_idx = opcode - 0xB0;
                self.set_reg8(reg_idx, bytes[1]);
                self.regs.rip += 2;
            }

            // ================================================================
            // MOV r32/r64, imm32 (0xB8-0xBF) — zero-extends in 32-bit mode
            // ================================================================
            0xB8..=0xBF => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let reg_idx = opcode - 0xB8;
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                self.set_reg32(reg_idx, imm);
                self.regs.rip += 5;
            }

            // ================================================================
            // Shift group (0xC1 r/m64, imm8 / 0xD1 r/m64, 1 / 0xD3 r/m64, CL)
            // ================================================================

            // Shift/Rotate r/m64 by imm8 (0xC1)
            0xC1 => {
                if bytes.len() < 3 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                let count = (bytes[2] & 0x3F) as u32; // masked to 6 bits for 64-bit
                if (modrm >> 6) == 0b11 {
                    let val = self.get_reg64(rm);
                    let result = self.shift_op64(op, val, count);
                    self.set_reg64(rm, result);
                    self.regs.rip += 3;
                } else {
                    return Err(CpuError::UnsupportedInstruction("0xC1 mem".into()));
                }
            }

            // Shift/Rotate r/m64 by 1 (0xD1)
            0xD1 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let val = self.get_reg64(rm);
                    let result = self.shift_op64(op, val, 1);
                    self.set_reg64(rm, result);
                    self.regs.rip += 2;
                } else {
                    return Err(CpuError::UnsupportedInstruction("0xD1 mem".into()));
                }
            }

            // Shift/Rotate r/m64 by CL (0xD3)
            0xD3 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                let count = (self.regs.rcx as u8 & 0x3F) as u32;
                if (modrm >> 6) == 0b11 {
                    let val = self.get_reg64(rm);
                    let result = self.shift_op64(op, val, count);
                    self.set_reg64(rm, result);
                    self.regs.rip += 2;
                } else {
                    return Err(CpuError::UnsupportedInstruction("0xD3 mem".into()));
                }
            }

            // ================================================================
            // RET (0xC3)
            // ================================================================
            0xC3 => {
                self.regs.rip = self.pop_u64_with_memory(memory)?;
            }

            // ================================================================
            // MOV r/m8, imm8 (0xC6) — group 11 /0
            // ================================================================
            0xC6 => {
                if bytes.len() < 3 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    self.set_reg8(rm, bytes[2]);
                    self.regs.rip += 3;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let imm_offset = 1 + consumed;
                    if imm_offset >= bytes.len() {
                        return Err(CpuError::InvalidInstruction);
                    }
                    memory.write_u8(addr, bytes[imm_offset])?;
                    self.regs.rip += 1 + consumed as u64 + 1;
                }
            }

            // MOV r/m64, imm32 (sign-extended) (0xC7)
            0xC7 => {
                if bytes.len() < 6 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let rm = modrm & 0x07;
                if (modrm >> 6) == 0b11 {
                    let imm =
                        i32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as i64 as u64;
                    self.set_reg64(rm, imm);
                    self.regs.rip += 6;
                } else {
                    let (addr, consumed, _) = self.modrm_effective_addr(bytes, 1)?;
                    let d = 1 + consumed;
                    if d + 4 > bytes.len() {
                        return Err(CpuError::InvalidInstruction);
                    }
                    let imm =
                        i32::from_le_bytes([bytes[d], bytes[d + 1], bytes[d + 2], bytes[d + 3]])
                            as i64 as u64;
                    memory.write_u64(addr, imm)?;
                    self.regs.rip += 1 + consumed as u64 + 4;
                }
            }

            // ================================================================
            // LEAVE (0xC9)
            // ================================================================
            0xC9 => {
                self.regs.rsp = self.regs.rbp;
                self.regs.rbp = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // ================================================================
            // INT imm8 (0xCD)
            // ================================================================
            0xCD => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let vector = bytes[1];
                self.regs.rip += 2;
                self.queue_interrupt(vector, None);
            }

            // ================================================================
            // IRET (0xCF)
            // ================================================================
            0xCF => {
                if self.long_mode {
                    self.regs.rip = self.pop_u64_with_memory(memory)?;
                    self.segments.cs.selector = self.pop_u64_with_memory(memory)? as u16;
                    self.regs.rflags = self.pop_u64_with_memory(memory)?;
                    self.regs.rsp = self.pop_u64_with_memory(memory)?;
                    self.segments.ss.selector = self.pop_u64_with_memory(memory)? as u16;
                } else {
                    self.regs.rip = self.pop_with_memory(memory)? as u64;
                    self.segments.cs.selector = self.pop_with_memory(memory)?;
                    self.regs.rflags =
                        (self.regs.rflags & 0xFFFF0000) | (self.pop_with_memory(memory)? as u64);
                }
            }

            // ================================================================
            // CALL rel32 (0xE8)
            // ================================================================
            0xE8 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let disp = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                let ret_addr = self.regs.rip + 5;
                self.push_u64_with_memory(memory, ret_addr)?;
                self.regs.rip = ret_addr.wrapping_add(disp as i64 as u64);
            }

            // ================================================================
            // JMP rel32 (0xE9)
            // ================================================================
            0xE9 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let disp = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                self.regs.rip = (self.regs.rip + 5).wrapping_add(disp as i64 as u64);
            }

            // ================================================================
            // JMP rel8 (0xEB)
            // ================================================================
            0xEB => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let disp = bytes[1] as i8 as i64;
                self.regs.rip = (self.regs.rip + 2).wrapping_add(disp as u64);
            }

            // ================================================================
            // HLT (0xF4)
            // ================================================================
            0xF4 => {
                self.halted = true;
                self.regs.rip += 1;
            }

            // ================================================================
            // Group 3: Unary (0xF6 r/m8 / 0xF7 r/m64)
            // TEST, NOT, NEG, MUL, IMUL, DIV, IDIV
            // ================================================================
            0xF6 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) != 0b11 {
                    return Err(CpuError::UnsupportedInstruction("0xF6 mem".into()));
                }
                match op {
                    0 | 1 => {
                        // TEST r/m8, imm8
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let val = self.get_reg8(rm);
                        let result = val & bytes[2];
                        self.update_flags_logic(result as u64);
                        self.regs.rip += 3;
                    }
                    2 => {
                        // NOT r/m8
                        self.set_reg8(rm, !self.get_reg8(rm));
                        self.regs.rip += 2;
                    }
                    3 => {
                        // NEG r/m8
                        let val = self.get_reg8(rm);
                        let result = (val as i8).wrapping_neg() as u8;
                        self.set_reg8(rm, result);
                        self.update_flags_sub(result as u64, 0, val as u64);
                        self.regs.rip += 2;
                    }
                    4 => {
                        // MUL AL * r/m8 → AX
                        let al = (self.regs.rax & 0xFF) as u8;
                        let val = self.get_reg8(rm);
                        let result = (al as u16) * (val as u16);
                        self.regs.rax = (self.regs.rax & !0xFFFF) | result as u64;
                        self.regs.rip += 2;
                    }
                    _ => {
                        return Err(CpuError::UnsupportedInstruction(format!("0xF6 /{}", op)));
                    }
                }
            }

            0xF7 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                if (modrm >> 6) != 0b11 {
                    return Err(CpuError::UnsupportedInstruction("0xF7 mem".into()));
                }
                match op {
                    0 | 1 => {
                        // TEST r/m64, imm32
                        if bytes.len() < 6 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let imm =
                            u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as u64;
                        let val = self.get_reg64(rm);
                        self.update_flags_logic(val & imm);
                        self.regs.rip += 6;
                    }
                    2 => {
                        // NOT r/m64
                        self.set_reg64(rm, !self.get_reg64(rm));
                        self.regs.rip += 2;
                    }
                    3 => {
                        // NEG r/m64
                        let val = self.get_reg64(rm);
                        let result = (val as i64).wrapping_neg() as u64;
                        self.set_reg64(rm, result);
                        self.update_flags_sub(result, 0, val);
                        self.regs.rip += 2;
                    }
                    4 => {
                        // MUL RAX * r/m64 → RDX:RAX
                        let a = self.regs.rax as u128;
                        let b = self.get_reg64(rm) as u128;
                        let result = a * b;
                        self.regs.rax = result as u64;
                        self.regs.rdx = (result >> 64) as u64;
                        // CF=OF=1 if high half non-zero
                        if self.regs.rdx != 0 {
                            self.regs.rflags |= flags::CF | flags::OF;
                        } else {
                            self.regs.rflags &= !(flags::CF | flags::OF);
                        }
                        self.regs.rip += 2;
                    }
                    5 => {
                        // IMUL RAX * r/m64 → RDX:RAX (signed)
                        let a = self.regs.rax as i64 as i128;
                        let b = self.get_reg64(rm) as i64 as i128;
                        let result = a * b;
                        self.regs.rax = result as u64;
                        self.regs.rdx = (result >> 64) as u64;
                        let sign_ext = (self.regs.rax as i64 >> 63) as u64;
                        if self.regs.rdx != sign_ext {
                            self.regs.rflags |= flags::CF | flags::OF;
                        } else {
                            self.regs.rflags &= !(flags::CF | flags::OF);
                        }
                        self.regs.rip += 2;
                    }
                    6 => {
                        // DIV RDX:RAX / r/m64
                        let divisor = self.get_reg64(rm);
                        if divisor == 0 {
                            self.queue_interrupt(0, None); // #DE
                            return Ok(());
                        }
                        let dividend = ((self.regs.rdx as u128) << 64) | (self.regs.rax as u128);
                        let quotient = dividend / (divisor as u128);
                        let remainder = dividend % (divisor as u128);
                        if quotient > u64::MAX as u128 {
                            self.queue_interrupt(0, None); // #DE overflow
                            return Ok(());
                        }
                        self.regs.rax = quotient as u64;
                        self.regs.rdx = remainder as u64;
                        self.regs.rip += 2;
                    }
                    7 => {
                        // IDIV RDX:RAX / r/m64 (signed)
                        let divisor = self.get_reg64(rm) as i64;
                        if divisor == 0 {
                            self.queue_interrupt(0, None);
                            return Ok(());
                        }
                        let dividend =
                            ((self.regs.rdx as i128) << 64) | (self.regs.rax as u128 as i128);
                        let quotient = dividend / (divisor as i128);
                        let remainder = dividend % (divisor as i128);
                        if quotient > i64::MAX as i128 || quotient < i64::MIN as i128 {
                            self.queue_interrupt(0, None);
                            return Ok(());
                        }
                        self.regs.rax = quotient as u64;
                        self.regs.rdx = remainder as u64;
                        self.regs.rip += 2;
                    }
                    _ => unreachable!(),
                }
            }

            // ================================================================
            // CLI, STI, CLD, STD (0xFA-0xFD)
            // ================================================================
            0xFA => {
                self.regs.rflags &= !flags::IF;
                self.regs.rip += 1;
            }
            0xFB => {
                self.regs.rflags |= flags::IF;
                self.regs.rip += 1;
            }
            0xFC => {
                self.regs.rflags &= !flags::DF;
                self.regs.rip += 1;
            }
            0xFD => {
                self.regs.rflags |= flags::DF;
                self.regs.rip += 1;
            }

            // ================================================================
            // Group 5: INC/DEC/CALL/JMP r/m64 (0xFF)
            // ================================================================
            0xFF => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let op = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;
                match op {
                    0 => {
                        // INC r/m64
                        if (modrm >> 6) == 0b11 {
                            let old = self.get_reg64(rm);
                            let new = old.wrapping_add(1);
                            self.set_reg64(rm, new);
                            let cf = self.regs.rflags & flags::CF;
                            self.update_flags_add(new, old, 1);
                            self.regs.rflags = (self.regs.rflags & !flags::CF) | cf;
                            self.regs.rip += 2;
                        } else {
                            return Err(CpuError::UnsupportedInstruction("0xFF/0 mem".into()));
                        }
                    }
                    1 => {
                        // DEC r/m64
                        if (modrm >> 6) == 0b11 {
                            let old = self.get_reg64(rm);
                            let new = old.wrapping_sub(1);
                            self.set_reg64(rm, new);
                            let cf = self.regs.rflags & flags::CF;
                            self.update_flags_sub(new, old, 1);
                            self.regs.rflags = (self.regs.rflags & !flags::CF) | cf;
                            self.regs.rip += 2;
                        } else {
                            return Err(CpuError::UnsupportedInstruction("0xFF/1 mem".into()));
                        }
                    }
                    2 => {
                        // CALL r/m64 (indirect)
                        let (target, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                        let ret_addr = self.regs.rip + 1 + consumed as u64;
                        self.push_u64_with_memory(memory, ret_addr)?;
                        self.regs.rip = target;
                    }
                    4 => {
                        // JMP r/m64 (indirect)
                        let (target, _consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                        self.regs.rip = target;
                    }
                    6 => {
                        // PUSH r/m64
                        let (val, consumed) = self.decode_modrm_rm64(bytes, 1, memory)?;
                        self.push_u64_with_memory(memory, val)?;
                        self.regs.rip += 1 + consumed as u64;
                    }
                    _ => {
                        return Err(CpuError::UnsupportedInstruction(format!("0xFF /{}", op)));
                    }
                }
            }

            // ================================================================
            // Two-byte opcode escape (0x0F ...)
            // ================================================================
            0x0F => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let opcode2 = bytes[1];
                match opcode2 {
                    // SYSCALL (0x0F 05)
                    0x05 => {
                        // In a full emulator this would switch to kernel mode.
                        // Here we just advance RIP and set RCX = return address.
                        self.regs.rcx = self.regs.rip + 2;
                        self.regs.r11 = self.regs.rflags;
                        self.regs.rip += 2;
                    }

                    // SYSRET (0x0F 07)
                    0x07 => {
                        self.regs.rip = self.regs.rcx;
                        self.regs.rflags = self.regs.r11;
                    }

                    // Two-byte Jcc near (0x0F 80-8F) — rel32
                    0x80..=0x8F => {
                        if bytes.len() < 6 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let cc = opcode2 & 0x0F;
                        let disp = i32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
                        self.regs.rip += 6;
                        if self.check_condition(cc) {
                            self.regs.rip = self.regs.rip.wrapping_add(disp as i64 as u64);
                        }
                    }

                    // SETcc r/m8 (0x0F 90-9F)
                    0x90..=0x9F => {
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let cc = opcode2 & 0x0F;
                        let modrm = bytes[2];
                        let rm = modrm & 0x07;
                        let val = if self.check_condition(cc) { 1u8 } else { 0u8 };
                        if (modrm >> 6) == 0b11 {
                            self.set_reg8(rm, val);
                            self.regs.rip += 3;
                        } else {
                            return Err(CpuError::UnsupportedInstruction("SETcc mem".into()));
                        }
                    }

                    // CPUID (0x0F A2)
                    0xA2 => {
                        // Return synthetic CPUID data
                        let leaf = self.regs.rax as u32;
                        match leaf {
                            0 => {
                                // Vendor string "AetherVMx86"
                                self.regs.rax = 1; // max leaf
                                self.regs.rbx = u32::from_le_bytes(*b"Aeth") as u64;
                                self.regs.rdx = u32::from_le_bytes(*b"erVM") as u64;
                                self.regs.rcx = u32::from_le_bytes(*b"x86!") as u64;
                            }
                            1 => {
                                // Family/Model/Stepping
                                self.regs.rax = 0x0006_0F01; // Family 6, Model 15
                                self.regs.rbx = 0;
                                self.regs.rcx = 0;
                                self.regs.rdx = 0;
                            }
                            _ => {
                                self.regs.rax = 0;
                                self.regs.rbx = 0;
                                self.regs.rcx = 0;
                                self.regs.rdx = 0;
                            }
                        }
                        self.regs.rip += 2;
                    }

                    // IMUL r64, r/m64 (0x0F AF)
                    0xAF => {
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let reg = (bytes[2] >> 3) & 0x07;
                        let rm = bytes[2] & 0x07;
                        if (bytes[2] >> 6) == 0b11 {
                            let a = self.get_reg64(reg) as i64;
                            let b = self.get_reg64(rm) as i64;
                            let result = a.wrapping_mul(b);
                            self.set_reg64(reg, result as u64);
                            // Set OF/CF if result truncated
                            let full = (a as i128) * (b as i128);
                            if full != result as i128 {
                                self.regs.rflags |= flags::CF | flags::OF;
                            } else {
                                self.regs.rflags &= !(flags::CF | flags::OF);
                            }
                            self.regs.rip += 3;
                        } else {
                            return Err(CpuError::UnsupportedInstruction("0x0F AF mem".into()));
                        }
                    }

                    // MOVZX r64, r/m8 (0x0F B6)
                    0xB6 => {
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let reg = (bytes[2] >> 3) & 0x07;
                        let rm = bytes[2] & 0x07;
                        if (bytes[2] >> 6) == 0b11 {
                            let val = self.get_reg8(rm) as u64;
                            self.set_reg64(reg, val);
                            self.regs.rip += 3;
                        } else {
                            let (addr, consumed, _) = self.modrm_effective_addr(bytes, 2)?;
                            let val = memory.read_u8(addr)? as u64;
                            self.set_reg64(reg, val);
                            self.regs.rip += 2 + consumed as u64;
                        }
                    }

                    // MOVZX r64, r/m16 (0x0F B7)
                    0xB7 => {
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let reg = (bytes[2] >> 3) & 0x07;
                        let rm = bytes[2] & 0x07;
                        if (bytes[2] >> 6) == 0b11 {
                            let val = self.get_reg64(rm) & 0xFFFF;
                            self.set_reg64(reg, val);
                            self.regs.rip += 3;
                        } else {
                            let (addr, consumed, _) = self.modrm_effective_addr(bytes, 2)?;
                            let val = memory.read_u16(addr)? as u64;
                            self.set_reg64(reg, val);
                            self.regs.rip += 2 + consumed as u64;
                        }
                    }

                    // MOVSX r64, r/m8 (0x0F BE)
                    0xBE => {
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let reg = (bytes[2] >> 3) & 0x07;
                        let rm = bytes[2] & 0x07;
                        if (bytes[2] >> 6) == 0b11 {
                            let val = self.get_reg8(rm) as i8 as i64 as u64;
                            self.set_reg64(reg, val);
                            self.regs.rip += 3;
                        } else {
                            let (addr, consumed, _) = self.modrm_effective_addr(bytes, 2)?;
                            let val = memory.read_u8(addr)? as i8 as i64 as u64;
                            self.set_reg64(reg, val);
                            self.regs.rip += 2 + consumed as u64;
                        }
                    }

                    // MOVSX r64, r/m16 (0x0F BF)
                    0xBF => {
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let reg = (bytes[2] >> 3) & 0x07;
                        let rm = bytes[2] & 0x07;
                        if (bytes[2] >> 6) == 0b11 {
                            let val = (self.get_reg64(rm) & 0xFFFF) as u16 as i16 as i64 as u64;
                            self.set_reg64(reg, val);
                            self.regs.rip += 3;
                        } else {
                            let (addr, consumed, _) = self.modrm_effective_addr(bytes, 2)?;
                            let val = memory.read_u16(addr)? as i16 as i64 as u64;
                            self.set_reg64(reg, val);
                            self.regs.rip += 2 + consumed as u64;
                        }
                    }

                    // CMOVcc r64, r/m64 (0x0F 40-4F)
                    0x40..=0x4F => {
                        if bytes.len() < 3 {
                            return Err(CpuError::InvalidInstruction);
                        }
                        let cc = opcode2 & 0x0F;
                        let reg = (bytes[2] >> 3) & 0x07;
                        let (src, consumed) = self.decode_modrm_rm64(bytes, 2, memory)?;
                        if self.check_condition(cc) {
                            self.set_reg64(reg, src);
                        }
                        self.regs.rip += 2 + consumed as u64;
                    }

                    _ => {
                        return Err(CpuError::UnsupportedInstruction(format!(
                            "0x0F 0x{:02X}",
                            opcode2
                        )));
                    }
                }
            }

            // Unknown opcode
            _ => {
                return Err(CpuError::UnsupportedInstruction(format!(
                    "0x{:02X}",
                    opcode
                )));
            }
        }

        Ok(())
    }

    /// Perform a Group 1 ALU operation on 64-bit operands.
    /// Returns (result, is_logic_op).
    fn alu_op64(op: u8, dst: u64, src: u64) -> (u64, bool) {
        match op {
            0 => (dst.wrapping_add(src), false), // ADD
            1 => (dst | src, true),              // OR
            2 => {
                // ADC — add with carry (simplified: no carry-in)
                (dst.wrapping_add(src), false)
            }
            3 => {
                // SBB — subtract with borrow (simplified)
                (dst.wrapping_sub(src), false)
            }
            4 => (dst & src, true),              // AND
            5 => (dst.wrapping_sub(src), false), // SUB
            6 => (dst ^ src, true),              // XOR
            7 => (dst.wrapping_sub(src), false), // CMP (result not stored)
            _ => unreachable!(),
        }
    }

    /// Perform a Group 1 ALU operation on 8-bit operands.
    fn alu_op8(op: u8, dst: u8, src: u8) -> (u8, bool) {
        match op {
            0 => (dst.wrapping_add(src), false),
            1 => (dst | src, true),
            2 => (dst.wrapping_add(src), false),
            3 => (dst.wrapping_sub(src), false),
            4 => (dst & src, true),
            5 => (dst.wrapping_sub(src), false),
            6 => (dst ^ src, true),
            7 => (dst.wrapping_sub(src), false),
            _ => unreachable!(),
        }
    }

    /// Apply flags after a Group 1 ALU operation (64-bit)
    fn apply_alu_flags64(&mut self, op: u8, result: u64, dst: u64, src: u64, is_logic: bool) {
        if is_logic {
            self.update_flags_logic(result);
        } else if op == 0 || op == 2 {
            self.update_flags_add(result, dst, src);
        } else {
            self.update_flags_sub(result, dst, src);
        }
    }

    /// Apply flags after a Group 1 ALU operation (8-bit)
    fn apply_alu_flags8(&mut self, op: u8, result: u8, dst: u8, src: u8, is_logic: bool) {
        if is_logic {
            self.update_flags_logic(result as u64);
        } else if op == 0 || op == 2 {
            self.update_flags_add(result as u64, dst as u64, src as u64);
        } else {
            self.update_flags_sub(result as u64, dst as u64, src as u64);
        }
    }

    /// Perform a shift/rotate operation on a 64-bit value (Group 2)
    fn shift_op64(&mut self, op: u8, val: u64, count: u32) -> u64 {
        if count == 0 {
            return val;
        }
        match op {
            0 => {
                // ROL
                val.rotate_left(count)
            }
            1 => {
                // ROR
                val.rotate_right(count)
            }
            4 => {
                // SHL / SAL
                let result = val.wrapping_shl(count);
                self.update_flags_logic(result);
                if count == 1 {
                    // OF = MSB(result) XOR CF
                    let msb = (result >> 63) & 1;
                    let cf_bit = if count <= 64 {
                        (val >> (64 - count)) & 1
                    } else {
                        0
                    };
                    if msb != cf_bit {
                        self.regs.rflags |= flags::OF;
                    }
                    if cf_bit != 0 {
                        self.regs.rflags |= flags::CF;
                    }
                }
                result
            }
            5 => {
                // SHR
                let cf_bit = if count <= 64 {
                    (val >> (count - 1)) & 1
                } else {
                    0
                };
                let result = val.wrapping_shr(count);
                self.update_flags_logic(result);
                if cf_bit != 0 {
                    self.regs.rflags |= flags::CF;
                }
                result
            }
            7 => {
                // SAR (arithmetic)
                let result = (val as i64).wrapping_shr(count) as u64;
                let cf_bit = if count <= 64 {
                    (val >> (count - 1)) & 1
                } else {
                    0
                };
                self.update_flags_logic(result);
                if cf_bit != 0 {
                    self.regs.rflags |= flags::CF;
                }
                result
            }
            2 => {
                // RCL — rotate left through carry
                let size = 64u32;
                let count = count % (size + 1);
                if count == 0 {
                    return val;
                }
                let cf_in = (self.regs.rflags & flags::CF != 0) as u64;
                // Build a 65-bit value: [CF | val(63..0)]
                // Then rotate left by count positions.
                let mut result = val;
                let mut cf = cf_in;
                for _ in 0..count {
                    let msb = (result >> 63) & 1;
                    result = (result << 1) | cf;
                    cf = msb;
                }
                // Set CF = last bit shifted out
                self.regs.rflags &= !flags::CF;
                if cf != 0 {
                    self.regs.rflags |= flags::CF;
                }
                // OF defined only for count == 1: OF = MSB(result) XOR CF
                if count == 1 {
                    self.regs.rflags &= !flags::OF;
                    if ((result >> 63) & 1) ^ cf != 0 {
                        self.regs.rflags |= flags::OF;
                    }
                }
                result
            }
            3 => {
                // RCR — rotate right through carry
                let size = 64u32;
                let count = count % (size + 1);
                if count == 0 {
                    return val;
                }
                let cf_in = (self.regs.rflags & flags::CF != 0) as u64;
                let mut result = val;
                let mut cf = cf_in;
                // For count==1, OF = MSB(val) XOR CF (before rotation)
                if count == 1 {
                    self.regs.rflags &= !flags::OF;
                    if ((val >> 63) & 1) ^ cf != 0 {
                        self.regs.rflags |= flags::OF;
                    }
                }
                for _ in 0..count {
                    let lsb = result & 1;
                    result = (result >> 1) | (cf << 63);
                    cf = lsb;
                }
                // Set CF = last bit shifted out
                self.regs.rflags &= !flags::CF;
                if cf != 0 {
                    self.regs.rflags |= flags::CF;
                }
                result
            }
            _ => {
                // reserved(6) — not a valid x86 encoding, return unchanged
                val
            }
        }
    }

    /// Get 64-bit register by index (0-15)
    fn get_reg64(&self, idx: u8) -> u64 {
        match idx & 0x0F {
            0 => self.regs.rax,
            1 => self.regs.rcx,
            2 => self.regs.rdx,
            3 => self.regs.rbx,
            4 => self.regs.rsp,
            5 => self.regs.rbp,
            6 => self.regs.rsi,
            7 => self.regs.rdi,
            8 => self.regs.r8,
            9 => self.regs.r9,
            10 => self.regs.r10,
            11 => self.regs.r11,
            12 => self.regs.r12,
            13 => self.regs.r13,
            14 => self.regs.r14,
            15 => self.regs.r15,
            _ => unreachable!(),
        }
    }

    /// Set 64-bit register by index (0-15)
    fn set_reg64(&mut self, idx: u8, value: u64) {
        match idx & 0x0F {
            0 => self.regs.rax = value,
            1 => self.regs.rcx = value,
            2 => self.regs.rdx = value,
            3 => self.regs.rbx = value,
            4 => self.regs.rsp = value,
            5 => self.regs.rbp = value,
            6 => self.regs.rsi = value,
            7 => self.regs.rdi = value,
            8 => self.regs.r8 = value,
            9 => self.regs.r9 = value,
            10 => self.regs.r10 = value,
            11 => self.regs.r11 = value,
            12 => self.regs.r12 = value,
            13 => self.regs.r13 = value,
            14 => self.regs.r14 = value,
            15 => self.regs.r15 = value,
            _ => unreachable!(),
        }
    }

    /// Get 32-bit register value by index, zero-extending to 64 bits
    fn get_reg32(&self, idx: u8) -> u32 {
        self.get_reg64(idx) as u32
    }

    /// Set 32-bit register by index (zero-extends to 64 bits per x86-64 spec)
    fn set_reg32(&mut self, idx: u8, value: u32) {
        self.set_reg64(idx, value as u64);
    }

    /// Get 8-bit register value by index.
    ///
    /// Legacy encoding (no REX): 0-3 = AL/CL/DL/BL, 4-7 = AH/CH/DH/BH.
    /// REX encoding: 8-15 = R8B-R15B (low byte of R8-R15).
    fn get_reg8(&self, idx: u8) -> u8 {
        match idx {
            0 => self.regs.rax as u8,
            1 => self.regs.rcx as u8,
            2 => self.regs.rdx as u8,
            3 => self.regs.rbx as u8,
            4 => (self.regs.rax >> 8) as u8, // AH
            5 => (self.regs.rcx >> 8) as u8, // CH
            6 => (self.regs.rdx >> 8) as u8, // DH
            7 => (self.regs.rbx >> 8) as u8, // BH
            8 => self.regs.r8 as u8,         // R8B
            9 => self.regs.r9 as u8,         // R9B
            10 => self.regs.r10 as u8,       // R10B
            11 => self.regs.r11 as u8,       // R11B
            12 => self.regs.r12 as u8,       // R12B
            13 => self.regs.r13 as u8,       // R13B
            14 => self.regs.r14 as u8,       // R14B
            15 => self.regs.r15 as u8,       // R15B
            _ => 0,
        }
    }

    /// Set 8-bit register by index.
    ///
    /// Legacy encoding (no REX): 0-3 = AL/CL/DL/BL, 4-7 = AH/CH/DH/BH.
    /// REX encoding: 8-15 = R8B-R15B (low byte of R8-R15).
    fn set_reg8(&mut self, idx: u8, value: u8) {
        match idx {
            0 => self.regs.rax = (self.regs.rax & !0xFF) | value as u64,
            1 => self.regs.rcx = (self.regs.rcx & !0xFF) | value as u64,
            2 => self.regs.rdx = (self.regs.rdx & !0xFF) | value as u64,
            3 => self.regs.rbx = (self.regs.rbx & !0xFF) | value as u64,
            4 => self.regs.rax = (self.regs.rax & !0xFF00) | ((value as u64) << 8), // AH
            5 => self.regs.rcx = (self.regs.rcx & !0xFF00) | ((value as u64) << 8), // CH
            6 => self.regs.rdx = (self.regs.rdx & !0xFF00) | ((value as u64) << 8), // DH
            7 => self.regs.rbx = (self.regs.rbx & !0xFF00) | ((value as u64) << 8), // BH
            8 => self.regs.r8 = (self.regs.r8 & !0xFF) | value as u64,              // R8B
            9 => self.regs.r9 = (self.regs.r9 & !0xFF) | value as u64,              // R9B
            10 => self.regs.r10 = (self.regs.r10 & !0xFF) | value as u64,           // R10B
            11 => self.regs.r11 = (self.regs.r11 & !0xFF) | value as u64,           // R11B
            12 => self.regs.r12 = (self.regs.r12 & !0xFF) | value as u64,           // R12B
            13 => self.regs.r13 = (self.regs.r13 & !0xFF) | value as u64,           // R13B
            14 => self.regs.r14 = (self.regs.r14 & !0xFF) | value as u64,           // R14B
            15 => self.regs.r15 = (self.regs.r15 & !0xFF) | value as u64,           // R15B
            _ => {}
        }
    }

    /// Decode ModRM byte and compute the effective address.
    /// Returns (operand_value, instruction_length_consumed).
    /// For mod=11 (register-direct), returns the register value.
    /// For memory modes, reads from memory.
    fn decode_modrm_rm64<M: MemoryAccess>(
        &self,
        bytes: &[u8],
        offset: usize,
        memory: &M,
    ) -> Result<(u64, usize)> {
        if offset >= bytes.len() {
            return Err(CpuError::InvalidInstruction);
        }
        let modrm = bytes[offset];
        let mod_bits = modrm >> 6;
        let rm = modrm & 0x07;

        match mod_bits {
            0b11 => {
                // Register direct
                Ok((self.get_reg64(rm), 1))
            }
            0b00 => {
                if rm == 0b101 {
                    // RIP-relative (disp32)
                    if offset + 5 > bytes.len() {
                        return Err(CpuError::InvalidInstruction);
                    }
                    let disp = i32::from_le_bytes([
                        bytes[offset + 1],
                        bytes[offset + 2],
                        bytes[offset + 3],
                        bytes[offset + 4],
                    ]);
                    // RIP points to the *next* instruction; caller must adjust
                    let addr = self.regs.rip.wrapping_add(disp as i64 as u64);
                    let val = memory.read_u64(addr)?;
                    Ok((val, 5))
                } else if rm == 0b100 {
                    // SIB follows – simplified: treat as [reg] for now
                    if offset + 1 >= bytes.len() {
                        return Err(CpuError::InvalidInstruction);
                    }
                    let sib = bytes[offset + 1];
                    let base = sib & 0x07;
                    let addr = self.get_reg64(base);
                    let val = memory.read_u64(addr)?;
                    Ok((val, 2))
                } else {
                    let addr = self.get_reg64(rm);
                    let val = memory.read_u64(addr)?;
                    Ok((val, 1))
                }
            }
            0b01 => {
                // [reg + disp8]
                let extra = if rm == 0b100 { 1 } else { 0 }; // SIB byte
                if offset + 2 + extra > bytes.len() {
                    return Err(CpuError::InvalidInstruction);
                }
                let base_reg = if rm == 0b100 {
                    bytes[offset + 1] & 0x07
                } else {
                    rm
                };
                let disp = bytes[offset + 1 + extra] as i8 as i64;
                let addr = self.get_reg64(base_reg).wrapping_add(disp as u64);
                let val = memory.read_u64(addr)?;
                Ok((val, 2 + extra))
            }
            0b10 => {
                // [reg + disp32]
                let extra = if rm == 0b100 { 1 } else { 0 };
                if offset + 5 + extra > bytes.len() {
                    return Err(CpuError::InvalidInstruction);
                }
                let base_reg = if rm == 0b100 {
                    bytes[offset + 1] & 0x07
                } else {
                    rm
                };
                let d = offset + 1 + extra;
                let disp = i32::from_le_bytes([bytes[d], bytes[d + 1], bytes[d + 2], bytes[d + 3]]);
                let addr = self.get_reg64(base_reg).wrapping_add(disp as i64 as u64);
                let val = memory.read_u64(addr)?;
                Ok((val, 5 + extra))
            }
            _ => unreachable!(),
        }
    }

    /// Compute effective address from ModRM (without reading memory).
    /// Returns (effective_address, bytes_consumed) or register index for mod=11.
    fn modrm_effective_addr(&self, bytes: &[u8], offset: usize) -> Result<(u64, usize, bool)> {
        if offset >= bytes.len() {
            return Err(CpuError::InvalidInstruction);
        }
        let modrm = bytes[offset];
        let mod_bits = modrm >> 6;
        let rm = modrm & 0x07;

        match mod_bits {
            0b11 => Ok((rm as u64, 1, true)), // register mode
            0b00 => {
                if rm == 0b101 {
                    if offset + 5 > bytes.len() {
                        return Err(CpuError::InvalidInstruction);
                    }
                    let disp = i32::from_le_bytes([
                        bytes[offset + 1],
                        bytes[offset + 2],
                        bytes[offset + 3],
                        bytes[offset + 4],
                    ]);
                    let addr = self.regs.rip.wrapping_add(disp as i64 as u64);
                    Ok((addr, 5, false))
                } else if rm == 0b100 {
                    if offset + 1 >= bytes.len() {
                        return Err(CpuError::InvalidInstruction);
                    }
                    let sib = bytes[offset + 1];
                    let base = sib & 0x07;
                    Ok((self.get_reg64(base), 2, false))
                } else {
                    Ok((self.get_reg64(rm), 1, false))
                }
            }
            0b01 => {
                let extra = if rm == 0b100 { 1 } else { 0 };
                if offset + 2 + extra > bytes.len() {
                    return Err(CpuError::InvalidInstruction);
                }
                let base_reg = if rm == 0b100 {
                    bytes[offset + 1] & 0x07
                } else {
                    rm
                };
                let disp = bytes[offset + 1 + extra] as i8 as i64;
                let addr = self.get_reg64(base_reg).wrapping_add(disp as u64);
                Ok((addr, 2 + extra, false))
            }
            0b10 => {
                let extra = if rm == 0b100 { 1 } else { 0 };
                if offset + 5 + extra > bytes.len() {
                    return Err(CpuError::InvalidInstruction);
                }
                let base_reg = if rm == 0b100 {
                    bytes[offset + 1] & 0x07
                } else {
                    rm
                };
                let d = offset + 1 + extra;
                let disp = i32::from_le_bytes([bytes[d], bytes[d + 1], bytes[d + 2], bytes[d + 3]]);
                let addr = self.get_reg64(base_reg).wrapping_add(disp as i64 as u64);
                Ok((addr, 5 + extra, false))
            }
            _ => unreachable!(),
        }
    }

    /// Write a 64-bit value to the operand specified by ModRM
    fn write_modrm_rm64<M: MemoryAccess>(
        &mut self,
        bytes: &[u8],
        offset: usize,
        value: u64,
        memory: &mut M,
    ) -> Result<usize> {
        let (addr_or_reg, consumed, is_reg) = self.modrm_effective_addr(bytes, offset)?;
        if is_reg {
            self.set_reg64(addr_or_reg as u8, value);
        } else {
            memory.write_u64(addr_or_reg, value)?;
        }
        Ok(consumed)
    }

    /// Check a condition code (used by Jcc, CMOVcc, SETcc)
    fn check_condition(&self, cc: u8) -> bool {
        let f = self.regs.rflags;
        match cc & 0x0F {
            0x0 => (f & flags::OF) != 0,               // O
            0x1 => (f & flags::OF) == 0,               // NO
            0x2 => (f & flags::CF) != 0,               // B/C/NAE
            0x3 => (f & flags::CF) == 0,               // NB/NC/AE
            0x4 => (f & flags::ZF) != 0,               // E/Z
            0x5 => (f & flags::ZF) == 0,               // NE/NZ
            0x6 => (f & (flags::CF | flags::ZF)) != 0, // BE/NA
            0x7 => (f & (flags::CF | flags::ZF)) == 0, // NBE/A
            0x8 => (f & flags::SF) != 0,               // S
            0x9 => (f & flags::SF) == 0,               // NS
            0xA => {
                // P/PE
                // Parity flag not fully tracked; default false
                false
            }
            0xB => true, // NP/PO
            0xC => {
                // L: SF != OF
                ((f & flags::SF) != 0) != ((f & flags::OF) != 0)
            }
            0xD => {
                // NL/GE: SF == OF
                ((f & flags::SF) != 0) == ((f & flags::OF) != 0)
            }
            0xE => {
                // LE: ZF=1 or SF!=OF
                (f & flags::ZF) != 0 || ((f & flags::SF) != 0) != ((f & flags::OF) != 0)
            }
            0xF => {
                // NLE/G: ZF=0 and SF==OF
                (f & flags::ZF) == 0 && ((f & flags::SF) != 0) == ((f & flags::OF) != 0)
            }
            _ => false,
        }
    }

    /// Legacy execute instruction (for backward compatibility)
    pub fn execute_instruction(&mut self, opcode: u8) -> Result<()> {
        let bytes = [opcode, 0xC0, 0, 0, 0, 0, 0, 0, 0]; // 0xC0 for modrm with reg,reg
        let mut dummy_mem = [0u8; 0x2000]; // 8KB buffer
        dummy_mem[0x1000..0x1008].copy_from_slice(&[0u8; 8]); // Stack area
        self.regs.rsp = 0x1008; // Set up stack for legacy tests (point to valid address)
        let mut slice_mem = SliceMemory::new(&mut dummy_mem);
        self.execute_instruction_with_memory(opcode, &bytes, &mut slice_mem)
    }

    /// Push value onto stack (legacy, without memory - for tests)
    fn push_u64(&mut self, _value: u64) {
        self.regs.rsp = self.regs.rsp.wrapping_sub(8);
    }

    /// Pop value from stack (legacy, without memory - for tests)
    fn pop_u64(&mut self) -> u64 {
        self.regs.rsp = self.regs.rsp.wrapping_add(8);
        0 // Return 0 for legacy tests
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
    pub fn step<M: MemoryAccess>(&mut self, memory: &mut M) -> Result<()> {
        if self.halted {
            return Ok(());
        }

        // Check for pending interrupts
        if self.has_pending_interrupt() {
            self.handle_interrupt(memory)?;
        }

        // Fetch instruction
        let bytes = self.fetch_instruction(memory, 15)?;

        // Execute
        let opcode = bytes[0];
        self.execute_instruction_with_memory(opcode, &bytes, memory)
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

    /// Helper: create a CPU with RIP=0 and a memory buffer, write instruction
    /// bytes at offset 0, and execute via `execute_with_memory`.
    fn exec_bytes(cpu: &mut X86_64Cpu, mem: &mut Vec<u8>, instr: &[u8]) {
        let rip = cpu.regs.rip as usize;
        // Ensure memory is large enough
        if mem.len() < rip + instr.len() + 0x100 {
            mem.resize(rip + instr.len() + 0x100, 0);
        }
        mem[rip..rip + instr.len()].copy_from_slice(instr);
        cpu.execute_with_memory(mem).unwrap();
    }

    fn new_cpu_with_stack() -> (X86_64Cpu, Vec<u8>) {
        let mut cpu = X86_64Cpu::new();
        cpu.regs.rip = 0;
        cpu.regs.rsp = 0x1000;
        let mem = vec![0u8; 0x2000];
        (cpu, mem)
    }

    #[test]
    fn test_nop() {
        let mut cpu = X86_64Cpu::new();
        cpu.regs.rip = 0;
        cpu.execute_instruction(0x90).unwrap();
        assert_eq!(cpu.regs.rip, 1);
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

    #[test]
    fn test_push_pop_with_memory() {
        let mut memory = vec![0u8; 0x2000];
        let mut cpu = X86_64Cpu::new();
        cpu.regs.rsp = 0x1000;
        cpu.regs.rax = 0xDEADBEEF;

        let mut slice_mem = SliceMemory::new(&mut memory);

        // Push RAX
        cpu.push_u64_with_memory(&mut slice_mem, cpu.regs.rax)
            .unwrap();
        assert_eq!(cpu.regs.rsp, 0x1000 - 8);

        // Pop into RBX
        cpu.regs.rbx = cpu.pop_u64_with_memory(&slice_mem).unwrap();
        assert_eq!(cpu.regs.rbx, 0xDEADBEEF);
        assert_eq!(cpu.regs.rsp, 0x1000);
    }

    #[test]
    fn test_interrupt_queue() {
        let mut cpu = X86_64Cpu::new();
        cpu.regs.rflags |= flags::IF;

        assert!(!cpu.has_pending_interrupt());

        cpu.queue_interrupt(0x80, None);
        assert!(cpu.has_pending_interrupt());

        // NMI should be deliverable even with IF clear
        cpu.regs.rflags &= !flags::IF;
        cpu.pending_interrupts.clear();
        cpu.queue_nmi();
        assert!(cpu.has_pending_interrupt());
    }

    #[test]
    fn test_cli_sti() {
        let mut cpu = X86_64Cpu::new();
        cpu.regs.rflags |= flags::IF;

        cpu.execute_instruction(0xFA).unwrap(); // CLI
        assert!(!cpu.interrupts_enabled());

        cpu.execute_instruction(0xFB).unwrap(); // STI
        assert!(cpu.interrupts_enabled());
    }

    #[test]
    fn test_idt_entry() {
        let entry = IdtEntry {
            offset_low: 0x1234,
            selector: 0x08,
            ist: 0,
            type_attr: 0x8E, // Present, DPL=0, Interrupt Gate
            offset_mid: 0x5678,
            offset_high: 0xABCD0000,
            reserved: 0,
        };

        assert!(entry.is_present());
        assert_eq!(entry.dpl(), 0);
        assert!(!entry.is_trap_gate());
        assert_eq!(entry.handler_address(), 0xABCD000056781234);
    }

    // ====================================================================
    // New tests for expanded instruction set
    // ====================================================================

    #[test]
    fn test_add_reg_reg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 10;
        cpu.regs.rcx = 20;
        // ADD RAX, RCX → 0x01 0xC8 (ModRM: mod=11, reg=RCX(1), rm=RAX(0))
        exec_bytes(&mut cpu, &mut mem, &[0x01, 0xC8]);
        assert_eq!(cpu.regs.rax, 30);
    }

    #[test]
    fn test_add_eax_imm32() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 100;
        // ADD EAX, 0x00000032 (50)
        exec_bytes(&mut cpu, &mut mem, &[0x05, 0x32, 0x00, 0x00, 0x00]);
        assert_eq!(cpu.regs.rax, 150);
    }

    #[test]
    fn test_sub_reg_reg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 50;
        cpu.regs.rcx = 20;
        // SUB RAX, RCX → 0x29 0xC8 (reg=RCX→rm=RAX)
        exec_bytes(&mut cpu, &mut mem, &[0x29, 0xC8]);
        assert_eq!(cpu.regs.rax, 30);
    }

    #[test]
    fn test_sub_sets_flags() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 10;
        cpu.regs.rcx = 10;
        exec_bytes(&mut cpu, &mut mem, &[0x29, 0xC8]); // SUB RAX, RCX
        assert_eq!(cpu.regs.rax, 0);
        assert!(cpu.regs.rflags & flags::ZF != 0);
    }

    #[test]
    fn test_and_reg_reg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0xFF00;
        cpu.regs.rcx = 0x0FF0;
        // AND RAX, RCX → 0x21 0xC8
        exec_bytes(&mut cpu, &mut mem, &[0x21, 0xC8]);
        assert_eq!(cpu.regs.rax, 0x0F00);
    }

    #[test]
    fn test_or_reg_reg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0xF0;
        cpu.regs.rcx = 0x0F;
        // OR RAX, RCX → 0x09 0xC8
        exec_bytes(&mut cpu, &mut mem, &[0x09, 0xC8]);
        assert_eq!(cpu.regs.rax, 0xFF);
    }

    #[test]
    fn test_cmp_reg_imm8() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 5;
        // CMP RAX, 5 → 0x83 0xF8 0x05
        exec_bytes(&mut cpu, &mut mem, &[0x83, 0xF8, 0x05]);
        assert!(cpu.regs.rflags & flags::ZF != 0);
    }

    #[test]
    fn test_cmp_reg_reg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 10;
        cpu.regs.rcx = 5;
        // CMP RAX, RCX → 0x39 0xC8 (r/m=RAX, reg=RCX)
        exec_bytes(&mut cpu, &mut mem, &[0x39, 0xC8]);
        assert_eq!(cpu.regs.rax, 10); // CMP doesn't modify operands
        assert!(cpu.regs.rflags & flags::ZF == 0);
    }

    #[test]
    fn test_test_reg_reg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0x00;
        cpu.regs.rcx = 0xFF;
        // TEST RAX, RCX → 0x85 0xC8
        exec_bytes(&mut cpu, &mut mem, &[0x85, 0xC8]);
        assert!(cpu.regs.rflags & flags::ZF != 0);
    }

    #[test]
    fn test_mov_reg_reg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0;
        cpu.regs.rcx = 0xCAFE;
        // MOV RAX, RCX → 0x8B 0xC1 (reg=RAX(0), rm=RCX(1))
        exec_bytes(&mut cpu, &mut mem, &[0x8B, 0xC1]);
        assert_eq!(cpu.regs.rax, 0xCAFE);
    }

    #[test]
    fn test_mov_reg_to_rm() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0;
        cpu.regs.rcx = 0xBEEF;
        // MOV RAX, RCX → 0x89 0xC8 (reg=RCX(1), rm=RAX(0))
        exec_bytes(&mut cpu, &mut mem, &[0x89, 0xC8]);
        assert_eq!(cpu.regs.rax, 0xBEEF);
    }

    #[test]
    fn test_lea() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rcx = 0x100;
        // LEA RAX, [RCX+0x10] → 0x8D 0x41 0x10 (reg=RAX, rm=RCX, disp8=0x10)
        exec_bytes(&mut cpu, &mut mem, &[0x8D, 0x41, 0x10]);
        assert_eq!(cpu.regs.rax, 0x110);
    }

    #[test]
    fn test_call_ret() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rip = 0;
        // CALL +0 (calls to address 5, the instruction right after CALL)
        // 0xE8 0x00 0x00 0x00 0x00
        exec_bytes(&mut cpu, &mut mem, &[0xE8, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(cpu.regs.rip, 5); // jumped to offset 5 (0 + 5 + 0)
                                     // Return address pushed should be 5
        let ret_addr = {
            let mem_ref = &mem;
            let rsp = cpu.regs.rsp as usize;
            u64::from_le_bytes([
                mem_ref[rsp],
                mem_ref[rsp + 1],
                mem_ref[rsp + 2],
                mem_ref[rsp + 3],
                mem_ref[rsp + 4],
                mem_ref[rsp + 5],
                mem_ref[rsp + 6],
                mem_ref[rsp + 7],
            ])
        };
        assert_eq!(ret_addr, 5);

        // Now RET
        // Write RET at address 5
        mem[5] = 0xC3;
        cpu.execute_with_memory(&mut mem).unwrap();
        assert_eq!(cpu.regs.rip, 5); // returned to address 5
    }

    #[test]
    fn test_jmp_rel8() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rip = 0;
        // JMP +3 → 0xEB 0x03
        exec_bytes(&mut cpu, &mut mem, &[0xEB, 0x03]);
        assert_eq!(cpu.regs.rip, 5); // 0 + 2 + 3
    }

    #[test]
    fn test_jmp_rel32() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rip = 0;
        // JMP +10 → 0xE9 0x0A 0x00 0x00 0x00
        exec_bytes(&mut cpu, &mut mem, &[0xE9, 0x0A, 0x00, 0x00, 0x00]);
        assert_eq!(cpu.regs.rip, 15); // 0 + 5 + 10
    }

    #[test]
    fn test_jcc_je_taken() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rflags |= flags::ZF;
        cpu.regs.rip = 0;
        // JE +5 → 0x74 0x05
        exec_bytes(&mut cpu, &mut mem, &[0x74, 0x05]);
        assert_eq!(cpu.regs.rip, 7); // 0 + 2 + 5
    }

    #[test]
    fn test_jcc_je_not_taken() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rflags &= !flags::ZF;
        cpu.regs.rip = 0;
        // JE +5 → 0x74 0x05
        exec_bytes(&mut cpu, &mut mem, &[0x74, 0x05]);
        assert_eq!(cpu.regs.rip, 2); // fell through
    }

    #[test]
    fn test_jcc_jne_taken() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rflags &= !flags::ZF;
        cpu.regs.rip = 0;
        // JNE +5 → 0x75 0x05
        exec_bytes(&mut cpu, &mut mem, &[0x75, 0x05]);
        assert_eq!(cpu.regs.rip, 7);
    }

    #[test]
    fn test_jcc_jl_taken() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        // SF != OF → less-than
        cpu.regs.rflags |= flags::SF;
        cpu.regs.rflags &= !flags::OF;
        cpu.regs.rip = 0;
        // JL +10 → 0x7C 0x0A
        exec_bytes(&mut cpu, &mut mem, &[0x7C, 0x0A]);
        assert_eq!(cpu.regs.rip, 12); // 2 + 10
    }

    #[test]
    fn test_near_jcc_rel32() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rflags |= flags::ZF;
        cpu.regs.rip = 0;
        mem.resize(0x2000, 0);
        // JE near +0x100 → 0x0F 0x84 0x00 0x01 0x00 0x00
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0x84, 0x00, 0x01, 0x00, 0x00]);
        assert_eq!(cpu.regs.rip, 6 + 0x100);
    }

    #[test]
    fn test_mov_imm8_all_regs() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        for i in 0u8..8 {
            cpu.regs.rip = 0;
            exec_bytes(&mut cpu, &mut mem, &[0xB0 + i, 0x42]);
        }
        assert_eq!(cpu.get_reg8(0), 0x42); // AL
        assert_eq!(cpu.get_reg8(7), 0x42); // BH
    }

    #[test]
    fn test_mov_imm32_all_regs() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        for i in 0u8..8 {
            cpu.regs.rip = 0;
            exec_bytes(&mut cpu, &mut mem, &[0xB8 + i, 0xEF, 0xBE, 0xAD, 0xDE]);
        }
        assert_eq!(cpu.regs.rax, 0xDEADBEEF);
        assert_eq!(cpu.regs.rdi, 0xDEADBEEF);
    }

    #[test]
    fn test_inc_dec_regs() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 10;
        // INC RAX via 0xFF 0xC0 (group 5 /0 mod=11 rm=0)
        exec_bytes(&mut cpu, &mut mem, &[0xFF, 0xC0]);
        assert_eq!(cpu.regs.rax, 11);

        cpu.regs.rip = 0;
        // DEC RAX via 0xFF 0xC8 (group 5 /1 mod=11 rm=0)
        exec_bytes(&mut cpu, &mut mem, &[0xFF, 0xC8]);
        assert_eq!(cpu.regs.rax, 10);
    }

    #[test]
    fn test_shl_shr_sar() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0x01;
        // SHL RAX, 4 → 0xC1 0xE0 0x04 (group 2 /4 rm=0 imm8=4)
        exec_bytes(&mut cpu, &mut mem, &[0xC1, 0xE0, 0x04]);
        assert_eq!(cpu.regs.rax, 0x10);

        cpu.regs.rip = 0;
        // SHR RAX, 2 → 0xC1 0xE8 0x02 (group 2 /5 rm=0 imm8=2)
        exec_bytes(&mut cpu, &mut mem, &[0xC1, 0xE8, 0x02]);
        assert_eq!(cpu.regs.rax, 0x04);

        cpu.regs.rip = 0;
        cpu.regs.rax = 0x8000_0000_0000_0000; // sign bit set
                                              // SAR RAX, 1 → 0xC1 0xF8 0x01 (/7 rm=0 imm8=1)
        exec_bytes(&mut cpu, &mut mem, &[0xC1, 0xF8, 0x01]);
        assert_eq!(cpu.regs.rax, 0xC000_0000_0000_0000); // sign-extended
    }

    #[test]
    fn test_shift_by_one() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0x08;
        // SHL RAX, 1 → 0xD1 0xE0
        exec_bytes(&mut cpu, &mut mem, &[0xD1, 0xE0]);
        assert_eq!(cpu.regs.rax, 0x10);
    }

    #[test]
    fn test_shift_by_cl() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0x01;
        cpu.regs.rcx = 3;
        // SHL RAX, CL → 0xD3 0xE0
        exec_bytes(&mut cpu, &mut mem, &[0xD3, 0xE0]);
        assert_eq!(cpu.regs.rax, 0x08);
    }

    #[test]
    fn test_not_neg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0x00FF;
        // NOT RAX → 0xF7 0xD0 (/2 rm=0)
        exec_bytes(&mut cpu, &mut mem, &[0xF7, 0xD0]);
        assert_eq!(cpu.regs.rax, !0x00FFu64);

        cpu.regs.rip = 0;
        cpu.regs.rcx = 5;
        // NEG RCX → 0xF7 0xD9 (/3 rm=1)
        exec_bytes(&mut cpu, &mut mem, &[0xF7, 0xD9]);
        assert_eq!(cpu.regs.rcx as i64, -5);
    }

    #[test]
    fn test_mul() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 100;
        cpu.regs.rcx = 200;
        cpu.regs.rdx = 0;
        // MUL RCX → 0xF7 0xE1 (/4 rm=1)
        exec_bytes(&mut cpu, &mut mem, &[0xF7, 0xE1]);
        assert_eq!(cpu.regs.rax, 20000);
        assert_eq!(cpu.regs.rdx, 0);
    }

    #[test]
    fn test_div() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 100;
        cpu.regs.rdx = 0;
        cpu.regs.rcx = 7;
        // DIV RCX → 0xF7 0xF1 (/6 rm=1)
        exec_bytes(&mut cpu, &mut mem, &[0xF7, 0xF1]);
        assert_eq!(cpu.regs.rax, 14); // 100/7 = 14
        assert_eq!(cpu.regs.rdx, 2); // 100%7 = 2
    }

    #[test]
    fn test_div_by_zero() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 100;
        cpu.regs.rdx = 0;
        cpu.regs.rcx = 0;
        // DIV RCX (div by zero) → should queue #DE
        exec_bytes(&mut cpu, &mut mem, &[0xF7, 0xF1]);
        assert!(!cpu.pending_interrupts.is_empty());
        assert_eq!(cpu.pending_interrupts[0].vector, 0); // #DE
    }

    #[test]
    fn test_imul_two_operand() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 7;
        cpu.regs.rcx = 6;
        // IMUL RAX, RCX → 0x0F 0xAF 0xC1 (reg=0, rm=1)
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0xAF, 0xC1]);
        assert_eq!(cpu.regs.rax, 42);
    }

    #[test]
    fn test_movzx_reg8() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rcx = 0xDEAD_00FF;
        // MOVZX RAX, CL → 0x0F 0xB6 0xC1 (reg=0, rm=1)
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0xB6, 0xC1]);
        assert_eq!(cpu.regs.rax, 0xFF);
    }

    #[test]
    fn test_movsx_reg8() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rcx = 0x80; // -128 as i8
                             // MOVSX RAX, CL → 0x0F 0xBE 0xC1
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0xBE, 0xC1]);
        assert_eq!(cpu.regs.rax as i64, -128);
    }

    #[test]
    fn test_xchg() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0xAAAA;
        cpu.regs.rcx = 0xBBBB;
        // XCHG RAX, RCX → 0x87 0xC1 (reg=0, rm=1, mod=11)
        exec_bytes(&mut cpu, &mut mem, &[0x87, 0xC1]);
        assert_eq!(cpu.regs.rax, 0xBBBB);
        assert_eq!(cpu.regs.rcx, 0xAAAA);
    }

    #[test]
    fn test_xchg_short() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 111;
        cpu.regs.rcx = 222;
        // XCHG EAX, ECX → 0x91
        exec_bytes(&mut cpu, &mut mem, &[0x91]);
        assert_eq!(cpu.regs.rax, 222);
        assert_eq!(cpu.regs.rcx, 111);
    }

    #[test]
    fn test_leave() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        // Set up a stack frame
        cpu.regs.rbp = 0x0F00;
        cpu.regs.rsp = 0x0E00;
        // Write old RBP value at 0x0F00
        let old_rbp: u64 = 0x1000;
        mem[0x0F00..0x0F08].copy_from_slice(&old_rbp.to_le_bytes());
        // LEAVE → 0xC9
        exec_bytes(&mut cpu, &mut mem, &[0xC9]);
        assert_eq!(cpu.regs.rsp, 0x0F08); // RSP = old RBP + 8 (after pop)
        assert_eq!(cpu.regs.rbp, 0x1000); // RBP restored
    }

    #[test]
    fn test_push_imm32() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        // PUSH 0x42 → 0x68 0x42 0x00 0x00 0x00
        exec_bytes(&mut cpu, &mut mem, &[0x68, 0x42, 0x00, 0x00, 0x00]);
        let rsp = cpu.regs.rsp as usize;
        let val = u64::from_le_bytes(mem[rsp..rsp + 8].try_into().unwrap());
        assert_eq!(val, 0x42);
    }

    #[test]
    fn test_push_imm8() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        // PUSH -1 (0xFF sign-extended) → 0x6A 0xFF
        exec_bytes(&mut cpu, &mut mem, &[0x6A, 0xFF]);
        let rsp = cpu.regs.rsp as usize;
        let val = u64::from_le_bytes(mem[rsp..rsp + 8].try_into().unwrap());
        assert_eq!(val as i64, -1);
    }

    #[test]
    fn test_group1_sub_imm8() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 100;
        // SUB RAX, 30 → 0x83 0xE8 0x1E (group1 /5 rm=0, imm8=30)
        exec_bytes(&mut cpu, &mut mem, &[0x83, 0xE8, 0x1E]);
        assert_eq!(cpu.regs.rax, 70);
    }

    #[test]
    fn test_group1_add_imm32() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 1000;
        // ADD RAX, 2000 → 0x81 0xC0 0xD0 0x07 0x00 0x00
        exec_bytes(&mut cpu, &mut mem, &[0x81, 0xC0, 0xD0, 0x07, 0x00, 0x00]);
        assert_eq!(cpu.regs.rax, 3000);
    }

    #[test]
    fn test_cpuid_vendor() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0; // Leaf 0
                          // CPUID → 0x0F 0xA2
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0xA2]);
        assert_eq!(cpu.regs.rax, 1); // max leaf
                                     // Check vendor string parts
        let ebx_bytes = (cpu.regs.rbx as u32).to_le_bytes();
        assert_eq!(&ebx_bytes, b"Aeth");
    }

    #[test]
    fn test_syscall() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rip = 0x100;
        cpu.regs.rflags = 0x202;
        mem.resize(0x200, 0);
        mem[0x100] = 0x0F;
        mem[0x101] = 0x05;
        cpu.execute_with_memory(&mut mem).unwrap();
        assert_eq!(cpu.regs.rcx, 0x102); // saved return address
        assert_eq!(cpu.regs.r11, 0x202); // saved flags
        assert_eq!(cpu.regs.rip, 0x102);
    }

    #[test]
    fn test_setcc() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rflags |= flags::ZF;
        cpu.regs.rax = 0;
        // SETE AL → 0x0F 0x94 0xC0
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0x94, 0xC0]);
        assert_eq!(cpu.regs.rax & 0xFF, 1);
    }

    #[test]
    fn test_cmovcc() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0;
        cpu.regs.rcx = 42;
        cpu.regs.rflags |= flags::ZF;
        // CMOVE RAX, RCX → 0x0F 0x44 0xC1
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0x44, 0xC1]);
        assert_eq!(cpu.regs.rax, 42);

        // Reset, with ZF cleared → should NOT move
        cpu.regs.rip = 0;
        cpu.regs.rax = 0;
        cpu.regs.rcx = 99;
        cpu.regs.rflags &= !flags::ZF;
        exec_bytes(&mut cpu, &mut mem, &[0x0F, 0x44, 0xC1]);
        assert_eq!(cpu.regs.rax, 0); // unchanged
    }

    #[test]
    fn test_cdq() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0xFFFF_FFFF_8000_0000u64; // negative as i32
                                                 // CDQ → 0x99
        exec_bytes(&mut cpu, &mut mem, &[0x99]);
        assert_eq!(cpu.regs.rdx, 0xFFFF_FFFF_FFFF_FFFF);
    }

    #[test]
    fn test_cdqe() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0x0000_0000_FFFF_FF80; // -128 as i32
                                              // CDQE → 0x98
        exec_bytes(&mut cpu, &mut mem, &[0x98]);
        assert_eq!(cpu.regs.rax as i64, -128);
    }

    #[test]
    fn test_register_r8_r15() {
        let mut cpu = X86_64Cpu::new();
        for i in 8u8..=15 {
            cpu.set_reg64(i, (i as u64) * 100);
        }
        for i in 8u8..=15 {
            assert_eq!(cpu.get_reg64(i), (i as u64) * 100);
        }
    }

    #[test]
    fn test_check_conditions() {
        let mut cpu = X86_64Cpu::new();

        // Test JE/JZ (cc=4)
        cpu.regs.rflags = flags::ZF;
        assert!(cpu.check_condition(0x4));
        assert!(!cpu.check_condition(0x5)); // JNE

        // Test JB/JC (cc=2)
        cpu.regs.rflags = flags::CF;
        assert!(cpu.check_condition(0x2));
        assert!(!cpu.check_condition(0x3)); // JAE/JNC

        // Test JL (cc=0xC): SF != OF
        cpu.regs.rflags = flags::SF; // SF=1, OF=0 → L
        assert!(cpu.check_condition(0xC));
        cpu.regs.rflags = flags::SF | flags::OF; // SF=1, OF=1 → GE
        assert!(!cpu.check_condition(0xC));

        // Test JG (cc=0xF): ZF=0 and SF==OF
        cpu.regs.rflags = 0; // all clear → G
        assert!(cpu.check_condition(0xF));
        cpu.regs.rflags = flags::ZF; // ZF=1 → not G
        assert!(!cpu.check_condition(0xF));
    }

    #[test]
    fn test_mov_memory_roundtrip() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rcx = 0x200; // memory address
        cpu.regs.rax = 0xDEAD_BEEF_CAFE_BABE;
        // MOV [RCX], RAX → 0x89 0x01 (reg=RAX(0), rm=RCX(1), mod=00)
        exec_bytes(&mut cpu, &mut mem, &[0x89, 0x01]);

        cpu.regs.rip = 0;
        cpu.regs.rdx = 0;
        // MOV RDX, [RCX] → 0x8B 0x11 (reg=RDX(2), rm=RCX(1), mod=00)
        exec_bytes(&mut cpu, &mut mem, &[0x8B, 0x11]);
        assert_eq!(cpu.regs.rdx, 0xDEAD_BEEF_CAFE_BABE);
    }

    #[test]
    fn test_add_to_memory() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rcx = 0x200;
        // Store 100 at memory address 0x200
        mem[0x200..0x208].copy_from_slice(&100u64.to_le_bytes());
        cpu.regs.rax = 50;
        // ADD [RCX], RAX → 0x01 0x01 (reg=RAX(0), rm=RCX(1), mod=00)
        exec_bytes(&mut cpu, &mut mem, &[0x01, 0x01]);
        let val = u64::from_le_bytes(mem[0x200..0x208].try_into().unwrap());
        assert_eq!(val, 150);
    }

    #[test]
    fn test_push_pop_general_regs() {
        let (mut cpu, mut mem) = new_cpu_with_stack();
        cpu.regs.rax = 0xAA;
        cpu.regs.rcx = 0xBB;
        cpu.regs.rdx = 0xCC;

        // Push all three
        exec_bytes(&mut cpu, &mut mem, &[0x50]); // PUSH RAX
        cpu.regs.rip = 0;
        exec_bytes(&mut cpu, &mut mem, &[0x51]); // PUSH RCX
        cpu.regs.rip = 0;
        exec_bytes(&mut cpu, &mut mem, &[0x52]); // PUSH RDX

        // Pop in reverse order into different regs
        cpu.regs.rip = 0;
        exec_bytes(&mut cpu, &mut mem, &[0x5B]); // POP RBX (gets RDX's value)
        assert_eq!(cpu.regs.rbx, 0xCC);
        cpu.regs.rip = 0;
        exec_bytes(&mut cpu, &mut mem, &[0x5E]); // POP RSI (gets RCX's value)
        assert_eq!(cpu.regs.rsi, 0xBB);
        cpu.regs.rip = 0;
        exec_bytes(&mut cpu, &mut mem, &[0x5F]); // POP RDI (gets RAX's value)
        assert_eq!(cpu.regs.rdi, 0xAA);
    }

    #[test]
    fn test_step_simple_program() {
        // Program: MOV EAX,5; ADD EAX,3; HLT
        let mut mem = vec![0u8; 0x2000];
        // MOV EAX, 5 → B8 05 00 00 00
        mem[0] = 0xB8;
        mem[1] = 0x05;
        // ADD EAX, 3 → 83 C0 03
        mem[5] = 0x83;
        mem[6] = 0xC0;
        mem[7] = 0x03;
        // HLT → F4
        mem[8] = 0xF4;

        let mut cpu = X86_64Cpu::new();
        cpu.regs.rip = 0;
        cpu.regs.rsp = 0x1000;

        let mut slice_mem = SliceMemory::new(&mut mem);
        cpu.step(&mut slice_mem).unwrap(); // MOV
        assert_eq!(cpu.regs.rax, 5);
        cpu.step(&mut slice_mem).unwrap(); // ADD
        assert_eq!(cpu.regs.rax, 8);
        cpu.step(&mut slice_mem).unwrap(); // HLT
        assert!(cpu.is_halted());
    }

    #[test]
    fn test_get_reg8_rex_indices() {
        let mut cpu = X86_64Cpu::new();
        cpu.regs.r8 = 0xDEAD_BEEF_1234_5678;
        cpu.regs.r9 = 0x00FF_00FF_00FF_00AB;
        cpu.regs.r15 = 0x0000_0000_0000_00CD;

        assert_eq!(cpu.get_reg8(8), 0x78); // R8B = low byte of R8
        assert_eq!(cpu.get_reg8(9), 0xAB); // R9B
        assert_eq!(cpu.get_reg8(15), 0xCD); // R15B
        assert_eq!(cpu.get_reg8(16), 0); // out of range
    }

    #[test]
    fn test_set_reg8_rex_indices() {
        let mut cpu = X86_64Cpu::new();
        cpu.regs.r10 = 0xFFFF_FFFF_FFFF_FF00;
        cpu.set_reg8(10, 0x42);
        assert_eq!(cpu.regs.r10, 0xFFFF_FFFF_FFFF_FF42); // only low byte changed

        cpu.regs.r12 = 0x1234_5678_9ABC_DEF0;
        cpu.set_reg8(12, 0xAA);
        assert_eq!(cpu.regs.r12, 0x1234_5678_9ABC_DEAA);

        // Verify legacy regs unaffected
        cpu.regs.rax = 0x1100;
        cpu.set_reg8(0, 0x55); // AL
        assert_eq!(cpu.regs.rax, 0x1155);
    }
}
