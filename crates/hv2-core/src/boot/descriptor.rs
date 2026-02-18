//! Descriptor Tables for x86/x86-64
//!
//! This module provides types and builders for x86 descriptor tables:
//! - Global Descriptor Table (GDT)
//! - Interrupt Descriptor Table (IDT)
//! - Task State Segment (TSS)
//!
//! # Descriptor Tables Overview
//!
//! ## GDT (Global Descriptor Table)
//! Contains segment descriptors defining memory segments. In long mode,
//! segmentation is largely disabled but the GDT is still required for:
//! - Code segment (for CS selector)
//! - Data segment (for SS/DS/ES/FS/GS selectors)
//! - TSS descriptor (for task switching and IST)
//!
//! ## IDT (Interrupt Descriptor Table)
//! Contains gate descriptors for interrupt and exception handling.
//! Each entry defines the handler address and privilege level.
//!
//! ## TSS (Task State Segment)
//! Contains stack pointers for privilege level transitions and
//! Interrupt Stack Table (IST) for specialized handlers.

use crate::{Error, Result};

/// GDT segment descriptor flags
pub mod gdt_flags {
    /// Access byte: Present bit
    pub const PRESENT: u8 = 1 << 7;
    /// Access byte: Descriptor privilege level 0
    pub const DPL_0: u8 = 0 << 5;
    /// Access byte: Descriptor privilege level 3
    pub const DPL_3: u8 = 3 << 5;
    /// Access byte: Descriptor type (1 = code/data)
    pub const DESCRIPTOR_TYPE: u8 = 1 << 4;
    /// Access byte: Executable (code segment)
    pub const EXECUTABLE: u8 = 1 << 3;
    /// Access byte: Direction/Conforming
    pub const DC: u8 = 1 << 2;
    /// Access byte: Readable (code) / Writable (data)
    pub const RW: u8 = 1 << 1;
    /// Access byte: Accessed
    pub const ACCESSED: u8 = 1 << 0;

    /// Flags: Granularity (4KB pages)
    pub const GRANULARITY: u8 = 1 << 3;
    /// Flags: Size (32-bit protected mode)
    pub const SIZE_32: u8 = 1 << 2;
    /// Flags: Long mode (64-bit code segment)
    pub const LONG_MODE: u8 = 1 << 1;

    /// Common: kernel code segment (32-bit)
    pub const KERNEL_CODE_32: u8 = PRESENT | DPL_0 | DESCRIPTOR_TYPE | EXECUTABLE | RW;
    /// Common: kernel data segment
    pub const KERNEL_DATA: u8 = PRESENT | DPL_0 | DESCRIPTOR_TYPE | RW;
    /// Common: kernel code segment (64-bit)
    pub const KERNEL_CODE_64: u8 = PRESENT | DPL_0 | DESCRIPTOR_TYPE | EXECUTABLE | RW;
    /// Common: user code segment (32-bit)
    pub const USER_CODE_32: u8 = PRESENT | DPL_3 | DESCRIPTOR_TYPE | EXECUTABLE | RW;
    /// Common: user data segment  
    pub const USER_DATA: u8 = PRESENT | DPL_3 | DESCRIPTOR_TYPE | RW;
    /// Common: user code segment (64-bit)
    pub const USER_CODE_64: u8 = PRESENT | DPL_3 | DESCRIPTOR_TYPE | EXECUTABLE | RW;
    /// TSS descriptor access byte (Available 64-bit TSS)
    pub const TSS_AVAILABLE: u8 = PRESENT | 0x09;
}

/// IDT gate descriptor flags
pub mod idt_flags {
    /// Gate type: 32-bit interrupt gate
    pub const INTERRUPT_GATE_32: u8 = 0x0E;
    /// Gate type: 32-bit trap gate
    pub const TRAP_GATE_32: u8 = 0x0F;
    /// Gate type: 64-bit interrupt gate
    pub const INTERRUPT_GATE_64: u8 = 0x0E;
    /// Gate type: 64-bit trap gate
    pub const TRAP_GATE_64: u8 = 0x0F;
    /// Present bit
    pub const PRESENT: u8 = 1 << 7;
    /// DPL 0 (kernel)
    pub const DPL_0: u8 = 0 << 5;
    /// DPL 3 (user)
    pub const DPL_3: u8 = 3 << 5;

