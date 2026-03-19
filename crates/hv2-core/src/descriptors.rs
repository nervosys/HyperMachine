//! x86-64 Descriptor Table Management
//!
//! This module provides structures and utilities for managing:
//! - **GDT (Global Descriptor Table)**: Segment descriptors for memory protection
//! - **IDT (Interrupt Descriptor Table)**: Interrupt and exception handlers
//! - **LDT (Local Descriptor Table)**: Task-specific segments (future)
//!
//! # Overview
//!
//! x86-64 processors use descriptor tables to manage memory segmentation,
//! protection, and interrupt handling. While long mode (64-bit) largely
//! deprecates segmentation, descriptor tables are still required for:
//!
//! - Code segment definition (ring 0/3 separation)
//! - Stack segment for privilege transitions
//! - Task State Segment (TSS) for interrupt handling
//! - Interrupt dispatch via IDT
//!
//! # References
//!
//! - Intel SDM Volume 3A, Chapter 3: Protected-Mode Memory Management
//! - Intel SDM Volume 3A, Section 3.4.5: Segment Descriptors
//! - Intel SDM Volume 3A, Section 6.14: Exception and Interrupt Handling
//!
//! # Example
//!
//! ```
//! use hv2_core::descriptors::{GdtBuilder, SegmentDescriptor, DESC_DPL_0};
//!
//! // Create a minimal GDT for long mode
//! let gdt = GdtBuilder::new()
//!     .add_null()                  // Entry 0: Null descriptor (required)
//!     .add_code_64bit(DESC_DPL_0)  // Entry 1: 64-bit code segment (ring 0)
//!     .add_data_64bit(DESC_DPL_0)  // Entry 2: 64-bit data segment (ring 0)
//!     .build();
//!
//! // GDT is now ready to be loaded into guest memory
//! assert_eq!(gdt.len(), 24); // 3 entries * 8 bytes each
//! ```

use serde::{Deserialize, Serialize};

// ============================================================================
// Segment Descriptor Constants
// ============================================================================

// Access byte flags (byte 5 of descriptor)

/// Descriptor Present bit (must be 1 for valid descriptor)
pub const DESC_PRESENT: u8 = 1 << 7;

/// Descriptor Privilege Level - Ring 0 (kernel/supervisor)
pub const DESC_DPL_0: u8 = 0 << 5;

/// Descriptor Privilege Level - Ring 1
pub const DESC_DPL_1: u8 = 1 << 5;

/// Descriptor Privilege Level - Ring 2
pub const DESC_DPL_2: u8 = 2 << 5;

/// Descriptor Privilege Level - Ring 3 (user/application)
pub const DESC_DPL_3: u8 = 3 << 5;

/// Descriptor Type - Code or Data segment (vs system segment)
pub const DESC_CODE_DATA: u8 = 1 << 4;

/// Executable segment (code segment if set, data segment if clear)
pub const DESC_EXECUTABLE: u8 = 1 << 3;

/// Direction/Conforming bit
/// - For data: 0=grows up, 1=grows down
/// - For code: 0=non-conforming, 1=conforming
pub const DESC_DIRECTION_CONFORMING: u8 = 1 << 2;

/// Readable bit (for code segments) / Writable bit (for data segments)
pub const DESC_READABLE: u8 = 1 << 1;
pub const DESC_WRITABLE: u8 = 1 << 1;

/// Accessed bit (set by CPU when segment is accessed)
pub const DESC_ACCESSED: u8 = 1 << 0;

// Granularity/Flags byte (byte 6, upper 4 bits)

/// Granularity: 1 = 4KB pages, 0 = 1 byte
pub const DESC_GRANULAR: u8 = 1 << 7;

/// Size: 1 = 32-bit protected mode, 0 = 16-bit protected mode
pub const DESC_32BIT: u8 = 1 << 6;

/// Long mode: 1 = 64-bit code segment (mutually exclusive with DESC_32BIT)
pub const DESC_64BIT: u8 = 1 << 5;

// System segment types (when DESC_CODE_DATA is clear)

/// LDT (Local Descriptor Table) system segment
pub const SYS_LDT: u8 = 0x2;

/// TSS (Task State Segment) - Available
pub const SYS_TSS_AVAILABLE: u8 = 0x9;

/// TSS (Task State Segment) - Busy
pub const SYS_TSS_BUSY: u8 = 0xB;

