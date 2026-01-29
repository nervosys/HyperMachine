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
pub const KVM_CHECK_EXTENSION: u64 = 0xae03; // _IO(KVMIO, 0x03)
pub const KVM_GET_VCPU_MMAP_SIZE: u64 = 0xae04; // _IO(KVMIO, 0x04)

// VM ioctls (on VM fd)
pub const KVM_SET_USER_MEMORY_REGION: u64 = 0x4020ae46; // _IOW(KVMIO, 0x46, struct kvm_userspace_memory_region)
pub const KVM_CREATE_VCPU: u64 = 0xae41; // _IO(KVMIO, 0x41)
pub const KVM_SET_TSS_ADDR: u64 = 0xae47; // _IO(KVMIO, 0x47)
pub const KVM_CREATE_IRQCHIP: u64 = 0xae60; // _IO(KVMIO, 0x60)
pub const KVM_CREATE_PIT2: u64 = 0x4040ae77; // _IOW(KVMIO, 0x77, struct kvm_pit_config)
pub const KVM_IOEVENTFD: u64 = 0x4040ae79; // _IOW(KVMIO, 0x79, struct kvm_ioeventfd)
pub const KVM_IRQFD: u64 = 0x4020ae76; // _IOW(KVMIO, 0x76, struct kvm_irqfd)

// vCPU ioctls (on vCPU fd)
pub const KVM_RUN: u64 = 0xae80; // _IO(KVMIO, 0x80)
pub const KVM_GET_REGS: u64 = 0x8090ae81; // _IOR(KVMIO, 0x81, struct kvm_regs)
pub const KVM_SET_REGS: u64 = 0x4090ae82; // _IOW(KVMIO, 0x82, struct kvm_regs)
pub const KVM_GET_SREGS: u64 = 0x8138ae83; // _IOR(KVMIO, 0x83, struct kvm_sregs)
pub const KVM_SET_SREGS: u64 = 0x4138ae84; // _IOW(KVMIO, 0x84, struct kvm_sregs)
pub const KVM_INTERRUPT: u64 = 0x4004ae86; // _IOW(KVMIO, 0x86, struct kvm_interrupt)
pub const KVM_SET_CPUID2: u64 = 0x4008ae90; // _IOW(KVMIO, 0x90, struct kvm_cpuid2)
pub const KVM_GET_LAPIC: u64 = 0x8400ae8e; // _IOR(KVMIO, 0x8e, struct kvm_lapic_state)
pub const KVM_SET_LAPIC: u64 = 0x4400ae8f; // _IOW(KVMIO, 0x8f, struct kvm_lapic_state)

// KVM capability flags
pub const KVM_CAP_IRQCHIP: u32 = 0;
pub const KVM_CAP_HLT: u32 = 1;
pub const KVM_CAP_USER_MEMORY: u32 = 3;
pub const KVM_CAP_SET_TSS_ADDR: u32 = 4;
pub const KVM_CAP_EXT_CPUID: u32 = 7;
pub const KVM_CAP_IOEVENTFD: u32 = 36;
pub const KVM_CAP_IRQFD: u32 = 32;
pub const KVM_CAP_PIT2: u32 = 33;

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
pub const KVM_EXIT_INTERNAL_ERROR: u32 = 17;
pub const KVM_EXIT_SYSTEM_EVENT: u32 = 24;

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
    let path = CString::new("/dev/kvm").unwrap();
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
