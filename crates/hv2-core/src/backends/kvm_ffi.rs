//! KVM FFI bindings
//!
//! This module provides Foreign Function Interface (FFI) bindings to the
//! Linux Kernel-based Virtual Machine (KVM) API.
//!
//! # KVM API Overview
//!
//! KVM exposes its API through ioctl() calls on file descriptors:
//! - `/dev/kvm` - Main KVM device (system-level operations)
//! - VM file descriptor - Per-VM operations (create vCPUs, memory mapping)
//! - vCPU file descriptor - Per-vCPU operations (run, register access)
//!
//! # Safety
//!
//! All functions in this module are `unsafe` because they:
//! - Make raw system calls (ioctl)
//! - Work with raw file descriptors
//! - Use raw pointers for data structures
//! - Have platform-specific behavior
//!
//! Callers must ensure:
//! - File descriptors are valid
//! - Pointers point to valid, initialized memory
//! - Data structures match kernel expectations
//! - Proper synchronization for concurrent access

#![allow(non_camel_case_types)]
// FFI bindings — not all constants/types are used yet
#![allow(dead_code)]

use std::os::unix::io::RawFd;

// KVM API version
pub const KVM_API_VERSION: u32 = 12;

// KVM ioctl numbers (generated from _IOWR/_IOW/_IOR macros)
// Format: ioctl(fd, request, arg)
// where request is built from: direction | size | type | number

// System ioctls (on /dev/kvm fd)
pub const KVM_GET_API_VERSION: u64 = 0xae00; // _IO(KVMIO, 0x00)
pub const KVM_CREATE_VM: u64 = 0xae01; // _IO(KVMIO, 0x01)
pub const KVM_GET_MSR_INDEX_LIST: u64 = 0xc004ae02; // _IOWR(KVMIO, 0x02, struct kvm_msr_list)
pub const KVM_CHECK_EXTENSION: u64 = 0xae03; // _IO(KVMIO, 0x03)
pub const KVM_GET_VCPU_MMAP_SIZE: u64 = 0xae04; // _IO(KVMIO, 0x04)
pub const KVM_GET_SUPPORTED_CPUID: u64 = 0xc008ae05; // _IOWR(KVMIO, 0x05, struct kvm_cpuid2)

// VM ioctls (on VM fd)
pub const KVM_CREATE_VCPU: u64 = 0xae41; // _IO(KVMIO, 0x41)
pub const KVM_GET_DIRTY_LOG: u64 = 0x4010ae42; // _IOW(KVMIO, 0x42, struct kvm_dirty_log)
pub const KVM_SET_USER_MEMORY_REGION: u64 = 0x4020ae46; // _IOW(KVMIO, 0x46, struct kvm_userspace_memory_region)
pub const KVM_SET_TSS_ADDR: u64 = 0xae47; // _IO(KVMIO, 0x47)
pub const KVM_SET_IDENTITY_MAP_ADDR: u64 = 0x4008ae48; // _IOW(KVMIO, 0x48, u64)
pub const KVM_CREATE_IRQCHIP: u64 = 0xae60; // _IO(KVMIO, 0x60)
pub const KVM_IRQ_LINE: u64 = 0x4008ae61; // _IOW(KVMIO, 0x61, struct kvm_irq_level)
pub const KVM_GET_IRQCHIP: u64 = 0xc208ae62; // _IOWR(KVMIO, 0x62, struct kvm_irqchip)
pub const KVM_SET_IRQCHIP: u64 = 0x8208ae63; // _IOR(KVMIO, 0x63, struct kvm_irqchip)
pub const KVM_SET_GSI_ROUTING: u64 = 0x4008ae6a; // _IOW(KVMIO, 0x6a, struct kvm_irq_routing)
pub const KVM_IRQFD: u64 = 0x4020ae76; // _IOW(KVMIO, 0x76, struct kvm_irqfd)
pub const KVM_CREATE_PIT2: u64 = 0x4040ae77; // _IOW(KVMIO, 0x77, struct kvm_pit_config)
pub const KVM_IOEVENTFD: u64 = 0x4040ae79; // _IOW(KVMIO, 0x79, struct kvm_ioeventfd)
pub const KVM_SIGNAL_MSI: u64 = 0x4020aea5; // _IOW(KVMIO, 0xa5, struct kvm_msi)