    /// Kernel interrupt gate
    pub const KERNEL_INTERRUPT: u8 = PRESENT | DPL_0 | INTERRUPT_GATE_64;
    /// Kernel trap gate
    pub const KERNEL_TRAP: u8 = PRESENT | DPL_0 | TRAP_GATE_64;
    /// User interrupt gate (for syscall)
    pub const USER_INTERRUPT: u8 = PRESENT | DPL_3 | INTERRUPT_GATE_64;
}

/// 64-bit GDT entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GdtEntry64 {
    /// Limit bits 0-15
    pub limit_low: u16,
    /// Base address bits 0-15
    pub base_low: u16,
    /// Base address bits 16-23
    pub base_middle: u8,
    /// Access byte
    pub access: u8,
    /// Limit bits 16-19 (bits 0-3) and flags (bits 4-7)
    pub limit_flags: u8,
    /// Base address bits 24-31
    pub base_high: u8,
}

impl GdtEntry64 {
    /// Create a null descriptor
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            limit_flags: 0,
            base_high: 0,
        }
    }

    /// Create a code segment descriptor for 32-bit protected mode
    pub const fn code_32(base: u32, limit: u32, dpl: u8) -> Self {
        let access = gdt_flags::PRESENT
            | (dpl << 5)
            | gdt_flags::DESCRIPTOR_TYPE
            | gdt_flags::EXECUTABLE
            | gdt_flags::RW;
        let flags = gdt_flags::GRANULARITY | gdt_flags::SIZE_32;
        Self::new(base, limit, access, flags)
    }

    /// Create a data segment descriptor for 32-bit protected mode
    pub const fn data_32(base: u32, limit: u32, dpl: u8) -> Self {
        let access = gdt_flags::PRESENT | (dpl << 5) | gdt_flags::DESCRIPTOR_TYPE | gdt_flags::RW;
        let flags = gdt_flags::GRANULARITY | gdt_flags::SIZE_32;
        Self::new(base, limit, access, flags)
    }

    /// Create a code segment descriptor for 64-bit long mode
    pub const fn code_64(dpl: u8) -> Self {
        let access = gdt_flags::PRESENT
            | (dpl << 5)
            | gdt_flags::DESCRIPTOR_TYPE
            | gdt_flags::EXECUTABLE
            | gdt_flags::RW;
        let flags = gdt_flags::LONG_MODE;
        Self::new(0, 0, access, flags)
    }

    /// Create a data segment descriptor for 64-bit long mode
    pub const fn data_64(dpl: u8) -> Self {
        let access = gdt_flags::PRESENT | (dpl << 5) | gdt_flags::DESCRIPTOR_TYPE | gdt_flags::RW;
        let flags = 0;
        Self::new(0, 0, access, flags)
    }

    /// Create a new GDT entry with explicit fields
    pub const fn new(base: u32, limit: u32, access: u8, flags: u8) -> Self {
        let limit_low = limit as u16;
        let limit_high = ((limit >> 16) & 0x0F) as u8;

        Self {
            limit_low,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access,
            limit_flags: limit_high | (flags << 4),
            base_high: (base >> 24) as u8,
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 8] {
        let limit_low = self.limit_low;
        let base_low = self.base_low;
        [
            (limit_low & 0xFF) as u8,
            (limit_low >> 8) as u8,
            (base_low & 0xFF) as u8,
            (base_low >> 8) as u8,
            self.base_middle,
            self.access,
            self.limit_flags,
            self.base_high,
        ]
    }
}

/// TSS descriptor (16 bytes in 64-bit mode)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TssDescriptor64 {
    /// Limit bits 0-15
    pub limit_low: u16,
    /// Base address bits 0-15
    pub base_low: u16,
    /// Base address bits 16-23
    pub base_middle: u8,
    /// Type and attributes
    pub access: u8,
    /// Limit bits 16-19 and flags
    pub limit_flags: u8,
    /// Base address bits 24-31
    pub base_high: u8,
    /// Base address bits 32-63
    pub base_upper: u32,
    /// Reserved
    pub reserved: u32,
}