/// Call Gate
pub const SYS_CALL_GATE: u8 = 0xC;

/// Interrupt Gate (64-bit)
pub const SYS_INTERRUPT_GATE: u8 = 0xE;

/// Trap Gate (64-bit)
pub const SYS_TRAP_GATE: u8 = 0xF;

// ============================================================================
// GDT (Global Descriptor Table) Structures
// ============================================================================

/// Segment Descriptor (8 bytes)
///
/// Describes a memory segment with base address, limit, and access rights.
/// Used in GDT and LDT.
///
/// # Layout (64 bits total)
///
/// ```text
/// Bits 0-15:   Limit Low (bits 0-15 of segment limit)
/// Bits 16-31:  Base Low (bits 0-15 of base address)
/// Bits 32-39:  Base Middle (bits 16-23 of base address)
/// Bits 40-47:  Access Byte (P, DPL, S, Type, A)
/// Bits 48-51:  Limit High (bits 16-19 of segment limit)
/// Bits 52-55:  Flags (G, D/B, L, AVL)
/// Bits 56-63:  Base High (bits 24-31 of base address)
/// ```
///
/// # Note on Long Mode
///
/// In 64-bit long mode, most segment attributes are ignored except:
/// - Code segment L (long) bit must be set for 64-bit code
/// - DPL (privilege level) for ring transitions
/// - Present bit must be set
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SegmentDescriptor {
    /// Segment limit bits 0-15
    pub limit_low: u16,

    /// Base address bits 0-15
    pub base_low: u16,

    /// Base address bits 16-23
    pub base_middle: u8,

    /// Access byte: Present, DPL, Type, etc.
    pub access: u8,

    /// Flags (upper 4 bits) + Limit bits 16-19 (lower 4 bits)
    pub granularity: u8,

    /// Base address bits 24-31
    pub base_high: u8,
}

impl SegmentDescriptor {
    /// Create a null descriptor (all zeros)
    ///
    /// The null descriptor must be the first entry in the GDT.
    /// Any attempt to load a null selector into a segment register
    /// (except CS) will cause a general protection fault.
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::SegmentDescriptor;
    /// let null_desc = SegmentDescriptor::null();
    /// assert_eq!(null_desc.access, 0);
    /// ```
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            granularity: 0,
            base_high: 0,
        }
    }

    /// Create a 64-bit code segment descriptor
    ///
    /// In long mode, code segments must have the L (long) bit set.
    /// Base and limit are ignored in 64-bit mode.
    ///
    /// # Arguments
    ///
    /// * `dpl` - Descriptor Privilege Level (0 = kernel, 3 = user)
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::{SegmentDescriptor, DESC_DPL_0};
    /// let code_seg = SegmentDescriptor::code_64bit(DESC_DPL_0);
    /// ```
    pub const fn code_64bit(dpl: u8) -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: DESC_PRESENT | dpl | DESC_CODE_DATA | DESC_EXECUTABLE | DESC_READABLE,
            granularity: DESC_64BIT, // L bit set for 64-bit
            base_high: 0,
        }
    }

    /// Create a 64-bit data segment descriptor
    ///
    /// In long mode, data segments are largely ignored, but must still
    /// be present in the GDT for loading into DS, ES, SS.
    ///
    /// # Arguments
    ///
    /// * `dpl` - Descriptor Privilege Level (0 = kernel, 3 = user)
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::{SegmentDescriptor, DESC_DPL_0};
    /// let data_seg = SegmentDescriptor::data_64bit(DESC_DPL_0);
    /// ```
    pub const fn data_64bit(dpl: u8) -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: DESC_PRESENT | dpl | DESC_CODE_DATA | DESC_WRITABLE,
            granularity: 0, // No L bit for data segments
            base_high: 0,
        }
    }

    /// Create a 32-bit code segment descriptor
    ///
    /// For protected mode (non-long mode) or compatibility mode.
    ///
    /// # Arguments
    ///
    /// * `base` - Segment base address
    /// * `limit` - Segment limit (in pages if G=1, bytes if G=0)
    /// * `dpl` - Descriptor Privilege Level
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::{SegmentDescriptor, DESC_DPL_0};
    /// // Flat segment: base=0, limit=4GB
    /// let code_seg = SegmentDescriptor::code_32bit(0, 0xFFFFF, DESC_DPL_0);
    /// ```
    pub const fn code_32bit(base: u32, limit: u32, dpl: u8) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_middle: ((base >> 16) & 0xFF) as u8,
            access: DESC_PRESENT | dpl | DESC_CODE_DATA | DESC_EXECUTABLE | DESC_READABLE,
            granularity: DESC_GRANULAR | DESC_32BIT | (((limit >> 16) & 0xF) as u8),
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }

    /// Create a 32-bit data segment descriptor
    ///
    /// # Arguments
    ///
    /// * `base` - Segment base address
    /// * `limit` - Segment limit (in pages if G=1, bytes if G=0)
    /// * `dpl` - Descriptor Privilege Level
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::{SegmentDescriptor, DESC_DPL_0};
    /// // Flat segment: base=0, limit=4GB
    /// let data_seg = SegmentDescriptor::data_32bit(0, 0xFFFFF, DESC_DPL_0);
    /// ```
    pub const fn data_32bit(base: u32, limit: u32, dpl: u8) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_middle: ((base >> 16) & 0xFF) as u8,
            access: DESC_PRESENT | dpl | DESC_CODE_DATA | DESC_WRITABLE,
            granularity: DESC_GRANULAR | DESC_32BIT | (((limit >> 16) & 0xF) as u8),
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }

    /// Create a 16-bit code segment descriptor
    ///
    /// For real mode or 16-bit protected mode.
    ///
    /// # Arguments
    ///
    /// * `base` - Segment base address
    /// * `limit` - Segment limit
    /// * `dpl` - Descriptor Privilege Level
    pub const fn code_16bit(base: u32, limit: u32, dpl: u8) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_middle: ((base >> 16) & 0xFF) as u8,
            access: DESC_PRESENT | dpl | DESC_CODE_DATA | DESC_EXECUTABLE | DESC_READABLE,
            granularity: (((limit >> 16) & 0xF) as u8), // No G or D bit
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }

    /// Convert descriptor to raw bytes
    pub fn to_bytes(&self) -> [u8; 8] {
        // SAFETY: `SegmentDescriptor` is `#[repr(C, packed)]` with exactly 8 bytes
        // of integer fields, making it layout-compatible with `[u8; 8]`.
        unsafe { std::mem::transmute(*self) }
    }

    /// Create descriptor from raw bytes
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        // SAFETY: `[u8; 8]` is layout-compatible with `SegmentDescriptor`
        // (`#[repr(C, packed)]`, 8 bytes total). Any bit pattern is valid.
        unsafe { std::mem::transmute(bytes) }
    }
}