// vCPU ioctls (on vCPU fd)
pub const KVM_RUN: u64 = 0xae80; // _IO(KVMIO, 0x80)
pub const KVM_GET_REGS: u64 = 0x8090ae81; // _IOR(KVMIO, 0x81, struct kvm_regs)
pub const KVM_SET_REGS: u64 = 0x4090ae82; // _IOW(KVMIO, 0x82, struct kvm_regs)
pub const KVM_GET_SREGS: u64 = 0x8138ae83; // _IOR(KVMIO, 0x83, struct kvm_sregs)
pub const KVM_SET_SREGS: u64 = 0x4138ae84; // _IOW(KVMIO, 0x84, struct kvm_sregs)
pub const KVM_TRANSLATE: u64 = 0xc018ae85; // _IOWR(KVMIO, 0x85, struct kvm_translation)
pub const KVM_INTERRUPT: u64 = 0x4004ae86; // _IOW(KVMIO, 0x86, struct kvm_interrupt)
pub const KVM_GET_MSRS: u64 = 0xc008ae88; // _IOWR(KVMIO, 0x88, struct kvm_msrs)
pub const KVM_SET_MSRS: u64 = 0x4008ae89; // _IOW(KVMIO, 0x89, struct kvm_msrs)
pub const KVM_SET_SIGNAL_MASK: u64 = 0x4004ae8b; // _IOW(KVMIO, 0x8b, struct kvm_signal_mask)
pub const KVM_GET_FPU: u64 = 0x81a0ae8c; // _IOR(KVMIO, 0x8c, struct kvm_fpu)
pub const KVM_SET_FPU: u64 = 0x41a0ae8d; // _IOW(KVMIO, 0x8d, struct kvm_fpu)
pub const KVM_GET_LAPIC: u64 = 0x8400ae8e; // _IOR(KVMIO, 0x8e, struct kvm_lapic_state)
pub const KVM_SET_LAPIC: u64 = 0x4400ae8f; // _IOW(KVMIO, 0x8f, struct kvm_lapic_state)
pub const KVM_SET_CPUID2: u64 = 0x4008ae90; // _IOW(KVMIO, 0x90, struct kvm_cpuid2)
pub const KVM_GET_MP_STATE: u64 = 0x8004ae98; // _IOR(KVMIO, 0x98, struct kvm_mp_state)
pub const KVM_SET_MP_STATE: u64 = 0x4004ae99; // _IOW(KVMIO, 0x99, struct kvm_mp_state)
pub const KVM_NMI: u64 = 0xae9a; // _IO(KVMIO, 0x9a)
pub const KVM_SET_GUEST_DEBUG: u64 = 0x4048ae9b; // _IOW(KVMIO, 0x9b, struct kvm_guest_debug)
pub const KVM_GET_VCPU_EVENTS: u64 = 0x8040ae9f; // _IOR(KVMIO, 0x9f, struct kvm_vcpu_events)
pub const KVM_SET_VCPU_EVENTS: u64 = 0x4040aea0; // _IOW(KVMIO, 0xa0, struct kvm_vcpu_events)
pub const KVM_GET_DEBUGREGS: u64 = 0x8080aea1; // _IOR(KVMIO, 0xa1, struct kvm_debugregs)
pub const KVM_SET_DEBUGREGS: u64 = 0x4080aea2; // _IOW(KVMIO, 0xa2, struct kvm_debugregs)
pub const KVM_GET_XSAVE: u64 = 0x9000aea4; // _IOR(KVMIO, 0xa4, struct kvm_xsave)
pub const KVM_SET_XSAVE: u64 = 0x5000aea3; // _IOW(KVMIO, 0xa3, struct kvm_xsave)
pub const KVM_GET_XCRS: u64 = 0x8188aea6; // _IOR(KVMIO, 0xa6, struct kvm_xcrs)
pub const KVM_SET_XCRS: u64 = 0x4188aea7; // _IOW(KVMIO, 0xa7, struct kvm_xcrs)

// KVM capability flags
pub const KVM_CAP_IRQCHIP: u32 = 0;
pub const KVM_CAP_HLT: u32 = 1;
pub const KVM_CAP_USER_MEMORY: u32 = 3;
pub const KVM_CAP_SET_TSS_ADDR: u32 = 4;
pub const KVM_CAP_EXT_CPUID: u32 = 7;
pub const KVM_CAP_NR_VCPUS: u32 = 9;
pub const KVM_CAP_MP_STATE: u32 = 14;
pub const KVM_CAP_SYNC_MMU: u32 = 16;
pub const KVM_CAP_IRQFD: u32 = 32;
pub const KVM_CAP_PIT2: u32 = 33;
pub const KVM_CAP_IOEVENTFD: u32 = 36;
pub const KVM_CAP_VCPU_EVENTS: u32 = 41;
pub const KVM_CAP_DEBUGREGS: u32 = 50;
pub const KVM_CAP_XSAVE: u32 = 55;
pub const KVM_CAP_XCRS: u32 = 56;
pub const KVM_CAP_TSC_CONTROL: u32 = 60;
pub const KVM_CAP_MAX_VCPUS: u32 = 66;
pub const KVM_CAP_READONLY_MEM: u32 = 81;
pub const KVM_CAP_SPLIT_IRQCHIP: u32 = 121;
pub const KVM_CAP_MAX_VCPU_ID: u32 = 128;
pub const KVM_CAP_X2APIC_API: u32 = 129;
pub const KVM_CAP_MSI_DEVID: u32 = 131;
pub const KVM_CAP_IMMEDIATE_EXIT: u32 = 136;
pub const KVM_CAP_NESTED_STATE: u32 = 157;

