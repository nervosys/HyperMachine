//! WHPX FFI bindings
//!
//! This module provides Foreign Function Interface (FFI) bindings to the
//! Windows Hypervisor Platform (WHPX) API.
//!
//! # WHPX API Overview
//!
//! WHPX exposes its API through Windows functions in WinHvPlatform.dll:
//! - Capability queries - Check hypervisor features
//! - Partition management - Create/setup virtual machines
//! - Virtual processor management - Create/run vCPUs
//! - Memory management - Map guest physical memory
//! - Register access - Get/set vCPU state
//!
//! # Safety
//!
//! All functions in this module are `unsafe` because they:
//! - Make raw Windows API calls
//! - Work with raw pointers
//! - Use Windows handles
//! - Have platform-specific behavior
//!
//! Callers must ensure:
//! - Handles are valid
//! - Pointers point to valid, initialized memory
//! - Data structures match Windows expectations
//! - Proper synchronization for concurrent access
//!
//! # Platform
//!
//! This module is only available on Windows and requires:
//! - Windows 10 version 1803 or later
//! - Intel VT-x or AMD-V enabled in BIOS
//! - Hyper-V feature enabled (but not Hyper-V itself running)

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_uint, c_void};

// Windows handle types
pub type WHV_PARTITION_HANDLE = *mut c_void;
pub type VOID = c_void;
pub type UINT32 = c_uint;
pub type UINT64 = u64;
pub type BOOL = i32;

// HRESULT - Windows error codes
pub type HRESULT = i32;
pub const S_OK: HRESULT = 0;
pub const E_FAIL: HRESULT = -2147467259; // 0x80004005

// WHV_CAPABILITY_CODE - Capability query types
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_CAPABILITY_CODE {
    WHvCapabilityCodeHypervisorPresent = 0x00000000,
    WHvCapabilityCodeFeatures = 0x00000001,
    WHvCapabilityCodeExtendedVmExits = 0x00000002,
    WHvCapabilityCodeProcessorVendor = 0x00001000,
    WHvCapabilityCodeProcessorFeatures = 0x00001001,
    WHvCapabilityCodeProcessorClFlushSize = 0x00001002,
}

// WHV_CAPABILITY - Capability query result
#[repr(C)]
#[derive(Copy, Clone)]
pub union WHV_CAPABILITY {
    pub HypervisorPresent: BOOL,
    pub Features: WHV_CAPABILITY_FEATURES,
    pub ProcessorVendor: WHV_PROCESSOR_VENDOR,
    pub ProcessorFeatures: WHV_PROCESSOR_FEATURES,
    pub ProcessorClFlushSize: u8,
}

// WHV_CAPABILITY_FEATURES - Hypervisor feature flags
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_CAPABILITY_FEATURES {
    pub PartialUnmap: BOOL,
    pub LocalApicEmulation: BOOL,
    pub Xsave: BOOL,
    pub DirtyPageTracking: BOOL,
    pub SpeculationControl: BOOL,
    pub ApicRemoteRead: BOOL,
    pub IdleMsrs: BOOL,
    pub VirtualPciDeviceSupport: BOOL,
    pub IommuSupport: BOOL,
}

// WHV_PROCESSOR_VENDOR - CPU vendor
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_PROCESSOR_VENDOR {
    WHvProcessorVendorAmd = 0x0000,
    WHvProcessorVendorIntel = 0x0001,
    WHvProcessorVendorHygon = 0x0002,
}

// WHV_PROCESSOR_FEATURES - CPU features
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_PROCESSOR_FEATURES {
    pub Sse3Support: BOOL,
    pub LahfSahfSupport: BOOL,
    pub Ssse3Support: BOOL,
    pub Sse4_1Support: BOOL,
    pub Sse4_2Support: BOOL,
    pub Sse4aSupport: BOOL,
    pub XopSupport: BOOL,
    pub PopCntSupport: BOOL,
    pub Cmpxchg16bSupport: BOOL,
    pub Altmovcr8Support: BOOL,
    pub LzcntSupport: BOOL,
    pub MisAlignSseSupport: BOOL,
    pub MmxExtSupport: BOOL,
    pub Amd3DNowSupport: BOOL,
    pub ExtendedAmd3DNowSupport: BOOL,
    pub Page1GbSupport: BOOL,
    pub AesSupport: BOOL,
    pub PclmulqdqSupport: BOOL,
    pub PcidSupport: BOOL,
    pub Fma4Support: BOOL,
    pub F16CSupport: BOOL,
    pub RdRandSupport: BOOL,
    pub RdWrFsGsSupport: BOOL,
    pub SmepSupport: BOOL,
    pub EnhancedFastStringSupport: BOOL,
    pub Bmi1Support: BOOL,
    pub Bmi2Support: BOOL,
    pub MovbeSupport: BOOL,
    pub Npiep1Support: BOOL,
    pub DepX87FPUSaveSupport: BOOL,
    pub RdSeedSupport: BOOL,
    pub AdxSupport: BOOL,
    pub IntelPrefetchSupport: BOOL,
    pub SmapSupport: BOOL,
    pub HleSupport: BOOL,
    pub RtmSupport: BOOL,
    pub RdtscpSupport: BOOL,
    pub ClflushoptSupport: BOOL,
    pub ClwbSupport: BOOL,
    pub ShaSupport: BOOL,
    pub X87PointersSavedSupport: BOOL,
    pub InvpcidSupport: BOOL,
    pub IbrsSupport: BOOL,
    pub StibpSupport: BOOL,
    pub IbpbSupport: BOOL,
    pub SsbdSupport: BOOL,
    pub FastShortRepMovSupport: BOOL,
    pub RdclNoSupport: BOOL,
    pub IbrsAllSupport: BOOL,
    pub SsbNoSupport: BOOL,
    pub RsbANoSupport: BOOL,
}