impl Default for SegmentDescriptor {
    fn default() -> Self {
        Self::null()
    }
}

/// GDTR/IDTR Pointer (10 bytes: 2-byte limit + 8-byte base)
///
/// This structure is used with LGDT/LIDT instructions to load
/// the base address and limit of the GDT or IDT.
///
/// # Layout
///
/// ```text
/// Bytes 0-1: Limit (size of table in bytes - 1)
/// Bytes 2-9: Base (64-bit linear address of table)
/// ```
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DescriptorTablePointer {
    /// Size of descriptor table in bytes minus 1
    pub limit: u16,

    /// Linear address of descriptor table
    pub base: u64,
}

impl DescriptorTablePointer {
    /// Create a new descriptor table pointer
    ///
    /// # Arguments
    ///
    /// * `base` - Linear address of the descriptor table
    /// * `limit` - Size of table in bytes minus 1
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::DescriptorTablePointer;
    /// // GDT with 3 entries (24 bytes)
    /// let gdtr = DescriptorTablePointer::new(0x1000, 23);
    /// ```
    pub const fn new(base: u64, limit: u16) -> Self {
        Self { limit, base }
    }

    /// Convert to raw bytes for loading with LGDT/LIDT
    pub fn to_bytes(&self) -> [u8; 10] {
        // SAFETY: `DescriptorTablePointer` is `#[repr(C, packed)]` with a u16
        // and a u64, totaling exactly 10 bytes. Layout is compatible with `[u8; 10]`.
        unsafe { std::mem::transmute(*self) }
    }
}

// ============================================================================
// GDT Builder
// ============================================================================

