//! Nested virtualization types
//!
//! This module defines types for nested VMX/SVM virtualization,
//! including VMCS fields, capabilities, and L1/L2 guest state.

use std::collections::HashMap;
use std::fmt;

/// VMX capability MSR values
#[derive(Debug, Clone, Default)]
pub struct VmxCapabilities {
    /// Basic VMX information (IA32_VMX_BASIC)
    pub basic: u64,
    /// Pin-based controls (IA32_VMX_PINBASED_CTLS)
    pub pinbased_ctls: u64,
    /// Primary processor-based controls
    pub procbased_ctls: u64,
    /// Secondary processor-based controls
    pub procbased_ctls2: u64,
    /// VM-exit controls
    pub exit_ctls: u64,
    /// VM-entry controls
    pub entry_ctls: u64,
    /// Miscellaneous data
    pub misc: u64,
    /// CR0 fixed bits
    pub cr0_fixed0: u64,
    pub cr0_fixed1: u64,
    /// CR4 fixed bits
    pub cr4_fixed0: u64,
    pub cr4_fixed1: u64,
    /// VMCS enumeration
    pub vmcs_enum: u64,
    /// EPT/VPID capabilities
    pub ept_vpid_cap: u64,
    /// True pin-based controls
    pub true_pinbased_ctls: u64,
    /// True primary proc-based controls
    pub true_procbased_ctls: u64,
    /// True exit controls
    pub true_exit_ctls: u64,
    /// True entry controls
    pub true_entry_ctls: u64,
    /// VM function controls
    pub vmfunc: u64,
}

impl VmxCapabilities {
    /// Create default VMX capabilities for nested virtualization
    pub fn default_nested() -> Self {
        Self {
            basic: 0x0000_0001_0000_0000 | (4096 << 32), // VMCS size = 4KB
            pinbased_ctls: 0x0000_003F_0000_0016,
            procbased_ctls: 0xFFF9_FFFE_0401_E172,
            procbased_ctls2: 0x0000_00FF_0000_0000,
            exit_ctls: 0x003F_FFFF_0003_6DFF,
            entry_ctls: 0x0000_F3FF_0000_11FF,
            misc: 0x0000_0000_0040_0000,
            cr0_fixed0: 0x0000_0000_8000_0021,
            cr0_fixed1: 0xFFFF_FFFF_FFFF_FFFF,
            cr4_fixed0: 0x0000_0000_0000_2000,
            cr4_fixed1: 0x0000_0000_003F_67FF,
            vmcs_enum: 0x0000_0000_0000_002A,
            ept_vpid_cap: 0x0000_0F01_0640_1076,
            true_pinbased_ctls: 0x0000_003F_0000_0016,
            true_procbased_ctls: 0xFFF9_FFFE_0401_E172,
            true_exit_ctls: 0x003F_FFFF_0003_6DFB,
            true_entry_ctls: 0x0000_F3FF_0000_11FB,
            vmfunc: 0x0000_0000_0000_0001,
        }
    }

    /// Check if EPT is supported
    pub fn supports_ept(&self) -> bool {
        (self.procbased_ctls2 >> 33) & 1 != 0
    }

    /// Check if VPID is supported
    pub fn supports_vpid(&self) -> bool {
        (self.procbased_ctls2 >> 37) & 1 != 0
    }

    /// Check if unrestricted guest is supported
    pub fn supports_unrestricted_guest(&self) -> bool {
        (self.procbased_ctls2 >> 39) & 1 != 0
    }
}

/// VMCS field encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VmcsField(pub u32);

impl VmcsField {
    // 16-bit control fields
    pub const VIRTUAL_PROCESSOR_ID: Self = Self(0x0000);
    pub const POSTED_INTR_NV: Self = Self(0x0002);
    pub const EPTP_INDEX: Self = Self(0x0004);

    // 16-bit guest state
    pub const GUEST_ES_SELECTOR: Self = Self(0x0800);
    pub const GUEST_CS_SELECTOR: Self = Self(0x0802);
    pub const GUEST_SS_SELECTOR: Self = Self(0x0804);
    pub const GUEST_DS_SELECTOR: Self = Self(0x0806);
    pub const GUEST_FS_SELECTOR: Self = Self(0x0808);
    pub const GUEST_GS_SELECTOR: Self = Self(0x080A);
    pub const GUEST_LDTR_SELECTOR: Self = Self(0x080C);
    pub const GUEST_TR_SELECTOR: Self = Self(0x080E);
    pub const GUEST_INTR_STATUS: Self = Self(0x0810);
    pub const GUEST_PML_INDEX: Self = Self(0x0812);