// KVM exit reasons (from kvm_run.exit_reason)
pub const KVM_EXIT_UNKNOWN: u32 = 0;
pub const KVM_EXIT_EXCEPTION: u32 = 1;
pub const KVM_EXIT_IO: u32 = 2;
pub const KVM_EXIT_HYPERCALL: u32 = 3;
pub const KVM_EXIT_DEBUG: u32 = 4;
pub const KVM_EXIT_HLT: u32 = 5;
pub const KVM_EXIT_MMIO: u32 = 6;
pub const KVM_EXIT_IRQ_WINDOW_OPEN: u32 = 7;
pub const KVM_EXIT_SHUTDOWN: u32 = 8;
pub const KVM_EXIT_FAIL_ENTRY: u32 = 9;
pub const KVM_EXIT_INTR: u32 = 10;
pub const KVM_EXIT_SET_TPR: u32 = 11;
pub const KVM_EXIT_TPR_ACCESS: u32 = 12;
pub const KVM_EXIT_NMI: u32 = 16;
pub const KVM_EXIT_INTERNAL_ERROR: u32 = 17;
pub const KVM_EXIT_SYSTEM_EVENT: u32 = 24;
pub const KVM_EXIT_IOAPIC_EOI: u32 = 26;
pub const KVM_EXIT_HYPERV: u32 = 27;
pub const KVM_EXIT_X86_RDMSR: u32 = 29;
pub const KVM_EXIT_X86_WRMSR: u32 = 30;
pub const KVM_EXIT_DIRTY_RING_FULL: u32 = 31;
pub const KVM_EXIT_X86_BUS_LOCK: u32 = 33;
pub const KVM_EXIT_NOTIFY: u32 = 37;
pub const KVM_EXIT_MEMORY_FAULT: u32 = 39;

// KVM MP (multiprocessor) state values
pub const KVM_MP_STATE_RUNNABLE: u32 = 0;
pub const KVM_MP_STATE_UNINITIALIZED: u32 = 1;
pub const KVM_MP_STATE_INIT_RECEIVED: u32 = 2;
pub const KVM_MP_STATE_HALTED: u32 = 3;
pub const KVM_MP_STATE_SIPI_RECEIVED: u32 = 4;
pub const KVM_MP_STATE_STOPPED: u32 = 5;
pub const KVM_MP_STATE_AP_RESET_HOLD: u32 = 9;
pub const KVM_MP_STATE_SUSPENDED: u32 = 10;

// KVM IRQ routing types
pub const KVM_IRQ_ROUTING_IRQCHIP: u32 = 1;
pub const KVM_IRQ_ROUTING_MSI: u32 = 2;
pub const KVM_IRQ_ROUTING_HV_SINT: u32 = 4;

// KVM IRQ chip IDs
pub const KVM_IRQCHIP_PIC_MASTER: u32 = 0;
pub const KVM_IRQCHIP_PIC_SLAVE: u32 = 1;
pub const KVM_IRQCHIP_IOAPIC: u32 = 2;

// KVM guest debug flags
pub const KVM_GUESTDBG_ENABLE: u32 = 0x0000_0001;
pub const KVM_GUESTDBG_SINGLESTEP: u32 = 0x0000_0002;
pub const KVM_GUESTDBG_USE_SW_BP: u32 = 0x0001_0000;
pub const KVM_GUESTDBG_USE_HW_BP: u32 = 0x0002_0000;
pub const KVM_GUESTDBG_INJECT_DB: u32 = 0x0004_0000;
pub const KVM_GUESTDBG_INJECT_BP: u32 = 0x0008_0000;

// KVM MSI flags
pub const KVM_MSI_VALID_DEVID: u32 = 1;

// I/O direction
pub const KVM_EXIT_IO_IN: u8 = 0;
pub const KVM_EXIT_IO_OUT: u8 = 1;

/// Memory region for KVM_SET_USER_MEMORY_REGION
///
/// This structure describes a region of guest physical memory that
/// is backed by host virtual memory.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_userspace_memory_region {
    /// Slot number (0-based, multiple regions can exist)
    pub slot: u32,
    /// Flags (currently unused, must be 0)
    pub flags: u32,
    /// Guest physical address where the region starts
    pub guest_phys_addr: u64,
    /// Size of the memory region in bytes
    pub memory_size: u64,
    /// Host virtual address backing the guest memory
    pub userspace_addr: u64,
}

/// KVM run structure
///
/// This structure is shared between KVM and userspace via mmap().
/// It contains:
/// - Control fields (request_interrupt_window, immediate_exit)
/// - Exit information (exit_reason, ready_for_interrupt_injection)
/// - Exit-specific data (union based on exit_reason)
///
/// # Memory Layout
///
/// The structure is mapped into the process's address space via:
/// ```text
/// let ptr = mmap(NULL, mmap_size, PROT_READ | PROT_WRITE,
///                MAP_SHARED, vcpu_fd, 0);
/// ```
///
/// KVM writes to this structure when returning from KVM_RUN.
/// Userspace reads exit_reason and the corresponding union field.
#[repr(C)]
pub struct kvm_run {
    // Input fields (set by userspace before KVM_RUN)
    /// Request notification when an interrupt window opens
    pub request_interrupt_window: u8,
    /// Request immediate exit from KVM_RUN (for testing)
    pub immediate_exit: u8,
    pub padding1: [u8; 6],