/// Builder for constructing Global Descriptor Tables
///
/// Provides a convenient API for creating GDTs with common descriptor types.
///
/// # Example
///
/// ```
/// # use hv2_core::descriptors::{GdtBuilder, DESC_DPL_0, DESC_DPL_3};
/// let gdt = GdtBuilder::new()
///     .add_null()                          // Selector 0x00
///     .add_code_64bit(DESC_DPL_0)          // Selector 0x08 (kernel code)
///     .add_data_64bit(DESC_DPL_0)          // Selector 0x10 (kernel data)
///     .add_code_64bit(DESC_DPL_3)          // Selector 0x18 (user code)
///     .add_data_64bit(DESC_DPL_3)          // Selector 0x20 (user data)
///     .build();
///
/// assert_eq!(gdt.len(), 40); // 5 entries * 8 bytes
/// ```
pub struct GdtBuilder {
    entries: Vec<SegmentDescriptor>,
}

impl GdtBuilder {
    /// Create a new empty GDT builder
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a null descriptor (should be first entry)
    pub fn add_null(mut self) -> Self {
        self.entries.push(SegmentDescriptor::null());
        self
    }

    /// Add a 64-bit code segment
    ///
    /// # Arguments
    ///
    /// * `dpl` - Privilege level (DESC_DPL_0 or DESC_DPL_3)
    pub fn add_code_64bit(mut self, dpl: u8) -> Self {
        self.entries.push(SegmentDescriptor::code_64bit(dpl));
        self
    }

    /// Add a 64-bit data segment
    ///
    /// # Arguments
    ///
    /// * `dpl` - Privilege level (DESC_DPL_0 or DESC_DPL_3)
    pub fn add_data_64bit(mut self, dpl: u8) -> Self {
        self.entries.push(SegmentDescriptor::data_64bit(dpl));
        self
    }

    /// Add a 32-bit code segment
    pub fn add_code_32bit(mut self, base: u32, limit: u32, dpl: u8) -> Self {
        self.entries
            .push(SegmentDescriptor::code_32bit(base, limit, dpl));
        self
    }

    /// Add a 32-bit data segment
    pub fn add_data_32bit(mut self, base: u32, limit: u32, dpl: u8) -> Self {
        self.entries
            .push(SegmentDescriptor::data_32bit(base, limit, dpl));
        self
    }

    /// Add a 16-bit code segment
    pub fn add_code_16bit(mut self, base: u32, limit: u32, dpl: u8) -> Self {
        self.entries
            .push(SegmentDescriptor::code_16bit(base, limit, dpl));
        self
    }

    /// Add a custom descriptor
    pub fn add_descriptor(mut self, desc: SegmentDescriptor) -> Self {
        self.entries.push(desc);
        self
    }

    /// Get the selector value for an entry index
    ///
    /// # Arguments
    ///
    /// * `index` - Entry index (0-based)
    ///
    /// # Returns
    ///
    /// Segment selector value (index * 8)
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::{GdtBuilder, DESC_DPL_0};
    /// let builder = GdtBuilder::new()
    ///     .add_null()             // Index 0 -> Selector 0x00
    ///     .add_code_64bit(DESC_DPL_0); // Index 1 -> Selector 0x08
    ///
    /// assert_eq!(builder.selector(1), 0x08);
    /// ```
    pub fn selector(&self, index: usize) -> u16 {
        (index * 8) as u16
    }

    /// Build the GDT as a byte vector
    ///
    /// Returns a `Vec<u8>` containing all descriptors in sequential order.
    pub fn build(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.entries.len() * 8);
        for entry in &self.entries {
            bytes.extend_from_slice(&entry.to_bytes());
        }
        bytes
    }

    /// Build a descriptor table pointer for this GDT
    ///
    /// # Arguments
    ///
    /// * `base` - Linear address where GDT will be located in guest memory
    ///
    /// # Returns
    ///
    /// DescriptorTablePointer suitable for LGDT instruction
    pub fn build_pointer(&self, base: u64) -> DescriptorTablePointer {
        let limit = (self.entries.len() * 8 - 1) as u16;
        DescriptorTablePointer::new(base, limit)
    }

    /// Get the number of entries in the GDT
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the GDT is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for GdtBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// IDT (Interrupt Descriptor Table) Structures
// ============================================================================

// IDT gate type constants

/// Interrupt Gate (64-bit) - Disables interrupts when invoked
pub const IDT_INTERRUPT_GATE_64: u8 = 0x0E;

/// Trap Gate (64-bit) - Does not disable interrupts when invoked
pub const IDT_TRAP_GATE_64: u8 = 0x0F;

/// Interrupt Gate (32-bit) - For compatibility mode
pub const IDT_INTERRUPT_GATE_32: u8 = 0x0E;