    // 16-bit host state
    pub const HOST_ES_SELECTOR: Self = Self(0x0C00);
    pub const HOST_CS_SELECTOR: Self = Self(0x0C02);
    pub const HOST_SS_SELECTOR: Self = Self(0x0C04);
    pub const HOST_DS_SELECTOR: Self = Self(0x0C06);
    pub const HOST_FS_SELECTOR: Self = Self(0x0C08);
    pub const HOST_GS_SELECTOR: Self = Self(0x0C0A);
    pub const HOST_TR_SELECTOR: Self = Self(0x0C0C);

    // 64-bit control fields
    pub const IO_BITMAP_A: Self = Self(0x2000);
    pub const IO_BITMAP_B: Self = Self(0x2002);
    pub const MSR_BITMAP: Self = Self(0x2004);
    pub const VM_EXIT_MSR_STORE_ADDR: Self = Self(0x2006);
    pub const VM_EXIT_MSR_LOAD_ADDR: Self = Self(0x2008);
    pub const VM_ENTRY_MSR_LOAD_ADDR: Self = Self(0x200A);
    pub const EXECUTIVE_VMCS_PTR: Self = Self(0x200C);
    pub const PML_ADDRESS: Self = Self(0x200E);
    pub const TSC_OFFSET: Self = Self(0x2010);
    pub const VIRTUAL_APIC_PAGE_ADDR: Self = Self(0x2012);
    pub const APIC_ACCESS_ADDR: Self = Self(0x2014);
    pub const POSTED_INTR_DESC_ADDR: Self = Self(0x2016);
    pub const VM_FUNCTION_CONTROL: Self = Self(0x2018);
    pub const EPT_POINTER: Self = Self(0x201A);
    pub const EOI_EXIT_BITMAP0: Self = Self(0x201C);
    pub const EOI_EXIT_BITMAP1: Self = Self(0x201E);
    pub const EOI_EXIT_BITMAP2: Self = Self(0x2020);
    pub const EOI_EXIT_BITMAP3: Self = Self(0x2022);
    pub const EPTP_LIST_ADDR: Self = Self(0x2024);
    pub const VMREAD_BITMAP: Self = Self(0x2026);
    pub const VMWRITE_BITMAP: Self = Self(0x2028);
    pub const XSS_EXIT_BITMAP: Self = Self(0x202C);
    pub const ENCLS_EXITING_BITMAP: Self = Self(0x202E);
    pub const TSC_MULTIPLIER: Self = Self(0x2032);

    // 64-bit read-only fields
    pub const GUEST_PHYSICAL_ADDRESS: Self = Self(0x2400);

    // 64-bit guest state
    pub const VMCS_LINK_POINTER: Self = Self(0x2800);
    pub const GUEST_IA32_DEBUGCTL: Self = Self(0x2802);
    pub const GUEST_IA32_PAT: Self = Self(0x2804);
    pub const GUEST_IA32_EFER: Self = Self(0x2806);
    pub const GUEST_IA32_PERF_GLOBAL_CTRL: Self = Self(0x2808);
    pub const GUEST_PDPTE0: Self = Self(0x280A);
    pub const GUEST_PDPTE1: Self = Self(0x280C);
    pub const GUEST_PDPTE2: Self = Self(0x280E);
    pub const GUEST_PDPTE3: Self = Self(0x2810);
    pub const GUEST_BNDCFGS: Self = Self(0x2812);
    pub const GUEST_IA32_RTIT_CTL: Self = Self(0x2814);

    // 64-bit host state
    pub const HOST_IA32_PAT: Self = Self(0x2C00);
    pub const HOST_IA32_EFER: Self = Self(0x2C02);
    pub const HOST_IA32_PERF_GLOBAL_CTRL: Self = Self(0x2C04);

    // 32-bit control fields
    pub const PIN_BASED_VM_EXEC_CONTROL: Self = Self(0x4000);
    pub const CPU_BASED_VM_EXEC_CONTROL: Self = Self(0x4002);
    pub const EXCEPTION_BITMAP: Self = Self(0x4004);
    pub const PAGE_FAULT_ERROR_CODE_MASK: Self = Self(0x4006);
    pub const PAGE_FAULT_ERROR_CODE_MATCH: Self = Self(0x4008);
    pub const CR3_TARGET_COUNT: Self = Self(0x400A);
    pub const VM_EXIT_CONTROLS: Self = Self(0x400C);
    pub const VM_EXIT_MSR_STORE_COUNT: Self = Self(0x400E);
    pub const VM_EXIT_MSR_LOAD_COUNT: Self = Self(0x4010);
    pub const VM_ENTRY_CONTROLS: Self = Self(0x4012);
    pub const VM_ENTRY_MSR_LOAD_COUNT: Self = Self(0x4014);
    pub const VM_ENTRY_INTR_INFO: Self = Self(0x4016);
    pub const VM_ENTRY_EXCEPTION_ERROR_CODE: Self = Self(0x4018);
    pub const VM_ENTRY_INSTRUCTION_LEN: Self = Self(0x401A);
    pub const TPR_THRESHOLD: Self = Self(0x401C);
    pub const SECONDARY_VM_EXEC_CONTROL: Self = Self(0x401E);
    pub const PLE_GAP: Self = Self(0x4020);
    pub const PLE_WINDOW: Self = Self(0x4022);

