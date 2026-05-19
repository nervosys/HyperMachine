//! AMD-V (SVM) support for Type-1 hypervisor
//!
//! This module implements AMD's Secure Virtual Machine (SVM) for
//! hardware-assisted virtualization. SVM provides:
//!
//! - VMCB (Virtual Machine Control Block) management
//! - VM entry and exit handling
//! - NPT (Nested Page Tables) for memory virtualization
//! - ASID (Address Space Identifiers) for TLB management
//! - Intercept configuration

use crate::vcpu::{GeneralRegisters, VcpuRegisters, VmExitInfo};
use crate::{Error, Result};
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

/// SVM-related MSRs
mod msr {
    pub const VM_CR: u32 = 0xC0010114;
    pub const VM_HSAVE_PA: u32 = 0xC0010117;
    pub const EFER: u32 = 0xC0000080;
}

/// EFER MSR bits
mod efer {
    pub const SVME: u64 = 1 << 12;
}

/// VM_CR MSR bits
mod vm_cr {
    pub const SVM_DISABLE: u64 = 1 << 4;
}

/// SVM is enabled on this CPU
static SVM_ENABLED: AtomicBool = AtomicBool::new(false);

/// VMCB clean field bits
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmcbCleanBits {
    Intercepts = 1 << 0,
    Iopm = 1 << 1,
    Asid = 1 << 2,
    Tpr = 1 << 3,
    Np = 1 << 4,
    Crx = 1 << 5,
    Drx = 1 << 6,
    Dt = 1 << 7,
    Seg = 1 << 8,
    Cr2 = 1 << 9,
    Lbr = 1 << 10,
    Avic = 1 << 11,
    All = 0xFFF,
}

/// VM exit codes
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmExitCode {
    ReadCr0 = 0x00,
    ReadCr2 = 0x02,
    ReadCr3 = 0x03,
    ReadCr4 = 0x04,
    ReadCr8 = 0x08,
    WriteCr0 = 0x10,
    WriteCr2 = 0x12,
    WriteCr3 = 0x13,
    WriteCr4 = 0x14,
    WriteCr8 = 0x18,
    ReadDr0 = 0x20,
    ReadDr1 = 0x21,
    ReadDr2 = 0x22,
    ReadDr3 = 0x23,
    ReadDr4 = 0x24,
    ReadDr5 = 0x25,
    ReadDr6 = 0x26,
    ReadDr7 = 0x27,
    WriteDr0 = 0x30,
    WriteDr1 = 0x31,
    WriteDr2 = 0x32,
    WriteDr3 = 0x33,
    WriteDr4 = 0x34,
    WriteDr5 = 0x35,
    WriteDr6 = 0x36,
    WriteDr7 = 0x37,
    ExceptionDe = 0x40,
    ExceptionDb = 0x41,
    ExceptionNmi = 0x42,
    ExceptionBp = 0x43,
    ExceptionOf = 0x44,
    ExceptionBr = 0x45,
    ExceptionUd = 0x46,
    ExceptionNm = 0x47,
    ExceptionDf = 0x48,
    ExceptionTs = 0x4A,
    ExceptionNp = 0x4B,
    ExceptionSs = 0x4C,
    ExceptionGp = 0x4D,
    ExceptionPf = 0x4E,
    ExceptionMf = 0x50,
    ExceptionAc = 0x51,
    ExceptionMc = 0x52,
    ExceptionXf = 0x53,
    Intr = 0x60,
    Nmi = 0x61,
    Smi = 0x62,
    Init = 0x63,
    Vintr = 0x64,
    Cr0SelWrite = 0x65,
    IdtrRead = 0x66,
    GdtrRead = 0x67,
    LdtrRead = 0x68,
    TrRead = 0x69,
    IdtrWrite = 0x6A,
    GdtrWrite = 0x6B,
    LdtrWrite = 0x6C,
    TrWrite = 0x6D,
    Rdtsc = 0x6E,
    Rdpmc = 0x6F,
    Pushf = 0x70,
    Popf = 0x71,
    Cpuid = 0x72,
    Rsm = 0x73,
    Iret = 0x74,
    Swint = 0x75,
    Invd = 0x76,
    Pause = 0x77,
    Hlt = 0x78,
    Invlpg = 0x79,
    Invlpga = 0x7A,
    IoIo = 0x7B,
    Msr = 0x7C,
    TaskSwitch = 0x7D,
    FerrFreeze = 0x7E,
    Shutdown = 0x7F,
    Vmrun = 0x80,
    Vmmcall = 0x81,
    Vmload = 0x82,
    Vmsave = 0x83,
    Stgi = 0x84,
    Clgi = 0x85,
    Skinit = 0x86,
    Rdtscp = 0x87,
    Icebp = 0x88,
    Wbinvd = 0x89,
    Monitor = 0x8A,
    Mwait = 0x8B,
    MwaitConditional = 0x8C,
    Xsetbv = 0x8D,
    Rdpru = 0x8E,
    Efer = 0x8F,
    Cr0 = 0x90,
    Cr1 = 0x91,
    Cr2 = 0x92,
    Cr3 = 0x93,
    Cr4 = 0x94,
    Cr5 = 0x95,
    Cr6 = 0x96,
    Cr7 = 0x97,
    Cr8 = 0x98,
    Cr9 = 0x99,
    Cr10 = 0x9A,
    Cr11 = 0x9B,
    Cr12 = 0x9C,
    Cr13 = 0x9D,
    Cr14 = 0x9E,
    Cr15 = 0x9F,
    NptFault = 0x400,
    AvicIncompleteIpi = 0x401,
    AvicNoAccel = 0x402,
    VmgExit = 0x403,
    Invalid = u64::MAX,
}