/// Trap Gate (32-bit) - For compatibility mode
pub const IDT_TRAP_GATE_32: u8 = 0x0F;

/// Task Gate (32-bit only) - Switches to different task
pub const IDT_TASK_GATE: u8 = 0x05;

/// Interrupt Descriptor (64-bit mode) - 16 bytes
///
/// Describes an interrupt or exception handler in 64-bit mode.
/// Used in the IDT to route interrupts to handler functions.
///
/// # Layout (128 bits total)
///
/// ```text
/// Bits 0-15:    Offset Low (bits 0-15 of handler address)
/// Bits 16-31:   Segment Selector (code segment)
/// Bits 32-34:   IST (Interrupt Stack Table index, 0 = don't switch)
/// Bits 35-39:   Reserved (must be zero)
/// Bits 40-43:   Gate Type (interrupt/trap gate)
/// Bit 44:       Zero
/// Bits 45-46:   DPL (Descriptor Privilege Level)
/// Bit 47:       Present
/// Bits 48-63:   Offset Middle (bits 16-31 of handler address)
/// Bits 64-95:   Offset High (bits 32-63 of handler address)
/// Bits 96-127:  Reserved (must be zero)
/// ```
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptDescriptor64 {
    /// Handler offset bits 0-15
    pub offset_low: u16,

    /// Code segment selector
    pub selector: u16,

    /// Interrupt Stack Table (IST) index (0-7, 0 = don't use IST)
    pub ist: u8,

    /// Type and attributes (gate type, DPL, present)
    pub attributes: u8,

    /// Handler offset bits 16-31
    pub offset_middle: u16,

    /// Handler offset bits 32-63
    pub offset_high: u32,

    /// Reserved (must be zero)
    pub reserved: u32,
}

impl InterruptDescriptor64 {
    /// Create a null interrupt descriptor
    pub const fn null() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// Create an interrupt gate descriptor
    ///
    /// Interrupt gates disable interrupts (clear IF flag) when invoked.
    ///
    /// # Arguments
    ///
    /// * `handler` - 64-bit address of interrupt handler function
    /// * `selector` - Code segment selector (typically 0x08 for kernel code)
    /// * `ist` - Interrupt Stack Table index (0 = use current stack)
    /// * `dpl` - Descriptor Privilege Level (0 = kernel, 3 = user)
    ///
    /// # Example
    ///
    /// ```
    /// # use hv2_core::descriptors::{InterruptDescriptor64, DESC_DPL_0};
    /// let gate = InterruptDescriptor64::interrupt_gate(
    ///     0xFFFF_8000_0010_0000, // Handler address
    ///     0x08,                  // Kernel code selector
    ///     0,                     // No IST switching
    ///     DESC_DPL_0             // Kernel privilege
    /// );
    /// ```
    pub const fn interrupt_gate(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: (handler & 0xFFFF) as u16,
            selector,
            ist: ist & 0x7, // Only 3 bits for IST
            attributes: DESC_PRESENT | dpl | IDT_INTERRUPT_GATE_64,
            offset_middle: ((handler >> 16) & 0xFFFF) as u16,
            offset_high: ((handler >> 32) & 0xFFFFFFFF) as u32,
            reserved: 0,
        }
    }

    /// Create a trap gate descriptor
    ///
    /// Trap gates do NOT disable interrupts when invoked (IF flag unchanged).
    ///
    /// # Arguments
    ///
    /// * `handler` - 64-bit address of trap handler function
    /// * `selector` - Code segment selector
    /// * `ist` - Interrupt Stack Table index
    /// * `dpl` - Descriptor Privilege Level
    pub const fn trap_gate(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: (handler & 0xFFFF) as u16,
            selector,
            ist: ist & 0x7,
            attributes: DESC_PRESENT | dpl | IDT_TRAP_GATE_64,
            offset_middle: ((handler >> 16) & 0xFFFF) as u16,
            offset_high: ((handler >> 32) & 0xFFFFFFFF) as u32,
            reserved: 0,
        }
    }

    /// Convert descriptor to raw bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        // SAFETY: `InterruptDescriptor64` is `#[repr(C, packed)]` with exactly
        // 16 bytes of integer fields, making it layout-compatible with `[u8; 16]`.
        unsafe { std::mem::transmute(*self) }
    }

    /// Create descriptor from raw bytes
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        // SAFETY: `[u8; 16]` is layout-compatible with `InterruptDescriptor64`
        // (`#[repr(C, packed)]`, 16 bytes total). Any bit pattern is valid.
        unsafe { std::mem::transmute(bytes) }
    }
}