    // 32-bit read-only data fields
    pub const VM_INSTRUCTION_ERROR: Self = Self(0x4400);
    pub const VM_EXIT_REASON: Self = Self(0x4402);
    pub const VM_EXIT_INTR_INFO: Self = Self(0x4404);
    pub const VM_EXIT_INTR_ERROR_CODE: Self = Self(0x4406);
    pub const IDT_VECTORING_INFO: Self = Self(0x4408);
    pub const IDT_VECTORING_ERROR_CODE: Self = Self(0x440A);
    pub const VM_EXIT_INSTRUCTION_LEN: Self = Self(0x440C);
    pub const VMX_INSTRUCTION_INFO: Self = Self(0x440E);

    // 32-bit guest state
    pub const GUEST_ES_LIMIT: Self = Self(0x4800);
    pub const GUEST_CS_LIMIT: Self = Self(0x4802);
    pub const GUEST_SS_LIMIT: Self = Self(0x4804);
    pub const GUEST_DS_LIMIT: Self = Self(0x4806);
    pub const GUEST_FS_LIMIT: Self = Self(0x4808);
    pub const GUEST_GS_LIMIT: Self = Self(0x480A);
    pub const GUEST_LDTR_LIMIT: Self = Self(0x480C);
    pub const GUEST_TR_LIMIT: Self = Self(0x480E);
    pub const GUEST_GDTR_LIMIT: Self = Self(0x4810);
    pub const GUEST_IDTR_LIMIT: Self = Self(0x4812);
    pub const GUEST_ES_AR_BYTES: Self = Self(0x4814);
    pub const GUEST_CS_AR_BYTES: Self = Self(0x4816);
    pub const GUEST_SS_AR_BYTES: Self = Self(0x4818);
    pub const GUEST_DS_AR_BYTES: Self = Self(0x481A);
    pub const GUEST_FS_AR_BYTES: Self = Self(0x481C);
    pub const GUEST_GS_AR_BYTES: Self = Self(0x481E);
    pub const GUEST_LDTR_AR_BYTES: Self = Self(0x4820);
    pub const GUEST_TR_AR_BYTES: Self = Self(0x4822);
    pub const GUEST_INTERRUPTIBILITY_INFO: Self = Self(0x4824);
    pub const GUEST_ACTIVITY_STATE: Self = Self(0x4826);
    pub const GUEST_SYSENTER_CS: Self = Self(0x482A);
    pub const VMX_PREEMPTION_TIMER_VALUE: Self = Self(0x482E);

    // 32-bit host state
    pub const HOST_SYSENTER_CS: Self = Self(0x4C00);

    // Natural-width control fields
    pub const CR0_GUEST_HOST_MASK: Self = Self(0x6000);
    pub const CR4_GUEST_HOST_MASK: Self = Self(0x6002);
    pub const CR0_READ_SHADOW: Self = Self(0x6004);
    pub const CR4_READ_SHADOW: Self = Self(0x6006);
    pub const CR3_TARGET_VALUE0: Self = Self(0x6008);
    pub const CR3_TARGET_VALUE1: Self = Self(0x600A);
    pub const CR3_TARGET_VALUE2: Self = Self(0x600C);
    pub const CR3_TARGET_VALUE3: Self = Self(0x600E);

    // Natural-width read-only fields
    pub const EXIT_QUALIFICATION: Self = Self(0x6400);
    pub const IO_RCX: Self = Self(0x6402);
    pub const IO_RSI: Self = Self(0x6404);
    pub const IO_RDI: Self = Self(0x6406);
    pub const IO_RIP: Self = Self(0x6408);
    pub const GUEST_LINEAR_ADDRESS: Self = Self(0x640A);