impl From<u64> for VmExitCode {
    fn from(value: u64) -> Self {
        match value {
            0x72 => VmExitCode::Cpuid,
            0x78 => VmExitCode::Hlt,
            0x7B => VmExitCode::IoIo,
            0x7C => VmExitCode::Msr,
            0x7F => VmExitCode::Shutdown,
            0x81 => VmExitCode::Vmmcall,
            0x400 => VmExitCode::NptFault,
            _ => VmExitCode::Invalid,
        }
    }
}

/// VMCB control area
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VmcbControlArea {
    /// Intercept reads of CR0-15
    pub intercept_cr_read: u16,
    /// Intercept writes to CR0-15
    pub intercept_cr_write: u16,
    /// Intercept reads of DR0-15
    pub intercept_dr_read: u16,
    /// Intercept writes to DR0-15
    pub intercept_dr_write: u16,
    /// Intercept exception vectors 0-31
    pub intercept_exceptions: u32,
    /// Intercept various instructions (word 1)
    pub intercept_instr1: u32,
    /// Intercept various instructions (word 2)
    pub intercept_instr2: u32,
    /// Reserved
    pub reserved1: [u8; 40],
    /// Pause filter threshold
    pub pause_filter_threshold: u16,
    /// Pause filter count
    pub pause_filter_count: u16,
    /// Physical address of IOPM
    pub iopm_base_pa: u64,
    /// Physical address of MSRPM
    pub msrpm_base_pa: u64,
    /// TSC offset
    pub tsc_offset: u64,
    /// Guest ASID
    pub guest_asid: u32,
    /// TLB control
    pub tlb_control: u8,
    /// Reserved
    pub reserved2: [u8; 3],
    /// Virtual interrupt control
    pub v_intr: u64,
    /// Interrupt shadow
    pub interrupt_shadow: u64,
    /// Exit code
    pub exit_code: u64,
    /// Exit info 1
    pub exit_info1: u64,
    /// Exit info 2
    pub exit_info2: u64,
    /// Exit interrupt info
    pub exit_int_info: u64,
    /// Nested paging enable and other controls
    pub np_enable: u64,
    /// AVIC APIC BAR
    pub avic_apic_bar: u64,
    /// Guest PA of GHCB
    pub ghcb_pa: u64,
    /// Event injection
    pub event_inject: u64,
    /// Nested CR3 (NCR3)
    pub n_cr3: u64,
    /// LBR virtualization enable
    pub lbr_virt_enable: u64,
    /// VMCB clean bits
    pub vmcb_clean: u32,
    /// Reserved
    pub reserved3: u32,
    /// Next RIP (for single-stepping)
    pub next_rip: u64,
    /// Number of bytes fetched
    pub num_bytes_fetched: u8,
    /// Guest instruction bytes
    pub guest_instr_bytes: [u8; 15],
    /// AVIC backing page pointer
    pub avic_backing_page: u64,
    /// Reserved
    pub reserved4: u64,
    /// AVIC logical table pointer
    pub avic_logical_table: u64,
    /// AVIC physical table pointer
    pub avic_physical_table: u64,
    /// Reserved
    pub reserved5: u64,
    /// VMCB save state pointer (for nested)
    pub vmsa_pa: u64,
    /// Reserved padding to 1024 bytes
    pub reserved6: [u8; 752],
}