    // Output fields (set by KVM after exit)
    /// Exit reason code (KVM_EXIT_*)
    pub exit_reason: u32,
    /// 1 if guest is ready to receive interrupts
    pub ready_for_interrupt_injection: u8,
    /// 1 if IF flag is set in guest RFLAGS
    pub if_flag: u8,
    pub flags: u16,

    // Input/Output fields
    /// CR8 (TPR) register value
    pub cr8: u64,
    /// APIC base address
    pub apic_base: u64,

    /// Exit-specific data
    ///
    /// The correct union member to access depends on exit_reason:
    /// - KVM_EXIT_IO → io
    /// - KVM_EXIT_MMIO → mmio
    /// - KVM_EXIT_HLT → (no specific data)
    /// - KVM_EXIT_SHUTDOWN → (no specific data)
    /// - KVM_EXIT_EXCEPTION → ex
    /// - etc.
    pub exit_data: kvm_run_exit,
}

/// Union of exit-specific data structures
///
/// This is a Rust representation of the C union in struct kvm_run.
/// Only one variant is valid at a time, determined by kvm_run.exit_reason.
///
/// # Safety
///
/// Accessing the wrong variant is undefined behavior. Always check
/// exit_reason before reading the union.
#[repr(C)]
pub union kvm_run_exit {
    pub hw: kvm_run_hw,
    pub fail_entry: kvm_run_fail_entry,
    pub ex: kvm_run_exception,
    pub io: kvm_run_io,
    pub mmio: kvm_run_mmio,
    pub hypercall: kvm_run_hypercall,
    pub internal: kvm_run_internal_error,
    pub eoi: kvm_run_eoi,
    pub msr: kvm_run_msr,
    pub system_event: kvm_run_system_event,
    // Padding to ensure the structure is large enough
    _padding: [u8; 256],
}

/// KVM_EXIT_UNKNOWN data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_hw {
    pub hardware_exit_reason: u64,
}

/// KVM_EXIT_FAIL_ENTRY data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_fail_entry {
    pub hardware_entry_failure_reason: u64,
    pub cpu: u32,
}

/// KVM_EXIT_EXCEPTION data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_exception {
    pub exception: u32,
    pub error_code: u32,
}

/// KVM_EXIT_IO data
///
/// For I/O port access (IN/OUT instructions).
/// Data is stored at offset `data_offset` from the start of kvm_run.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_io {
    /// Direction: KVM_EXIT_IO_IN (0) or KVM_EXIT_IO_OUT (1)
    pub direction: u8,
    /// Size in bytes: 1, 2, or 4
    pub size: u8,
    /// Port number (0-65535)
    pub port: u16,
    /// Number of I/O operations (usually 1, >1 for string I/O)
    pub count: u32,
    /// Offset from kvm_run start where data is stored
    pub data_offset: u64,
}

/// KVM_EXIT_MMIO data
///
/// For memory-mapped I/O access.
/// Data is embedded directly in the structure.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_mmio {
    /// Physical address being accessed
    pub phys_addr: u64,
    /// Data buffer (read or write)
    pub data: [u8; 8],
    /// Length of access in bytes (1, 2, 4, or 8)
    pub len: u32,
    /// 1 for write, 0 for read
    pub is_write: u8,
}

/// KVM_EXIT_HYPERCALL data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_hypercall {
    pub nr: u64,
    pub args: [u64; 6],
    pub ret: u64,
    pub flags: u64,
}

/// KVM_EXIT_INTERNAL_ERROR data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_internal_error {
    pub suberror: u32,
    pub ndata: u32,
    pub data: [u64; 16],
}

/// KVM_EXIT_IOAPIC_EOI data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_eoi {
    pub vector: u8,
}

/// KVM_EXIT_X86_RDMSR / KVM_EXIT_X86_WRMSR data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_msr {
    pub error: u8,
    pub pad: [u8; 7],
    pub reason: u32,
    pub index: u32,
    pub data: u64,
}

/// KVM_EXIT_SYSTEM_EVENT data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_run_system_event {
    pub type_: u32,
    pub ndata: u32,
    pub flags: u64,
    pub data: [u64; 16],
}

/// x86_64 general purpose registers
///
/// Used with KVM_GET_REGS and KVM_SET_REGS.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_regs {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rsp: u64,
    pub rbp: u64,
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

/// x86_64 segment register
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_segment {
    pub base: u64,
    pub limit: u32,
    pub selector: u16,
    pub type_: u8,
    pub present: u8,
    pub dpl: u8,
    pub db: u8,
    pub s: u8,
    pub l: u8,
    pub g: u8,
    pub avl: u8,
    pub unusable: u8,
    pub padding: u8,
}

/// x86_64 descriptor table register (GDTR/IDTR)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_dtable {
    pub base: u64,
    pub limit: u16,
    pub padding: [u16; 3],
}

