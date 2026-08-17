//! Apple Hypervisor Framework (HVF) FFI bindings
//!
//! Raw FFI declarations for macOS Hypervisor.framework API.
//! These bindings provide access to the hardware-accelerated virtualization
//! on macOS, supporting both Intel (VMX) and Apple Silicon.
//!
//! # Safety
//!
//! All functions in the `extern "C"` block are unsafe FFI calls. Callers must
//! ensure correct argument types, valid pointers, and proper lifecycle management
//! of HVF resources (VMs, vCPUs, memory mappings).
//!
//! # References
//!
//! - [Hypervisor Framework](https://developer.apple.com/documentation/hypervisor)

#![allow(non_upper_case_globals)]

use std::ffi::c_void;

// -- Return codes --

/// Operation completed successfully.
pub const HV_SUCCESS: i32 = 0;

/// An unspecified error occurred.
#[allow(dead_code)]
pub const HV_ERROR: i32 = -85_377_023;

/// The resource is busy.
#[allow(dead_code)]
pub const HV_BUSY: i32 = -85_377_022;

/// A bad argument was supplied.
#[allow(dead_code)]
pub const HV_BAD_ARGUMENT: i32 = -85_377_021;

/// Insufficient resources.
#[allow(dead_code)]
pub const HV_NO_RESOURCES: i32 = -85_377_020;

/// No hypervisor device found.
#[allow(dead_code)]
pub const HV_NO_DEVICE: i32 = -85_377_019;

/// The operation is not supported.
#[allow(dead_code)]
pub const HV_UNSUPPORTED: i32 = -85_377_018;

// -- VM exit reasons --

/// Exception or NMI.
pub const HV_EXIT_REASON_EXCEPTION: u64 = 0;

/// EPT violation (memory access fault).
#[allow(dead_code)]
pub const HV_EXIT_REASON_EPT_VIOLATION: u64 = 1;

/// I/O port instruction.
#[allow(dead_code)]
pub const HV_EXIT_REASON_IO_INSTRUCTION: u64 = 2;

/// HLT instruction.
#[allow(dead_code)]
pub const HV_EXIT_REASON_HLT: u64 = 3;

/// Interrupt window became available.
#[allow(dead_code)]
pub const HV_EXIT_REASON_INTERRUPT_WINDOW: u64 = 4;

/// Triple fault.
#[allow(dead_code)]
pub const HV_EXIT_REASON_TRIPLE_FAULT: u64 = 5;

// -- x86_64 register IDs --

/// x86-64 register identifiers for `hv_vcpu_read_register` / `hv_vcpu_write_register`.
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum HvX86Reg {
    Rip = 0,
    Rflags = 1,
    Rax = 2,
    Rcx = 3,
    Rdx = 4,
    Rbx = 5,
    Rsp = 6,
    Rbp = 7,
    Rsi = 8,
    Rdi = 9,
    R8 = 10,
    R9 = 11,
    R10 = 12,
    R11 = 13,
    R12 = 14,
    R13 = 15,
    R14 = 16,
    R15 = 17,
    Cr0 = 18,
    Cr2 = 19,
    Cr3 = 20,
    Cr4 = 21,
}

// -- VMCS fields --