impl Default for InterruptDescriptor64 {
    fn default() -> Self {
        Self::null()
    }
}

/// Interrupt Descriptor (32-bit mode) - 8 bytes
///
/// Describes an interrupt or exception handler in 32-bit protected mode.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptDescriptor32 {
    /// Handler offset bits 0-15
    pub offset_low: u16,

    /// Code segment selector
    pub selector: u16,

    /// Reserved (must be zero)
    pub reserved: u8,

    /// Type and attributes
    pub attributes: u8,

    /// Handler offset bits 16-31
    pub offset_high: u16,
}

impl InterruptDescriptor32 {
    /// Create a null interrupt descriptor
    pub const fn null() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            reserved: 0,
            attributes: 0,
            offset_high: 0,
        }
    }

    /// Create an interrupt gate descriptor for 32-bit mode
    pub const fn interrupt_gate(handler: u32, selector: u16, dpl: u8) -> Self {
        Self {
            offset_low: (handler & 0xFFFF) as u16,
            selector,
            reserved: 0,
            attributes: DESC_PRESENT | dpl | IDT_INTERRUPT_GATE_32,
            offset_high: ((handler >> 16) & 0xFFFF) as u16,
        }
    }

    /// Create a trap gate descriptor for 32-bit mode
    pub const fn trap_gate(handler: u32, selector: u16, dpl: u8) -> Self {
        Self {
            offset_low: (handler & 0xFFFF) as u16,
            selector,
            reserved: 0,
            attributes: DESC_PRESENT | dpl | IDT_TRAP_GATE_32,
            offset_high: ((handler >> 16) & 0xFFFF) as u16,
        }
    }
}

impl Default for InterruptDescriptor32 {
    fn default() -> Self {
        Self::null()
    }
}

// ============================================================================
// IDT Builder
// ============================================================================

/// Builder for constructing Interrupt Descriptor Tables
///
/// Provides a convenient API for creating IDTs with common interrupt handlers.
///
/// # Example
///
/// ```
/// # use hv2_core::descriptors::{IdtBuilder, DESC_DPL_0, DESC_DPL_3};
/// let idt = IdtBuilder::new_64bit()
///     .add_interrupt_gate(0, 0xFFFF_8000_0010_0000, 0x08, 0, DESC_DPL_0) // Divide by zero
///     .add_interrupt_gate(1, 0xFFFF_8000_0010_0100, 0x08, 0, DESC_DPL_0) // Debug
///     .add_interrupt_gate(3, 0xFFFF_8000_0010_0200, 0x08, 0, DESC_DPL_3) // Breakpoint
///     .add_interrupt_gate(14, 0xFFFF_8000_0010_0E00, 0x08, 0, DESC_DPL_0) // Page fault
///     .build();
/// ```
pub struct IdtBuilder {
    entries_64: Vec<InterruptDescriptor64>,
    entries_32: Vec<InterruptDescriptor32>,
    mode_64bit: bool,
}

impl IdtBuilder {
    /// Create a new IDT builder for 64-bit mode
    ///
    /// In 64-bit mode, each IDT entry is 16 bytes.
    pub fn new_64bit() -> Self {
        Self {
            entries_64: vec![InterruptDescriptor64::null(); 256],
            entries_32: Vec::new(),
            mode_64bit: true,
        }
    }

    /// Create a new IDT builder for 32-bit mode
    ///
    /// In 32-bit mode, each IDT entry is 8 bytes.
    pub fn new_32bit() -> Self {
        Self {
            entries_64: Vec::new(),
            entries_32: vec![InterruptDescriptor32::null(); 256],
            mode_64bit: false,
        }
    }

    /// Add an interrupt gate (64-bit mode)
    ///
    /// # Arguments
    ///
    /// * `vector` - Interrupt vector number (0-255)
    /// * `handler` - Handler function address
    /// * `selector` - Code segment selector
    /// * `ist` - Interrupt Stack Table index (0 = no switch)
    /// * `dpl` - Descriptor Privilege Level
    pub fn add_interrupt_gate(
        mut self,
        vector: u8,
        handler: u64,
        selector: u16,
        ist: u8,
        dpl: u8,
    ) -> Self {
        if self.mode_64bit {
            self.entries_64[vector as usize] =
                InterruptDescriptor64::interrupt_gate(handler, selector, ist, dpl);
        } else {
            self.entries_32[vector as usize] =
                InterruptDescriptor32::interrupt_gate(handler as u32, selector, dpl);
        }
        self
    }