    // Natural-width guest state
    pub const GUEST_CR0: Self = Self(0x6800);
    pub const GUEST_CR3: Self = Self(0x6802);
    pub const GUEST_CR4: Self = Self(0x6804);
    pub const GUEST_ES_BASE: Self = Self(0x6806);
    pub const GUEST_CS_BASE: Self = Self(0x6808);
    pub const GUEST_SS_BASE: Self = Self(0x680A);
    pub const GUEST_DS_BASE: Self = Self(0x680C);
    pub const GUEST_FS_BASE: Self = Self(0x680E);
    pub const GUEST_GS_BASE: Self = Self(0x6810);
    pub const GUEST_LDTR_BASE: Self = Self(0x6812);
    pub const GUEST_TR_BASE: Self = Self(0x6814);
    pub const GUEST_GDTR_BASE: Self = Self(0x6816);
    pub const GUEST_IDTR_BASE: Self = Self(0x6818);
    pub const GUEST_DR7: Self = Self(0x681A);
    pub const GUEST_RSP: Self = Self(0x681C);
    pub const GUEST_RIP: Self = Self(0x681E);
    pub const GUEST_RFLAGS: Self = Self(0x6820);
    pub const GUEST_PENDING_DBG_EXCEPTIONS: Self = Self(0x6822);
    pub const GUEST_SYSENTER_ESP: Self = Self(0x6824);
    pub const GUEST_SYSENTER_EIP: Self = Self(0x6826);

    // Natural-width host state
    pub const HOST_CR0: Self = Self(0x6C00);
    pub const HOST_CR3: Self = Self(0x6C02);
    pub const HOST_CR4: Self = Self(0x6C04);
    pub const HOST_FS_BASE: Self = Self(0x6C06);
    pub const HOST_GS_BASE: Self = Self(0x6C08);
    pub const HOST_TR_BASE: Self = Self(0x6C0A);
    pub const HOST_GDTR_BASE: Self = Self(0x6C0C);
    pub const HOST_IDTR_BASE: Self = Self(0x6C0E);
    pub const HOST_SYSENTER_ESP: Self = Self(0x6C10);
    pub const HOST_SYSENTER_EIP: Self = Self(0x6C12);
    pub const HOST_RSP: Self = Self(0x6C14);
    pub const HOST_RIP: Self = Self(0x6C16);

    /// Get the field width in bits
    pub fn width(&self) -> u32 {
        match (self.0 >> 13) & 0x3 {
            0 => 16,
            1 => 64,
            2 => 32,
            3 => 64, // Natural width (64 on x86_64)
            _ => unreachable!("2-bit mask value exceeded 0..=3"),
        }
    }

    /// Check if this is a read-only field
    pub fn is_read_only(&self) -> bool {
        ((self.0 >> 10) & 0x3) == 1
    }

    /// Get the access type
    pub fn access_type(&self) -> VmcsAccessType {
        match (self.0 >> 10) & 0x3 {
            0 => VmcsAccessType::Control,
            1 => VmcsAccessType::ReadOnly,
            2 => VmcsAccessType::GuestState,
            3 => VmcsAccessType::HostState,
            _ => unreachable!("2-bit mask value exceeded 0..=3"),
        }
    }
}

/// VMCS field access type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmcsAccessType {
    Control,
    ReadOnly,
    GuestState,
    HostState,
}

/// VM exit reason
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmExitReason(pub u32);

