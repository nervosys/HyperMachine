//! Run a guest under `hv1-core`, which it had never done.
//!
//! `initialize()` proved the hypervisor starts. Starting is not hosting: a
//! hypervisor that enables SVM and never enters a guest has exercised one MSR
//! write. What is under test here is `VMRUN` itself — a VMCB the hardware
//! accepts, a guest that executes, an exit that says why, and a second entry
//! afterwards, because one entry proves a guest ran and two prove a loop.
//!
//! # The guest
//!
//! Three bytes, in real mode, chosen so that each exit is unambiguous:
//!
//! ```text
//!   0F 01 D9   vmmcall    -> VMEXIT_VMMCALL (0x81). Nothing else produces it.
//!   F4         hlt        -> VMEXIT_HLT     (0x78)
//! ```
//!
//! Real mode, not long mode, because a real-mode guest needs no page tables of
//! its own: `CR0.PE` is clear, `CS.base` points at the code, and `RIP` starts
//! at zero. Nested paging does the guest-physical to host-physical half.
//!
//! # The nested page tables, and the bit that is easy to miss
//!
//! The first attempt handed the guest the page tables the trampoline had
//! already built. This image is identity mapped, so guest-physical and
//! host-physical are the same address, and it looked like the map was free.
//! Every entry into the guest exited immediately with `VMEXIT_NPF` — a nested
//! page fault on the very first instruction fetch, at an address that was
//! plainly mapped.
//!
//! A nested page walk is performed as a *user-mode* access, whatever the
//! guest's own privilege level. So every level of the nested tables needs the
//! U/S bit, and the host's tables do not have it — nothing in a kernel's own
//! map is user-accessible, which is the entire point of that bit. Tables built
//! for the host are therefore never usable as nested tables, however correct
//! their addresses are.
//!
//! These are built separately for that reason, and would be anyway: a
//! hypervisor that hands a guest its own page tables has given the guest a map
//! of the hypervisor.
//!
//! # Why the intercepts are set by hand
//!
//! `svm::setup_vmcb_controls` is the crate's own helper and sets `IOIO` and
//! `MSR` intercepts. Both of those require their bitmaps: with `INTERCEPT_IOIO`
//! set, the hardware reads a 12 KiB I/O permission map at `IOPM_BASE_PA`, and
//! with `INTERCEPT_MSR` an 8 KiB map at `MSRPM_BASE_PA`. The helper sets neither
//! address, so a VMCB built entirely by it fails `VMRUN`'s consistency check
//! before the guest runs. Both maps are provided here, zeroed, which is what
//! makes the helper usable — the alternative would be to not call it and prove
//! nothing about it.

use hv1_core::svm::{self, HostSaveArea, Vmcb};
use hv1_core::vcpu::GeneralRegisters;

/// `VMEXIT_HLT`. The guest executed `hlt` and the hypervisor asked to know.
pub const VMEXIT_HLT: u64 = 0x78;
/// `VMEXIT_VMMCALL`. A guest asking its hypervisor for something, and the one
/// exit reason no other instruction can produce.
pub const VMEXIT_VMMCALL: u64 = 0x81;
/// What the hardware writes when `VMRUN` fails its consistency checks: the VMCB
/// described a machine that cannot exist, and no guest instruction ran.
pub const VMEXIT_INVALID: u64 = u64::MAX;
/// A nested page fault: the guest touched a guest-physical address the nested
/// page tables did not translate.
pub const VMEXIT_NPF: u64 = 0x400;

/// The guest program. `vmmcall`, then `hlt`.
static GUEST_CODE: [u8; 4] = [0x0F, 0x01, 0xD9, 0xF4];

/// Where the guest's code is placed in guest-physical memory.
///
/// A fixed address rather than the address of `GUEST_CODE`, because a
/// real-mode segment base has to be reachable as `base:0` and because copying
/// makes the guest's memory unambiguously separate from the hypervisor's own
/// image. Identity-mapped by the trampoline's tables, which are also the
/// nested page tables, so this guest-physical address is a host-physical one.
const GUEST_CODE_ADDR: u64 = 0x0040_0000;

/// The host state `VMRUN` saves into and `VMEXIT` restores from.
static mut HOST_SAVE: HostSaveArea = HostSaveArea { data: [0; 4096] };

/// The I/O permission map, all zero: nothing is trapped by port.
///
/// 12 KiB and page-aligned, both required. The map exists because
/// `setup_vmcb_controls` sets `INTERCEPT_IOIO`, and with that bit set the
/// hardware reads this map whether or not anything in it is set.
#[repr(C, align(4096))]
struct Iopm([u8; 12 * 1024]);
static mut IOPM: Iopm = Iopm([0; 12 * 1024]);