// WHV_PARTITION_PROPERTY_CODE - Partition property types
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_PARTITION_PROPERTY_CODE {
    WHvPartitionPropertyCodeExtendedVmExits = 0x00000001,
    WHvPartitionPropertyCodeExceptionExitBitmap = 0x00000002,
    WHvPartitionPropertyCodeSeparateSecurityDomain = 0x00000003,
    WHvPartitionPropertyCodeNestedVirtualization = 0x00000004,
    WHvPartitionPropertyCodeX64MsrExitBitmap = 0x00000005,
    WHvPartitionPropertyCodePrimaryNumaNode = 0x00000006,
    WHvPartitionPropertyCodeCpuReserve = 0x00000007,
    WHvPartitionPropertyCodeCpuCap = 0x00000008,
    WHvPartitionPropertyCodeCpuWeight = 0x00000009,
    WHvPartitionPropertyCodeCpuGroupId = 0x0000000A,
    WHvPartitionPropertyCodeProcessorFrequencyCap = 0x0000000B,
    WHvPartitionPropertyCodeAllowDeviceAssignment = 0x0000000C,
    WHvPartitionPropertyCodeDisableSmt = 0x0000000D,
    WHvPartitionPropertyCodeProcessorCount = 0x00001000,
    WHvPartitionPropertyCodeProcessorRoot = 0x00001001,
    WHvPartitionPropertyCodeProcessorClFlushSize = 0x00001002,
}

// WHV_PARTITION_PROPERTY - Partition property value
#[repr(C)]
#[derive(Copy, Clone)]
pub union WHV_PARTITION_PROPERTY {
    pub ExtendedVmExits: WHV_EXTENDED_VM_EXITS,
    pub ProcessorCount: UINT32,
    pub CpuReserve: UINT32,
    pub CpuCap: UINT32,
    pub CpuWeight: UINT32,
}

// WHV_EXTENDED_VM_EXITS - Extended VM exit configuration
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_EXTENDED_VM_EXITS {
    pub X64CpuidExit: BOOL,
    pub X64MsrExit: BOOL,
    pub ExceptionExit: BOOL,
    pub X64RdtscExit: BOOL,
    pub X64ApicSmiExit: BOOL,
    pub HypercallExit: BOOL,
    pub X64ApicInitSipiExit: BOOL,
    pub X64ApicWriteLint0Exit: BOOL,
    pub X64ApicWriteLint1Exit: BOOL,
    pub X64ApicWriteSvrExit: BOOL,
}

// WHV_MAP_GPA_RANGE_FLAGS - Memory mapping flags
pub type WHV_MAP_GPA_RANGE_FLAGS = UINT32;
pub const WHvMapGpaRangeFlagNone: WHV_MAP_GPA_RANGE_FLAGS = 0x00000000;
pub const WHvMapGpaRangeFlagRead: WHV_MAP_GPA_RANGE_FLAGS = 0x00000001;
pub const WHvMapGpaRangeFlagWrite: WHV_MAP_GPA_RANGE_FLAGS = 0x00000002;
pub const WHvMapGpaRangeFlagExecute: WHV_MAP_GPA_RANGE_FLAGS = 0x00000004;