impl VmExitReason {
    pub const EXCEPTION_NMI: Self = Self(0);
    pub const EXTERNAL_INTERRUPT: Self = Self(1);
    pub const TRIPLE_FAULT: Self = Self(2);
    pub const INIT: Self = Self(3);
    pub const SIPI: Self = Self(4);
    pub const IO_SMI: Self = Self(5);
    pub const OTHER_SMI: Self = Self(6);
    pub const PENDING_INTERRUPT: Self = Self(7);
    pub const NMI_WINDOW: Self = Self(8);
    pub const TASK_SWITCH: Self = Self(9);
    pub const CPUID: Self = Self(10);
    pub const GETSEC: Self = Self(11);
    pub const HLT: Self = Self(12);
    pub const INVD: Self = Self(13);
    pub const INVLPG: Self = Self(14);
    pub const RDPMC: Self = Self(15);
    pub const RDTSC: Self = Self(16);
    pub const RSM: Self = Self(17);
    pub const VMCALL: Self = Self(18);
    pub const VMCLEAR: Self = Self(19);
    pub const VMLAUNCH: Self = Self(20);
    pub const VMPTRLD: Self = Self(21);
    pub const VMPTRST: Self = Self(22);
    pub const VMREAD: Self = Self(23);
    pub const VMRESUME: Self = Self(24);
    pub const VMWRITE: Self = Self(25);
    pub const VMXOFF: Self = Self(26);
    pub const VMXON: Self = Self(27);
    pub const CR_ACCESS: Self = Self(28);
    pub const DR_ACCESS: Self = Self(29);
    pub const IO_INSTRUCTION: Self = Self(30);
    pub const RDMSR: Self = Self(31);
    pub const WRMSR: Self = Self(32);
    pub const INVALID_GUEST_STATE: Self = Self(33);
    pub const MSR_LOADING: Self = Self(34);
    pub const MWAIT: Self = Self(36);
    pub const MONITOR_TRAP_FLAG: Self = Self(37);
    pub const MONITOR: Self = Self(39);
    pub const PAUSE: Self = Self(40);
    pub const MCE_DURING_ENTRY: Self = Self(41);
    pub const TPR_BELOW_THRESHOLD: Self = Self(43);
    pub const APIC_ACCESS: Self = Self(44);
    pub const VIRTUALIZED_EOI: Self = Self(45);
    pub const GDTR_IDTR_ACCESS: Self = Self(46);
    pub const LDTR_TR_ACCESS: Self = Self(47);
    pub const EPT_VIOLATION: Self = Self(48);
    pub const EPT_MISCONFIG: Self = Self(49);
    pub const INVEPT: Self = Self(50);
    pub const RDTSCP: Self = Self(51);
    pub const PREEMPTION_TIMER: Self = Self(52);
    pub const INVVPID: Self = Self(53);
    pub const WBINVD: Self = Self(54);
    pub const XSETBV: Self = Self(55);
    pub const APIC_WRITE: Self = Self(56);
    pub const RDRAND: Self = Self(57);
    pub const INVPCID: Self = Self(58);
    pub const VMFUNC: Self = Self(59);
    pub const ENCLS: Self = Self(60);
    pub const RDSEED: Self = Self(61);
    pub const PML_FULL: Self = Self(62);
    pub const XSAVES: Self = Self(63);
    pub const XRSTORS: Self = Self(64);
    pub const SPP_RELATED: Self = Self(66);
    pub const UMWAIT: Self = Self(67);
    pub const TPAUSE: Self = Self(68);

    /// Get the basic exit reason (low 16 bits)
    pub fn basic(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }

    /// Check if exit occurred during VM entry
    pub fn is_entry_failure(&self) -> bool {
        (self.0 >> 31) & 1 != 0
    }

    /// Check if exit is from VMX root operation
    pub fn from_vmx_root(&self) -> bool {
        (self.0 >> 29) & 1 != 0
    }
}

impl fmt::Display for VmExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.basic() {
            0 => "EXCEPTION_NMI",
            1 => "EXTERNAL_INTERRUPT",
            2 => "TRIPLE_FAULT",
            10 => "CPUID",
            12 => "HLT",
            18 => "VMCALL",
            19 => "VMCLEAR",
            20 => "VMLAUNCH",
            21 => "VMPTRLD",
            23 => "VMREAD",
            24 => "VMRESUME",
            25 => "VMWRITE",
            26 => "VMXOFF",
            27 => "VMXON",
            28 => "CR_ACCESS",
            30 => "IO_INSTRUCTION",
            31 => "RDMSR",
            32 => "WRMSR",
            48 => "EPT_VIOLATION",
            49 => "EPT_MISCONFIG",
            _ => "UNKNOWN",
        };
        write!(f, "{}({})", name, self.basic())
    }
}

/// VMX instruction error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VmxInstructionError {
    Success = 0,
    VmcallInRoot = 1,
    VmclearInvalidAddr = 2,
    VmclearWithVmxonPtr = 3,
    VmlaunchNonclearVmcs = 4,
    VmresumeNonlaunchedVmcs = 5,
    VmresumeAfterVmxoff = 6,
    VmEntryInvalidControlField = 7,
    VmEntryInvalidHostState = 8,
    VmptrldInvalidAddr = 9,
    VmptrldWithVmxonPtr = 10,
    VmptrldIncorrectVmcsRevision = 11,
    VmreadVmwriteUnsupportedField = 12,
    VmwriteReadonlyField = 13,
    VmxonInRoot = 15,
    VmEntryInvalidExecVmcsPtr = 16,
    VmEntryNonlaunchedExecVmcs = 17,
    VmEntryExecVmcsPtrNotVmxonPtr = 18,
    VmcallNonclearVmcs = 19,
    VmcallInvalidVmExitCtl = 20,
    VmcallIncorrectMsegRevision = 22,
    VmxoffUnderDualMonitor = 23,
    VmcallInvalidSmmFeatures = 24,
    VmEntryInvalidVmExecCtl = 25,
    VmEntryEventsBlockedMovSs = 26,
    InvalidOperandToInveptInvvpid = 28,
}

