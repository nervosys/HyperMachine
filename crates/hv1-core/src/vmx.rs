//! Intel VT-x (VMX) support for Type-1 hypervisor
//!
//! This module implements Intel's Virtual Machine Extensions (VMX) for
//! hardware-assisted virtualization. VMX provides:
//!
//! - VMCS (Virtual Machine Control Structure) management
//! - VM entry and exit handling
//! - EPT (Extended Page Tables) for memory virtualization
//! - VPID (Virtual Processor IDs) for TLB management
//! - Posted interrupts and interrupt virtualization

use crate::vcpu::{GeneralRegisters, VcpuRegisters, VmExitInfo};
use crate::{Error, Result};
use core::arch::asm;
use core::arch::naked_asm;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::registers::control::{Cr0, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;

/// VMX feature MSRs
mod msr {
    pub const IA32_VMX_BASIC: u32 = 0x480;
    pub const IA32_VMX_PINBASED_CTLS: u32 = 0x481;
    pub const IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
    pub const IA32_VMX_EXIT_CTLS: u32 = 0x483;
    pub const IA32_VMX_ENTRY_CTLS: u32 = 0x484;
    pub const IA32_VMX_MISC: u32 = 0x485;
    pub const IA32_VMX_CR0_FIXED0: u32 = 0x486;
    pub const IA32_VMX_CR0_FIXED1: u32 = 0x487;
    pub const IA32_VMX_CR4_FIXED0: u32 = 0x488;
    pub const IA32_VMX_CR4_FIXED1: u32 = 0x489;
    pub const IA32_VMX_VMCS_ENUM: u32 = 0x48A;
    pub const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
    pub const IA32_VMX_EPT_VPID_CAP: u32 = 0x48C;
    pub const IA32_VMX_TRUE_PINBASED_CTLS: u32 = 0x48D;
    pub const IA32_VMX_TRUE_PROCBASED_CTLS: u32 = 0x48E;
    pub const IA32_VMX_TRUE_EXIT_CTLS: u32 = 0x48F;
    pub const IA32_VMX_TRUE_ENTRY_CTLS: u32 = 0x490;
    pub const IA32_FEATURE_CONTROL: u32 = 0x3A;
}

/// VMX is enabled on this CPU
static VMX_ENABLED: AtomicBool = AtomicBool::new(false);

/// VMCS field encodings
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmcsField {
    // 16-bit control fields
    VirtualProcessorId = 0x0000,
    PostedInterruptVector = 0x0002,
    EptpIndex = 0x0004,

    // 16-bit guest-state fields
    GuestEsSelector = 0x0800,
    GuestCsSelector = 0x0802,
    GuestSsSelector = 0x0804,
    GuestDsSelector = 0x0806,
    GuestFsSelector = 0x0808,
    GuestGsSelector = 0x080A,
    GuestLdtrSelector = 0x080C,
    GuestTrSelector = 0x080E,
    GuestInterruptStatus = 0x0810,

    // 16-bit host-state fields
    HostEsSelector = 0x0C00,
    HostCsSelector = 0x0C02,
    HostSsSelector = 0x0C04,
    HostDsSelector = 0x0C06,
    HostFsSelector = 0x0C08,
    HostGsSelector = 0x0C0A,
    HostTrSelector = 0x0C0C,

    // 64-bit control fields
    IoAddress = 0x2000,
    TscOffset = 0x2010,
    VirtualApicPage = 0x2012,
    ApicAccessAddress = 0x2014,
    PostedInterruptDescriptor = 0x2016,
    VmFunctionControls = 0x2018,
    EptPointer = 0x201A,
    EoiExitBitmap0 = 0x201C,
    EoiExitBitmap1 = 0x201E,
    EoiExitBitmap2 = 0x2020,
    EoiExitBitmap3 = 0x2022,
    EptpListAddress = 0x2024,
    VmreadBitmap = 0x2026,
    VmwriteBitmap = 0x2028,
    XssExitingBitmap = 0x202C,
    EnclsExitingBitmap = 0x202E,
    TscMultiplier = 0x2032,

    // 64-bit read-only data fields
    GuestPhysicalAddress = 0x2400,

    // 64-bit guest-state fields
    VmcsLinkPointer = 0x2800,
    GuestIa32Debugctl = 0x2802,
    GuestIa32Pat = 0x2804,
    GuestIa32Efer = 0x2806,
    GuestIa32PerfGlobalCtrl = 0x2808,
    GuestPdpte0 = 0x280A,
    GuestPdpte1 = 0x280C,
    GuestPdpte2 = 0x280E,
    GuestPdpte3 = 0x2810,
    GuestIa32Bndcfgs = 0x2812,

    // 64-bit host-state fields
    HostIa32Pat = 0x2C00,
    HostIa32Efer = 0x2C02,
    HostIa32PerfGlobalCtrl = 0x2C04,

    // 32-bit control fields
    PinBasedVmExecControls = 0x4000,
    CpuBasedVmExecControls = 0x4002,
    ExceptionBitmap = 0x4004,
    PageFaultErrorCodeMask = 0x4006,
    PageFaultErrorCodeMatch = 0x4008,
    Cr3TargetCount = 0x400A,
    VmExitControls = 0x400C,
    VmExitMsrStoreCount = 0x400E,
    VmExitMsrLoadCount = 0x4010,
    VmEntryControls = 0x4012,
    VmEntryMsrLoadCount = 0x4014,
    VmEntryInterruptionInfo = 0x4016,
    VmEntryExceptionErrorCode = 0x4018,
    VmEntryInstructionLen = 0x401A,
    TprThreshold = 0x401C,
    SecondaryVmExecControls = 0x401E,
    PleGap = 0x4020,
    PleWindow = 0x4022,

    // 32-bit read-only data fields
    VmInstructionError = 0x4400,
    VmExitReason = 0x4402,
    VmExitInterruptionInfo = 0x4404,
    VmExitInterruptionErrorCode = 0x4406,
    IdtVectoringInfo = 0x4408,
    IdtVectoringErrorCode = 0x440A,
    VmExitInstructionLen = 0x440C,
    VmExitInstructionInfo = 0x440E,

    // 32-bit guest-state fields
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
    GuestInterruptibilityState = 0x4824,
    GuestActivityState = 0x4826,
    GuestSmbase = 0x4828,
    GuestIa32SysenterCs = 0x482A,
    VmxPreemptionTimerValue = 0x482E,

    // 32-bit host-state fields
    HostIa32SysenterCs = 0x4C00,

    // Natural-width control fields
    Cr0GuestHostMask = 0x6000,
    Cr4GuestHostMask = 0x6002,
    Cr0ReadShadow = 0x6004,
    Cr4ReadShadow = 0x6006,
    Cr3Target0 = 0x6008,
    Cr3Target1 = 0x600A,
    Cr3Target2 = 0x600C,
    Cr3Target3 = 0x600E,

    // Natural-width read-only data fields
    ExitQualification = 0x6400,
    IoRcx = 0x6402,
    IoRsi = 0x6404,
    IoRdi = 0x6406,
    IoRip = 0x6408,
    GuestLinearAddress = 0x640A,

    // Natural-width guest-state fields
    GuestCr0 = 0x6800,
    GuestCr3 = 0x6802,
    GuestCr4 = 0x6804,
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
    GuestDr7 = 0x681A,
    GuestRsp = 0x681C,
    GuestRip = 0x681E,
    GuestRflags = 0x6820,
    GuestPendingDebugExceptions = 0x6822,
    GuestIa32SysenterEsp = 0x6824,
    GuestIa32SysenterEip = 0x6826,

    // Natural-width host-state fields
    HostCr0 = 0x6C00,
    HostCr3 = 0x6C02,
    HostCr4 = 0x6C04,
    HostFsBase = 0x6C06,
    HostGsBase = 0x6C08,
    HostTrBase = 0x6C0A,
    HostGdtrBase = 0x6C0C,
    HostIdtrBase = 0x6C0E,
    HostIa32SysenterEsp = 0x6C10,
    HostIa32SysenterEip = 0x6C12,
    HostRsp = 0x6C14,
    HostRip = 0x6C16,
}

/// VM exit reasons
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmExitReason {
    ExceptionOrNmi = 0,
    ExternalInterrupt = 1,
    TripleFault = 2,
    InitSignal = 3,
    StartupIpi = 4,
    IoSmi = 5,
    OtherSmi = 6,
    InterruptWindow = 7,
    NmiWindow = 8,
    TaskSwitch = 9,
    Cpuid = 10,
    Getsec = 11,
    Hlt = 12,
    Invd = 13,
    Invlpg = 14,
    Rdpmc = 15,
    Rdtsc = 16,
    Rsm = 17,
    Vmcall = 18,
    Vmclear = 19,
    Vmlaunch = 20,
    Vmptrld = 21,
    Vmptrst = 22,
    Vmread = 23,
    Vmresume = 24,
    Vmwrite = 25,
    Vmxoff = 26,
    Vmxon = 27,
    CrAccess = 28,
    DrAccess = 29,
    IoInstruction = 30,
    Rdmsr = 31,
    Wrmsr = 32,
    VmEntryFailInvalidGuestState = 33,
    VmEntryFailMsrLoading = 34,
    Mwait = 36,
    MonitorTrapFlag = 37,
    Monitor = 39,
    Pause = 40,
    VmEntryFailMachineCheckEvent = 41,
    TprBelowThreshold = 43,
    ApicAccess = 44,
    VirtualizedEoi = 45,
    AccessGdtrOrIdtr = 46,
    AccessLdtrOrTr = 47,
    EptViolation = 48,
    EptMisconfiguration = 49,
    Invept = 50,
    Rdtscp = 51,
    VmxPreemptionTimerExpired = 52,
    Invvpid = 53,
    Wbinvd = 54,
    Xsetbv = 55,
    ApicWrite = 56,
    Rdrand = 57,
    Invpcid = 58,
    Vmfunc = 59,
    Encls = 60,
    Rdseed = 61,
    PageModificationLogFull = 62,
    Xsaves = 63,
    Xrstors = 64,
    Unknown = 0xFFFF,
}

impl From<u32> for VmExitReason {
    fn from(value: u32) -> Self {
        match value & 0xFFFF {
            0 => VmExitReason::ExceptionOrNmi,
            1 => VmExitReason::ExternalInterrupt,
            2 => VmExitReason::TripleFault,
            3 => VmExitReason::InitSignal,
            4 => VmExitReason::StartupIpi,
            5 => VmExitReason::IoSmi,
            6 => VmExitReason::OtherSmi,
            7 => VmExitReason::InterruptWindow,
            8 => VmExitReason::NmiWindow,
            9 => VmExitReason::TaskSwitch,
            10 => VmExitReason::Cpuid,
            11 => VmExitReason::Getsec,
            12 => VmExitReason::Hlt,
            13 => VmExitReason::Invd,
            14 => VmExitReason::Invlpg,
            15 => VmExitReason::Rdpmc,
            16 => VmExitReason::Rdtsc,
            17 => VmExitReason::Rsm,
            18 => VmExitReason::Vmcall,
            28 => VmExitReason::CrAccess,
            29 => VmExitReason::DrAccess,
            30 => VmExitReason::IoInstruction,
            31 => VmExitReason::Rdmsr,
            32 => VmExitReason::Wrmsr,
            40 => VmExitReason::Pause,
            48 => VmExitReason::EptViolation,
            49 => VmExitReason::EptMisconfiguration,
            _ => VmExitReason::Unknown,
        }
    }
}

/// VMCS region (4KB aligned)
#[repr(C, align(4096))]
pub struct VmcsRegion {
    /// VMCS revision identifier
    pub revision_id: u32,
    /// VMX-abort indicator
    pub abort_indicator: u32,
    /// VMCS data (implementation-specific)
    pub data: [u8; 4088],
}

impl VmcsRegion {
    /// Create a new VMCS region
    pub fn new() -> Self {
        let revision_id = unsafe {
            let basic = x86::msr::rdmsr(msr::IA32_VMX_BASIC);
            (basic & 0x7FFF_FFFF) as u32
        };

        Self {
            revision_id,
            abort_indicator: 0,
            data: [0; 4088],
        }
    }
}

impl Default for VmcsRegion {
    fn default() -> Self {
        Self::new()
    }
}

/// VMXON region (4KB aligned)
#[repr(C, align(4096))]
pub struct VmxonRegion {
    /// VMCS revision identifier
    pub revision_id: u32,
    /// Reserved
    pub reserved: [u8; 4092],
}

impl VmxonRegion {
    /// Create a new VMXON region
    pub fn new() -> Self {
        let revision_id = unsafe {
            let basic = x86::msr::rdmsr(msr::IA32_VMX_BASIC);
            (basic & 0x7FFF_FFFF) as u32
        };

        Self {
            revision_id,
            reserved: [0; 4092],
        }
    }
}

impl Default for VmxonRegion {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if VMX is supported
pub fn is_supported() -> bool {
    let cpuid = raw_cpuid::CpuId::new();
    cpuid
        .get_feature_info()
        .map(|f| f.has_vmx())
        .unwrap_or(false)
}

/// Initialize VMX on the current CPU
pub fn initialize() -> Result<()> {
    if !is_supported() {
        return Err(Error::NoHardwareSupport);
    }

    // Check IA32_FEATURE_CONTROL MSR
    let feature_control = unsafe { x86::msr::rdmsr(msr::IA32_FEATURE_CONTROL) };

    // Bit 0: Lock bit
    // Bit 2: Enable VMX outside SMX
    if feature_control & 0x1 != 0 && feature_control & 0x4 == 0 {
        // Locked but VMX disabled - cannot enable
        return Err(Error::VmxInitFailed);
    }

    // Set required CR0 bits
    unsafe {
        let cr0_fixed0 = x86::msr::rdmsr(msr::IA32_VMX_CR0_FIXED0);
        let cr0_fixed1 = x86::msr::rdmsr(msr::IA32_VMX_CR0_FIXED1);
        let mut cr0 = Cr0::read_raw();
        cr0 |= cr0_fixed0;
        cr0 &= cr0_fixed1;
        Cr0::write_raw(cr0);
    }

    // Set required CR4 bits (including VMXE)
    unsafe {
        let cr4_fixed0 = x86::msr::rdmsr(msr::IA32_VMX_CR4_FIXED0);
        let cr4_fixed1 = x86::msr::rdmsr(msr::IA32_VMX_CR4_FIXED1);
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::VIRTUAL_MACHINE_EXTENSIONS);
        // Apply fixed bits
        let raw = cr4.bits() | cr4_fixed0 & cr4_fixed1;
        Cr4::write_raw(raw);
    }

    VMX_ENABLED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Execute VMXON instruction
pub unsafe fn vmxon(vmxon_region: &VmxonRegion) -> Result<()> {
    let addr = vmxon_region as *const _ as u64;
    let mut flags: u64;

    asm!(
        "vmxon [{0}]",
        "pushfq",
        "pop {1}",
        in(reg) &addr,
        out(reg) flags,
        options(nostack)
    );

    // Check CF and ZF flags
    if flags & 0x41 != 0 {
        return Err(Error::VmxonFailed);
    }

    Ok(())
}

/// Execute VMXOFF instruction
pub unsafe fn vmxoff() -> Result<()> {
    asm!("vmxoff", options(nostack));
    VMX_ENABLED.store(false, Ordering::SeqCst);
    Ok(())
}

/// Execute VMCLEAR instruction
pub unsafe fn vmclear(vmcs_region: &VmcsRegion) -> Result<()> {
    let addr = vmcs_region as *const _ as u64;
    let mut flags: u64;

    asm!(
        "vmclear [{0}]",
        "pushfq",
        "pop {1}",
        in(reg) &addr,
        out(reg) flags,
        options(nostack)
    );

    if flags & 0x41 != 0 {
        return Err(Error::VmclearFailed);
    }

    Ok(())
}

/// Execute VMPTRLD instruction
pub unsafe fn vmptrld(vmcs_region: &VmcsRegion) -> Result<()> {
    let addr = vmcs_region as *const _ as u64;
    let mut flags: u64;

    asm!(
        "vmptrld [{0}]",
        "pushfq",
        "pop {1}",
        in(reg) &addr,
        out(reg) flags,
        options(nostack)
    );

    if flags & 0x41 != 0 {
        return Err(Error::VmptrldFailed);
    }

    Ok(())
}

/// Write to VMCS field
pub unsafe fn vmwrite(field: VmcsField, value: u64) -> Result<()> {
    let mut flags: u64;

    asm!(
        "vmwrite {1}, {0}",
        "pushfq",
        "pop {2}",
        in(reg) value,
        in(reg) field as u64,
        out(reg) flags,
        options(nostack)
    );

    if flags & 0x41 != 0 {
        return Err(Error::VmwriteFailed);
    }

    Ok(())
}

/// Read from VMCS field
pub unsafe fn vmread(field: VmcsField) -> Result<u64> {
    let mut value: u64;
    let mut flags: u64;

    asm!(
        "vmread {0}, {1}",
        "pushfq",
        "pop {2}",
        out(reg) value,
        in(reg) field as u64,
        out(reg) flags,
        options(nostack)
    );

    if flags & 0x41 != 0 {
        return Err(Error::VmreadFailed);
    }

    Ok(value)
}

/// Execute VMLAUNCH instruction
#[allow(unused_assignments)]
pub unsafe fn vmlaunch() -> Result<()> {
    let mut flags: u64;

    asm!(
        "vmlaunch",
        "pushfq",
        "pop {0}",
        out(reg) flags,
        options(nostack)
    );

    // If we get here, vmlaunch failed
    Err(Error::VmlaunchFailed)
}

/// Execute VMRESUME instruction
#[allow(unused_assignments)]
pub unsafe fn vmresume() -> Result<()> {
    let mut flags: u64;

    asm!(
        "vmresume",
        "pushfq",
        "pop {0}",
        out(reg) flags,
        options(nostack)
    );

    // If we get here, vmresume failed
    Err(Error::VmresumeFailed)
}

/// Check if VMX is enabled
pub fn is_enabled() -> bool {
    VMX_ENABLED.load(Ordering::SeqCst)
}

// ─── VMX Control Constants ──────────────────────────────────────────

/// Pin-based VM-execution controls
pub mod pin_based {
    pub const EXTERNAL_INTERRUPT_EXITING: u32 = 1 << 0;
    pub const NMI_EXITING: u32 = 1 << 3;
    pub const VIRTUAL_NMIS: u32 = 1 << 5;
    pub const VMX_PREEMPTION_TIMER: u32 = 1 << 6;
    pub const POSTED_INTERRUPTS: u32 = 1 << 7;
}

/// Primary processor-based VM-execution controls
pub mod proc_based {
    pub const INTERRUPT_WINDOW_EXITING: u32 = 1 << 2;
    pub const USE_TSC_OFFSETTING: u32 = 1 << 3;
    pub const HLT_EXITING: u32 = 1 << 7;
    pub const INVLPG_EXITING: u32 = 1 << 9;
    pub const MWAIT_EXITING: u32 = 1 << 10;
    pub const RDPMC_EXITING: u32 = 1 << 11;
    pub const RDTSC_EXITING: u32 = 1 << 12;
    pub const CR3_LOAD_EXITING: u32 = 1 << 15;
    pub const CR3_STORE_EXITING: u32 = 1 << 16;
    pub const CR8_LOAD_EXITING: u32 = 1 << 19;
    pub const CR8_STORE_EXITING: u32 = 1 << 20;
    pub const TPR_SHADOW: u32 = 1 << 21;
    pub const NMI_WINDOW_EXITING: u32 = 1 << 22;
    pub const MOV_DR_EXITING: u32 = 1 << 23;
    pub const UNCONDITIONAL_IO_EXITING: u32 = 1 << 24;
    pub const USE_IO_BITMAPS: u32 = 1 << 25;
    pub const MONITOR_TRAP_FLAG: u32 = 1 << 27;
    pub const USE_MSR_BITMAPS: u32 = 1 << 28;
    pub const MONITOR_EXITING: u32 = 1 << 29;
    pub const PAUSE_EXITING: u32 = 1 << 30;
    pub const ACTIVATE_SECONDARY: u32 = 1u32 << 31;
}

/// Secondary processor-based VM-execution controls
pub mod proc_based2 {
    pub const VIRTUALIZE_APIC_ACCESSES: u32 = 1 << 0;
    pub const ENABLE_EPT: u32 = 1 << 1;
    pub const DESCRIPTOR_TABLE_EXITING: u32 = 1 << 2;
    pub const ENABLE_RDTSCP: u32 = 1 << 3;
    pub const VIRTUALIZE_X2APIC: u32 = 1 << 4;
    pub const ENABLE_VPID: u32 = 1 << 5;
    pub const WBINVD_EXITING: u32 = 1 << 6;
    pub const UNRESTRICTED_GUEST: u32 = 1 << 7;
    pub const APIC_REGISTER_VIRTUALIZATION: u32 = 1 << 8;
    pub const VIRTUAL_INTERRUPT_DELIVERY: u32 = 1 << 9;
    pub const PAUSE_LOOP_EXITING: u32 = 1 << 10;
    pub const ENABLE_INVPCID: u32 = 1 << 12;
    pub const ENABLE_XSAVES: u32 = 1 << 20;
}

/// VM-exit controls
pub mod exit_controls {
    pub const SAVE_DEBUG_CONTROLS: u32 = 1 << 2;
    pub const HOST_ADDRESS_SPACE_SIZE: u32 = 1 << 9;
    pub const ACKNOWLEDGE_INTERRUPT_ON_EXIT: u32 = 1 << 15;
    pub const SAVE_IA32_PAT: u32 = 1 << 18;
    pub const LOAD_IA32_PAT: u32 = 1 << 19;
    pub const SAVE_IA32_EFER: u32 = 1 << 20;
    pub const LOAD_IA32_EFER: u32 = 1 << 21;
    pub const SAVE_VMX_PREEMPTION_TIMER: u32 = 1 << 22;
}

/// VM-entry controls
pub mod entry_controls {
    pub const LOAD_DEBUG_CONTROLS: u32 = 1 << 2;
    pub const IA32E_MODE_GUEST: u32 = 1 << 9;
    pub const LOAD_IA32_PAT: u32 = 1 << 14;
    pub const LOAD_IA32_EFER: u32 = 1 << 15;
}

// ─── VM-entry interruption-info field encoding ──────────────────────

/// VM-entry interruption-info field: vector (bits 7:0), type (bits 10:8),
/// error code valid (bit 11), valid (bit 31).
pub mod interrupt_info {
    /// External interrupt
    pub const TYPE_EXTERNAL: u32 = 0 << 8;
    /// NMI
    pub const TYPE_NMI: u32 = 2 << 8;
    /// Hardware exception
    pub const TYPE_HARDWARE_EXCEPTION: u32 = 3 << 8;
    /// Software interrupt (INT n)
    pub const TYPE_SOFTWARE: u32 = 4 << 8;
    /// Privileged software exception
    pub const TYPE_PRIV_SOFTWARE_EXCEPTION: u32 = 5 << 8;
    /// Software exception (INT3, INTO)
    pub const TYPE_SOFTWARE_EXCEPTION: u32 = 6 << 8;
    /// Error code is valid
    pub const ERROR_CODE_VALID: u32 = 1 << 11;
    /// Entry is valid
    pub const VALID: u32 = 1u32 << 31;
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Adjust VMX controls by applying the mandatory 0 and 1 bits from capability MSR.
///
/// # Safety
/// Reads a VMX capability MSR.
unsafe fn adjust_controls(desired: u32, capability_msr: u32) -> u32 {
    let caps = x86::msr::rdmsr(capability_msr);
    let allowed_0 = caps as u32; // bits that must be 1
    let allowed_1 = (caps >> 32) as u32; // bits that may be 1
    (desired | allowed_0) & allowed_1
}

/// Read a segment selector register.
macro_rules! read_seg_selector {
    ($seg:literal) => {{
        let sel: u16;
        asm!(concat!("mov {:x}, ", $seg), out(reg) sel, options(nomem, nostack));
        sel
    }};
}

/// Read GDTR base and limit.
#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

unsafe fn read_gdtr() -> DescriptorTablePointer {
    let mut gdtr = DescriptorTablePointer { limit: 0, base: 0 };
    asm!("sgdt [{}]", in(reg) &mut gdtr, options(nostack));
    gdtr
}

unsafe fn read_idtr() -> DescriptorTablePointer {
    let mut idtr = DescriptorTablePointer { limit: 0, base: 0 };
    asm!("sidt [{}]", in(reg) &mut idtr, options(nostack));
    idtr
}

/// Read TR base from GDT.
unsafe fn read_tr_base() -> u64 {
    let tr: u16;
    asm!("str {:x}", out(reg) tr, options(nomem, nostack));
    let gdtr = read_gdtr();
    // TR is a system segment descriptor (16 bytes in 64-bit mode)
    let desc_addr = gdtr.base + (tr & !0x7) as u64;
    let desc = desc_addr as *const u64;
    let low = core::ptr::read_volatile(desc);
    let high = core::ptr::read_volatile(desc.add(1));
    // Base from TSS descriptor: bits 63:56, 39:16 of low qword + bits 31:0 of high qword
    let base_low =
        ((low >> 16) & 0xFFFF) | (((low >> 32) & 0xFF) << 16) | (((low >> 56) & 0xFF) << 24);
    let base_high = high & 0xFFFF_FFFF;
    base_low | (base_high << 32)
}

// ─── VMCS setup ─────────────────────────────────────────────────────

/// Set up VMCS execution controls.
///
/// # Safety
/// A VMCS must be current (via VMPTRLD).
pub unsafe fn setup_vmcs_controls(ept_pointer: u64) -> Result<()> {
    // Pin-based controls: intercept external interrupts and NMIs
    let pin = adjust_controls(
        pin_based::EXTERNAL_INTERRUPT_EXITING | pin_based::NMI_EXITING,
        msr::IA32_VMX_TRUE_PINBASED_CTLS,
    );
    vmwrite(VmcsField::PinBasedVmExecControls, pin as u64)?;

    // Primary processor-based controls
    let proc = adjust_controls(
        proc_based::HLT_EXITING
            | proc_based::UNCONDITIONAL_IO_EXITING
            | proc_based::ACTIVATE_SECONDARY,
        msr::IA32_VMX_TRUE_PROCBASED_CTLS,
    );
    vmwrite(VmcsField::CpuBasedVmExecControls, proc as u64)?;

    // Secondary processor-based controls: EPT, RDTSCP, unrestricted guest
    let proc2 = adjust_controls(
        proc_based2::ENABLE_EPT | proc_based2::ENABLE_RDTSCP | proc_based2::UNRESTRICTED_GUEST,
        msr::IA32_VMX_PROCBASED_CTLS2,
    );
    vmwrite(VmcsField::SecondaryVmExecControls, proc2 as u64)?;

    // VM-exit controls: 64-bit host, save/load EFER, ack interrupt on exit
    let exit = adjust_controls(
        exit_controls::HOST_ADDRESS_SPACE_SIZE
            | exit_controls::SAVE_IA32_EFER
            | exit_controls::LOAD_IA32_EFER
            | exit_controls::ACKNOWLEDGE_INTERRUPT_ON_EXIT,
        msr::IA32_VMX_TRUE_EXIT_CTLS,
    );
    vmwrite(VmcsField::VmExitControls, exit as u64)?;

    // VM-entry controls
    let entry = adjust_controls(
        entry_controls::LOAD_DEBUG_CONTROLS,
        msr::IA32_VMX_TRUE_ENTRY_CTLS,
    );
    vmwrite(VmcsField::VmEntryControls, entry as u64)?;

    // EPT pointer (WB memory type = 6, page-walk length 4-1 = 3)
    vmwrite(VmcsField::EptPointer, ept_pointer)?;

    // VMCS link pointer (required; -1 = no shadow VMCS)
    vmwrite(VmcsField::VmcsLinkPointer, u64::MAX)?;

    // Exception bitmap: don't intercept any exceptions
    vmwrite(VmcsField::ExceptionBitmap, 0)?;

    // CR guest/host masks: don't trap CR accesses
    vmwrite(VmcsField::Cr0GuestHostMask, 0)?;
    vmwrite(VmcsField::Cr4GuestHostMask, 0)?;
    vmwrite(VmcsField::Cr0ReadShadow, 0)?;
    vmwrite(VmcsField::Cr4ReadShadow, 0)?;

    // MSR store/load counts (none)
    vmwrite(VmcsField::VmExitMsrStoreCount, 0)?;
    vmwrite(VmcsField::VmExitMsrLoadCount, 0)?;
    vmwrite(VmcsField::VmEntryMsrLoadCount, 0)?;

    Ok(())
}

/// Write VMCS host-state fields from current CPU state.
///
/// # Safety
/// A VMCS must be current. `host_rsp` and `host_rip` must be valid addresses
/// for the VM-exit handler.
pub unsafe fn setup_vmcs_host_state(host_rsp: u64, host_rip: u64) -> Result<()> {
    // Control registers
    vmwrite(VmcsField::HostCr0, Cr0::read_raw())?;
    vmwrite(
        VmcsField::HostCr3,
        x86_64::registers::control::Cr3::read()
            .0
            .start_address()
            .as_u64(),
    )?;
    vmwrite(VmcsField::HostCr4, Cr4::read().bits())?;

    // Segment selectors
    vmwrite(VmcsField::HostCsSelector, read_seg_selector!("cs") as u64)?;
    vmwrite(VmcsField::HostSsSelector, read_seg_selector!("ss") as u64)?;
    vmwrite(VmcsField::HostDsSelector, read_seg_selector!("ds") as u64)?;
    vmwrite(VmcsField::HostEsSelector, read_seg_selector!("es") as u64)?;
    vmwrite(VmcsField::HostFsSelector, read_seg_selector!("fs") as u64)?;
    vmwrite(VmcsField::HostGsSelector, read_seg_selector!("gs") as u64)?;
    let tr: u16;
    asm!("str {:x}", out(reg) tr, options(nomem, nostack));
    vmwrite(VmcsField::HostTrSelector, tr as u64)?;

    // Segment bases
    vmwrite(VmcsField::HostFsBase, x86::msr::rdmsr(0xC000_0100))?; // IA32_FS_BASE
    vmwrite(VmcsField::HostGsBase, x86::msr::rdmsr(0xC000_0101))?; // IA32_GS_BASE
    vmwrite(VmcsField::HostTrBase, read_tr_base())?;

    // GDT/IDT bases
    let gdtr = read_gdtr();
    let idtr = read_idtr();
    vmwrite(VmcsField::HostGdtrBase, gdtr.base)?;
    vmwrite(VmcsField::HostIdtrBase, idtr.base)?;

    // SYSENTER fields
    vmwrite(VmcsField::HostIa32SysenterCs, x86::msr::rdmsr(0x174) as u64)?;
    vmwrite(VmcsField::HostIa32SysenterEsp, x86::msr::rdmsr(0x175))?;
    vmwrite(VmcsField::HostIa32SysenterEip, x86::msr::rdmsr(0x176))?;

    // EFER
    vmwrite(VmcsField::HostIa32Efer, x86::msr::rdmsr(0xC000_0080))?;

    // RSP and RIP where execution resumes on VM exit
    vmwrite(VmcsField::HostRsp, host_rsp)?;
    vmwrite(VmcsField::HostRip, host_rip)?;

    Ok(())
}

/// Write VMCS guest-state fields from `VcpuRegisters`.
///
/// # Safety
/// A VMCS must be current.
pub unsafe fn setup_vmcs_guest_state(regs: &VcpuRegisters) -> Result<()> {
    // Control registers
    vmwrite(VmcsField::GuestCr0, regs.cr.cr0)?;
    vmwrite(VmcsField::GuestCr3, regs.cr.cr3)?;
    vmwrite(VmcsField::GuestCr4, regs.cr.cr4)?;
    vmwrite(VmcsField::GuestDr7, regs.dr.dr7)?;

    // CS
    vmwrite(VmcsField::GuestCsSelector, regs.seg.cs.selector as u64)?;
    vmwrite(VmcsField::GuestCsBase, regs.seg.cs.base)?;
    vmwrite(VmcsField::GuestCsLimit, regs.seg.cs.limit as u64)?;
    vmwrite(
        VmcsField::GuestCsAccessRights,
        regs.seg.cs.attributes as u64,
    )?;

    // SS
    vmwrite(VmcsField::GuestSsSelector, regs.seg.ss.selector as u64)?;
    vmwrite(VmcsField::GuestSsBase, regs.seg.ss.base)?;
    vmwrite(VmcsField::GuestSsLimit, regs.seg.ss.limit as u64)?;
    vmwrite(
        VmcsField::GuestSsAccessRights,
        regs.seg.ss.attributes as u64,
    )?;

    // DS
    vmwrite(VmcsField::GuestDsSelector, regs.seg.ds.selector as u64)?;
    vmwrite(VmcsField::GuestDsBase, regs.seg.ds.base)?;
    vmwrite(VmcsField::GuestDsLimit, regs.seg.ds.limit as u64)?;
    vmwrite(
        VmcsField::GuestDsAccessRights,
        regs.seg.ds.attributes as u64,
    )?;

    // ES
    vmwrite(VmcsField::GuestEsSelector, regs.seg.es.selector as u64)?;
    vmwrite(VmcsField::GuestEsBase, regs.seg.es.base)?;
    vmwrite(VmcsField::GuestEsLimit, regs.seg.es.limit as u64)?;
    vmwrite(
        VmcsField::GuestEsAccessRights,
        regs.seg.es.attributes as u64,
    )?;

    // FS
    vmwrite(VmcsField::GuestFsSelector, regs.seg.fs.selector as u64)?;
    vmwrite(VmcsField::GuestFsBase, regs.seg.fs.base)?;
    vmwrite(VmcsField::GuestFsLimit, regs.seg.fs.limit as u64)?;
    vmwrite(
        VmcsField::GuestFsAccessRights,
        regs.seg.fs.attributes as u64,
    )?;

    // GS
    vmwrite(VmcsField::GuestGsSelector, regs.seg.gs.selector as u64)?;
    vmwrite(VmcsField::GuestGsBase, regs.seg.gs.base)?;
    vmwrite(VmcsField::GuestGsLimit, regs.seg.gs.limit as u64)?;
    vmwrite(
        VmcsField::GuestGsAccessRights,
        regs.seg.gs.attributes as u64,
    )?;

    // LDTR
    vmwrite(VmcsField::GuestLdtrSelector, regs.seg.ldtr.selector as u64)?;
    vmwrite(VmcsField::GuestLdtrBase, regs.seg.ldtr.base)?;
    vmwrite(VmcsField::GuestLdtrLimit, regs.seg.ldtr.limit as u64)?;
    vmwrite(
        VmcsField::GuestLdtrAccessRights,
        regs.seg.ldtr.attributes as u64,
    )?;

    // TR
    vmwrite(VmcsField::GuestTrSelector, regs.seg.tr.selector as u64)?;
    vmwrite(VmcsField::GuestTrBase, regs.seg.tr.base)?;
    vmwrite(VmcsField::GuestTrLimit, regs.seg.tr.limit as u64)?;
    vmwrite(
        VmcsField::GuestTrAccessRights,
        regs.seg.tr.attributes as u64,
    )?;

    // GDTR / IDTR
    vmwrite(VmcsField::GuestGdtrBase, regs.dt.gdtr_base)?;
    vmwrite(VmcsField::GuestGdtrLimit, regs.dt.gdtr_limit as u64)?;
    vmwrite(VmcsField::GuestIdtrBase, regs.dt.idtr_base)?;
    vmwrite(VmcsField::GuestIdtrLimit, regs.dt.idtr_limit as u64)?;

    // RIP, RSP, RFLAGS
    vmwrite(VmcsField::GuestRip, regs.gp.rip)?;
    vmwrite(VmcsField::GuestRsp, regs.gp.rsp)?;
    vmwrite(VmcsField::GuestRflags, regs.gp.rflags)?;

    // EFER
    vmwrite(VmcsField::GuestIa32Efer, regs.cr.efer)?;

    // Guest interruptibility and activity state
    vmwrite(VmcsField::GuestInterruptibilityState, 0)?;
    vmwrite(VmcsField::GuestActivityState, 0)?;

    // Guest IA32_DEBUGCTL
    vmwrite(VmcsField::GuestIa32Debugctl, 0)?;

    // Guest SYSENTER fields
    vmwrite(VmcsField::GuestIa32SysenterCs, 0)?;
    vmwrite(VmcsField::GuestIa32SysenterEsp, 0)?;
    vmwrite(VmcsField::GuestIa32SysenterEip, 0)?;

    // Pending debug exceptions
    vmwrite(VmcsField::GuestPendingDebugExceptions, 0)?;

    Ok(())
}

/// Sync guest GP registers to VMCS (RIP, RSP, RFLAGS only — others saved via asm).
///
/// # Safety
/// A VMCS must be current.
pub unsafe fn sync_guest_state_to_vmcs(regs: &VcpuRegisters) -> Result<()> {
    vmwrite(VmcsField::GuestRip, regs.gp.rip)?;
    vmwrite(VmcsField::GuestRsp, regs.gp.rsp)?;
    vmwrite(VmcsField::GuestRflags, regs.gp.rflags)?;
    vmwrite(VmcsField::GuestCr0, regs.cr.cr0)?;
    vmwrite(VmcsField::GuestCr3, regs.cr.cr3)?;
    vmwrite(VmcsField::GuestCr4, regs.cr.cr4)?;
    Ok(())
}

/// Read guest state back from VMCS after a VM exit.
///
/// # Safety
/// A VMCS must be current and a VM exit must have occurred.
pub unsafe fn sync_guest_state_from_vmcs(regs: &mut VcpuRegisters) -> Result<()> {
    regs.gp.rip = vmread(VmcsField::GuestRip)?;
    regs.gp.rsp = vmread(VmcsField::GuestRsp)?;
    regs.gp.rflags = vmread(VmcsField::GuestRflags)?;
    regs.cr.cr0 = vmread(VmcsField::GuestCr0)?;
    regs.cr.cr3 = vmread(VmcsField::GuestCr3)?;
    regs.cr.cr4 = vmread(VmcsField::GuestCr4)?;
    Ok(())
}

/// Read VM-exit information from the VMCS.
///
/// # Safety
/// A VMCS must be current and a VM exit must have occurred.
pub unsafe fn read_exit_info() -> Result<VmExitInfo> {
    let reason = vmread(VmcsField::VmExitReason)? as u32;
    let qualification = vmread(VmcsField::ExitQualification)?;
    let instruction_length = vmread(VmcsField::VmExitInstructionLen)? as u32;
    let instruction_info = vmread(VmcsField::VmExitInstructionInfo)? as u32;

    let guest_physical_addr = if (reason & 0xFFFF) == VmExitReason::EptViolation as u32
        || (reason & 0xFFFF) == VmExitReason::EptMisconfiguration as u32
    {
        Some(vmread(VmcsField::GuestPhysicalAddress)?)
    } else {
        None
    };

    let guest_linear_addr = Some(vmread(VmcsField::GuestLinearAddress)?);

    Ok(VmExitInfo {
        reason: reason & 0xFFFF,
        qualification,
        guest_physical_addr,
        guest_linear_addr,
        instruction_length,
        instruction_info,
    })
}

/// Execute a VMX VM-entry (VMLAUNCH or VMRESUME), saving and restoring GP registers.
///
/// On success the guest runs until a VM exit, then this function returns `Ok(())`.
/// On failure (e.g. invalid guest state) it returns an error.
///
/// # Safety
/// VMCS must be properly configured and current. `regs` must point to a valid
/// `GeneralRegisters` struct. The caller must handle the VM exit afterwards.
pub unsafe fn vmx_run(regs: &mut GeneralRegisters, launched: bool) -> Result<()> {
    let error: u64;
    let launched_flag: u64 = if launched { 1 } else { 0 };

    asm!(
        // Save host callee-saved registers
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "push rdi",

        // Load guest GP registers from GeneralRegisters struct
        // rdi = pointer to GeneralRegisters
        "mov rax, [rdi + 0x00]",
        "mov rbx, [rdi + 0x08]",
        "mov rcx, [rdi + 0x10]",
        "mov rdx, [rdi + 0x18]",
        "mov rsi, [rdi + 0x20]",
        // rdi loaded last (we're using it as base pointer)
        "mov rbp, [rdi + 0x30]",
        // rsp is managed by VMCS (GuestRsp)
        "mov r8,  [rdi + 0x40]",
        "mov r9,  [rdi + 0x48]",
        "mov r10, [rdi + 0x50]",
        "mov r11, [rdi + 0x58]",
        "mov r12, [rdi + 0x60]",
        "mov r13, [rdi + 0x68]",
        "mov r14, [rdi + 0x70]",
        "mov r15, [rdi + 0x78]",
        "mov rdi, [rdi + 0x28]", // load guest rdi last

        // VM entry: vmlaunch or vmresume
        "test {launched}, {launched}",
        "jnz 20f",
        "vmlaunch",
        "jmp 30f",        // entry failure
        "20:",
        "vmresume",
        "30:",
        // If we reach here, VM entry failed (CF or ZF set).
        // On a *successful* VM entry the guest runs; on VM exit the
        // CPU loads HostRip/HostRsp from VMCS and jumps there, so
        // successful entry+exit does NOT return here.
        // We still need to restore host state.
        "setc {error:l}",

        // Restore host callee-saved (guest rdi is lost on entry failure,
        // but that's acceptable — the guest didn't run)
        "pop rdi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        launched = in(reg) launched_flag,
        error = out(reg) error,
        // All GP registers are clobbered by guest execution
        out("rax") _,
        out("rcx") _,
        out("rdx") _,
        out("rsi") _,
        out("r8") _,
        out("r9") _,
        out("r10") _,
        out("r11") _,
        options(nostack),
    );

    if error != 0 {
        return Err(Error::VmlaunchFailed);
    }

    Ok(())
}

/// VM-exit handler function to be used as HostRip.
///
/// When a VM exit occurs, the CPU loads HostRsp and HostRip from the VMCS
/// and jumps here. This function saves guest GP registers, restores the
/// host callee-saved registers (which were pushed before VM entry), and
/// returns to the caller of `vmx_run_with_exit`.
///
/// The protocol is:
///   HostRsp points to the stack frame created by `vmx_run_with_exit`,
///   with the GeneralRegisters pointer at `[rsp]` (top of the saved frame).
///
/// # Safety
/// Only called as a VM-exit entry point with the correct HostRsp layout.
#[unsafe(naked)]
unsafe extern "C" fn vmx_exit_handler() {
    naked_asm!(
        // On entry: guest GP regs are in CPU registers.
        // HostRsp was set so [rsp] = saved rdi (GeneralRegisters pointer),
        // followed by saved r15..rbx.

        // Temporarily save guest rax on stack
        "push rax",
        // Load the GeneralRegisters pointer (saved rdi is at rsp+8 after our push)
        "mov rax, [rsp + 8]",
        // Save guest GP registers to the struct
        "pop QWORD PTR [rax + 0x00]", // guest rax (from our push)
        "mov [rax + 0x08], rbx",
        "mov [rax + 0x10], rcx",
        "mov [rax + 0x18], rdx",
        "mov [rax + 0x20], rsi",
        "mov [rax + 0x28], rdi",
        "mov [rax + 0x30], rbp",
        // rsp is in VMCS (GuestRsp) — not changed
        "mov [rax + 0x40], r8",
        "mov [rax + 0x48], r9",
        "mov [rax + 0x50], r10",
        "mov [rax + 0x58], r11",
        "mov [rax + 0x60], r12",
        "mov [rax + 0x68], r13",
        "mov [rax + 0x70], r14",
        "mov [rax + 0x78], r15",
        // Restore host callee-saved registers (match vmx_run_with_exit push order)
        "pop rdi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        // Return to the caller of vmx_run_with_exit.
        // xor error flag to indicate success.
        "xor eax, eax",
        "ret",
    );
}

/// Combined VMX run: sets up HostRip/HostRsp, executes VM entry, handles exit.
///
/// Returns `Ok(())` on a successful VM-entry → exit cycle. The guest GP
/// registers in `regs` are updated with the state at the time of the VM exit.
///
/// # Safety
/// VMCS must be fully configured (controls, guest state, EPT, etc.).
/// `regs` must be valid. Only the first call should pass `launched = false`.
pub unsafe fn vmx_run_with_exit(
    vmcs: &VmcsRegion,
    regs: &mut GeneralRegisters,
    launched: bool,
) -> Result<()> {
    // Make this VMCS current
    vmptrld(vmcs)?;

    // Write HostRip to our exit handler
    vmwrite(VmcsField::HostRip, vmx_exit_handler as *const () as u64)?;

    let result: u64;
    let launched_flag: u64 = if launched { 1 } else { 0 };

    asm!(
        // Save host callee-saved registers
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "push rdi",  // GeneralRegisters pointer — exit handler reads this

        // Set HostRsp to current rsp so exit handler sees our frame
        "mov rax, rsp",
        // vmwrite HostRsp (field 0x6C14), rax
        "mov rdx, 0x6C14",
        "vmwrite rdx, rax",

        // Load guest GP registers
        "mov rax, [rdi + 0x00]",
        "mov rbx, [rdi + 0x08]",
        "mov rcx, [rdi + 0x10]",
        "mov rdx, [rdi + 0x18]",
        "mov rsi, [rdi + 0x20]",
        "mov rbp, [rdi + 0x30]",
        "mov r8,  [rdi + 0x40]",
        "mov r9,  [rdi + 0x48]",
        "mov r10, [rdi + 0x50]",
        "mov r11, [rdi + 0x58]",
        "mov r12, [rdi + 0x60]",
        "mov r13, [rdi + 0x68]",
        "mov r14, [rdi + 0x70]",
        "mov r15, [rdi + 0x78]",
        "mov rdi, [rdi + 0x28]",

        // VM entry
        "test {launched}, {launched}",
        "jnz 200f",
        "vmlaunch",
        "jmp 300f",
        "200:",
        "vmresume",
        "300:",
        // VM entry failed — restore host frame
        "pop rdi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "mov {result}, 1",
        "jmp 400f",

        // (on successful entry+exit the naked exit handler returns here
        //  via `ret` with rax=0, host callee-saved already restored)
        "400:",
        launched = in(reg) launched_flag,
        result = out(reg) result,
        out("rax") _,
        out("rcx") _,
        out("rdx") _,
        out("rsi") _,
        out("r8") _,
        out("r9") _,
        out("r10") _,
        out("r11") _,
        options(nostack),
    );

    if result != 0 {
        return Err(Error::VmlaunchFailed);
    }

    Ok(())
}

/// Inject an interrupt into the guest via the VMCS VM-entry interruption-info field.
///
/// # Safety
/// A VMCS must be current.
pub unsafe fn inject_interrupt(vector: u8, is_nmi: bool) -> Result<()> {
    let int_type = if is_nmi {
        interrupt_info::TYPE_NMI
    } else {
        interrupt_info::TYPE_EXTERNAL
    };
    let info = (vector as u32) | int_type | interrupt_info::VALID;
    vmwrite(VmcsField::VmEntryInterruptionInfo, info as u64)?;
    Ok(())
}

/// Inject a hardware exception into the guest.
///
/// # Safety
/// A VMCS must be current.
pub unsafe fn inject_exception(vector: u8, error_code: Option<u32>) -> Result<()> {
    let mut info =
        (vector as u32) | interrupt_info::TYPE_HARDWARE_EXCEPTION | interrupt_info::VALID;
    if let Some(code) = error_code {
        info |= interrupt_info::ERROR_CODE_VALID;
        vmwrite(VmcsField::VmEntryExceptionErrorCode, code as u64)?;
    }
    vmwrite(VmcsField::VmEntryInterruptionInfo, info as u64)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vmcs_region_size() {
        assert_eq!(core::mem::size_of::<VmcsRegion>(), 4096);
    }

    #[test]
    fn test_vmxon_region_size() {
        assert_eq!(core::mem::size_of::<VmxonRegion>(), 4096);
    }

    #[test]
    fn test_vm_exit_reason_from() {
        assert_eq!(VmExitReason::from(12), VmExitReason::Hlt);
        assert_eq!(VmExitReason::from(48), VmExitReason::EptViolation);
    }
}