// WHV_REGISTER_NAME - vCPU register names
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_REGISTER_NAME {
    // General purpose registers (64-bit)
    WHvX64RegisterRax = 0x00000000,
    WHvX64RegisterRcx = 0x00000001,
    WHvX64RegisterRdx = 0x00000002,
    WHvX64RegisterRbx = 0x00000003,
    WHvX64RegisterRsp = 0x00000004,
    WHvX64RegisterRbp = 0x00000005,
    WHvX64RegisterRsi = 0x00000006,
    WHvX64RegisterRdi = 0x00000007,
    WHvX64RegisterR8 = 0x00000008,
    WHvX64RegisterR9 = 0x00000009,
    WHvX64RegisterR10 = 0x0000000A,
    WHvX64RegisterR11 = 0x0000000B,
    WHvX64RegisterR12 = 0x0000000C,
    WHvX64RegisterR13 = 0x0000000D,
    WHvX64RegisterR14 = 0x0000000E,
    WHvX64RegisterR15 = 0x0000000F,
    WHvX64RegisterRip = 0x00000010,
    WHvX64RegisterRflags = 0x00000011,

    // Segment registers
    WHvX64RegisterEs = 0x00000012,
    WHvX64RegisterCs = 0x00000013,
    WHvX64RegisterSs = 0x00000014,
    WHvX64RegisterDs = 0x00000015,
    WHvX64RegisterFs = 0x00000016,
    WHvX64RegisterGs = 0x00000017,
    WHvX64RegisterLdtr = 0x00000018,
    WHvX64RegisterTr = 0x00000019,

    // Table registers
    WHvX64RegisterIdtr = 0x0000001A,
    WHvX64RegisterGdtr = 0x0000001B,

    // Control registers
    WHvX64RegisterCr0 = 0x0000001C,
    WHvX64RegisterCr2 = 0x0000001D,
    WHvX64RegisterCr3 = 0x0000001E,
    WHvX64RegisterCr4 = 0x0000001F,
    WHvX64RegisterCr8 = 0x00000020,

    // Debug registers
    WHvX64RegisterDr0 = 0x00000021,
    WHvX64RegisterDr1 = 0x00000022,
    WHvX64RegisterDr2 = 0x00000023,
    WHvX64RegisterDr3 = 0x00000024,
    WHvX64RegisterDr6 = 0x00000025,
    WHvX64RegisterDr7 = 0x00000026,

    // Extended control registers
    WHvX64RegisterXCr0 = 0x00000027,

    // MSRs
    WHvX64RegisterEfer = 0x00000029,
    WHvX64RegisterKernelGsBase = 0x0000002A,
    WHvX64RegisterApicBase = 0x0000002B,
    WHvX64RegisterPat = 0x0000002C,
    WHvX64RegisterSysenterCs = 0x0000002D,
    WHvX64RegisterSysenterEip = 0x0000002E,
    WHvX64RegisterSysenterEsp = 0x0000002F,
    WHvX64RegisterStar = 0x00000030,
    WHvX64RegisterLstar = 0x00000031,
    WHvX64RegisterCstar = 0x00000032,
    WHvX64RegisterSfmask = 0x00000033,

    // Internal registers (for interrupt/state management)
    WHvX64RegisterPendingInterruption = 0x00010002,
    WHvX64RegisterInterruptState = 0x00010003,
    WHvX64RegisterPendingEvent = 0x00010005,
    WHvX64RegisterDeliverabilityNotifications = 0x00010006,
}

// WHV_REGISTER_VALUE - Register value (union of all possible types)
#[repr(C)]
#[derive(Copy, Clone)]
pub union WHV_REGISTER_VALUE {
    pub Reg128: WHV_UINT128,
    pub Reg64: UINT64,
    pub Reg32: UINT32,
    pub Reg16: u16,
    pub Reg8: u8,
    pub Fp: WHV_X64_FP_REGISTER,
    pub FpControlStatus: WHV_X64_FP_CONTROL_STATUS_REGISTER,
    pub XmmControlStatus: WHV_X64_XMM_CONTROL_STATUS_REGISTER,
    pub Segment: WHV_X64_SEGMENT_REGISTER,
    pub Table: WHV_X64_TABLE_REGISTER,
    pub InterruptState: WHV_X64_INTERRUPT_STATE_REGISTER,
    pub PendingInterruption: WHV_X64_PENDING_INTERRUPTION_REGISTER,
    pub DeliverabilityNotifications: WHV_X64_DELIVERABILITY_NOTIFICATIONS_REGISTER,
}

// ============================================================================
// Control Register Bit Flags (Intel SDM Vol. 3A)
// ============================================================================

// CR0 - Control Register 0 bit flags
// System control register that controls operating mode and processor state
// Reference: Intel SDM Vol. 3A, Section 2.5

/// CR0.PE (bit 0) - Protected Mode Enable
/// When set, the processor operates in protected mode
pub const CR0_PE: u64 = 1 << 0;

/// CR0.MP (bit 1) - Monitor Coprocessor
/// Controls interaction of WAIT/FWAIT instructions with TS flag
pub const CR0_MP: u64 = 1 << 1;

/// CR0.EM (bit 2) - Emulation
/// When set, floating-point instructions generate #NM exception
pub const CR0_EM: u64 = 1 << 2;

/// CR0.TS (bit 3) - Task Switched
/// Set on task switch, allows lazy FPU context saving
pub const CR0_TS: u64 = 1 << 3;

/// CR0.ET (bit 4) - Extension Type
/// Reserved, always 1 on modern processors (indicates 387 DX math coprocessor)
pub const CR0_ET: u64 = 1 << 4;

/// CR0.NE (bit 5) - Numeric Error
/// When set, enables native (internal) mechanism for reporting FPU errors
pub const CR0_NE: u64 = 1 << 5;

/// CR0.WP (bit 16) - Write Protect
/// When set, inhibits supervisor-level writes to read-only pages
pub const CR0_WP: u64 = 1 << 16;

/// CR0.AM (bit 18) - Alignment Mask
/// When set with AC flag in EFLAGS, enables alignment checking
pub const CR0_AM: u64 = 1 << 18;

/// CR0.NW (bit 29) - Not Write-through
/// Controls write-through/write-back cache strategy
pub const CR0_NW: u64 = 1 << 29;

/// CR0.CD (bit 30) - Cache Disable
/// When set, disables internal cache
pub const CR0_CD: u64 = 1 << 30;

/// CR0.PG (bit 31) - Paging
/// When set, enables paging (requires PE=1)
pub const CR0_PG: u64 = 1 << 31;

// CR4 - Control Register 4 bit flags
// Extended processor feature control register
// Reference: Intel SDM Vol. 3A, Section 2.5

/// CR4.VME (bit 0) - Virtual-8086 Mode Extensions
/// Enables hardware support for interrupt and exception handling in virtual-8086 mode
pub const CR4_VME: u64 = 1 << 0;