impl TssDescriptor64 {
    /// Create a TSS descriptor
    pub fn new(base: u64, limit: u32) -> Self {
        Self {
            limit_low: limit as u16,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access: gdt_flags::TSS_AVAILABLE,
            limit_flags: ((limit >> 16) & 0x0F) as u8,
            base_high: (base >> 24) as u8,
            base_upper: (base >> 32) as u32,
            reserved: 0,
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        let limit_low = self.limit_low;
        let base_low = self.base_low;
        let base_upper = self.base_upper;
        let reserved = self.reserved;
        let mut bytes = [0u8; 16];
        bytes[0..2].copy_from_slice(&limit_low.to_le_bytes());
        bytes[2..4].copy_from_slice(&base_low.to_le_bytes());
        bytes[4] = self.base_middle;
        bytes[5] = self.access;
        bytes[6] = self.limit_flags;
        bytes[7] = self.base_high;
        bytes[8..12].copy_from_slice(&base_upper.to_le_bytes());
        bytes[12..16].copy_from_slice(&reserved.to_le_bytes());
        bytes
    }
}

/// 64-bit Task State Segment
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Tss64 {
    /// Reserved
    pub reserved0: u32,
    /// RSP for privilege level 0
    pub rsp0: u64,
    /// RSP for privilege level 1
    pub rsp1: u64,
    /// RSP for privilege level 2
    pub rsp2: u64,
    /// Reserved
    pub reserved1: u64,
    /// IST1
    pub ist1: u64,
    /// IST2
    pub ist2: u64,
    /// IST3
    pub ist3: u64,
    /// IST4
    pub ist4: u64,
    /// IST5
    pub ist5: u64,
    /// IST6
    pub ist6: u64,
    /// IST7
    pub ist7: u64,
    /// Reserved
    pub reserved2: u64,
    /// Reserved
    pub reserved3: u16,
    /// I/O Map Base Address
    pub iomap_base: u16,
}

impl Default for Tss64 {
    fn default() -> Self {
        Self {
            reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            reserved1: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            ist4: 0,
            ist5: 0,
            ist6: 0,
            ist7: 0,
            reserved2: 0,
            reserved3: 0,
            iomap_base: 104, // Size of TSS
        }
    }
}

impl Tss64 {
    /// TSS size
    pub const SIZE: usize = 104;

    /// Create a new TSS with kernel stack
    pub fn new(kernel_stack: u64) -> Self {
        Self {
            rsp0: kernel_stack,
            ..Default::default()
        }
    }