/// The MSR permission map, all zero: no MSR is trapped.
#[repr(C, align(4096))]
struct Msrpm([u8; 8 * 1024]);
static mut MSRPM: Msrpm = Msrpm([0; 8 * 1024]);

/// The VMCB. 4 KiB aligned by its own `repr`.
static mut VMCB: Option<Vmcb> = None;

/// One level of a nested page table: 512 eight-byte entries in a 4 KiB page.
#[repr(C, align(4096))]
struct PageTable([u64; 512]);

/// The guest's nested page tables: PML4, PDPT and one page directory, which
/// between them identity-map the first gigabyte with 2 MiB pages.
static mut NPT_PML4: PageTable = PageTable([0; 512]);
static mut NPT_PDPT: PageTable = PageTable([0; 512]);
static mut NPT_PD: PageTable = PageTable([0; 512]);

/// Present, writable, and — the one that matters — user.
///
/// A nested page walk is a user-mode access regardless of the guest's CPL, so
/// an entry without `U/S` faults for every guest at every privilege level.
const PTE_PRESENT: u64 = 1 << 0;
const PTE_WRITE: u64 = 1 << 1;
const PTE_USER: u64 = 1 << 2;
/// A page-directory entry that maps 2 MiB directly rather than pointing at
/// another level.
const PTE_LARGE: u64 = 1 << 7;

/// How much guest-physical address space the tables below cover.
const NPT_COVERAGE: u64 = 1 << 30;
/// The page size those tables map with.
const LARGE_PAGE: u64 = 2 * 1024 * 1024;

/// Build the guest's nested page tables and return the root's address.
///
/// # Safety
///
/// Writes the three statics above, and must be called once, before `VMRUN`.
unsafe fn build_npt() -> u64 {
    let pml4 = core::ptr::addr_of_mut!(NPT_PML4);
    let pdpt = core::ptr::addr_of_mut!(NPT_PDPT);
    let pd = core::ptr::addr_of_mut!(NPT_PD);

    (*pml4).0[0] = pdpt as u64 | PTE_PRESENT | PTE_WRITE | PTE_USER;
    (*pdpt).0[0] = pd as u64 | PTE_PRESENT | PTE_WRITE | PTE_USER;

    for (i, entry) in (*pd).0.iter_mut().enumerate() {
        let frame = i as u64 * LARGE_PAGE;
        if frame >= NPT_COVERAGE {
            break;
        }
        *entry = frame | PTE_PRESENT | PTE_WRITE | PTE_USER | PTE_LARGE;
    }

    pml4 as u64
}

/// What one entry into the guest did.
pub struct Exit {
    /// The exit code the hardware wrote.
    pub code: u64,
    /// Where the guest was when it exited.
    pub rip: u64,
    /// The guest-physical address a nested page fault was for. Meaningless for
    /// every other exit, and reported anyway on a fault because "it faulted"
    /// and "it faulted *there*" are different amounts of help.
    pub fault_addr: u64,
}

/// What running the guest amounted to.
pub enum Outcome {
    /// SVM is not on, so there is nothing to run a guest with.
    NotEnabled,
    /// `VMRUN` was executed. The exits are in the order they happened.
    Ran { first: Exit, second: Exit },
}

/// Attributes for a real-mode code segment: present, ring 0, code,
/// execute/read/accessed.
const CODE_ATTRIB: u16 = 0x009B;
/// The same for data: present, ring 0, data, read/write/accessed.
const DATA_ATTRIB: u16 = 0x0093;

/// The default `PAT`, which the hardware requires be valid when nested paging
/// is on. Six memory types, the same set a reset CPU has.
const DEFAULT_PAT: u64 = 0x0007_0406_0007_0406;

/// `EFER.SVME`. `VMRUN` refuses a guest whose saved `EFER` does not have it,
/// which is a consistency check rather than a statement about the guest.
const EFER_SVME: u64 = 1 << 12;

/// Flush the whole TLB on entry. Correct on a first entry and cheap after.
const TLB_FLUSH_ALL: u8 = 1;