/// CR4.PVI (bit 1) - Protected-Mode Virtual Interrupts
/// Enables hardware support for virtual interrupt flag (VIF) in protected mode
pub const CR4_PVI: u64 = 1 << 1;

/// CR4.TSD (bit 2) - Time Stamp Disable
/// When set, RDTSC instruction can only be executed at CPL 0
pub const CR4_TSD: u64 = 1 << 2;

/// CR4.DE (bit 3) - Debugging Extensions
/// Enables I/O breakpoint capability and changes DR4/DR5 handling
pub const CR4_DE: u64 = 1 << 3;

/// CR4.PSE (bit 4) - Page Size Extensions
/// Enables 4-MB pages in 32-bit paging mode
pub const CR4_PSE: u64 = 1 << 4;

/// CR4.PAE (bit 5) - Physical Address Extension
/// Enables paging to produce physical addresses with more than 32 bits
pub const CR4_PAE: u64 = 1 << 5;

/// CR4.MCE (bit 6) - Machine-Check Enable
/// Enables machine-check exceptions
pub const CR4_MCE: u64 = 1 << 6;

/// CR4.PGE (bit 7) - Page Global Enable
/// Enables global page feature (PTE/PDE with G flag not flushed on CR3 reload)
pub const CR4_PGE: u64 = 1 << 7;

/// CR4.PCE (bit 8) - Performance-Monitoring Counter Enable
/// Enables RDPMC instruction at any privilege level when set
pub const CR4_PCE: u64 = 1 << 8;

/// CR4.OSFXSR (bit 9) - OS Support for FXSAVE/FXRSTOR
/// Enables FXSAVE/FXRSTOR instructions and SSE/SSE2 extensions
pub const CR4_OSFXSR: u64 = 1 << 9;

/// CR4.OSXMMEXCPT (bit 10) - OS Support for Unmasked SIMD Floating-Point Exceptions
/// Enables unmasked SSE floating-point exception (#XM) handler
pub const CR4_OSXMMEXCPT: u64 = 1 << 10;

/// CR4.UMIP (bit 11) - User-Mode Instruction Prevention
/// When set, SGDT, SIDT, SLDT, SMSW, and STR cause #GP in user mode
pub const CR4_UMIP: u64 = 1 << 11;

/// CR4.LA57 (bit 12) - 57-bit Linear Addresses
/// Enables 5-level paging and 57-bit linear addresses
pub const CR4_LA57: u64 = 1 << 12;

/// CR4.VMXE (bit 13) - VMX Enable
/// Enables VMX (Virtual Machine Extensions) operation
pub const CR4_VMXE: u64 = 1 << 13;

/// CR4.SMXE (bit 14) - SMX Enable
/// Enables SMX (Safer Mode Extensions) operation
pub const CR4_SMXE: u64 = 1 << 14;

/// CR4.FSGSBASE (bit 16) - FSGSBASE Enable
/// Enables RDFSBASE, RDGSBASE, WRFSBASE, and WRGSBASE instructions
pub const CR4_FSGSBASE: u64 = 1 << 16;

/// CR4.PCIDE (bit 17) - PCID Enable
/// Enables process-context identifiers (PCIDs)
pub const CR4_PCIDE: u64 = 1 << 17;

/// CR4.OSXSAVE (bit 18) - XSAVE and Processor Extended States Enable
/// Enables XSAVE/XRSTOR instructions and extended control register XCR0
pub const CR4_OSXSAVE: u64 = 1 << 18;

/// CR4.SMEP (bit 20) - Supervisor Mode Execution Prevention
/// Prevents execution of code in supervisor mode from pages accessible in user mode
pub const CR4_SMEP: u64 = 1 << 20;

/// CR4.SMAP (bit 21) - Supervisor Mode Access Prevention
/// Prevents supervisor mode from accessing user-mode pages unless EFLAGS.AC is set
pub const CR4_SMAP: u64 = 1 << 21;

/// CR4.PKE (bit 22) - Protection Key Enable
/// Enables protection keys for user-mode pages
pub const CR4_PKE: u64 = 1 << 22;

// ==================== IA32_EFER (Extended Feature Enable Register) ====================
// Reference: Intel SDM Vol. 3A, Section 9.8.5 - IA32_EFER MSR
// MSR address: 0xC0000080

/// IA32_EFER.SCE (bit 0) - System Call Extensions
/// Enables SYSCALL/SYSRET instructions in 64-bit mode
pub const IA32_EFER_SCE: u64 = 1 << 0;

/// IA32_EFER.LME (bit 8) - Long Mode Enable
/// When set, enables IA-32e (long) mode operation
/// Must be set before enabling paging (CR0.PG) to activate long mode
pub const IA32_EFER_LME: u64 = 1 << 8;

/// IA32_EFER.LMA (bit 10) - Long Mode Active (READ-ONLY)
/// Set by the processor when IA-32e mode is active
/// Automatically set when: EFER.LME=1, CR0.PG=1, CR4.PAE=1
/// Cannot be directly written by software
pub const IA32_EFER_LMA: u64 = 1 << 10;

/// IA32_EFER.NXE (bit 11) - No-Execute Enable
/// Enables page access restriction by preventing instruction fetches from PAE pages
/// with the XD (execute-disable) bit set
pub const IA32_EFER_NXE: u64 = 1 << 11;