    /// Set an IST entry
    pub fn set_ist(&mut self, index: usize, stack: u64) -> Result<()> {
        match index {
            1 => self.ist1 = stack,
            2 => self.ist2 = stack,
            3 => self.ist3 = stack,
            4 => self.ist4 = stack,
            5 => self.ist5 = stack,
            6 => self.ist6 = stack,
            7 => self.ist7 = stack,
            _ => return Err(Error::VM(format!("Invalid IST index: {}", index))),
        }
        Ok(())
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let reserved0 = self.reserved0;
        let rsp0 = self.rsp0;
        let rsp1 = self.rsp1;
        let rsp2 = self.rsp2;
        let reserved1 = self.reserved1;
        let ist1 = self.ist1;
        let ist2 = self.ist2;
        let ist3 = self.ist3;
        let ist4 = self.ist4;
        let ist5 = self.ist5;
        let ist6 = self.ist6;
        let ist7 = self.ist7;
        let reserved2 = self.reserved2;
        let reserved3 = self.reserved3;
        let iomap_base = self.iomap_base;

        let mut bytes = [0u8; Self::SIZE];
        bytes[0..4].copy_from_slice(&reserved0.to_le_bytes());
        bytes[4..12].copy_from_slice(&rsp0.to_le_bytes());
        bytes[12..20].copy_from_slice(&rsp1.to_le_bytes());
        bytes[20..28].copy_from_slice(&rsp2.to_le_bytes());
        bytes[28..36].copy_from_slice(&reserved1.to_le_bytes());
        bytes[36..44].copy_from_slice(&ist1.to_le_bytes());
        bytes[44..52].copy_from_slice(&ist2.to_le_bytes());
        bytes[52..60].copy_from_slice(&ist3.to_le_bytes());
        bytes[60..68].copy_from_slice(&ist4.to_le_bytes());
        bytes[68..76].copy_from_slice(&ist5.to_le_bytes());
        bytes[76..84].copy_from_slice(&ist6.to_le_bytes());
        bytes[84..92].copy_from_slice(&ist7.to_le_bytes());
        bytes[92..100].copy_from_slice(&reserved2.to_le_bytes());
        bytes[100..102].copy_from_slice(&reserved3.to_le_bytes());
        bytes[102..104].copy_from_slice(&iomap_base.to_le_bytes());
        bytes
    }
}

/// 64-bit IDT entry (interrupt gate descriptor)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IdtEntry64 {
    /// Offset bits 0-15
    pub offset_low: u16,
    /// Code segment selector
    pub selector: u16,
    /// IST index (bits 0-2), reserved (bits 3-7)
    pub ist: u8,
    /// Type and attributes
    pub type_attr: u8,
    /// Offset bits 16-31
    pub offset_middle: u16,
    /// Offset bits 32-63
    pub offset_high: u32,
    /// Reserved
    pub reserved: u32,
}

impl IdtEntry64 {
    /// Entry size
    pub const SIZE: usize = 16;

    /// Create a null entry
    pub const fn null() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// Create an interrupt gate
    pub fn interrupt_gate(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: ist & 0x07,
            type_attr: idt_flags::PRESENT | (dpl << 5) | idt_flags::INTERRUPT_GATE_64,
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }

    /// Create a trap gate
    pub fn trap_gate(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: ist & 0x07,
            type_attr: idt_flags::PRESENT | (dpl << 5) | idt_flags::TRAP_GATE_64,
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let offset_low = self.offset_low;
        let selector = self.selector;
        let offset_middle = self.offset_middle;
        let offset_high = self.offset_high;
        let reserved = self.reserved;

        let mut bytes = [0u8; Self::SIZE];
        bytes[0..2].copy_from_slice(&offset_low.to_le_bytes());
        bytes[2..4].copy_from_slice(&selector.to_le_bytes());
        bytes[4] = self.ist;
        bytes[5] = self.type_attr;
        bytes[6..8].copy_from_slice(&offset_middle.to_le_bytes());
        bytes[8..12].copy_from_slice(&offset_high.to_le_bytes());
        bytes[12..16].copy_from_slice(&reserved.to_le_bytes());
        bytes
    }
}

/// GDTR/IDTR register value
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorTableRegister {
    /// Table limit (size - 1)
    pub limit: u16,
    /// Base address
    pub base: u64,
}

impl DescriptorTableRegister {
    /// Create a new descriptor table register value
    pub const fn new(base: u64, size: u16) -> Self {
        Self {
            limit: size.saturating_sub(1),
            base,
        }
    }

    /// Convert to bytes (for writing to memory)
    pub fn to_bytes(&self) -> [u8; 10] {
        let limit = self.limit;
        let base = self.base;
        let mut bytes = [0u8; 10];
        bytes[0..2].copy_from_slice(&limit.to_le_bytes());
        bytes[2..10].copy_from_slice(&base.to_le_bytes());
        bytes
    }
}

/// IDT builder for creating interrupt descriptor tables
pub struct IdtBuilder {
    entries: [IdtEntry64; 256],
    code_selector: u16,
}

impl IdtBuilder {
    /// Create a new IDT builder
    pub fn new(code_selector: u16) -> Self {
        Self {
            entries: [IdtEntry64::null(); 256],
            code_selector,
        }
    }

    /// Set an interrupt gate for a vector
    pub fn set_interrupt_gate(&mut self, vector: u8, handler: u64, ist: u8) -> &mut Self {
        self.entries[vector as usize] = IdtEntry64::interrupt_gate(
            handler,
            self.code_selector,
            ist,
            0, // DPL 0 for kernel
        );
        self
    }