/// x86_64 special registers
///
/// Used with KVM_GET_SREGS and KVM_SET_SREGS.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_sregs {
    pub cs: kvm_segment,
    pub ds: kvm_segment,
    pub es: kvm_segment,
    pub fs: kvm_segment,
    pub gs: kvm_segment,
    pub ss: kvm_segment,
    pub tr: kvm_segment,
    pub ldt: kvm_segment,
    pub gdt: kvm_dtable,
    pub idt: kvm_dtable,
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,
    pub apic_base: u64,
    pub interrupt_bitmap: [u64; 4],
}

/// Interrupt injection
///
/// Used with KVM_INTERRUPT to inject an interrupt.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_interrupt {
    /// Interrupt vector (0-255)
    pub irq: u32,
}

/// CPUID entry
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_cpuid_entry2 {
    pub function: u32,
    pub index: u32,
    pub flags: u32,
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub padding: [u32; 3],
}

/// CPUID data (variable-length)
///
/// Used with KVM_SET_CPUID2.
#[repr(C)]
pub struct kvm_cpuid2 {
    pub nent: u32,
    pub padding: u32,
    // Variable-length array follows
    // pub entries: [kvm_cpuid_entry2; nent],
}

/// PIT configuration
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_pit_config {
    pub flags: u32,
    pub pad: [u32; 15],
}

/// MSR (Model Specific Register) entry
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_msr_entry {
    pub index: u32,
    pub reserved: u32,
    pub data: u64,
}

/// MSR list for get/set operations
///
/// Variable-length structure. The ntries array has 
msrs elements.
#[repr(C)]
pub struct kvm_msrs {
    pub nmsrs: u32,
    pub pad: u32,
    pub entries: [kvm_msr_entry; 0],
}

/// Floating-point state (x87 + SSE)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_fpu {
    pub fpr: [[u8; 16]; 8],
    pub fcw: u16,
    pub fsw: u16,
    pub ftwx: u8,
    pub pad1: u8,
    pub last_opcode: u16,
    pub last_ip: u64,
    pub last_dp: u64,
    pub xmm: [[u8; 16]; 16],
    pub mxcsr: u32,
    pub pad2: u32,
}

/// Multiprocessor state
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_mp_state {
    pub mp_state: u32,
}

/// IRQ level for KVM_IRQ_LINE
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kvm_irq_level {
    pub irq: u32,
    pub level: u32,
}

/// Dirty log for tracking modified guest pages
#[repr(C)]
pub struct kvm_dirty_log {
    pub slot: u32,
    pub padding1: u32,
    pub dirty_bitmap: *mut u8,
}

/// LAPIC (Local APIC) state - 1 KiB register page
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct kvm_lapic_state {
    pub regs: [u8; 1024],
}

/// In-kernel IRQ chip state (PIC or IOAPIC)
#[repr(C)]
pub struct kvm_irqchip {
    pub chip_id: u32,
    pub pad: u32,
    pub chip: [u8; 512],
}

/// MSI (Message Signaled Interrupt) injection
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_msi {
    pub address_lo: u32,
    pub address_hi: u32,
    pub data: u32,
    pub flags: u32,
    pub devid: u32,
    pub pad: [u8; 12],
}

/// Virtual address to physical translation result
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_translation {
    pub linear_address: u64,
    pub physical_address: u64,
    pub valid: u8,
    pub writeable: u8,
    pub usermode: u8,
    pub pad: [u8; 5],
}

/// vCPU events - exception, interrupt, NMI, and SMI state
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_vcpu_events {
    pub exception: kvm_vcpu_events_exception,
    pub interrupt: kvm_vcpu_events_interrupt,
    pub nmi: kvm_vcpu_events_nmi,
    pub sipi_vector: u32,
    pub flags: u32,
    pub smi: kvm_vcpu_events_smi,
    pub reserved: [u8; 27],
    pub exception_has_payload: u8,
    pub exception_payload: u64,
}

/// Exception sub-state of vCPU events
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_vcpu_events_exception {
    pub injected: u8,
    pub nr: u8,
    pub has_error_code: u8,
    pub pending: u8,
    pub error_code: u32,
}

/// Interrupt sub-state of vCPU events
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_vcpu_events_interrupt {
    pub injected: u8,
    pub nr: u8,
    pub soft: u8,
    pub shadow: u8,
}

/// NMI sub-state of vCPU events
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_vcpu_events_nmi {
    pub injected: u8,
    pub pending: u8,
    pub masked: u8,
    pub pad: u8,
}

/// SMI sub-state of vCPU events
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_vcpu_events_smi {
    pub smm: u8,
    pub pending: u8,
    pub smm_inside_nmi: u8,
    pub latched_init: u8,
}

/// Guest debug control
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_guest_debug {
    pub control: u32,
    pub pad: u32,
    pub arch: kvm_guest_debug_arch,
}

/// Architecture-specific guest debug (x86)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_guest_debug_arch {
    pub debugreg: [u64; 8],
}

/// XSAVE state (extended processor state)
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct kvm_xsave {
    pub region: [u32; 1024],
}