impl std::fmt::Display for VmxInstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "VMX success"),
            Self::VmcallInRoot => write!(f, "VMCALL executed in VMX root operation"),
            Self::VmclearInvalidAddr => write!(f, "VMCLEAR with invalid physical address"),
            Self::VmclearWithVmxonPtr => write!(f, "VMCLEAR with VMXON pointer"),
            Self::VmlaunchNonclearVmcs => write!(f, "VMLAUNCH with non-clear VMCS"),
            Self::VmresumeNonlaunchedVmcs => write!(f, "VMRESUME with non-launched VMCS"),
            Self::VmresumeAfterVmxoff => write!(f, "VMRESUME after VMXOFF"),
            Self::VmEntryInvalidControlField => write!(f, "VM entry with invalid control field(s)"),
            Self::VmEntryInvalidHostState => write!(f, "VM entry with invalid host-state field(s)"),
            Self::VmptrldInvalidAddr => write!(f, "VMPTRLD with invalid physical address"),
            Self::VmptrldWithVmxonPtr => write!(f, "VMPTRLD with VMXON pointer"),
            Self::VmptrldIncorrectVmcsRevision => write!(f, "VMPTRLD with incorrect VMCS revision"),
            Self::VmreadVmwriteUnsupportedField => write!(f, "VMREAD/VMWRITE from/to unsupported VMCS field"),
            Self::VmwriteReadonlyField => write!(f, "VMWRITE to read-only VMCS field"),
            Self::VmxonInRoot => write!(f, "VMXON executed in VMX root operation"),
            Self::VmEntryInvalidExecVmcsPtr => write!(f, "VM entry with invalid executive-VMCS pointer"),
            Self::VmEntryNonlaunchedExecVmcs => write!(f, "VM entry with non-launched executive VMCS"),
            Self::VmEntryExecVmcsPtrNotVmxonPtr => write!(f, "VM entry with executive-VMCS pointer not VMXON pointer"),
            Self::VmcallNonclearVmcs => write!(f, "VMCALL with non-clear VMCS"),
            Self::VmcallInvalidVmExitCtl => write!(f, "VMCALL with invalid VM-exit control fields"),
            Self::VmcallIncorrectMsegRevision => write!(f, "VMCALL with incorrect MSEG revision"),
            Self::VmxoffUnderDualMonitor => write!(f, "VMXOFF under dual-monitor treatment"),
            Self::VmcallInvalidSmmFeatures => write!(f, "VMCALL with invalid SMM-monitor features"),
            Self::VmEntryInvalidVmExecCtl => write!(f, "VM entry with invalid VM-execution control fields"),
            Self::VmEntryEventsBlockedMovSs => write!(f, "VM entry with events blocked by MOV SS"),
            Self::InvalidOperandToInveptInvvpid => write!(f, "invalid operand to INVEPT/INVVPID"),
        }
    }
}

impl std::error::Error for VmxInstructionError {}

/// Guest activity state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum GuestActivityState {
    #[default]
    Active = 0,
    Hlt = 1,
    Shutdown = 2,
    WaitForSipi = 3,
}

/// Guest interruptibility state bits
pub mod interruptibility {
    pub const BLOCKING_BY_STI: u32 = 1 << 0;
    pub const BLOCKING_BY_MOV_SS: u32 = 1 << 1;
    pub const BLOCKING_BY_SMI: u32 = 1 << 2;
    pub const BLOCKING_BY_NMI: u32 = 1 << 3;
    pub const ENCLAVE_INTERRUPTION: u32 = 1 << 4;
}

/// Nested virtualization level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NestedLevel {
    /// L0 - The physical hypervisor (us)
    #[default]
    L0,
    /// L1 - The guest hypervisor
    L1,
    /// L2 - The nested guest
    L2,
}

impl fmt::Display for NestedLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NestedLevel::L0 => write!(f, "L0"),
            NestedLevel::L1 => write!(f, "L1"),
            NestedLevel::L2 => write!(f, "L2"),
        }
    }
}

/// Nested guest state
#[derive(Debug, Clone, Default)]
pub struct NestedGuestState {
    /// Current nesting level
    pub level: NestedLevel,
    /// VMX operation enabled
    pub vmx_enabled: bool,
    /// VMXON region physical address
    pub vmxon_region: u64,
    /// Current VMCS pointer (L1's VMCS for L2)
    pub current_vmcs: u64,
    /// L1's VMCS12 (shadow VMCS)
    pub vmcs12_fields: HashMap<u32, u64>,
    /// L1 guest state saved during L2 entry
    pub l1_state: Option<SavedL1State>,
    /// VMX preemption timer
    pub preemption_timer: u64,
}