    /// Set a trap gate for a vector
    pub fn set_trap_gate(&mut self, vector: u8, handler: u64, ist: u8) -> &mut Self {
        self.entries[vector as usize] = IdtEntry64::trap_gate(handler, self.code_selector, ist, 0);
        self
    }

    /// Set a user-callable interrupt gate (DPL 3)
    pub fn set_user_interrupt_gate(&mut self, vector: u8, handler: u64, ist: u8) -> &mut Self {
        self.entries[vector as usize] = IdtEntry64::interrupt_gate(
            handler,
            self.code_selector,
            ist,
            3, // DPL 3 for user
        );
        self
    }

    /// Build the IDT as bytes
    pub fn build(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256 * IdtEntry64::SIZE);
        for entry in &self.entries {
            bytes.extend_from_slice(&entry.to_bytes());
        }
        bytes
    }

    /// Get the size of the IDT
    pub const fn size() -> usize {
        256 * IdtEntry64::SIZE
    }
}

/// Exception vectors
pub mod exceptions {
    /// Division Error
    pub const DIVIDE_ERROR: u8 = 0;
    /// Debug Exception
    pub const DEBUG: u8 = 1;
    /// NMI Interrupt
    pub const NMI: u8 = 2;
    /// Breakpoint
    pub const BREAKPOINT: u8 = 3;
    /// Overflow
    pub const OVERFLOW: u8 = 4;
    /// BOUND Range Exceeded
    pub const BOUND: u8 = 5;
    /// Invalid Opcode
    pub const INVALID_OPCODE: u8 = 6;
    /// Device Not Available
    pub const DEVICE_NOT_AVAILABLE: u8 = 7;
    /// Double Fault
    pub const DOUBLE_FAULT: u8 = 8;
    /// Coprocessor Segment Overrun
    pub const COPROCESSOR_SEGMENT: u8 = 9;
    /// Invalid TSS
    pub const INVALID_TSS: u8 = 10;
    /// Segment Not Present
    pub const SEGMENT_NOT_PRESENT: u8 = 11;
    /// Stack-Segment Fault
    pub const STACK_FAULT: u8 = 12;
    /// General Protection Fault
    pub const GENERAL_PROTECTION: u8 = 13;
    /// Page Fault
    pub const PAGE_FAULT: u8 = 14;
    /// x87 FPU Error
    pub const X87_FPU: u8 = 16;
    /// Alignment Check
    pub const ALIGNMENT_CHECK: u8 = 17;
    /// Machine Check
    pub const MACHINE_CHECK: u8 = 18;
    /// SIMD Floating Point Exception
    pub const SIMD_FP: u8 = 19;
    /// Virtualization Exception
    pub const VIRTUALIZATION: u8 = 20;
    /// Control Protection Exception
    pub const CONTROL_PROTECTION: u8 = 21;

    /// Returns true if the exception pushes an error code
    pub const fn has_error_code(vector: u8) -> bool {
        matches!(
            vector,
            DOUBLE_FAULT
                | INVALID_TSS
                | SEGMENT_NOT_PRESENT
                | STACK_FAULT
                | GENERAL_PROTECTION
                | PAGE_FAULT
                | ALIGNMENT_CHECK
                | CONTROL_PROTECTION
        )
    }
}

/// GDT builder for creating global descriptor tables
pub struct GdtBuilder {
    entries: Vec<u8>,
}

impl GdtBuilder {
    /// Create a new GDT builder with null descriptor
    pub fn new() -> Self {
        let mut builder = Self {
            entries: Vec::new(),
        };
        // First entry must be null
        builder.entries.extend_from_slice(&[0u8; 8]);
        builder
    }

    /// Add a 64-bit GDT entry
    pub fn add_entry(&mut self, entry: GdtEntry64) -> u16 {
        let selector = self.entries.len() as u16;
        self.entries.extend_from_slice(&entry.to_bytes());
        selector
    }