/// Intel VMX VMCS field encodings for `hv_vmx_vcpu_read_vmcs` / `hv_vmx_vcpu_write_vmcs`.
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum VmcsField {
    GuestRip = 0x681e,
    GuestRsp = 0x681c,
    GuestRflags = 0x6820,
    GuestCr0 = 0x6800,
    GuestCr3 = 0x6802,
    GuestCr4 = 0x6804,
    VmExitReason = 0x4402,
    VmExitQualification = 0x6400,
    VmEntryInterruptionInfo = 0x4016,
    VmEntryExceptionErrorCode = 0x4018,
    GuestInterruptibilityState = 0x4824,
    GuestActivityState = 0x4826,

    // -- Guest segment state (Intel SDM Vol. 3, Appendix B) --
    //
    // A segment needs all four of selector, base, limit, and access rights
    // written together: the CPU loads the hidden descriptor cache from these
    // fields on VM entry rather than walking the GDT, so a half-written
    // segment produces a VM-entry failure rather than a fault the guest sees.
    GuestEsSelector = 0x0800,
    GuestCsSelector = 0x0802,
    GuestSsSelector = 0x0804,
    GuestDsSelector = 0x0806,
    GuestFsSelector = 0x0808,
    GuestGsSelector = 0x080A,
    GuestLdtrSelector = 0x080C,
    GuestTrSelector = 0x080E,

    GuestEsLimit = 0x4800,
    GuestCsLimit = 0x4802,
    GuestSsLimit = 0x4804,
    GuestDsLimit = 0x4806,
    GuestFsLimit = 0x4808,
    GuestGsLimit = 0x480A,
    GuestLdtrLimit = 0x480C,
    GuestTrLimit = 0x480E,
    GuestGdtrLimit = 0x4810,
    GuestIdtrLimit = 0x4812,

    GuestEsAccessRights = 0x4814,
    GuestCsAccessRights = 0x4816,
    GuestSsAccessRights = 0x4818,
    GuestDsAccessRights = 0x481A,
    GuestFsAccessRights = 0x481C,
    GuestGsAccessRights = 0x481E,
    GuestLdtrAccessRights = 0x4820,
    GuestTrAccessRights = 0x4822,

    GuestEsBase = 0x6806,
    GuestCsBase = 0x6808,
    GuestSsBase = 0x680A,
    GuestDsBase = 0x680C,
    GuestFsBase = 0x680E,
    GuestGsBase = 0x6810,
    GuestLdtrBase = 0x6812,
    GuestTrBase = 0x6814,
    GuestGdtrBase = 0x6816,
    GuestIdtrBase = 0x6818,
}

// -- Opaque types --

/// Opaque vCPU identifier used by Hypervisor.framework.
pub type HvVcpuId = u64;

// -- Memory mapping flags --

/// Allow reads from the mapped guest physical range.
pub const HV_MEMORY_READ: u64 = 1 << 0;

/// Allow writes to the mapped guest physical range.
pub const HV_MEMORY_WRITE: u64 = 1 << 1;

/// Allow execution from the mapped guest physical range.
pub const HV_MEMORY_EXEC: u64 = 1 << 2;

// -- Guest physical address for VMCS reads --

/// VMCS field encoding for `GUEST_PHYSICAL_ADDRESS` (read after EPT violation).
pub const VMCS_GUEST_PHYSICAL_ADDRESS: u32 = 0x2400;

// -- Hypervisor.framework API --

#[link(name = "Hypervisor", kind = "framework")]
extern "C" {
    // -- VM lifecycle --

    /// Create a VM instance for the current process. Only one VM per process.
    pub fn hv_vm_create(flags: u64) -> i32;

    /// Destroy the current VM instance.
    pub fn hv_vm_destroy() -> i32;

    /// Map a region of user virtual address space into guest physical address space.
    pub fn hv_vm_map(uva: *mut c_void, gpa: u64, size: u64, flags: u64) -> i32;

    /// Unmap a region of guest physical address space.
    pub fn hv_vm_unmap(gpa: u64, size: u64) -> i32;

    // -- vCPU lifecycle --

    /// Create a vCPU instance.
    pub fn hv_vcpu_create(vcpu: *mut HvVcpuId, flags: u64) -> i32;

    /// Destroy a vCPU instance.
    pub fn hv_vcpu_destroy(vcpu: HvVcpuId) -> i32;

    /// Execute the vCPU until a VM exit occurs.
    pub fn hv_vcpu_run(vcpu: HvVcpuId) -> i32;

    /// Force an immediate exit of vCPU(s).
    pub fn hv_vcpu_interrupt(vcpu: *const HvVcpuId, count: u32) -> i32;

    // -- Register access --

    /// Read a vCPU architectural register.
    pub fn hv_vcpu_read_register(vcpu: HvVcpuId, reg: u32, value: *mut u64) -> i32;

    /// Write a vCPU architectural register.
    pub fn hv_vcpu_write_register(vcpu: HvVcpuId, reg: u32, value: u64) -> i32;

    // -- VMX VMCS access --

    /// Read a VMCS field.
    pub fn hv_vmx_vcpu_read_vmcs(vcpu: HvVcpuId, field: u32, value: *mut u64) -> i32;

    /// Write a VMCS field.
    pub fn hv_vmx_vcpu_write_vmcs(vcpu: HvVcpuId, field: u32, value: u64) -> i32;
}