/// Extended control register entry
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_xcr {
    pub xcr: u32,
    pub reserved: u32,
    pub value: u64,
}

/// Maximum number of XCRs
pub const KVM_MAX_XCRS: usize = 16;

/// Extended control registers
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_xcrs {
    pub nr_xcrs: u32,
    pub flags: u32,
    pub xcrs: [kvm_xcr; KVM_MAX_XCRS],
    pub padding: [u64; 16],
}

/// Debug registers
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_debugregs {
    pub db: [u64; 4],
    pub dr6: u64,
    pub dr7: u64,
    pub flags: u64,
    pub reserved: [u64; 9],
}

/// IRQ routing - irqchip entry
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_irq_routing_irqchip {
    pub irqchip: u32,
    pub pin: u32,
}

/// IRQ routing - MSI entry
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct kvm_irq_routing_msi {
    pub address_lo: u32,
    pub address_hi: u32,
    pub data: u32,
    pub devid: u32,
}

/// Single IRQ routing entry
#[repr(C)]
pub struct kvm_irq_routing_entry {
    pub gsi: u32,
    pub type_: u32,
    pub flags: u32,
    pub pad: u32,
    pub u: [u8; 16], // union of irqchip/msi/etc
}

/// IRQ routing table (variable-length)
#[repr(C)]
pub struct kvm_irq_routing {
    pub nr: u32,
    pub flags: u32,
    pub entries: [kvm_irq_routing_entry; 0],
}

/// Signal mask for KVM_SET_SIGNAL_MASK
#[repr(C)]
pub struct kvm_signal_mask {
    pub len: u32,
    pub sigset: [u8; 0],
}

// Unsafe wrapper functions for ioctl

/// Issue an ioctl command to a file descriptor
///
/// # Safety
///
/// - `fd` must be a valid open file descriptor
/// - `request` must be a valid ioctl request number
/// - `arg` must point to valid memory of the expected type
unsafe fn ioctl(fd: RawFd, request: u64, arg: usize) -> i32 {
    libc::ioctl(fd, request, arg)
}