// ==================== RFLAGS (Flags Register) ====================
// Reference: Intel SDM Vol. 1, Section 3.4.3 - EFLAGS Register
// Reference: Intel SDM Vol. 3A, Section 2.3 - System Flags and Fields in EFLAGS

/// RFLAGS.CF (bit 0) - Carry Flag
/// Set by arithmetic operations that generate a carry or borrow
pub const RFLAGS_CF: u64 = 1 << 0;

/// RFLAGS.PF (bit 2) - Parity Flag
/// Set if the least-significant byte of the result has an even number of 1 bits
pub const RFLAGS_PF: u64 = 1 << 2;

/// RFLAGS.AF (bit 4) - Auxiliary Carry Flag
/// Set on carry from bit 3 to bit 4 during arithmetic operations
pub const RFLAGS_AF: u64 = 1 << 4;

/// RFLAGS.ZF (bit 6) - Zero Flag
/// Set if the result of an operation is zero
pub const RFLAGS_ZF: u64 = 1 << 6;

/// RFLAGS.SF (bit 7) - Sign Flag
/// Set equal to the most-significant bit of the result
pub const RFLAGS_SF: u64 = 1 << 7;

/// RFLAGS.TF (bit 8) - Trap Flag
/// When set, enables single-step mode for debugging
pub const RFLAGS_TF: u64 = 1 << 8;

/// RFLAGS.IF (bit 9) - Interrupt Enable Flag
/// When set, the processor responds to maskable hardware interrupts
/// When clear, maskable interrupts are blocked (NMIs are still delivered)
/// Modified by STI, CLI, POPF, and IRET instructions
pub const RFLAGS_IF: u64 = 1 << 9;

/// RFLAGS.DF (bit 10) - Direction Flag
/// Controls string operation direction (0=increment, 1=decrement)
pub const RFLAGS_DF: u64 = 1 << 10;

/// RFLAGS.OF (bit 11) - Overflow Flag
/// Set when an arithmetic operation results in overflow
pub const RFLAGS_OF: u64 = 1 << 11;

/// RFLAGS.IOPL (bits 12-13) - I/O Privilege Level
/// Indicates the I/O privilege level of the current task (0-3)
pub const RFLAGS_IOPL_MASK: u64 = 0b11 << 12;

/// RFLAGS.NT (bit 14) - Nested Task
/// Set when current task is nested (used in task switching)
pub const RFLAGS_NT: u64 = 1 << 14;

/// RFLAGS.RF (bit 16) - Resume Flag
/// Controls processor's response to debug exceptions
pub const RFLAGS_RF: u64 = 1 << 16;

/// RFLAGS.VM (bit 17) - Virtual-8086 Mode
/// When set in protected mode, enables virtual-8086 mode
pub const RFLAGS_VM: u64 = 1 << 17;

/// RFLAGS.AC (bit 18) - Alignment Check
/// When set with CR0.AM, enables alignment checking
pub const RFLAGS_AC: u64 = 1 << 18;

/// RFLAGS.VIF (bit 19) - Virtual Interrupt Flag
/// Virtual copy of IF flag for virtual-8086 mode
pub const RFLAGS_VIF: u64 = 1 << 19;

/// RFLAGS.VIP (bit 20) - Virtual Interrupt Pending
/// Set to indicate an interrupt is pending in virtual-8086 mode
pub const RFLAGS_VIP: u64 = 1 << 20;

/// RFLAGS.ID (bit 21) - Identification Flag
/// Ability to set/clear this flag indicates CPUID support
pub const RFLAGS_ID: u64 = 1 << 21;

// WHV_UINT128 - 128-bit integer
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_UINT128 {
    pub Low64: UINT64,
    pub High64: UINT64,
}

// WHV_X64_FP_REGISTER - x87 floating-point register
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_FP_REGISTER {
    pub Mantissa: UINT64,
    pub BiasedExponent: UINT64,
}

// WHV_X64_FP_CONTROL_STATUS_REGISTER - x87 FPU control/status
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_FP_CONTROL_STATUS_REGISTER {
    pub FpControl: u16,
    pub FpStatus: u16,
    pub FpTag: u8,
    pub Reserved: u8,
    pub LastFpOp: u16,
    pub LastFpRip: UINT64,
}

// WHV_X64_XMM_CONTROL_STATUS_REGISTER - SSE MXCSR
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_XMM_CONTROL_STATUS_REGISTER {
    pub XmmStatusControl: UINT32,
    pub XmmStatusControlMask: UINT32,
}

// WHV_X64_SEGMENT_REGISTER - Segment register (CS, DS, ES, FS, GS, SS)
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_SEGMENT_REGISTER {
    pub Base: UINT64,
    pub Limit: UINT32,
    pub Selector: u16,
    pub Attributes: u16,
}

// WHV_X64_TABLE_REGISTER - Table register (GDTR, IDTR)
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_TABLE_REGISTER {
    pub Pad: [u16; 3],
    pub Limit: u16,
    pub Base: UINT64,
}

// WHV_X64_INTERRUPT_STATE_REGISTER - Interrupt state
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_INTERRUPT_STATE_REGISTER {
    pub InterruptShadow: BOOL,
    pub NmiMasked: BOOL,
}

