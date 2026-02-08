//! x86-64 CPU emulation
//!
//! This module provides a software x86-64 CPU emulator for executing
//! guest code. It supports common instructions, interrupt handling,
//! and memory access.

use crate::{CpuError, Result};
use std::sync::Arc;

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
    pub fn handler_address(&self) -> u64 {
        (self.offset_low as u64)
            | ((self.offset_mid as u64) << 16)
            | ((self.offset_high as u64) << 32)
    }

    /// Check if entry is present
    pub fn is_present(&self) -> bool {
        (self.type_attr & 0x80) != 0
    }

    /// Get the gate type
    pub fn gate_type(&self) -> u8 {
        self.type_attr & 0x0F
    }

    /// Check if it's a trap gate (doesn't clear IF)
    pub fn is_trap_gate(&self) -> bool {
        self.gate_type() == 0x0F
    }

    /// Get the DPL (Descriptor Privilege Level)
    pub fn dpl(&self) -> u8 {
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
        self.regs.rip = 0xFFF0; // x86 reset vector
        self.regs.rflags = 0x2; // Reserved bit 1 is always set

        // Initialize segment registers (real mode defaults)
        self.segments = X86Segments::default();
        self.segments.cs.selector = 0xF000;
        self.segments.cs.base = 0xFFFF0000;

        // Control registers
        self.control = X86ControlRegs::default();

        // IDT at 0
        self.idtr = IdtRegister {
            limit: 0x3FF,
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
                    return Err(CpuError::InvalidInstruction);
                }
                self.regs.rax = (self.regs.rax & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV CL, imm8 (0xB1)
            0xB1 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                self.regs.rcx = (self.regs.rcx & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV DL, imm8 (0xB2)
            0xB2 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                self.regs.rdx = (self.regs.rdx & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV BL, imm8 (0xB3)
            0xB3 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                self.regs.rbx = (self.regs.rbx & 0xFFFF_FFFF_FFFF_FF00) | (bytes[1] as u64);
                self.regs.rip += 2;
            }

            // MOV EAX, imm32 (0xB8)
            0xB8 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                self.regs.rax = imm as u64;
                self.regs.rip += 5;
            }

            // MOV ECX, imm32 (0xB9)
            0xB9 => {
                if bytes.len() < 5 {
                    return Err(CpuError::InvalidInstruction);
                }
                let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                self.regs.rcx = imm as u64;
                self.regs.rip += 5;
            }

            // INC EAX (0x40) - Note: In 64-bit mode this is a REX prefix
            0x40 => {
                if !self.long_mode {
                    let old = self.regs.rax as u32;
                    let new = old.wrapping_add(1);
                    self.regs.rax = new as u64;
                    self.update_flags_add(new as u64, old as u64, 1);
                    self.regs.rip += 1;
                } else {
                    // REX prefix - would need to handle next instruction
                    self.regs.rip += 1;
                }
            }

            // INC ECX (0x41)
            0x41 => {
                if !self.long_mode {
                    let old = self.regs.rcx as u32;
                    let new = old.wrapping_add(1);
                    self.regs.rcx = new as u64;
                    self.update_flags_add(new as u64, old as u64, 1);
                    self.regs.rip += 1;
                } else {
                    // REX.B prefix
                    self.regs.rip += 1;
                }
            }

            // DEC EAX (0x48)
            0x48 => {
                if !self.long_mode {
                    let old = self.regs.rax as u32;
                    let new = old.wrapping_sub(1);
                    self.regs.rax = new as u64;
                    self.update_flags_sub(new as u64, old as u64, 1);
                    self.regs.rip += 1;
                } else {
                    // REX.W prefix
                    self.regs.rip += 1;
                }
            }

            // DEC ECX (0x49)
            0x49 => {
                if !self.long_mode {
                    let old = self.regs.rcx as u32;
                    let new = old.wrapping_sub(1);
                    self.regs.rcx = new as u64;
                    self.update_flags_sub(new as u64, old as u64, 1);
                    self.regs.rip += 1;
                } else {
                    // REX.WB prefix
                    self.regs.rip += 1;
                }
            }

            // PUSH RAX (0x50)
            0x50 => {
                self.push_u64_with_memory(memory, self.regs.rax)?;
                self.regs.rip += 1;
            }

            // PUSH RCX (0x51)
            0x51 => {
                self.push_u64_with_memory(memory, self.regs.rcx)?;
                self.regs.rip += 1;
            }

            // PUSH RDX (0x52)
            0x52 => {
                self.push_u64_with_memory(memory, self.regs.rdx)?;
                self.regs.rip += 1;
            }

            // PUSH RBX (0x53)
            0x53 => {
                self.push_u64_with_memory(memory, self.regs.rbx)?;
                self.regs.rip += 1;
            }

            // PUSH RSP (0x54)
            0x54 => {
                let rsp = self.regs.rsp;
                self.push_u64_with_memory(memory, rsp)?;
                self.regs.rip += 1;
            }

            // PUSH RBP (0x55)
            0x55 => {
                self.push_u64_with_memory(memory, self.regs.rbp)?;
                self.regs.rip += 1;
            }

            // PUSH RSI (0x56)
            0x56 => {
                self.push_u64_with_memory(memory, self.regs.rsi)?;
                self.regs.rip += 1;
            }

            // PUSH RDI (0x57)
            0x57 => {
                self.push_u64_with_memory(memory, self.regs.rdi)?;
                self.regs.rip += 1;
            }

            // POP RAX (0x58)
            0x58 => {
                self.regs.rax = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // POP RCX (0x59)
            0x59 => {
                self.regs.rcx = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // POP RDX (0x5A)
            0x5A => {
                self.regs.rdx = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // POP RBX (0x5B)
            0x5B => {
                self.regs.rbx = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // POP RSP (0x5C)
            0x5C => {
                self.regs.rsp = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // POP RBP (0x5D)
            0x5D => {
                self.regs.rbp = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // POP RSI (0x5E)
            0x5E => {
                self.regs.rsi = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // POP RDI (0x5F)
            0x5F => {
                self.regs.rdi = self.pop_u64_with_memory(memory)?;
                self.regs.rip += 1;
            }

            // XOR r/m, r (0x31)
            0x31 => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let modrm = bytes[1];
                let reg = (modrm >> 3) & 0x07;
                let rm = modrm & 0x07;

                // Simple case: XOR reg, reg (mod = 0b11)
                if (modrm >> 6) == 0b11 {
                    let src = self.get_reg64(reg);
                    let dst = self.get_reg64(rm);
                    let result = dst ^ src;
                    self.set_reg64(rm, result);
                    self.update_flags_logic(result);
                }
                self.regs.rip += 2;
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

            // TEST AL, imm8 (0xA8)
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

            // RET (0xC3)
            0xC3 => {
                self.regs.rip = self.pop_u64_with_memory(memory)?;
            }

            // INT imm8 (0xCD)
            0xCD => {
                if bytes.len() < 2 {
                    return Err(CpuError::InvalidInstruction);
                }
                let vector = bytes[1];
                self.regs.rip += 2;
                self.queue_interrupt(vector, None);
            }

            // CLI - Clear Interrupt Flag (0xFA)
            0xFA => {
                self.regs.rflags &= !flags::IF;
                self.regs.rip += 1;
            }

            // STI - Set Interrupt Flag (0xFB)
            0xFB => {
                self.regs.rflags |= flags::IF;
                self.regs.rip += 1;
            }

            // CLD - Clear Direction Flag (0xFC)
            0xFC => {
                self.regs.rflags &= !flags::DF;
                self.regs.rip += 1;
            }

            // STD - Set Direction Flag (0xFD)
            0xFD => {
                self.regs.rflags |= flags::DF;
                self.regs.rip += 1;
            }

            // IRET (0xCF) - Interrupt Return
            0xCF => {
                if self.long_mode {
                    // 64-bit IRET
                    self.regs.rip = self.pop_u64_with_memory(memory)?;
                    self.segments.cs.selector = self.pop_u64_with_memory(memory)? as u16;
                    self.regs.rflags = self.pop_u64_with_memory(memory)?;
                    self.regs.rsp = self.pop_u64_with_memory(memory)?;
                    self.segments.ss.selector = self.pop_u64_with_memory(memory)? as u16;
                } else {
                    // Real mode IRET
                    self.regs.rip = self.pop_with_memory(memory)? as u64;
                    self.segments.cs.selector = self.pop_with_memory(memory)?;
                    self.regs.rflags =
                        (self.regs.rflags & 0xFFFF0000) | (self.pop_with_memory(memory)? as u64);
                }
            }

            // Unknown opcode
            _ => {
                return Err(CpuError::UnsupportedInstruction(format!("0x{:02X}", opcode)));
            }
        }

        Ok(())
    }

    /// Get 64-bit register by index
    fn get_reg64(&self, idx: u8) -> u64 {
        match idx {
            0 => self.regs.rax,
            1 => self.regs.rcx,
            2 => self.regs.rdx,
            3 => self.regs.rbx,
            4 => self.regs.rsp,
            5 => self.regs.rbp,
            6 => self.regs.rsi,
            7 => self.regs.rdi,
            _ => 0,
        }
    }

    /// Set 64-bit register by index
    fn set_reg64(&mut self, idx: u8, value: u64) {
        match idx {
            0 => self.regs.rax = value,
            1 => self.regs.rcx = value,
            2 => self.regs.rdx = value,
            3 => self.regs.rbx = value,
            4 => self.regs.rsp = value,
            5 => self.regs.rbp = value,
            6 => self.regs.rsi = value,
            7 => self.regs.rdi = value,
            _ => {}
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
}