impl Default for VmcbControlArea {
    fn default() -> Self {
        Self {
            intercept_cr_read: 0,
            intercept_cr_write: 0,
            intercept_dr_read: 0,
            intercept_dr_write: 0,
            intercept_exceptions: 0,
            intercept_instr1: 0,
            intercept_instr2: 0,
            reserved1: [0; 40],
            pause_filter_threshold: 0,
            pause_filter_count: 0,
            iopm_base_pa: 0,
            msrpm_base_pa: 0,
            tsc_offset: 0,
            guest_asid: 0,
            tlb_control: 0,
            reserved2: [0; 3],
            v_intr: 0,
            interrupt_shadow: 0,
            exit_code: 0,
            exit_info1: 0,
            exit_info2: 0,
            exit_int_info: 0,
            np_enable: 0,
            avic_apic_bar: 0,
            ghcb_pa: 0,
            event_inject: 0,
            n_cr3: 0,
            lbr_virt_enable: 0,
            vmcb_clean: 0,
            reserved3: 0,
            next_rip: 0,
            num_bytes_fetched: 0,
            guest_instr_bytes: [0; 15],
            avic_backing_page: 0,
            reserved4: 0,
            avic_logical_table: 0,
            avic_physical_table: 0,
            reserved5: 0,
            vmsa_pa: 0,
            reserved6: [0; 752],
        }
    }
}

/// VMCB save state area
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VmcbSaveArea {
    /// ES segment
    pub es: SegmentRegister,
    /// CS segment
    pub cs: SegmentRegister,
    /// SS segment
    pub ss: SegmentRegister,
    /// DS segment
    pub ds: SegmentRegister,
    /// FS segment
    pub fs: SegmentRegister,
    /// GS segment
    pub gs: SegmentRegister,
    /// GDTR
    pub gdtr: SegmentRegister,
    /// LDTR
    pub ldtr: SegmentRegister,
    /// IDTR
    pub idtr: SegmentRegister,
    /// TR
    pub tr: SegmentRegister,
    /// Reserved
    pub reserved1: [u8; 43],
    /// CPL
    pub cpl: u8,
    /// Reserved
    pub reserved2: [u8; 4],
    /// EFER
    pub efer: u64,
    /// Reserved
    pub reserved3: [u8; 112],
    /// CR4
    pub cr4: u64,
    /// CR3
    pub cr3: u64,
    /// CR0
    pub cr0: u64,
    /// DR7
    pub dr7: u64,
    /// DR6
    pub dr6: u64,
    /// RFLAGS
    pub rflags: u64,
    /// RIP
    pub rip: u64,
    /// Reserved
    pub reserved4: [u8; 88],
    /// RSP
    pub rsp: u64,
    /// Reserved
    pub reserved5: [u8; 24],
    /// RAX
    pub rax: u64,
    /// STAR
    pub star: u64,
    /// LSTAR
    pub lstar: u64,
    /// CSTAR
    pub cstar: u64,
    /// SFMASK
    pub sfmask: u64,
    /// Kernel GS base
    pub kernel_gs_base: u64,
    /// SYSENTER CS
    pub sysenter_cs: u64,
    /// SYSENTER ESP
    pub sysenter_esp: u64,
    /// SYSENTER EIP
    pub sysenter_eip: u64,
    /// CR2
    pub cr2: u64,
    /// Reserved
    pub reserved6: [u8; 32],
    /// Guest PAT
    pub g_pat: u64,
    /// Debug control
    pub dbg_ctl: u64,
    /// BR from
    pub br_from: u64,
    /// BR to
    pub br_to: u64,
    /// Last exception from
    pub last_excp_from: u64,
    /// Last exception to
    pub last_excp_to: u64,
    /// Reserved padding to 2048 bytes
    pub reserved7: [u8; 2408],
}