/// Run the guest, twice.
///
/// # Safety
///
/// Requires `EFER.SVME` — that is, `hv1_core::initialize()` having succeeded —
/// ring 0, and an identity-mapped address space, since every address written
/// into the VMCB is used by the hardware as a physical one.
pub unsafe fn run() -> Outcome {
    if !svm::is_enabled() {
        return Outcome::NotEnabled;
    }

    // Copy the guest's program to where the guest will look for it. Not a
    // reference to the static: a guest reading its own code out of the
    // hypervisor's image would be a guest sharing memory with its hypervisor,
    // which is the one thing this whole layer exists to prevent.
    let dest = GUEST_CODE_ADDR as *mut u8;
    for (i, byte) in GUEST_CODE.iter().enumerate() {
        core::ptr::write_volatile(dest.add(i), *byte);
    }

    // The host save area has to exist before VMRUN: it is where the CPU puts
    // the host's own state, and its address goes in an MSR rather than the
    // VMCB.
    let host_save = &*core::ptr::addr_of!(HOST_SAVE);
    let _ = svm::set_host_save_area(host_save);

    VMCB = Some(Vmcb::new());
    let vmcb = (*core::ptr::addr_of_mut!(VMCB))
        .as_mut()
        .expect("just assigned");

    // The crate's own helper, which is the point: this runs hv1's VMCB setup,
    // not a reimplementation of it.
    svm::setup_vmcb_controls(vmcb, build_npt(), 1);

    // The two addresses the helper leaves at zero and the hardware reads
    // anyway. See the module documentation.
    vmcb.control.iopm_base_pa = core::ptr::addr_of!(IOPM) as u64;
    vmcb.control.msrpm_base_pa = core::ptr::addr_of!(MSRPM) as u64;
    vmcb.control.tlb_control = TLB_FLUSH_ALL;

    // Guest state: real mode, at GUEST_CODE_ADDR, with nothing else set up.
    let save = &mut vmcb.save;
    save.cs.selector = (GUEST_CODE_ADDR >> 4) as u16;
    save.cs.base = GUEST_CODE_ADDR;
    save.cs.limit = 0xFFFF;
    save.cs.attrib = CODE_ATTRIB;

    for seg in [
        &mut save.ds,
        &mut save.es,
        &mut save.fs,
        &mut save.gs,
        &mut save.ss,
    ] {
        seg.selector = 0;
        seg.base = 0;
        seg.limit = 0xFFFF;
        seg.attrib = DATA_ATTRIB;
    }

    save.rip = 0;
    save.rsp = 0;
    save.rflags = 0x2;
    // ET, and no PE: a real-mode guest.
    save.cr0 = 0x10;
    save.cr3 = 0;
    save.cr4 = 0;
    save.efer = EFER_SVME;
    save.g_pat = DEFAULT_PAT;
    save.dr6 = 0xFFFF_0FF0;
    save.dr7 = 0x400;
    save.cpl = 0;

    // `svm_run` rather than `svm::vmrun`: the bare helper executes VMRUN and
    // nothing else, and `#VMEXIT` restores only RAX, RSP and RIP. Every other
    // register comes back holding a guest value, so the first thing the host
    // does afterwards runs on the guest's data — which here meant the console
    // stopping mid-word, and is why this path is the one worth exercising.
    let mut regs = GeneralRegisters::default();

    // ── First entry ─────────────────────────────────────────────────────
    let _ = svm::svm_run(vmcb, &mut regs);
    let first = Exit {
        code: vmcb.control.exit_code,
        rip: vmcb.save.rip,
        fault_addr: vmcb.control.exit_info2,
    };

    // Step over the `vmmcall` and go back in. The hardware does not advance
    // RIP past an intercepted instruction -- that is the hypervisor's job, and
    // getting it wrong means re-executing the same instruction forever, which
    // is the classic way an exit loop becomes an infinite one.
    if first.code == VMEXIT_VMMCALL {
        vmcb.save.rip = first.rip.wrapping_add(3);
    }
    vmcb.control.vmcb_clean = 0;

    let _ = svm::svm_run(vmcb, &mut regs);
    let second = Exit {
        code: vmcb.control.exit_code,
        rip: vmcb.save.rip,
        fault_addr: vmcb.control.exit_info2,
    };

    Outcome::Ran { first, second }
}

/// A name for an exit code, for a console with no formatter.
pub fn exit_name(code: u64) -> &'static str {
    match code {
        VMEXIT_HLT => "VMEXIT_HLT — the guest halted",
        VMEXIT_VMMCALL => "VMEXIT_VMMCALL — the guest called its hypervisor",
        VMEXIT_INVALID => "VMEXIT_INVALID — VMRUN refused the VMCB; no guest instruction ran",
        0x60 => "VMEXIT_INTR",
        0x61 => "VMEXIT_NMI",
        0x40..=0x5F => "VMEXIT_EXCP — the guest faulted",
        VMEXIT_NPF => "VMEXIT_NPF — a nested page fault",
        _ => "an exit this demonstration did not expect",
    }
}