// WHV_X64_PENDING_INTERRUPTION_REGISTER - Pending interrupt
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_PENDING_INTERRUPTION_REGISTER {
    pub InterruptionPending: BOOL,
    pub InterruptionType: WHV_X64_PENDING_INTERRUPTION_TYPE,
    pub DeliverErrorCode: BOOL,
    pub InstructionLength: UINT32,
    pub InterruptionVector: UINT32,
    pub ErrorCode: UINT32,
}

// WHV_X64_PENDING_INTERRUPTION_TYPE - Interrupt type
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_X64_PENDING_INTERRUPTION_TYPE {
    WHvX64PendingInterrupt = 0,
    WHvX64PendingNmi = 2,
    WHvX64PendingException = 3,
    WHvX64PendingSoftwareInterrupt = 4,
    WHvX64PendingPrivilegedSoftwareException = 5,
    WHvX64PendingSoftwareException = 6,
}

// WHV_X64_DELIVERABILITY_NOTIFICATIONS_REGISTER - Interrupt deliverability
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_DELIVERABILITY_NOTIFICATIONS_REGISTER {
    pub NmiNotification: BOOL,
    pub InterruptNotification: BOOL,
    pub InterruptPriority: u8,
}

// WHV_RUN_VP_EXIT_REASON - VM exit reasons
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_RUN_VP_EXIT_REASON {
    WHvRunVpExitReasonNone = 0x00000000,
    WHvRunVpExitReasonMemoryAccess = 0x00000001,
    WHvRunVpExitReasonX64IoPortAccess = 0x00000002,
    WHvRunVpExitReasonUnrecoverableException = 0x00000004,
    WHvRunVpExitReasonInvalidVpRegisterValue = 0x00000005,
    WHvRunVpExitReasonUnsupportedFeature = 0x00000006,
    WHvRunVpExitReasonX64InterruptWindow = 0x00000007,
    WHvRunVpExitReasonX64Halt = 0x00000008,
    WHvRunVpExitReasonX64ApicEoi = 0x00000009,
    WHvRunVpExitReasonX64MsrAccess = 0x00001000,
    WHvRunVpExitReasonX64Cpuid = 0x00001001,
    WHvRunVpExitReasonException = 0x00001002,
    WHvRunVpExitReasonX64Rdtsc = 0x00001003,
    WHvRunVpExitReasonX64ApicSmiTrap = 0x00001004,
    WHvRunVpExitReasonHypercall = 0x00001005,
    WHvRunVpExitReasonX64ApicInitSipiTrap = 0x00001006,
    WHvRunVpExitReasonX64ApicWriteTrap = 0x00001007,
    WHvRunVpExitReasonCanceled = 0x00002001,
}

// WHV_RUN_VP_EXIT_CONTEXT - VM exit context (main structure)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WHV_RUN_VP_EXIT_CONTEXT {
    pub ExitReason: WHV_RUN_VP_EXIT_REASON,
    pub Reserved: UINT32,
    pub VpContext: WHV_VP_EXIT_CONTEXT,
    pub ExitData: WHV_RUN_VP_EXIT_DATA,
}

// WHV_VP_EXIT_CONTEXT - vCPU context at exit
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_VP_EXIT_CONTEXT {
    pub ExecutionState: WHV_X64_VP_EXECUTION_STATE,
    pub InstructionLength: u8,
    pub Cr8: u8,
    pub Reserved: [u8; 6],
    pub Cs: WHV_X64_SEGMENT_REGISTER,
    pub Rip: UINT64,
    pub Rflags: UINT64,
}

// WHV_X64_VP_EXECUTION_STATE - vCPU execution state
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_VP_EXECUTION_STATE {
    pub Cpl: u16,
    pub Reserved1: u16,
    pub Reserved2: UINT32,
    pub EferLma: u16,
    pub DebugActive: u16,
    pub InterruptionPending: u16,
    pub Reserved3: [u16; 5],
}

// WHV_RUN_VP_EXIT_DATA - Exit data (union of all exit types)
#[repr(C)]
#[derive(Copy, Clone)]
pub union WHV_RUN_VP_EXIT_DATA {
    pub MemoryAccess: WHV_MEMORY_ACCESS_CONTEXT,
    pub IoPortAccess: WHV_X64_IO_PORT_ACCESS_CONTEXT,
    pub MsrAccess: WHV_X64_MSR_ACCESS_CONTEXT,
    pub Cpuid: WHV_X64_CPUID_ACCESS_CONTEXT,
    pub Exception: WHV_VP_EXCEPTION_CONTEXT,
    pub InterruptWindow: WHV_X64_INTERRUPT_WINDOW_CONTEXT,
    pub ApicEoi: WHV_X64_APIC_EOI_CONTEXT,
    pub Halt: WHV_X64_HALT_CONTEXT,
}

// WHV_MEMORY_ACCESS_CONTEXT - MMIO access context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_MEMORY_ACCESS_CONTEXT {
    pub InstructionByteCount: u8,
    pub Reserved: [u8; 3],
    pub InstructionBytes: [u8; 16],
    pub AccessInfo: WHV_MEMORY_ACCESS_INFO,
    pub Gpa: UINT64,
    pub Gva: UINT64,
}

// WHV_MEMORY_ACCESS_INFO - Memory access information
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_MEMORY_ACCESS_INFO {
    pub AccessType: WHV_MEMORY_ACCESS_TYPE,
    pub GpaUnmapped: BOOL,
    pub GvaValid: BOOL,
}