    /// Add a trap gate (64-bit mode)
    pub fn add_trap_gate(
        mut self,
        vector: u8,
        handler: u64,
        selector: u16,
        ist: u8,
        dpl: u8,
    ) -> Self {
        if self.mode_64bit {
            self.entries_64[vector as usize] =
                InterruptDescriptor64::trap_gate(handler, selector, ist, dpl);
        } else {
            self.entries_32[vector as usize] =
                InterruptDescriptor32::trap_gate(handler as u32, selector, dpl);
        }
        self
    }

    /// Build the IDT as a byte vector
    pub fn build(&self) -> Vec<u8> {
        if self.mode_64bit {
            let mut bytes = Vec::with_capacity(self.entries_64.len() * 16);
            for entry in &self.entries_64 {
                bytes.extend_from_slice(&entry.to_bytes());
            }
            bytes
        } else {
            let mut bytes = Vec::with_capacity(self.entries_32.len() * 8);
            for entry in &self.entries_32 {
                // SAFETY: InterruptDescriptor32 is repr(C, packed) with exactly 8
                // bytes, making it layout-compatible with [u8; 8]. Read-only reinterpretation.
                let entry_bytes: &[u8; 8] = unsafe { std::mem::transmute(entry) };
                bytes.extend_from_slice(entry_bytes);
            }
            bytes
        }
    }

    /// Build a descriptor table pointer for this IDT
    ///
    /// # Arguments
    ///
    /// * `base` - Linear address where IDT will be located in guest memory
    pub fn build_pointer(&self, base: u64) -> DescriptorTablePointer {
        let limit = if self.mode_64bit {
            (self.entries_64.len() * 16 - 1) as u16
        } else {
            (self.entries_32.len() * 8 - 1) as u16
        };
        DescriptorTablePointer::new(base, limit)
    }

    /// Get the number of entries in the IDT
    pub fn len(&self) -> usize {
        if self.mode_64bit {
            self.entries_64.len()
        } else {
            self.entries_32.len()
        }
    }