impl Default for VmcbSaveArea {
    fn default() -> Self {
        Self {
            es: SegmentRegister::default(),
            cs: SegmentRegister::default(),
            ss: SegmentRegister::default(),
            ds: SegmentRegister::default(),
            fs: SegmentRegister::default(),
            gs: SegmentRegister::default(),
            gdtr: SegmentRegister::default(),
            ldtr: SegmentRegister::default(),
            idtr: SegmentRegister::default(),
            tr: SegmentRegister::default(),
            reserved1: [0; 43],
            cpl: 0,
            reserved2: [0; 4],
            efer: 0,
            reserved3: [0; 112],
            cr4: 0,
            cr3: 0,
            cr0: 0,
            dr7: 0,
            dr6: 0,
            rflags: 0,
            rip: 0,
            reserved4: [0; 88],
            rsp: 0,
            reserved5: [0; 24],
            rax: 0,
            star: 0,
            lstar: 0,
            cstar: 0,
            sfmask: 0,
            kernel_gs_base: 0,
            sysenter_cs: 0,
            sysenter_esp: 0,
            sysenter_eip: 0,
            cr2: 0,
            reserved6: [0; 32],
            g_pat: 0,
            dbg_ctl: 0,
            br_from: 0,
            br_to: 0,
            last_excp_from: 0,
            last_excp_to: 0,
            reserved7: [0; 2408],
        }
    }
}

/// Segment register in VMCB
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentRegister {
    /// Segment selector
    pub selector: u16,
    /// Segment attributes
    pub attrib: u16,
    /// Segment limit
    pub limit: u32,
    /// Segment base
    pub base: u64,
}

/// VMCB (Virtual Machine Control Block)
#[repr(C, align(4096))]
pub struct Vmcb {
    /// Control area
    pub control: VmcbControlArea,
    /// Save state area
    pub save: VmcbSaveArea,
}

impl Vmcb {
    /// Create a new VMCB
    pub fn new() -> Self {
        Self {
            control: VmcbControlArea::default(),
            save: VmcbSaveArea::default(),
        }
    }
}

impl Default for Vmcb {
    fn default() -> Self {
        Self::new()
    }
}

/// Host save area (4KB aligned)
#[repr(C, align(4096))]
pub struct HostSaveArea {
    /// Host state data
    pub data: [u8; 4096],
}

impl HostSaveArea {
    /// Create a new host save area
    pub fn new() -> Self {
        Self { data: [0; 4096] }
    }
}