/// Open /dev/kvm and return the file descriptor
///
/// # Safety
///
/// Returns a file descriptor that must be closed with close().
pub unsafe fn kvm_open() -> Result<RawFd, std::io::Error> {
    use std::ffi::CString;
    let path = CString::new("/dev/kvm").expect("static path has no NUL bytes");
    let fd = libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

/// Get KVM API version
///
/// # Safety
///
/// `kvm_fd` must be a valid /dev/kvm file descriptor.
pub unsafe fn kvm_get_api_version(kvm_fd: RawFd) -> Result<i32, std::io::Error> {
    let ret = ioctl(kvm_fd, KVM_GET_API_VERSION, 0);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Check if a KVM extension is supported
///
/// # Safety
///
/// `kvm_fd` must be a valid /dev/kvm file descriptor.
pub unsafe fn kvm_check_extension(kvm_fd: RawFd, cap: u32) -> Result<i32, std::io::Error> {
    let ret = ioctl(kvm_fd, KVM_CHECK_EXTENSION, cap as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Create a new VM
///
/// # Safety
///
/// `kvm_fd` must be a valid /dev/kvm file descriptor.
/// Returns a VM file descriptor that must be closed.
pub unsafe fn kvm_create_vm(kvm_fd: RawFd, vm_type: u64) -> Result<RawFd, std::io::Error> {
    let ret = ioctl(kvm_fd, KVM_CREATE_VM, vm_type as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Get the size of the kvm_run mmap region
///
/// # Safety
///
/// `kvm_fd` must be a valid /dev/kvm file descriptor.
pub unsafe fn kvm_get_vcpu_mmap_size(kvm_fd: RawFd) -> Result<usize, std::io::Error> {
    let ret = ioctl(kvm_fd, KVM_GET_VCPU_MMAP_SIZE, 0);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ret as usize)
    }
}

/// Set a user memory region
///
/// # Safety
///
/// - `vm_fd` must be a valid VM file descriptor
/// - `region` must point to a valid kvm_userspace_memory_region
pub unsafe fn kvm_set_user_memory_region(
    vm_fd: RawFd,
    region: &kvm_userspace_memory_region,
) -> Result<(), std::io::Error> {
    let ret = ioctl(
        vm_fd,
        KVM_SET_USER_MEMORY_REGION,
        region as *const _ as usize,
    );
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Create a vCPU
///
/// # Safety
///
/// - `vm_fd` must be a valid VM file descriptor
/// - `vcpu_id` must be < max_vcpus
/// Returns a vCPU file descriptor that must be closed.
pub unsafe fn kvm_create_vcpu(vm_fd: RawFd, vcpu_id: u32) -> Result<RawFd, std::io::Error> {
    let ret = ioctl(vm_fd, KVM_CREATE_VCPU, vcpu_id as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Run a vCPU
///
/// # Safety
///
/// `vcpu_fd` must be a valid vCPU file descriptor.
/// This blocks until the vCPU exits.
pub unsafe fn kvm_run(vcpu_fd: RawFd) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_RUN, 0);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get general purpose registers
///
/// # Safety
///
/// - `vcpu_fd` must be a valid vCPU file descriptor
/// - `regs` must point to valid memory
pub unsafe fn kvm_get_regs(vcpu_fd: RawFd, regs: &mut kvm_regs) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_REGS, regs as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set general purpose registers
///
/// # Safety
///
/// - `vcpu_fd` must be a valid vCPU file descriptor
/// - `regs` must point to valid memory
pub unsafe fn kvm_set_regs(vcpu_fd: RawFd, regs: &kvm_regs) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_REGS, regs as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get special registers
///
/// # Safety
///
/// - `vcpu_fd` must be a valid vCPU file descriptor
/// - `sregs` must point to valid memory
pub unsafe fn kvm_get_sregs(vcpu_fd: RawFd, sregs: &mut kvm_sregs) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_SREGS, sregs as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set special registers
///
/// # Safety
///
/// - `vcpu_fd` must be a valid vCPU file descriptor
/// - `sregs` must point to valid memory
pub unsafe fn kvm_set_sregs(vcpu_fd: RawFd, sregs: &kvm_sregs) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_SREGS, sregs as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Inject an interrupt
///
/// # Safety
///
/// - `vcpu_fd` must be a valid vCPU file descriptor
/// - `irq` must point to valid memory
pub unsafe fn kvm_interrupt(vcpu_fd: RawFd, irq: &kvm_interrupt) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_INTERRUPT, irq as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Create an in-kernel IRQ chip (PIC, IOAPIC, LAPIC)
///
/// # Safety
///
/// `vm_fd` must be a valid VM file descriptor.
pub unsafe fn kvm_create_irqchip(vm_fd: RawFd) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_CREATE_IRQCHIP, 0);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Create a PIT (Programmable Interval Timer)
///
/// # Safety
///
/// - `vm_fd` must be a valid VM file descriptor
/// - `config` must point to valid memory
pub unsafe fn kvm_create_pit2(vm_fd: RawFd, config: &kvm_pit_config) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_CREATE_PIT2, config as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set the TSS (Task State Segment) address
///
/// # Safety
///
/// `vm_fd` must be a valid VM file descriptor.
pub unsafe fn kvm_set_tss_addr(vm_fd: RawFd, addr: u64) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_SET_TSS_ADDR, addr as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get supported CPUID entries from KVM
///
/// # Safety
///
/// - kvm_fd must be a valid /dev/kvm file descriptor
/// - cpuid must point to a valid kvm_cpuid2 with enough space for entries
pub unsafe fn kvm_get_supported_cpuid(
    kvm_fd: RawFd,
    cpuid: &mut kvm_cpuid2,
) -> Result<(), std::io::Error> {
    let ret = ioctl(kvm_fd, KVM_GET_SUPPORTED_CPUID, cpuid as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set the identity map address for the VM
///
/// # Safety
///
/// - m_fd must be a valid VM file descriptor
/// - ddr must be a valid guest physical address
pub unsafe fn kvm_set_identity_map_addr(
    vm_fd: RawFd,
    addr: u64,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_SET_IDENTITY_MAP_ADDR, &addr as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Assert/deassert an IRQ line
///
/// # Safety
///
/// - m_fd must be a valid VM file descriptor
/// - irq_level must point to valid memory
pub unsafe fn kvm_irq_line(
    vm_fd: RawFd,
    irq_level: &kvm_irq_level,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_IRQ_LINE, irq_level as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get IRQ chip state (PIC or IOAPIC)
///
/// # Safety
///
/// - m_fd must be a valid VM file descriptor
/// - chip must point to valid memory with chip_id set
pub unsafe fn kvm_get_irqchip(
    vm_fd: RawFd,
    chip: &mut kvm_irqchip,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_GET_IRQCHIP, chip as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set IRQ chip state (PIC or IOAPIC)
///
/// # Safety
///
/// - m_fd must be a valid VM file descriptor
/// - chip must point to valid, initialized memory
pub unsafe fn kvm_set_irqchip(
    vm_fd: RawFd,
    chip: &kvm_irqchip,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_SET_IRQCHIP, chip as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set GSI (Global System Interrupt) routing table
///
/// # Safety
///
/// - m_fd must be a valid VM file descriptor
/// - 
outing must point to a valid kvm_irq_routing with 
r entries
pub unsafe fn kvm_set_gsi_routing(
    vm_fd: RawFd,
    routing: &kvm_irq_routing,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_SET_GSI_ROUTING, routing as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Signal an MSI (Message Signaled Interrupt)
///
/// # Safety
///
/// - m_fd must be a valid VM file descriptor
/// - msi must point to valid memory
pub unsafe fn kvm_signal_msi(
    vm_fd: RawFd,
    msi: &kvm_msi,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_SIGNAL_MSI, msi as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get the dirty page log for a memory slot
///
/// # Safety
///
/// - m_fd must be a valid VM file descriptor
/// - log must point to valid memory with dirty_bitmap allocated
pub unsafe fn kvm_get_dirty_log(
    vm_fd: RawFd,
    log: &mut kvm_dirty_log,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vm_fd, KVM_GET_DIRTY_LOG, log as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get MSR values
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - msrs must point to a valid kvm_msrs with pre-set indices
pub unsafe fn kvm_get_msrs(
    vcpu_fd: RawFd,
    msrs: &mut kvm_msrs,
) -> Result<i32, std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_MSRS, msrs as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Set MSR values
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - msrs must point to a valid kvm_msrs with entries filled
pub unsafe fn kvm_set_msrs(
    vcpu_fd: RawFd,
    msrs: &kvm_msrs,
) -> Result<i32, std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_MSRS, msrs as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Get floating-point state
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - pu must point to valid memory
pub unsafe fn kvm_get_fpu(
    vcpu_fd: RawFd,
    fpu: &mut kvm_fpu,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_FPU, fpu as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set floating-point state
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - pu must point to valid, initialized memory
pub unsafe fn kvm_set_fpu(
    vcpu_fd: RawFd,
    fpu: &kvm_fpu,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_FPU, fpu as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get LAPIC state
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - lapic must point to valid memory
pub unsafe fn kvm_get_lapic(
    vcpu_fd: RawFd,
    lapic: &mut kvm_lapic_state,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_LAPIC, lapic as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set LAPIC state
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - lapic must point to valid, initialized memory
pub unsafe fn kvm_set_lapic(
    vcpu_fd: RawFd,
    lapic: &kvm_lapic_state,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_LAPIC, lapic as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set CPUID entries for a vCPU
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - cpuid must point to a valid kvm_cpuid2 with entries
pub unsafe fn kvm_set_cpuid2(
    vcpu_fd: RawFd,
    cpuid: &kvm_cpuid2,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_CPUID2, cpuid as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get multiprocessor state
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - mp_state must point to valid memory
pub unsafe fn kvm_get_mp_state(
    vcpu_fd: RawFd,
    mp_state: &mut kvm_mp_state,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_MP_STATE, mp_state as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set multiprocessor state
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - mp_state must point to valid, initialized memory
pub unsafe fn kvm_set_mp_state(
    vcpu_fd: RawFd,
    mp_state: &kvm_mp_state,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_MP_STATE, mp_state as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Inject a NMI (Non-Maskable Interrupt) into a vCPU
///
/// # Safety
///
/// cpu_fd must be a valid vCPU file descriptor.
pub unsafe fn kvm_nmi(vcpu_fd: RawFd) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_NMI, 0);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get vCPU events (exceptions, interrupts, NMIs)
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - vents must point to valid memory
pub unsafe fn kvm_get_vcpu_events(
    vcpu_fd: RawFd,
    events: &mut kvm_vcpu_events,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_VCPU_EVENTS, events as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set vCPU events (exceptions, interrupts, NMIs)
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - vents must point to valid, initialized memory
pub unsafe fn kvm_set_vcpu_events(
    vcpu_fd: RawFd,
    events: &kvm_vcpu_events,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_VCPU_EVENTS, events as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set guest debug mode
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - debug must point to valid memory
pub unsafe fn kvm_set_guest_debug(
    vcpu_fd: RawFd,
    debug: &kvm_guest_debug,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_GUEST_DEBUG, debug as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Translate a guest virtual address to a guest physical address
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - 	ranslation must point to valid memory with linear_address set
pub unsafe fn kvm_translate(
    vcpu_fd: RawFd,
    translation: &mut kvm_translation,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_TRANSLATE, translation as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get extended processor state (XSAVE)
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - xsave must point to valid memory
pub unsafe fn kvm_get_xsave(
    vcpu_fd: RawFd,
    xsave: &mut kvm_xsave,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_XSAVE, xsave as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set extended processor state (XSAVE)
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - xsave must point to valid, initialized memory
pub unsafe fn kvm_set_xsave(
    vcpu_fd: RawFd,
    xsave: &kvm_xsave,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_XSAVE, xsave as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get extended control registers (XCRs)
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - xcrs must point to valid memory
pub unsafe fn kvm_get_xcrs(
    vcpu_fd: RawFd,
    xcrs: &mut kvm_xcrs,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_XCRS, xcrs as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set extended control registers (XCRs)
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - xcrs must point to valid, initialized memory
pub unsafe fn kvm_set_xcrs(
    vcpu_fd: RawFd,
    xcrs: &kvm_xcrs,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_XCRS, xcrs as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Get debug registers
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - debugregs must point to valid memory
pub unsafe fn kvm_get_debugregs(
    vcpu_fd: RawFd,
    debugregs: &mut kvm_debugregs,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_GET_DEBUGREGS, debugregs as *mut _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set debug registers
///
/// # Safety
///
/// - cpu_fd must be a valid vCPU file descriptor
/// - debugregs must point to valid, initialized memory
pub unsafe fn kvm_set_debugregs(
    vcpu_fd: RawFd,
    debugregs: &kvm_debugregs,
) -> Result<(), std::io::Error> {
    let ret = ioctl(vcpu_fd, KVM_SET_DEBUGREGS, debugregs as *const _ as usize);
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