impl NestedGuestState {
    /// Create new nested guest state
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if VMX is enabled
    pub fn is_vmx_enabled(&self) -> bool {
        self.vmx_enabled
    }

    /// Check if in VMX non-root operation (L2)
    pub fn is_in_l2(&self) -> bool {
        self.level == NestedLevel::L2
    }

    /// Enable VMX operation
    pub fn enable_vmx(&mut self, vmxon_region: u64) {
        self.vmx_enabled = true;
        self.vmxon_region = vmxon_region;
        self.level = NestedLevel::L1;
    }

    /// Disable VMX operation
    pub fn disable_vmx(&mut self) {
        self.vmx_enabled = false;
        self.vmxon_region = 0;
        self.current_vmcs = 0;
        self.vmcs12_fields.clear();
        self.l1_state = None;
        self.level = NestedLevel::L0;
    }

    /// Set current VMCS
    pub fn set_current_vmcs(&mut self, vmcs_ptr: u64) {
        self.current_vmcs = vmcs_ptr;
    }

    /// Clear current VMCS
    pub fn clear_current_vmcs(&mut self) {
        self.current_vmcs = 0;
    }

    /// Enter L2 (nested guest)
    pub fn enter_l2(&mut self) {
        self.level = NestedLevel::L2;
    }

    /// Exit L2 back to L1
    pub fn exit_l2(&mut self) {
        self.level = NestedLevel::L1;
    }

    /// Read VMCS12 field
    pub fn read_vmcs12(&self, field: VmcsField) -> Option<u64> {
        self.vmcs12_fields.get(&field.0).copied()
    }

    /// Write VMCS12 field
    pub fn write_vmcs12(&mut self, field: VmcsField, value: u64) {
        self.vmcs12_fields.insert(field.0, value);
    }
}

/// Saved L1 state when entering L2
#[derive(Debug, Clone, Default)]
pub struct SavedL1State {
    /// General purpose registers
    pub gprs: [u64; 16],
    /// RIP
    pub rip: u64,
    /// RSP
    pub rsp: u64,
    /// RFLAGS
    pub rflags: u64,
    /// CR0
    pub cr0: u64,
    /// CR3
    pub cr3: u64,
    /// CR4
    pub cr4: u64,
    /// EFER MSR
    pub efer: u64,
    /// Segment selectors
    pub cs_selector: u16,
    pub ss_selector: u16,
    pub ds_selector: u16,
    pub es_selector: u16,
    pub fs_selector: u16,
    pub gs_selector: u16,
}

/// VMX instruction result
pub type VmxResult<T> = Result<T, VmxInstructionError>;

/// Nested virtualization statistics
#[derive(Debug, Clone, Default)]
pub struct NestedStats {
    /// Number of L2 entries
    pub l2_entries: u64,
    /// Number of L2 exits
    pub l2_exits: u64,
    /// Number of VMCS switches
    pub vmcs_switches: u64,
    /// Number of EPT violations in L2
    pub ept_violations: u64,
    /// Number of reflected exits
    pub reflected_exits: u64,
    /// Number of emulated VMX instructions
    pub emulated_vmx_instructions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vmx_capabilities_default() {
        let caps = VmxCapabilities::default_nested();
        assert!(caps.basic != 0);
        assert!(caps.supports_ept());
    }

    #[test]
    fn test_vmx_capabilities_features() {
        let caps = VmxCapabilities::default_nested();
        assert!(caps.supports_ept());
        assert!(caps.supports_vpid());
        assert!(caps.supports_unrestricted_guest());
    }

    #[test]
    fn test_vmcs_field_width() {
        assert_eq!(VmcsField::GUEST_CS_SELECTOR.width(), 16);
        assert_eq!(VmcsField::EPT_POINTER.width(), 64);
        assert_eq!(VmcsField::VM_EXIT_REASON.width(), 32);
        assert_eq!(VmcsField::GUEST_CR0.width(), 64);
    }

    #[test]
    fn test_vmcs_field_access_type() {
        assert_eq!(
            VmcsField::PIN_BASED_VM_EXEC_CONTROL.access_type(),
            VmcsAccessType::Control
        );
        assert_eq!(
            VmcsField::VM_EXIT_REASON.access_type(),
            VmcsAccessType::ReadOnly
        );
        assert_eq!(
            VmcsField::GUEST_CR0.access_type(),
            VmcsAccessType::GuestState
        );
        assert_eq!(VmcsField::HOST_CR0.access_type(), VmcsAccessType::HostState);
    }