impl Default for HostSaveArea {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if SVM is supported
pub fn is_supported() -> bool {
    let cpuid = raw_cpuid::CpuId::new();

    // Check CPUID.80000001H:ECX[SVM]
    cpuid
        .get_extended_processor_and_feature_identifiers()
        .map(|f| f.has_svm())
        .unwrap_or(false)
}

/// Check if SVM is disabled by BIOS
pub fn is_disabled_by_bios() -> bool {
    // SAFETY: Reading the VM_CR MSR is valid on AMD CPUs with SVM support
    // (confirmed by `is_supported()` check at the call site).
    unsafe {
        let vm_cr = x86::msr::rdmsr(msr::VM_CR);
        vm_cr & vm_cr::SVM_DISABLE != 0
    }
}

/// Initialize SVM on the current CPU
pub fn initialize() -> Result<()> {
    if !is_supported() {
        return Err(Error::NoHardwareSupport);
    }

    if is_disabled_by_bios() {
        return Err(Error::SvmInitFailed);
    }

    // Enable SVM by setting EFER.SVME
    // SAFETY: SVM support was confirmed above and BIOS has not locked it out.
    // Reading and writing the EFER MSR is valid on AMD CPUs; we only set the
    // SVME bit while preserving all other bits.
    unsafe {
        let efer = x86::msr::rdmsr(msr::EFER);
        x86::msr::wrmsr(msr::EFER, efer | efer::SVME);
    }

    SVM_ENABLED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Set the host save area physical address
pub unsafe fn set_host_save_area(host_save_area: &HostSaveArea) -> Result<()> {
    let addr = host_save_area as *const _ as u64;
    x86::msr::wrmsr(msr::VM_HSAVE_PA, addr);
    Ok(())
}

/// Execute VMRUN instruction
pub unsafe fn vmrun(vmcb: &mut Vmcb) -> Result<()> {
    let vmcb_pa = vmcb as *mut _ as u64;

    asm!(
        "vmrun",
        in("rax") vmcb_pa,
        options(nostack)
    );

    Ok(())
}

/// Execute VMSAVE instruction
pub unsafe fn vmsave(vmcb: &Vmcb) {
    let vmcb_pa = vmcb as *const _ as u64;

    asm!(
        "vmsave",
        in("rax") vmcb_pa,
        options(nostack)
    );
}

/// Execute VMLOAD instruction
pub unsafe fn vmload(vmcb: &Vmcb) {
    let vmcb_pa = vmcb as *const _ as u64;

    asm!(
        "vmload",
        in("rax") vmcb_pa,
        options(nostack)
    );
}

/// Execute STGI instruction (set global interrupt flag)
pub unsafe fn stgi() {
    asm!("stgi", options(nostack));
}

/// Execute CLGI instruction (clear global interrupt flag)
pub unsafe fn clgi() {
    asm!("clgi", options(nostack));
}

/// Check if SVM is enabled
pub fn is_enabled() -> bool {
    SVM_ENABLED.load(Ordering::SeqCst)
}

// ─── SVM Intercept bit constants ────────────────────────────────────

/// Instruction intercept word 1 bits
pub mod intercept1 {
    pub const INTR: u32 = 1 << 0;
    pub const NMI: u32 = 1 << 1;
    pub const SMI: u32 = 1 << 2;
    pub const INIT: u32 = 1 << 3;
    pub const VINTR: u32 = 1 << 4;
    pub const CR0_SEL_WRITE: u32 = 1 << 5;
    pub const IDTR_READ: u32 = 1 << 6;
    pub const GDTR_READ: u32 = 1 << 7;
    pub const LDTR_READ: u32 = 1 << 8;
    pub const TR_READ: u32 = 1 << 9;
    pub const IDTR_WRITE: u32 = 1 << 10;
    pub const GDTR_WRITE: u32 = 1 << 11;
    pub const LDTR_WRITE: u32 = 1 << 12;
    pub const TR_WRITE: u32 = 1 << 13;
    pub const RDTSC: u32 = 1 << 14;
    pub const RDPMC: u32 = 1 << 15;
    pub const PUSHF: u32 = 1 << 16;
    pub const POPF: u32 = 1 << 17;
    pub const CPUID: u32 = 1 << 18;
    pub const RSM: u32 = 1 << 19;
    pub const IRET: u32 = 1 << 20;
    pub const SWINT: u32 = 1 << 21;
    pub const INVD: u32 = 1 << 22;
    pub const PAUSE: u32 = 1 << 23;
    pub const HLT: u32 = 1 << 24;
    pub const INVLPG: u32 = 1 << 25;
    pub const INVLPGA: u32 = 1 << 26;
    pub const IOIO: u32 = 1 << 27;
    pub const MSR: u32 = 1 << 28;
    pub const TASK_SWITCH: u32 = 1 << 29;
    pub const FERR_FREEZE: u32 = 1 << 30;
    pub const SHUTDOWN: u32 = 1u32 << 31;
}

/// Instruction intercept word 2 bits
pub mod intercept2 {
    pub const VMRUN: u32 = 1 << 0;
    pub const VMMCALL: u32 = 1 << 1;
    pub const VMLOAD: u32 = 1 << 2;
    pub const VMSAVE: u32 = 1 << 3;
    pub const STGI: u32 = 1 << 4;
    pub const CLGI: u32 = 1 << 5;
    pub const SKINIT: u32 = 1 << 6;
    pub const RDTSCP: u32 = 1 << 7;
    pub const ICEBP: u32 = 1 << 8;
    pub const WBINVD: u32 = 1 << 9;
    pub const MONITOR: u32 = 1 << 10;
    pub const MWAIT: u32 = 1 << 11;
    pub const MWAIT_CONDITIONAL: u32 = 1 << 12;
    pub const XSETBV: u32 = 1 << 13;
}

/// Nested paging enable bit (in np_enable field)
pub const NP_ENABLE: u64 = 1 << 0;

/// Event injection encoding bits
pub mod event_inject {
    /// External interrupt
    pub const TYPE_INTR: u64 = 0 << 8;
    /// NMI
    pub const TYPE_NMI: u64 = 2 << 8;
    /// Exception
    pub const TYPE_EXCEPTION: u64 = 3 << 8;
    /// Software interrupt
    pub const TYPE_SOFT: u64 = 4 << 8;
    /// Error code valid
    pub const ERROR_CODE_VALID: u64 = 1 << 11;
    /// Valid
    pub const VALID: u64 = 1u64 << 31;
}

// ─── VMCB setup ─────────────────────────────────────────────────────

/// Configure VMCB intercepts and control fields for a standard guest.
///
/// Sets up intercepts for CPUID, HLT, I/O, VMRUN, VMMCALL, MSR,
/// shutdown, and NPT faults. Enables nested paging.
pub fn setup_vmcb_controls(vmcb: &mut Vmcb, ncr3: u64, asid: u32) {
    let ctrl = &mut vmcb.control;

    // Intercept CPUID, HLT, I/O, MSR, SHUTDOWN
    ctrl.intercept_instr1 = intercept1::CPUID
        | intercept1::HLT
        | intercept1::IOIO
        | intercept1::MSR
        | intercept1::SHUTDOWN;

    // Intercept VMRUN (mandatory) and VMMCALL
    ctrl.intercept_instr2 = intercept2::VMRUN | intercept2::VMMCALL;

    // Guest ASID (must be non-zero)
    ctrl.guest_asid = if asid == 0 { 1 } else { asid };

    // Enable nested paging
    ctrl.np_enable = NP_ENABLE;
    ctrl.n_cr3 = ncr3;

    // Mark all VMCB fields as dirty (not clean)
    ctrl.vmcb_clean = 0;
}

/// Populate VMCB guest save-area from `VcpuRegisters`.
pub fn setup_vmcb_guest_state(vmcb: &mut Vmcb, regs: &VcpuRegisters) {
    let save = &mut vmcb.save;

    // Segment registers
    save.cs = SegmentRegister {
        selector: regs.seg.cs.selector,
        attrib: regs.seg.cs.attributes,
        limit: regs.seg.cs.limit,
        base: regs.seg.cs.base,
    };
    save.ss = SegmentRegister {
        selector: regs.seg.ss.selector,
        attrib: regs.seg.ss.attributes,
        limit: regs.seg.ss.limit,
        base: regs.seg.ss.base,
    };
    save.ds = SegmentRegister {
        selector: regs.seg.ds.selector,
        attrib: regs.seg.ds.attributes,
        limit: regs.seg.ds.limit,
        base: regs.seg.ds.base,
    };
    save.es = SegmentRegister {
        selector: regs.seg.es.selector,
        attrib: regs.seg.es.attributes,
        limit: regs.seg.es.limit,
        base: regs.seg.es.base,
    };
    save.fs = SegmentRegister {
        selector: regs.seg.fs.selector,
        attrib: regs.seg.fs.attributes,
        limit: regs.seg.fs.limit,
        base: regs.seg.fs.base,
    };
    save.gs = SegmentRegister {
        selector: regs.seg.gs.selector,
        attrib: regs.seg.gs.attributes,
        limit: regs.seg.gs.limit,
        base: regs.seg.gs.base,
    };
    save.gdtr = SegmentRegister {
        selector: 0,
        attrib: 0,
        limit: regs.dt.gdtr_limit as u32,
        base: regs.dt.gdtr_base,
    };
    save.idtr = SegmentRegister {
        selector: 0,
        attrib: 0,
        limit: regs.dt.idtr_limit as u32,
        base: regs.dt.idtr_base,
    };
    save.ldtr = SegmentRegister {
        selector: regs.seg.ldtr.selector,
        attrib: regs.seg.ldtr.attributes,
        limit: regs.seg.ldtr.limit,
        base: regs.seg.ldtr.base,
    };
    save.tr = SegmentRegister {
        selector: regs.seg.tr.selector,
        attrib: regs.seg.tr.attributes,
        limit: regs.seg.tr.limit,
        base: regs.seg.tr.base,
    };

    // Control registers
    save.cr0 = regs.cr.cr0;
    save.cr2 = regs.cr.cr2;
    save.cr3 = regs.cr.cr3;
    save.cr4 = regs.cr.cr4;
    save.efer = regs.cr.efer;

    // Debug registers
    save.dr6 = regs.dr.dr6;
    save.dr7 = regs.dr.dr7;

    // GPRs stored in VMCB save area
    save.rax = regs.gp.rax;
    save.rip = regs.gp.rip;
    save.rsp = regs.gp.rsp;
    save.rflags = regs.gp.rflags;

    // PAT (default value)
    save.g_pat = 0x0007_0406_0007_0406;
}

/// Sync guest state from VMCB save-area back into `VcpuRegisters` after a VM exit.
pub fn sync_vmcb_to_registers(vmcb: &Vmcb, regs: &mut VcpuRegisters) {
    let save = &vmcb.save;

    regs.gp.rax = save.rax;
    regs.gp.rip = save.rip;
    regs.gp.rsp = save.rsp;
    regs.gp.rflags = save.rflags;

    regs.cr.cr0 = save.cr0;
    regs.cr.cr2 = save.cr2;
    regs.cr.cr3 = save.cr3;
    regs.cr.cr4 = save.cr4;
    regs.cr.efer = save.efer;
}

/// Read VM-exit information from the VMCB control area.
pub fn read_exit_info(vmcb: &Vmcb) -> VmExitInfo {
    let ctrl = &vmcb.control;
    let exit_code = ctrl.exit_code;

    // Map SVM exit codes to a generic reason u32
    let reason = exit_code as u32;

    let guest_physical_addr = if exit_code == VmExitCode::NptFault as u64 {
        Some(ctrl.exit_info2)
    } else {
        None
    };

    VmExitInfo {
        reason,
        qualification: ctrl.exit_info1,
        guest_physical_addr,
        guest_linear_addr: Some(ctrl.exit_info2),
        instruction_length: ctrl.next_rip.wrapping_sub(vmcb.save.rip) as u32,
        instruction_info: 0,
    }
}

/// Execute VMRUN with full GP register save/restore.
///
/// Saves host callee-saved registers, loads guest GP registers into CPU,
/// executes VMRUN, then saves guest GP registers and restores host.
/// RAX is saved/restored via the VMCB save area.
///
/// # Safety
/// VMCB must be fully configured. `regs` must be valid.
pub unsafe fn svm_run(vmcb: &mut Vmcb, regs: &mut GeneralRegisters) -> Result<()> {
    // RAX goes via VMCB save area (VMRUN uses RAX for VMCB physical address)
    vmcb.save.rax = regs.rax;

    let vmcb_pa = vmcb as *mut Vmcb as u64;

    asm!(
        // Save host callee-saved
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "push rdi",   // GeneralRegisters pointer
        "push rsi",   // VMCB PA

        // Load guest GP registers from struct (rdi = regs pointer)
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
        "mov rdi, [rdi + 0x28]",   // guest rdi last

        // rax = VMCB PA (from stack)
        "mov rax, [rsp]",

        // Enter guest
        "vmrun",

        // VM exit: save guest registers
        // Recover regs pointer from stack (rsp+8 since rsp+0 = VMCB PA, rsp+8 = rdi=regs)
        "push rdi",            // save guest rdi temporarily
        "mov rdi, [rsp + 16]", // regs pointer (pushed as third from top)

        "mov [rdi + 0x08], rbx",
        "mov [rdi + 0x10], rcx",
        "mov [rdi + 0x18], rdx",
        "mov [rdi + 0x20], rsi",
        "pop QWORD PTR [rdi + 0x28]", // guest rdi
        "mov [rdi + 0x30], rbp",
        "mov [rdi + 0x40], r8",
        "mov [rdi + 0x48], r9",
        "mov [rdi + 0x50], r10",
        "mov [rdi + 0x58], r11",
        "mov [rdi + 0x60], r12",
        "mov [rdi + 0x68], r13",
        "mov [rdi + 0x70], r14",
        "mov [rdi + 0x78], r15",

        // Restore host callee-saved
        "pop rsi",
        "pop rdi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",

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

    // RAX comes back via VMCB save area
    regs.rax = vmcb.save.rax;

    Ok(())
}

/// Inject an interrupt into the guest via the VMCB event_inject field.
pub fn inject_interrupt(vmcb: &mut Vmcb, vector: u8, is_nmi: bool) {
    let int_type = if is_nmi {
        event_inject::TYPE_NMI
    } else {
        event_inject::TYPE_INTR
    };
    vmcb.control.event_inject = (vector as u64) | int_type | event_inject::VALID;
}

/// Inject a hardware exception into the guest via the VMCB event_inject field.
pub fn inject_exception(vmcb: &mut Vmcb, vector: u8, error_code: Option<u32>) {
    let mut val = (vector as u64) | event_inject::TYPE_EXCEPTION | event_inject::VALID;
    if let Some(code) = error_code {
        val |= event_inject::ERROR_CODE_VALID;
        val |= (code as u64) << 32;
    }
    vmcb.control.event_inject = val;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vmcb_size() {
        assert!(core::mem::size_of::<Vmcb>() >= 4096);
    }

    #[test]
    fn test_vmcb_control_area_size() {
        assert_eq!(core::mem::size_of::<VmcbControlArea>(), 1024);
    }

    #[test]
    fn test_vm_exit_code_from() {
        assert_eq!(VmExitCode::from(0x72), VmExitCode::Cpuid);
        assert_eq!(VmExitCode::from(0x78), VmExitCode::Hlt);
        assert_eq!(VmExitCode::from(0x400), VmExitCode::NptFault);
    }
}
