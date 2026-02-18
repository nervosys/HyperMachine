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

use crate::{Error, Result};
use core::arch::asm;
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