    #[test]
    fn test_vmcs_field_is_read_only() {
        assert!(!VmcsField::GUEST_CR0.is_read_only());
        assert!(VmcsField::VM_EXIT_REASON.is_read_only());
        assert!(VmcsField::EXIT_QUALIFICATION.is_read_only());
    }

    #[test]
    fn test_vm_exit_reason_basic() {
        let reason = VmExitReason(10);
        assert_eq!(reason.basic(), 10);
        assert!(!reason.is_entry_failure());
    }

    #[test]
    fn test_vm_exit_reason_entry_failure() {
        let reason = VmExitReason(0x8000_0000 | 33);
        assert_eq!(reason.basic(), 33);
        assert!(reason.is_entry_failure());
    }

    #[test]
    fn test_vm_exit_reason_display() {
        assert_eq!(format!("{}", VmExitReason::CPUID), "CPUID(10)");
        assert_eq!(format!("{}", VmExitReason::HLT), "HLT(12)");
        assert_eq!(
            format!("{}", VmExitReason::EPT_VIOLATION),
            "EPT_VIOLATION(48)"
        );
    }

    #[test]
    fn test_nested_level_display() {
        assert_eq!(format!("{}", NestedLevel::L0), "L0");
        assert_eq!(format!("{}", NestedLevel::L1), "L1");
        assert_eq!(format!("{}", NestedLevel::L2), "L2");
    }

    #[test]
    fn test_nested_guest_state_creation() {
        let state = NestedGuestState::new();
        assert_eq!(state.level, NestedLevel::L0);
        assert!(!state.vmx_enabled);
    }

    #[test]
    fn test_nested_guest_state_enable_vmx() {
        let mut state = NestedGuestState::new();
        state.enable_vmx(0x1000);

        assert!(state.is_vmx_enabled());
        assert_eq!(state.vmxon_region, 0x1000);
        assert_eq!(state.level, NestedLevel::L1);
    }

    #[test]
    fn test_nested_guest_state_disable_vmx() {
        let mut state = NestedGuestState::new();
        state.enable_vmx(0x1000);
        state.disable_vmx();

        assert!(!state.is_vmx_enabled());
        assert_eq!(state.vmxon_region, 0);
        assert_eq!(state.level, NestedLevel::L0);
    }

    #[test]
    fn test_nested_guest_state_vmcs() {
        let mut state = NestedGuestState::new();
        state.enable_vmx(0x1000);
        state.set_current_vmcs(0x2000);

        assert_eq!(state.current_vmcs, 0x2000);

        state.clear_current_vmcs();
        assert_eq!(state.current_vmcs, 0);
    }

    #[test]
    fn test_nested_guest_state_l2_entry_exit() {
        let mut state = NestedGuestState::new();
        state.enable_vmx(0x1000);

        let l1_state = SavedL1State {
            rip: 0x1234,
            ..Default::default()
        };

        state.enter_l2();
        assert!(state.is_in_l2());
        assert_eq!(state.level, NestedLevel::L2);

        state.exit_l2();
        assert!(!state.is_in_l2());
        assert_eq!(state.level, NestedLevel::L1);
    }

    #[test]
    fn test_nested_guest_state_vmcs12() {
        let mut state = NestedGuestState::new();
        state.enable_vmx(0x1000);

        state.write_vmcs12(VmcsField::GUEST_CR0, 0x8000_0011);
        assert_eq!(state.read_vmcs12(VmcsField::GUEST_CR0), Some(0x8000_0011));
        assert_eq!(state.read_vmcs12(VmcsField::GUEST_CR3), None);
    }

    #[test]
    fn test_guest_activity_state() {
        assert_eq!(GuestActivityState::Active as u32, 0);
        assert_eq!(GuestActivityState::Hlt as u32, 1);
        assert_eq!(GuestActivityState::Shutdown as u32, 2);
    }

    #[test]
    fn test_interruptibility_bits() {
        assert_eq!(interruptibility::BLOCKING_BY_STI, 1);
        assert_eq!(interruptibility::BLOCKING_BY_MOV_SS, 2);
        assert_eq!(interruptibility::BLOCKING_BY_NMI, 8);
    }

    #[test]
    fn test_nested_stats_default() {
        let stats = NestedStats::default();
        assert_eq!(stats.l2_entries, 0);
        assert_eq!(stats.l2_exits, 0);
    }

    #[test]
    fn test_saved_l1_state() {
        let state = SavedL1State {
            rip: 0x1000,
            rsp: 0x2000,
            cr0: 0x8000_0011,
            cr3: 0x3000,
            cr4: 0x2620,
            ..Default::default()
        };

        assert_eq!(state.rip, 0x1000);
        assert_eq!(state.cr0, 0x8000_0011);
    }
}