// WHV_MEMORY_ACCESS_TYPE - Memory access type
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_MEMORY_ACCESS_TYPE {
    WHvMemoryAccessRead = 0,
    WHvMemoryAccessWrite = 1,
    WHvMemoryAccessExecute = 2,
}

// WHV_X64_IO_PORT_ACCESS_CONTEXT - I/O port access context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_IO_PORT_ACCESS_CONTEXT {
    pub InstructionByteCount: u8,
    pub Reserved: [u8; 3],
    pub InstructionBytes: [u8; 16],
    pub AccessInfo: WHV_X64_IO_PORT_ACCESS_INFO,
    pub PortNumber: u16,
    pub Reserved2: [u16; 3],
    pub Rax: UINT64,
    pub Rcx: UINT64,
    pub Rsi: UINT64,
    pub Rdi: UINT64,
    pub Ds: WHV_X64_SEGMENT_REGISTER,
    pub Es: WHV_X64_SEGMENT_REGISTER,
}

// WHV_X64_IO_PORT_ACCESS_INFO - I/O port access information
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_IO_PORT_ACCESS_INFO {
    pub IsWrite: BOOL,
    pub AccessSize: UINT32,
    pub StringOp: BOOL,
    pub RepPrefix: BOOL,
}

// WHV_X64_MSR_ACCESS_CONTEXT - MSR access context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_MSR_ACCESS_CONTEXT {
    pub MsrNumber: UINT32,
    pub IsWrite: BOOL,
    pub Rax: UINT64,
    pub Rdx: UINT64,
}

// WHV_X64_CPUID_ACCESS_CONTEXT - CPUID access context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_CPUID_ACCESS_CONTEXT {
    pub Rax: UINT64,
    pub Rcx: UINT64,
    pub Rdx: UINT64,
    pub Rbx: UINT64,
    pub DefaultResultRax: UINT64,
    pub DefaultResultRcx: UINT64,
    pub DefaultResultRdx: UINT64,
    pub DefaultResultRbx: UINT64,
}

// WHV_VP_EXCEPTION_CONTEXT - Exception context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_VP_EXCEPTION_CONTEXT {
    pub InstructionByteCount: u8,
    pub Reserved: [u8; 3],
    pub InstructionBytes: [u8; 16],
    pub ExceptionInfo: WHV_VP_EXCEPTION_INFO,
    pub ExceptionParameter: UINT64,
}

// WHV_VP_EXCEPTION_INFO - Exception information
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_VP_EXCEPTION_INFO {
    pub ExceptionType: WHV_EXCEPTION_TYPE,
    pub ErrorCodeValid: BOOL,
    pub SoftwareException: BOOL,
}

// WHV_EXCEPTION_TYPE - Exception type
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WHV_EXCEPTION_TYPE {
    WHvX64ExceptionTypeDivideError = 0,
    WHvX64ExceptionTypeDebug = 1,
    WHvX64ExceptionTypeBreakpoint = 3,
    WHvX64ExceptionTypeOverflow = 4,
    WHvX64ExceptionTypeBoundRangeExceeded = 5,
    WHvX64ExceptionTypeInvalidOpcode = 6,
    WHvX64ExceptionTypeDeviceNotAvailable = 7,
    WHvX64ExceptionTypeDoubleFault = 8,
    WHvX64ExceptionTypeInvalidTaskStateSegment = 10,
    WHvX64ExceptionTypeSegmentNotPresent = 11,
    WHvX64ExceptionTypeStackFault = 12,
    WHvX64ExceptionTypeGeneralProtectionFault = 13,
    WHvX64ExceptionTypePageFault = 14,
    WHvX64ExceptionTypeFloatingPointError = 16,
    WHvX64ExceptionTypeAlignmentCheck = 17,
    WHvX64ExceptionTypeMachineCheck = 18,
    WHvX64ExceptionTypeSimdFloatingPointError = 19,
}

// WHV_X64_INTERRUPT_WINDOW_CONTEXT - Interrupt window context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_INTERRUPT_WINDOW_CONTEXT {
    pub DeliverabilityNotifications: WHV_X64_DELIVERABILITY_NOTIFICATIONS_REGISTER,
}

// WHV_X64_APIC_EOI_CONTEXT - APIC EOI context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_APIC_EOI_CONTEXT {
    pub InterruptVector: UINT32,
}

// WHV_X64_HALT_CONTEXT - HLT context
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct WHV_X64_HALT_CONTEXT {
    // Empty structure, just indicates HLT was executed
}