    /// Check if the IDT is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_descriptor() {
        let desc = SegmentDescriptor::null();
        let bytes = desc.to_bytes();
        assert_eq!(bytes, [0; 8]);
    }

    #[test]
    fn test_code_64bit_descriptor() {
        let desc = SegmentDescriptor::code_64bit(DESC_DPL_0);
        assert_eq!(desc.access & DESC_PRESENT, DESC_PRESENT);
        assert_eq!(desc.access & DESC_EXECUTABLE, DESC_EXECUTABLE);
        assert_eq!(desc.granularity & DESC_64BIT, DESC_64BIT);
    }

    #[test]
    fn test_data_64bit_descriptor() {
        let desc = SegmentDescriptor::data_64bit(DESC_DPL_0);
        assert_eq!(desc.access & DESC_PRESENT, DESC_PRESENT);
        assert_eq!(desc.access & DESC_EXECUTABLE, 0);
    }

    #[test]
    fn test_gdt_builder_basic() {
        let gdt = GdtBuilder::new()
            .add_null()
            .add_code_64bit(DESC_DPL_0)
            .add_data_64bit(DESC_DPL_0)
            .build();

        assert_eq!(gdt.len(), 24); // 3 entries * 8 bytes
    }

    #[test]
    fn test_gdt_builder_selectors() {
        let builder = GdtBuilder::new()
            .add_null()
            .add_code_64bit(DESC_DPL_0)
            .add_data_64bit(DESC_DPL_0);

        assert_eq!(builder.selector(0), 0x00);
        assert_eq!(builder.selector(1), 0x08);
        assert_eq!(builder.selector(2), 0x10);
    }

    #[test]
    fn test_descriptor_table_pointer() {
        let ptr = DescriptorTablePointer::new(0x1000, 23);
        // Copy fields to avoid packed struct reference issues
        let base = ptr.base;
        let limit = ptr.limit;
        assert_eq!(base, 0x1000);
        assert_eq!(limit, 23);

        let bytes = ptr.to_bytes();
        assert_eq!(bytes.len(), 10);
    }

    #[test]
    fn test_gdt_builder_pointer() {
        let builder = GdtBuilder::new()
            .add_null()
            .add_code_64bit(DESC_DPL_0)
            .add_data_64bit(DESC_DPL_0);

        let ptr = builder.build_pointer(0x1000);
        let base = ptr.base;
        let limit = ptr.limit;
        assert_eq!(base, 0x1000);
        assert_eq!(limit, 23); // 3 entries * 8 - 1
    }

    #[test]
    fn test_32bit_segments() {
        let code = SegmentDescriptor::code_32bit(0, 0xFFFFF, DESC_DPL_0);
        let data = SegmentDescriptor::data_32bit(0, 0xFFFFF, DESC_DPL_0);

        let code_limit = code.limit_low;
        let data_limit = data.limit_low;
        assert_eq!(code_limit, 0xFFFF);
        assert_eq!(data_limit, 0xFFFF);
        assert_eq!(code.granularity & DESC_32BIT, DESC_32BIT);
        assert_eq!(data.granularity & DESC_32BIT, DESC_32BIT);
    }

    #[test]
    fn test_descriptor_privilege_levels() {
        let kernel_code = SegmentDescriptor::code_64bit(DESC_DPL_0);
        let user_code = SegmentDescriptor::code_64bit(DESC_DPL_3);

        assert_eq!(kernel_code.access & (3 << 5), 0);
        assert_eq!(user_code.access & (3 << 5), DESC_DPL_3);
    }

    #[test]
    fn test_interrupt_descriptor_64() {
        let handler_addr = 0xFFFF_8000_0010_0000u64;
        let desc = InterruptDescriptor64::interrupt_gate(handler_addr, 0x08, 0, DESC_DPL_0);

        // Verify handler address reconstruction
        let offset = (desc.offset_low as u64)
            | ((desc.offset_middle as u64) << 16)
            | ((desc.offset_high as u64) << 32);
        assert_eq!(offset, handler_addr);

        // Verify attributes
        assert_eq!(desc.attributes & DESC_PRESENT, DESC_PRESENT);
        assert_eq!(desc.attributes & 0x0F, IDT_INTERRUPT_GATE_64);
    }

    #[test]
    fn test_interrupt_descriptor_32() {
        let handler_addr = 0x0010_0000u32;
        let desc = InterruptDescriptor32::interrupt_gate(handler_addr, 0x08, DESC_DPL_0);

        let offset = (desc.offset_low as u32) | ((desc.offset_high as u32) << 16);
        assert_eq!(offset, handler_addr);
    }

    #[test]
    fn test_trap_gate_64() {
        let desc = InterruptDescriptor64::trap_gate(0x1000, 0x08, 0, DESC_DPL_0);
        assert_eq!(desc.attributes & 0x0F, IDT_TRAP_GATE_64);
    }

    #[test]
    fn test_idt_builder_64bit() {
        let idt = IdtBuilder::new_64bit()
            .add_interrupt_gate(0, 0x1000, 0x08, 0, DESC_DPL_0) // Divide by zero
            .add_interrupt_gate(14, 0x2000, 0x08, 0, DESC_DPL_0) // Page fault
            .build();

        // 256 entries * 16 bytes each
        assert_eq!(idt.len(), 256 * 16);
    }

    #[test]
    fn test_idt_builder_32bit() {
        let idt = IdtBuilder::new_32bit()
            .add_interrupt_gate(0, 0x1000, 0x08, 0, DESC_DPL_0)
            .build();

        // 256 entries * 8 bytes each
        assert_eq!(idt.len(), 256 * 8);
    }

    #[test]
    fn test_idt_pointer() {
        let builder = IdtBuilder::new_64bit();
        let ptr = builder.build_pointer(0x3000);

        let base = ptr.base;
        let limit = ptr.limit;
        assert_eq!(base, 0x3000);
        assert_eq!(limit, 256 * 16 - 1); // 256 entries * 16 bytes - 1
    }

    #[test]
    fn test_ist_field() {
        let desc = InterruptDescriptor64::interrupt_gate(0x1000, 0x08, 5, DESC_DPL_0);
        assert_eq!(desc.ist, 5);

        // Verify IST is limited to 3 bits (0-7)
        let desc_overflow = InterruptDescriptor64::interrupt_gate(0x1000, 0x08, 15, DESC_DPL_0);
        assert_eq!(desc_overflow.ist, 7); // 15 & 0x7 = 7
    }
}