    /// Add a TSS descriptor (16 bytes)
    pub fn add_tss(&mut self, tss: TssDescriptor64) -> u16 {
        let selector = self.entries.len() as u16;
        self.entries.extend_from_slice(&tss.to_bytes());
        selector
    }

    /// Build the GDT as bytes
    pub fn build(&self) -> Vec<u8> {
        self.entries.clone()
    }

    /// Get the current size
    pub fn size(&self) -> usize {
        self.entries.len()
    }
}

impl Default for GdtBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard GDT layout for 64-bit mode
pub struct StandardGdt64 {
    /// GDT data
    pub data: Vec<u8>,
    /// Kernel code selector
    pub kernel_code: u16,
    /// Kernel data selector
    pub kernel_data: u16,
    /// User code selector
    pub user_code: u16,
    /// User data selector
    pub user_data: u16,
    /// TSS selector
    pub tss: u16,
}

impl StandardGdt64 {
    /// Create a standard 64-bit GDT
    pub fn new(tss_base: u64) -> Self {
        let mut builder = GdtBuilder::new();

        // Entry 1: Kernel code (64-bit)
        let kernel_code = builder.add_entry(GdtEntry64::code_64(0));

        // Entry 2: Kernel data
        let kernel_data = builder.add_entry(GdtEntry64::data_64(0));

        // Entry 3: User code (64-bit)
        let user_code = builder.add_entry(GdtEntry64::code_64(3));

        // Entry 4: User data
        let user_data = builder.add_entry(GdtEntry64::data_64(3));

        // Entry 5-6: TSS (16 bytes)
        let tss = builder.add_tss(TssDescriptor64::new(tss_base, Tss64::SIZE as u32 - 1));

        Self {
            data: builder.build(),
            kernel_code,
            kernel_data,
            user_code,
            user_data,
            tss,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gdt_entry_null() {
        let entry = GdtEntry64::null();
        assert_eq!(entry.to_bytes(), [0u8; 8]);
    }

    #[test]
    fn test_gdt_entry_code_64() {
        let entry = GdtEntry64::code_64(0);

        // Check access byte
        let bytes = entry.to_bytes();
        assert_eq!(bytes[5] & gdt_flags::PRESENT, gdt_flags::PRESENT);
        assert_eq!(bytes[5] & gdt_flags::EXECUTABLE, gdt_flags::EXECUTABLE);

        // Check long mode flag
        assert_eq!(bytes[6] >> 4 & gdt_flags::LONG_MODE, gdt_flags::LONG_MODE);
    }

    #[test]
    fn test_gdt_entry_data_64() {
        let entry = GdtEntry64::data_64(0);
        let bytes = entry.to_bytes();

        assert_eq!(bytes[5] & gdt_flags::PRESENT, gdt_flags::PRESENT);
        assert_eq!(bytes[5] & gdt_flags::EXECUTABLE, 0); // Not executable
    }

    #[test]
    fn test_gdt_entry_code_32() {
        let entry = GdtEntry64::code_32(0, 0xFFFFF, 0);
        let bytes = entry.to_bytes();

        // Check granularity and size flags
        let flags = bytes[6] >> 4;
        assert_eq!(flags & gdt_flags::GRANULARITY, gdt_flags::GRANULARITY);
        assert_eq!(flags & gdt_flags::SIZE_32, gdt_flags::SIZE_32);
    }

    #[test]
    fn test_tss_descriptor() {
        let tss_base: u64 = 0x1234567890ABCDEF;
        let desc = TssDescriptor64::new(tss_base, 103);
        let bytes = desc.to_bytes();

        // Check limit
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 103);

        // Check base low
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 0xCDEF);

        // Check base upper
        assert_eq!(
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            0x12345678
        );
    }

    #[test]
    fn test_tss64_default() {
        let tss = Tss64::default();
        // Copy packed fields to avoid unaligned reference errors
        let iomap = tss.iomap_base;
        let rsp = tss.rsp0;
        assert_eq!(iomap, 104);
        assert_eq!(rsp, 0);
    }