// External FFI functions from WinHvPlatform.dll
#[cfg(target_os = "windows")]
#[link(name = "WinHvPlatform")]
extern "system" {
    /// Query hypervisor capability
    pub fn WHvGetCapability(
        CapabilityCode: WHV_CAPABILITY_CODE,
        CapabilityBuffer: *mut VOID,
        CapabilityBufferSizeInBytes: UINT32,
        WrittenSizeInBytes: *mut UINT32,
    ) -> HRESULT;

    /// Create a partition (VM)
    pub fn WHvCreatePartition(Partition: *mut WHV_PARTITION_HANDLE) -> HRESULT;

    /// Setup partition (finalize configuration)
    pub fn WHvSetupPartition(Partition: WHV_PARTITION_HANDLE) -> HRESULT;

    /// Delete partition
    pub fn WHvDeletePartition(Partition: WHV_PARTITION_HANDLE) -> HRESULT;

    /// Set partition property
    pub fn WHvSetPartitionProperty(
        Partition: WHV_PARTITION_HANDLE,
        PropertyCode: WHV_PARTITION_PROPERTY_CODE,
        PropertyBuffer: *const VOID,
        PropertyBufferSizeInBytes: UINT32,
    ) -> HRESULT;

    /// Get partition property
    pub fn WHvGetPartitionProperty(
        Partition: WHV_PARTITION_HANDLE,
        PropertyCode: WHV_PARTITION_PROPERTY_CODE,
        PropertyBuffer: *mut VOID,
        PropertyBufferSizeInBytes: UINT32,
        WrittenSizeInBytes: *mut UINT32,
    ) -> HRESULT;

    /// Map guest physical address range to host virtual memory
    pub fn WHvMapGpaRange(
        Partition: WHV_PARTITION_HANDLE,
        SourceAddress: *const VOID,
        GuestAddress: UINT64,
        SizeInBytes: UINT64,
        Flags: WHV_MAP_GPA_RANGE_FLAGS,
    ) -> HRESULT;

    /// Unmap guest physical address range
    pub fn WHvUnmapGpaRange(
        Partition: WHV_PARTITION_HANDLE,
        GuestAddress: UINT64,
        SizeInBytes: UINT64,
    ) -> HRESULT;

    /// Create virtual processor (vCPU)
    pub fn WHvCreateVirtualProcessor(
        Partition: WHV_PARTITION_HANDLE,
        VpIndex: UINT32,
        Flags: UINT32,
    ) -> HRESULT;

    /// Delete virtual processor
    pub fn WHvDeleteVirtualProcessor(Partition: WHV_PARTITION_HANDLE, VpIndex: UINT32) -> HRESULT;

    /// Run virtual processor until exit
    pub fn WHvRunVirtualProcessor(
        Partition: WHV_PARTITION_HANDLE,
        VpIndex: UINT32,
        ExitContext: *mut VOID,
        ExitContextSizeInBytes: UINT32,
    ) -> HRESULT;

    /// Cancel virtual processor run
    pub fn WHvCancelRunVirtualProcessor(
        Partition: WHV_PARTITION_HANDLE,
        VpIndex: UINT32,
        Flags: UINT32,
    ) -> HRESULT;

    /// Get virtual processor registers
    pub fn WHvGetVirtualProcessorRegisters(
        Partition: WHV_PARTITION_HANDLE,
        VpIndex: UINT32,
        RegisterNames: *const WHV_REGISTER_NAME,
        RegisterCount: UINT32,
        RegisterValues: *mut WHV_REGISTER_VALUE,
    ) -> HRESULT;

    /// Set virtual processor registers
    pub fn WHvSetVirtualProcessorRegisters(
        Partition: WHV_PARTITION_HANDLE,
        VpIndex: UINT32,
        RegisterNames: *const WHV_REGISTER_NAME,
        RegisterCount: UINT32,
        RegisterValues: *const WHV_REGISTER_VALUE,
    ) -> HRESULT;
}

// Stub implementations for non-Windows platforms
#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvGetCapability(
    _: WHV_CAPABILITY_CODE,
    _: *mut VOID,
    _: UINT32,
    _: *mut UINT32,
) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvCreatePartition(_: *mut WHV_PARTITION_HANDLE) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvSetupPartition(_: WHV_PARTITION_HANDLE) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvDeletePartition(_: WHV_PARTITION_HANDLE) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvSetPartitionProperty(
    _: WHV_PARTITION_HANDLE,
    _: WHV_PARTITION_PROPERTY_CODE,
    _: *const VOID,
    _: UINT32,
) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvGetPartitionProperty(
    _: WHV_PARTITION_HANDLE,
    _: WHV_PARTITION_PROPERTY_CODE,
    _: *mut VOID,
    _: UINT32,
    _: *mut UINT32,
) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvMapGpaRange(
    _: WHV_PARTITION_HANDLE,
    _: *const VOID,
    _: UINT64,
    _: UINT64,
    _: WHV_MAP_GPA_RANGE_FLAGS,
) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvUnmapGpaRange(_: WHV_PARTITION_HANDLE, _: UINT64, _: UINT64) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvCreateVirtualProcessor(_: WHV_PARTITION_HANDLE, _: UINT32, _: UINT32) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvDeleteVirtualProcessor(_: WHV_PARTITION_HANDLE, _: UINT32) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvRunVirtualProcessor(
    _: WHV_PARTITION_HANDLE,
    _: UINT32,
    _: *mut VOID,
    _: UINT32,
) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvCancelRunVirtualProcessor(
    _: WHV_PARTITION_HANDLE,
    _: UINT32,
    _: UINT32,
) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvGetVirtualProcessorRegisters(
    _: WHV_PARTITION_HANDLE,
    _: UINT32,
    _: *const WHV_REGISTER_NAME,
    _: UINT32,
    _: *mut WHV_REGISTER_VALUE,
) -> HRESULT {
    E_FAIL
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn WHvSetVirtualProcessorRegisters(
    _: WHV_PARTITION_HANDLE,
    _: UINT32,
    _: *const WHV_REGISTER_NAME,
    _: UINT32,
    _: *const WHV_REGISTER_VALUE,
) -> HRESULT {
    E_FAIL
}