    #[test]
    fn test_tss64_with_stack() {
        let tss = Tss64::new(0xFFFF_0000);
        let rsp = tss.rsp0;
        assert_eq!(rsp, 0xFFFF_0000);
    }

    #[test]
    fn test_tss64_ist() {
        let mut tss = Tss64::default();
        tss.set_ist(1, 0x1000).unwrap();
        tss.set_ist(7, 0x7000).unwrap();

        // Copy packed fields to avoid unaligned reference errors
        let ist1 = tss.ist1;
        let ist7 = tss.ist7;
        assert_eq!(ist1, 0x1000);
        assert_eq!(ist7, 0x7000);

        // Invalid index
        assert!(tss.set_ist(0, 0).is_err());
        assert!(tss.set_ist(8, 0).is_err());
    }

    #[test]
    fn test_idt_entry_interrupt_gate() {
        let handler: u64 = 0x0000_1000_0000_5678;
        let entry = IdtEntry64::interrupt_gate(handler, 0x08, 1, 0);

        let bytes = entry.to_bytes();

        // Check offset parts
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 0x5678);
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 0x0000);
        assert_eq!(
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            0x0000_1000
        );

        // Check selector
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 0x08);

        // Check IST
        assert_eq!(bytes[4], 1);

        // Check type
        assert_eq!(bytes[5] & 0x0F, idt_flags::INTERRUPT_GATE_64);
        assert_eq!(bytes[5] & idt_flags::PRESENT, idt_flags::PRESENT);
    }

    #[test]
    fn test_idt_builder() {
        let mut builder = IdtBuilder::new(0x08);

        builder.set_interrupt_gate(0, 0x1000, 0);
        builder.set_trap_gate(14, 0x2000, 1);
        builder.set_user_interrupt_gate(0x80, 0x3000, 0);

        let idt = builder.build();
        assert_eq!(idt.len(), 256 * 16);
    }

    #[test]
    fn test_descriptor_table_register() {
        let dtr = DescriptorTableRegister::new(0x1000, 256);
        let bytes = dtr.to_bytes();

        // Limit should be size - 1
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 255);
        assert_eq!(
            u64::from_le_bytes([
                bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9]
            ]),
            0x1000
        );
    }

    #[test]
    fn test_gdt_builder() {
        let mut builder = GdtBuilder::new();

        let code = builder.add_entry(GdtEntry64::code_64(0));
        let data = builder.add_entry(GdtEntry64::data_64(0));

        assert_eq!(code, 8); // After null descriptor
        assert_eq!(data, 16); // After code descriptor
        assert_eq!(builder.size(), 24);
    }

    #[test]
    fn test_standard_gdt64() {
        let gdt = StandardGdt64::new(0x5000);

        // Check selectors
        assert_eq!(gdt.kernel_code, 8);
        assert_eq!(gdt.kernel_data, 16);
        assert_eq!(gdt.user_code, 24);
        assert_eq!(gdt.user_data, 32);
        assert_eq!(gdt.tss, 40);

        // GDT should be 56 bytes (null + 4 segments + TSS)
        assert_eq!(gdt.data.len(), 56);
    }

    #[test]
    fn test_exceptions_has_error_code() {
        assert!(!exceptions::has_error_code(exceptions::DIVIDE_ERROR));
        assert!(!exceptions::has_error_code(exceptions::BREAKPOINT));
        assert!(exceptions::has_error_code(exceptions::DOUBLE_FAULT));
        assert!(exceptions::has_error_code(exceptions::GENERAL_PROTECTION));
        assert!(exceptions::has_error_code(exceptions::PAGE_FAULT));
    }

    #[test]
    fn test_tss64_to_bytes() {
        let tss = Tss64::new(0xDEAD_BEEF);
        let bytes = tss.to_bytes();

        assert_eq!(bytes.len(), Tss64::SIZE);

        // Check RSP0 at offset 4
        let rsp0 = u64::from_le_bytes([
            bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11],
        ]);
        assert_eq!(rsp0, 0xDEAD_BEEF);

        // Check iomap_base at offset 102
        let iomap = u16::from_le_bytes([bytes[102], bytes[103]]);
        assert_eq!(iomap, 104);
    }
}
