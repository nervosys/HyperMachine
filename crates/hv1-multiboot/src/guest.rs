//! Run a guest under `hv1-core`, and be a hypervisor to it.
//!
//! `initialize()` proved the hypervisor starts. `VMRUN` proved a guest
//! executes. Neither is *hosting*: a hypervisor that enters a guest twice and
//! stops has run a guest the way a launcher runs a program. What separates the
//! two is what happens on an exit — whether the thing above the guest can
//! answer it, put something back, and let the guest carry on none the wiser.
//!
//! So this is an exit loop with a hypervisor behind it:
//!
//! - **A device.** The guest writes a line to COM1. Every byte is an
//!   intercepted `out`, so there is no serial port on the other side of it —
//!   there is `hv1`, decoding the port and the width out of the exit
//!   information and putting the byte on its own console. That is emulation:
//!   the guest cannot tell the difference, and the difference is the whole job.
//! - **A hypercall.** `vmmcall` with a number in `EAX`, answered and stepped
//!   over. The guest asks four times and gets four answers.
//! - **An interrupt, delivered.** The guest halts. The hypervisor injects
//!   vector 0x20 through the VMCB, the guest's own handler runs, says so with a
//!   hypercall, and `iret`s back to where it was. That is the piece that cannot
//!   be faked from outside: the only way that hypercall happens is if the CPU
//!   really took the vector through the guest's own interrupt table.
//!
//! # The guest
//!
//! Assembled by the toolchain rather than written out as hex. Every guest this
//! project has run until now was hand-assembled bytes — four of them here, and
//! seventy-three in `hv2`'s first unikernel — and hand-assembly is why they
//! stayed four bytes long. `.code16` in a section of its own costs nothing and
//! removes the ceiling.
//!
//! It starts in real mode, because that is where a `VMRUN` with `CR0.PE` clear
//! puts it, and then leaves: a descriptor table of its own, `CR0.PE`, and a far
//! jump into 32-bit code. That is the step that separates a guest from a
//! program a hypervisor happens to be running — it builds the environment it
//! runs in rather than being handed one, and everything after the jump uses
//! *linear* addresses, which is visible from outside as the RIP in the exit log
//! going from `0x21` to `0x1060`.
//!
//! In protected mode the interrupt table is thirty-three eight-byte gates
//! rather than four-byte vectors: each has a selector, a privilege level and a
//! present bit, and the thirty-two the processor reserves for itself point at a
//! handler that reports rather than at nothing. A guest that faults now says
//! so with a hypercall instead of triple-faulting into a shutdown exit with no
//! diagnostic.
//!
//! # Two copies of one number, and a check
//!
//! Once the guest is in protected mode every address it forms is linear, so it
//! has to know where it was loaded — which means the load address exists twice,
//! in this module and in the assembly. A mismatch would not fail to build; it
//! would far-jump somewhere plausible and die. So the assembly exports its copy
//! as an absolute symbol and `run` compares them before `VMRUN`.
//!
//! # The guest has its own memory now
//!
//! It did not. The nested page tables identity-mapped the first gigabyte, so
//! the guest's physical address space *was* the hypervisor's, and the only
//! reason that was survivable is that a four-byte guest touches nothing. A
//! guest with a stack, an interrupt table and a string does touch things — its
//! table lives at address zero — and writing that through an identity map means
//! writing over whatever the hypervisor has at zero.
//!
//! So the tables translate: guest-physical 0 is the base of a 2 MiB region this
//! image owns, and everything above that region is unmapped. A guest that
//! wanders takes a nested page fault instead of editing its hypervisor, which
//! is what the tables were always supposed to be for.
//!
//! # The bit that is easy to miss
//!
//! A nested page walk is performed as a *user-mode* access, whatever the
//! guest's own privilege level, so every level needs the U/S bit. The first
//! attempt at this handed the guest the trampoline's own page tables — correct
//! addresses, identity mapped — and every entry exited immediately with
//! `VMEXIT_NPF` on the first instruction fetch, at an address that was plainly
//! mapped. Nothing in a kernel's own map is user-accessible, which is that
//! bit's entire purpose.
//!
//! # What only running it found, again
//!
//! Two defects, both invisible to review and both one line.
//!
//! **`mov ax, HANDLER_OFFSET` is a load, not an immediate.** In the Intel
//! syntax the assembler uses, a bare symbol is an *address*: the guest was
//! assembling to `mov ax, [0x42]` and putting whatever happened to be at that
//! address into its interrupt vector, and `mov si, [0x4c]` for the message. It
//! assembles without a warning and it reads plausibly. The symptom was a guest
//! that printed nothing and then triple-faulted, and the diagnosis was
//! `objdump -m i8086` over the section — the fix is `offset`.
//!
//! **`IDTR` is not a protected-mode register.** Real mode consults it too, and
//! a reset CPU has base 0 with limit 0xFFFF; `Vmcb::new()` zeroes the save
//! area, so the guest had a vector table with a limit of *zero*. Every vector
//! is outside a table of that size, so the injected interrupt raised #GP, which
//! is also outside it, which is a triple fault. It presented as
//! `VMEXIT_SHUTDOWN` at exactly the RIP the hypervisor had just set — a fault
//! during delivery rather than anything the guest executed, which is what said
//! to look at the delivery machinery rather than at the handler.
//!
//! # Why the intercept bitmaps are set by hand
//!
//! `svm::setup_vmcb_controls` is the crate's own helper and sets the `IOIO` and
//! `MSR` intercepts. Both require their bitmaps: with `INTERCEPT_IOIO` set the
//! hardware reads a 12 KiB I/O permission map at `IOPM_BASE_PA`, and with
//! `INTERCEPT_MSR` an 8 KiB map at `MSRPM_BASE_PA`. The helper sets neither
//! address, so a VMCB built entirely by it fails `VMRUN`'s consistency check
//! before a guest instruction runs. Both are provided here — and the I/O map is
//! no longer all zeros, because a zero map means *nothing* is intercepted and
//! the guest's `out` would go past this hypervisor to whatever is underneath
//! it.

use hv1_core::svm::{self, HostSaveArea, Vmcb};
use hv1_core::vcpu::GeneralRegisters;

use crate::print;

/// `VMEXIT_HLT`. The guest executed `hlt` and the hypervisor asked to know.
pub const VMEXIT_HLT: u64 = 0x78;
/// `VMEXIT_IOIO`. The guest touched a port the I/O permission map claims.
pub const VMEXIT_IOIO: u64 = 0x7B;
/// `VMEXIT_CPUID`.
pub const VMEXIT_CPUID: u64 = 0x72;
/// `VMEXIT_VMMCALL`. A guest asking its hypervisor for something, and the one
/// exit reason no other instruction can produce.
pub const VMEXIT_VMMCALL: u64 = 0x81;
/// What the hardware writes when `VMRUN` fails its consistency checks: the VMCB
/// described a machine that cannot exist, and no guest instruction ran.
pub const VMEXIT_INVALID: u64 = u64::MAX;
/// A nested page fault: the guest touched a guest-physical address the nested
/// page tables did not translate.
pub const VMEXIT_NPF: u64 = 0x400;

// The guest, assembled rather than spelled out in hex.
//
// Placed in `.text.guest16` so the linker's ordinary `.text` rule keeps it in
// the image; it is copied into the guest's memory before entry and never
// executed where it is linked.
//
// Every address inside it is written as a distance from `guest_start`, because
// the guest runs with `CS.base` at its own load address and `RIP` at zero. A
// label used directly would be this image's link address, which is the kind of
// mistake that assembles cleanly and jumps into nothing.
core::arch::global_asm!(
    r#"
    .section .text.guest16, "ax"
    .code16

    // Where the hypervisor loads this. Protected mode addresses are linear and
    // a flat code segment has base zero, so every address after the far jump
    // has to be the load address plus an offset — which means the guest has to
    // know where it was put. Checked against the Rust constant at run time
    // rather than kept in step by hand: see `GUEST_BASE_FROM_ASM`.
    .set GUEST_BASE, 0x1000
    .global guest_base_marker
    .set guest_base_marker, GUEST_BASE

    // Absolute constants, defined before they are used. The assembler will not
    // take a difference of two symbols written at the point of use, where it is
    // two symbols in one operand.
    .set MESSAGE_OFFSET, message - guest_start
    .set RING_MSG_LINEAR, GUEST_BASE + ring_message - guest_start
    .set RING_MSG_LEN,    ring_message_end - ring_message
    .set GDT_PTR_OFFSET, gdt_pointer - guest_start
    .set IDT_PTR_LINEAR, GUEST_BASE + idt_pointer - guest_start
    .set STACK_TOP,      GUEST_BASE + 0xF00
    .set PM_ENTRY,       GUEST_BASE + protected - guest_start
    .set TIMER_LINEAR,   GUEST_BASE + timer - guest_start
    .set FAULT_LINEAR,   GUEST_BASE + fault - guest_start

    .global guest_start
guest_start:
    // ── Real mode ───────────────────────────────────────────────────────
    // Data and stack. `mov ax, cs` is the only way a real-mode program learns
    // where it is, and it works here because the guest is loaded low enough for
    // its base to be a selector — which is why GUEST_BASE is 0x1000 and not
    // somewhere more comfortable.
    mov ax, cs
    mov ds, ax
    xor ax, ax
    mov ss, ax
    mov sp, 0xF00

    // Say hello before changing anything, one intercepted `out` at a time.
    // There is no serial port on the other side of these.
    mov si, offset MESSAGE_OFFSET
1:
    mov al, [si]
    test al, al
    jz 2f
    mov dx, 0x3F8
    out dx, al
    inc si
    jmp 1b
2:
    // Hypercall 1: about to leave real mode.
    mov eax, 1
    vmmcall

    // ── The crossing ────────────────────────────────────────────────────
    // A descriptor table of its own, then CR0.PE, then a far jump. The jump is
    // what makes the CPU 32-bit: until it retires, CS still has a real-mode
    // base and the next instruction would be decoded under the old rules.
    //
    // The loader's tables are not reused and could not be — this guest has no
    // loader. Every table below is its own, in its own memory, which is the
    // difference between a guest that has been handed an environment and one
    // that builds one.
    cli
    mov bx, offset GDT_PTR_OFFSET
    lgdt [bx]

    mov eax, cr0
    or eax, 1
    mov cr0, eax

    // 0x08 is the flat 32-bit code descriptor, the first after the null.
    ljmp 0x08, offset PM_ENTRY

    .code32
protected:
    // Flat data everywhere. In protected mode a selector is an index into the
    // table just loaded, and a stale real-mode one refers to nothing.
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax
    mov esp, offset STACK_TOP

    // An interrupt table with eight-byte gates, not four-byte vectors. This is
    // the piece a real-mode guest does not have: every vector has a descriptor
    // with a selector, a privilege level and a present bit, and the thirty-two
    // the processor reserves point at a reporter rather than at nothing.
    mov ebx, offset IDT_PTR_LINEAR
    lidt [ebx]
    sti

    // Hypercall 2: in protected mode, with an IDT of its own.
    mov eax, 2
    vmmcall

    // Idle. Nothing in the guest arranges what happens next: the hypervisor
    // sees the halt and injects a vector.
    hlt

    // Hypercall 4: the handler returned here, so `iretd` came back to 32-bit
    // code at the right address.
    mov eax, 4
    vmmcall

    // ── A device with a ring ────────────────────────────────────────────
    // Everything above this point is the guest being told things. This is the
    // guest *driving* something: it puts a message somewhere of its own
    // choosing, describes where in a structure the device has to walk, and
    // rings a doorbell. The hypervisor learns the address from the descriptor
    // and from nowhere else.
    //
    //   0x2000  avail index     requests the guest has published
    //   0x2002  used index      requests the device has completed
    //   0x2004  descriptor 0    address and length of the message going out
    //   0x200C  descriptor 1    address and length of the buffer for a reply
    //
    // The message is copied into a buffer that has nothing to do with the image
    // it was assembled into, so the address in the descriptor is the only way
    // to find it.
    mov esi, offset RING_MSG_LINEAR
    mov edi, 0x3000
    mov ecx, offset RING_MSG_LEN
    rep movsb

    mov dword ptr [0x2004], 0x3000
    mov dword ptr [0x2008], offset RING_MSG_LEN
    mov dword ptr [0x200C], 0x3100
    mov dword ptr [0x2010], 64
    mov word ptr [0x2002], 0
    mov word ptr [0x2000], 1

    // The doorbell: an intercepted port, so the device runs inside the exit and
    // has finished before the guest's next instruction.
    mov dx, 0x100
    xor al, al
    out dx, al

5:
    mov ax, [0x2002]
    cmp ax, 1
    jne 5b

    // Read the reply out of the buffer the *guest* named, and put it on the
    // console — so the round trip is visible from outside rather than asserted.
    mov esi, 0x3100
6:
    mov al, [esi]
    test al, al
    jz 7f
    mov dx, 0x3F8
    out dx, al
    inc esi
    jmp 6b
7:
    // Hypercall 5: the ring completed.
    mov eax, 5
    vmmcall

    // And now a descriptor that points outside the guest's own memory. Nested
    // paging stops this guest reaching there itself; it does nothing about the
    // guest *asking its hypervisor* to reach there on its behalf, which is what
    // a device model that trusts a descriptor would do.
    //
    // 10 MiB, against a guest that has two.
    mov dword ptr [0x2004], 0x00A00000
    mov dword ptr [0x2008], 16
    mov word ptr [0x2000], 2
    mov dx, 0x100
    xor al, al
    out dx, al

    // Hypercall 6: asked for something out of range.
    mov eax, 6
    vmmcall
3:
    hlt
    jmp 3b

    // Hypercall 3, from inside the handler. The only way this runs is if the
    // CPU took vector 0x20 through a gate in the table above — in protected
    // mode, where a gate is a descriptor and not an address.
timer:
    mov eax, 3
    vmmcall
    // `iretd`, not `iret`. In Intel syntax bare `iret` is the 16-bit form and
    // would pop a 16-bit frame off a 32-bit one, returning to a garbage
    // selector. The same mistake cost hv2's guest a debugging session.
    iretd

    // Any of the processor's own exceptions. A guest that faults should say so
    // rather than triple-fault into a shutdown exit with no diagnostic: this
    // hypercall is how the hypervisor learns the guest broke rather than
    // finished.
fault:
    mov eax, 0xFF
    vmmcall
4:
    hlt
    jmp 4b

    .align 8
gdt:
    .quad 0                                  // null
    .quad 0x00CF9A000000FFFF                 // 0x08 code32: base 0, limit 4 GiB
    .quad 0x00CF92000000FFFF                 // 0x10 data32: the same, writable
gdt_end:
gdt_pointer:
    .word gdt_end - gdt - 1
    .long GUEST_BASE + gdt - guest_start

    // Thirty-three gates: the thirty-two the architecture reserves, all
    // pointing at the reporter, and then the one the hypervisor injects.
    .align 8
idt:
    .rept 32
    .word FAULT_LINEAR & 0xFFFF
    .word 0x08
    .byte 0
    .byte 0x8E
    .word (FAULT_LINEAR >> 16) & 0xFFFF
    .endr
    .word TIMER_LINEAR & 0xFFFF
    .word 0x08
    .byte 0
    .byte 0x8E
    .word (TIMER_LINEAR >> 16) & 0xFFFF
idt_end:
idt_pointer:
    .word idt_end - idt - 1
    .long GUEST_BASE + idt - guest_start

message:
    .asciz "hello from a guest of hv1\n"
ring_message:
    .ascii "a request through a ring"
ring_message_end:
    .global guest_end
guest_end:
    .code64
"#
);

extern "C" {
    static guest_start: u8;
    static guest_end: u8;
    /// An absolute symbol whose *value* is the load address the guest was
    /// assembled for.
    ///
    /// Protected mode addresses are linear, so the guest has to know where it
    /// was put in order to far-jump into itself and to point its own
    /// descriptor tables at themselves. That means the same number exists twice
    /// — once in the assembly and once in `GUEST_CODE_ADDR` — and a mismatch
    /// would not fail to build, it would jump somewhere plausible and die. So
    /// the assembly exports its copy and [`run`] checks them.
    static guest_base_marker: u8;
}

/// Where the guest's code sits in *guest*-physical memory.
///
/// 0x1000: above the interrupt table and the area a real machine's BIOS uses,
/// and low enough that its address divided by sixteen is a real-mode selector.
/// That second constraint is the binding one — a guest loaded at 4 MiB cannot
/// name its own segment, and this guest starts in real mode.
///
/// The assembly needs the same number, because after it enters protected mode
/// every address it forms is linear. `guest_base_marker` is its copy and [`run`]
/// checks the two agree.
const GUEST_CODE_ADDR: u64 = 0x1000;

/// How much memory the guest has.
const GUEST_RAM_SIZE: usize = 2 * 1024 * 1024;

/// The guest's memory: a 2 MiB region this image owns.
///
/// Aligned to its own size so it can be mapped by a single large page, and in
/// `.bss`, so it costs nothing in the image and the loader guarantees it reads
/// as zero — which matters here more than usual, since the guest's interrupt
/// table is at its address zero and an unwritten vector should be an obvious
/// null rather than whatever was in RAM.
#[repr(C, align(2097152))]
struct GuestRam([u8; GUEST_RAM_SIZE]);
static mut GUEST_RAM: GuestRam = GuestRam([0; GUEST_RAM_SIZE]);

/// The host state `VMRUN` saves into and `VMEXIT` restores from.
static mut HOST_SAVE: HostSaveArea = HostSaveArea { data: [0; 4096] };

/// The I/O permission map: one bit per port, set to intercept.
///
/// Not all zeros any more. A zero map with `INTERCEPT_IOIO` set means the
/// hardware intercepts nothing, so the guest's `out` would be executed for real
/// — past this hypervisor, to whatever is under it. Emulating a device starts
/// with claiming its ports.
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

static mut NPT_PML4: PageTable = PageTable([0; 512]);
static mut NPT_PDPT: PageTable = PageTable([0; 512]);
static mut NPT_PD: PageTable = PageTable([0; 512]);

const PTE_PRESENT: u64 = 1 << 0;
const PTE_WRITE: u64 = 1 << 1;
/// The one that matters. A nested page walk is a user-mode access regardless of
/// the guest's CPL, so an entry without this faults for every guest at every
/// privilege level.
const PTE_USER: u64 = 1 << 2;
/// A page-directory entry that maps 2 MiB directly rather than pointing at
/// another level.
const PTE_LARGE: u64 = 1 << 7;

/// The serial port the guest writes to, and this hypervisor answers.
const COM1: u16 = 0x3F8;

/// The doorbell the guest rings when it has put something in the ring.
const NOTIFY: u16 = 0x100;

/// Where the ring lives in guest-physical memory, and what is at each offset.
///
/// Fixed rather than negotiated. A real device tells its driver where to put
/// these, through a register window the driver reads; that is a bring-up
/// protocol and not the thing being shown here, which is a device walking
/// structures the *guest* filled in and following addresses the guest chose.
mod ring {
    pub const AVAIL: u64 = 0x2000;
    pub const USED: u64 = 0x2002;
    /// Address and length, twice: one descriptor out, one for the reply.
    pub const DESC0_ADDR: u64 = 0x2004;
    pub const DESC0_LEN: u64 = 0x2008;
    pub const DESC1_ADDR: u64 = 0x200C;
    pub const DESC1_LEN: u64 = 0x2010;
}

/// What this hypervisor answers a request with.
const RING_REPLY: &[u8] = b"and a reply through the same ring
";

/// The vector the hypervisor injects while the guest is halted.
const TIMER_VECTOR: u64 = 0x20;

/// `EVENTINJ`: an external interrupt, and the bit that makes the field mean
/// anything.
const INJECT_TYPE_INTR: u64 = 0 << 8;
const INJECT_VALID: u64 = 1 << 31;

/// Read `len` bytes of guest-physical memory, or nothing if that would leave
/// the guest's own region.
///
/// The bounds check is the whole point of this function existing. Every address
/// it is called with comes out of a descriptor the *guest* wrote, and a device
/// model that follows one without checking is a guest that can read and write
/// its hypervisor's memory by writing a number into a struct. Nested paging
/// stops the guest reaching out on its own; it does nothing about the
/// hypervisor being asked to reach out on the guest's behalf.
///
/// # Safety
///
/// Reads `GUEST_RAM`, which is this image's own memory.
unsafe fn guest_slice(at: u64, len: u64) -> Option<&'static [u8]> {
    let end = at.checked_add(len)?;
    if end > GUEST_RAM_SIZE as u64 {
        return None;
    }
    let base = core::ptr::addr_of!(GUEST_RAM) as *const u8;
    Some(core::slice::from_raw_parts(base.add(at as usize), len as usize))
}

/// Write `bytes` into guest-physical memory, if the whole of it fits.
///
/// # Safety
///
/// Writes `GUEST_RAM`, bounded as above.
unsafe fn guest_write(at: u64, bytes: &[u8]) -> bool {
    let Some(end) = at.checked_add(bytes.len() as u64) else {
        return false;
    };
    if end > GUEST_RAM_SIZE as u64 {
        return false;
    }
    let base = core::ptr::addr_of_mut!(GUEST_RAM) as *mut u8;
    for (i, byte) in bytes.iter().enumerate() {
        core::ptr::write_volatile(base.add(at as usize + i), *byte);
    }
    true
}

/// Read a little-endian value out of guest memory.
///
/// # Safety
///
/// As `guest_slice`.
unsafe fn guest_u16(at: u64) -> u16 {
    guest_slice(at, 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

/// # Safety
///
/// As `guest_slice`.
unsafe fn guest_u32(at: u64) -> u32 {
    guest_slice(at, 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Build the guest's nested page tables and return the root's address.
///
/// One 2 MiB page, and only one: guest-physical 0 to 2 MiB translates to the
/// region this image owns, and every guest-physical address above that is
/// absent. A guest that runs off the end takes a nested page fault instead of
/// finding its hypervisor there.
///
/// # Safety
///
/// Writes the statics above, and must be called once, before `VMRUN`.
unsafe fn build_npt() -> u64 {
    let pml4 = core::ptr::addr_of_mut!(NPT_PML4);
    let pdpt = core::ptr::addr_of_mut!(NPT_PDPT);
    let pd = core::ptr::addr_of_mut!(NPT_PD);

    (*pml4).0[0] = pdpt as u64 | PTE_PRESENT | PTE_WRITE | PTE_USER;
    (*pdpt).0[0] = pd as u64 | PTE_PRESENT | PTE_WRITE | PTE_USER;
    (*pd).0[0] =
        core::ptr::addr_of!(GUEST_RAM) as u64 | PTE_PRESENT | PTE_WRITE | PTE_USER | PTE_LARGE;

    pml4 as u64
}

/// Claim a port in the I/O permission map, so that touching it exits.
///
/// # Safety
///
/// Writes `IOPM`, before `VMRUN`.
unsafe fn intercept_port(port: u16) {
    let iopm = core::ptr::addr_of_mut!(IOPM);
    let index = port as usize / 8;
    (*iopm).0[index] |= 1 << (port % 8);
}

/// What one entry into the guest did.
pub struct Exit {
    /// The exit code the hardware wrote.
    pub code: u64,
    /// Where the guest was when it exited.
    pub rip: u64,
    /// The hardware's first exit-information word, which means something
    /// different for every exit and is worth printing for the ones this loop
    /// does not understand — "an unexpected exit" and "an unexpected exit whose
    /// information word was this" are different amounts of help.
    pub info: u64,
    /// What the hypervisor did about it.
    pub answer: &'static str,
}

/// What running the guest amounted to.
pub enum Outcome {
    /// SVM is not on, so there is nothing to run a guest with.
    NotEnabled,
    /// The guest's assembly and this module disagree about where the guest is
    /// loaded, which would be a far jump into nothing.
    BaseMismatch { asm: u64, rust: u64 },
    /// The loop ran. Everything it saw, and everything it did.
    Ran(Transcript),
}

/// How many exits are recorded before the loop stops recording.
///
/// A bound rather than a `Vec`, because there is no allocator worth using here
/// and a hypervisor whose exit log can grow without limit is a hypervisor a
/// guest can exhaust.
///
/// Sixty-four was enough until the guest had a reply to read back. A serial port
/// written a byte at a time costs one exit per character, so two short lines of
/// console are sixty exits on their own — which is the reason real device models
/// batch, and the reason this number is what it is.
const MAX_EXITS: usize = 160;

/// What the hypervisor saw and said.
pub struct Transcript {
    pub exits: [Exit; MAX_EXITS],
    pub count: usize,
    /// Bytes the guest wrote to its serial port, and how many.
    pub console: [u8; 128],
    pub console_len: usize,
    /// The hypercall numbers the guest made, in order.
    pub calls: [u32; 16],
    pub call_count: usize,
    /// Whether the guest reached protected mode.
    pub protected_mode: bool,
    /// Whether the injected interrupt reached the guest's own handler.
    pub interrupt_handled: bool,
    /// Whether the guest resumed after the handler returned.
    pub resumed: bool,
    /// Whether the guest took one of the processor's own exceptions.
    pub faulted: bool,
    /// What the guest sent through the ring, and how much of it.
    pub ring: [u8; 64],
    pub ring_len: usize,
    /// Whether the guest read the reply back and completed the round trip.
    pub ring_returned: bool,
    /// Whether a descriptor pointed outside the guest's own memory.
    pub ring_refused: bool,
    /// Why the loop stopped.
    pub stopped: &'static str,
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

/// Instruction lengths, for stepping over an intercepted instruction when the
/// hardware did not say how long it was.
///
/// `next_rip` is filled in by every AMD part that has the decode assists, and
/// is used in preference; these are the fallback. Getting this wrong does not
/// crash — it re-executes the instruction forever, which is the classic way an
/// exit loop becomes an infinite one, so the guest's instructions are named
/// rather than guessed at by a general decoder that is not here.
const LEN_VMMCALL: u64 = 3;
const LEN_HLT: u64 = 1;
const LEN_CPUID: u64 = 2;

/// Step over the instruction that exited.
///
/// `next_rip` when the hardware provided one, and the known length otherwise. A
/// zero `next_rip` means the field was not written, not that the guest is about
/// to execute at zero.
fn step_over(vmcb: &Vmcb, fallback: u64) -> u64 {
    let next = vmcb.control.next_rip;
    if next > vmcb.save.rip {
        next
    } else {
        vmcb.save.rip.wrapping_add(fallback)
    }
}

/// Run the guest until it is done, answering everything it does.
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

    // Copy the guest into the guest's own memory. Not a reference to the
    // section it was linked in: a guest executing out of its hypervisor's image
    // is a guest sharing memory with its hypervisor, which is the one thing
    // this layer exists to prevent.
    let length = core::ptr::addr_of!(guest_end) as usize - core::ptr::addr_of!(guest_start) as usize;
    let source = core::ptr::addr_of!(guest_start);
    let dest = (core::ptr::addr_of_mut!(GUEST_RAM) as *mut u8).add(GUEST_CODE_ADDR as usize);
    for i in 0..length {
        core::ptr::write_volatile(dest.add(i), core::ptr::read_volatile(source.add(i)));
    }

    // The load address exists twice: here and in the guest's assembly, which
    // needs it to form linear addresses once it is in protected mode. A
    // mismatch would not fail to build — it would far-jump somewhere plausible
    // and die — so the assembly exports its copy and it is compared here.
    if core::ptr::addr_of!(guest_base_marker) as u64 != GUEST_CODE_ADDR {
        return Outcome::BaseMismatch {
            asm: core::ptr::addr_of!(guest_base_marker) as u64,
            rust: GUEST_CODE_ADDR,
        };
    }

    intercept_port(COM1);
    intercept_port(NOTIFY);

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

    // Guest state: real mode, entered at GUEST_CODE_ADDR.
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

    // The real-mode interrupt vector table. Real mode still consults IDTR --
    // it is not a protected-mode-only register -- and a reset CPU has base 0
    // with limit 0xFFFF. `Vmcb::new()` zeroes the save area, so leaving this
    // alone gives a table with a *zero limit*: every vector is outside it, so
    // the first interrupt delivered is a #GP, which has no vector either, which
    // is a triple fault.
    //
    // That is exactly how it presented. The guest wrote its own vector, the
    // hypervisor injected 0x20, and the next exit was VMEXIT_SHUTDOWN with the
    // RIP the hypervisor had just set -- a fault during delivery rather than
    // anything the guest executed.
    save.idtr.base = 0;
    save.idtr.limit = 0xFFFF;

    save.rip = 0;
    save.rsp = 0xF00;
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

    let mut log = Transcript {
        exits: core::array::from_fn(|_| Exit {
            code: 0,
            rip: 0,
            info: 0,
            answer: "",
        }),
        count: 0,
        console: [0; 128],
        console_len: 0,
        calls: [0; 16],
        call_count: 0,
        protected_mode: false,
        interrupt_handled: false,
        resumed: false,
        faulted: false,
        ring: [0; 64],
        ring_len: 0,
        ring_returned: false,
        ring_refused: false,
        stopped: "the guest halted with nothing left to do",
    };
    let mut halts = 0usize;

    while log.count < MAX_EXITS {
        vmcb.control.vmcb_clean = 0;
        let _ = svm::svm_run(vmcb, &mut regs);

        let code = vmcb.control.exit_code;
        let rip = vmcb.save.rip;
        let info = vmcb.control.exit_info1;
        let answer: &'static str;
        let mut done = false;

        match code {
            VMEXIT_IOIO => {
                // The port and the direction come out of the exit information;
                // the byte itself is in the guest's RAX, which is the one
                // register the VMCB carries.
                let info = vmcb.control.exit_info1;
                let port = (info >> 16) as u16;
                let is_in = info & 1 != 0;
                if is_in {
                    // Nothing is behind this port to read. Zero, and say so:
                    // an emulated device that invents data is worse than one
                    // that admits it has none.
                    vmcb.save.rax = 0;
                    answer = "read as zero";
                } else if port == NOTIFY {
                    // The device. It runs here, inside the exit the doorbell
                    // caused, so it has finished before the guest's next
                    // instruction — which is why the guest's wait loop below
                    // terminates on its first look.
                    let published = guest_u16(ring::AVAIL);
                    let completed = guest_u16(ring::USED);
                    if published > completed {
                        let out_at = u64::from(guest_u32(ring::DESC0_ADDR));
                        let out_len = u64::from(guest_u32(ring::DESC0_LEN));
                        let reply_at = u64::from(guest_u32(ring::DESC1_ADDR));
                        let reply_len = u64::from(guest_u32(ring::DESC1_LEN));

                        match guest_slice(out_at, out_len) {
                            Some(bytes) => {
                                let take = bytes.len().min(log.ring.len());
                                log.ring[..take].copy_from_slice(&bytes[..take]);
                                log.ring_len = take;
                            }
                            None => log.ring_refused = true,
                        }

                        // The reply goes where the guest asked, up to the length
                        // the guest said it had room for — both from the
                        // descriptor, both checked.
                        let fits = reply_len as usize >= RING_REPLY.len();
                        if !fits || !guest_write(reply_at, RING_REPLY) {
                            log.ring_refused = true;
                        }
                        let _ = guest_write(ring::USED, &published.to_le_bytes());
                    }
                    answer = "the ring: a request taken and a reply left";
                } else if port == COM1 {
                    let byte = vmcb.save.rax as u8;
                    if log.console_len < log.console.len() {
                        log.console[log.console_len] = byte;
                        log.console_len += 1;
                    }
                    answer = "COM1, emulated";
                } else {
                    answer = "a port with nothing behind it";
                }
                // For an I/O intercept the next instruction's address is in
                // exit_info2, which is architecture rather than a decode
                // assist, so it is used directly.
                vmcb.save.rip = vmcb.control.exit_info2;
            }
            VMEXIT_VMMCALL => {
                let call = vmcb.save.rax as u32;
                if log.call_count < log.calls.len() {
                    log.calls[log.call_count] = call;
                    log.call_count += 1;
                }
                answer = match call {
                    1 => "real mode, and about to leave it",
                    2 => {
                        log.protected_mode = true;
                        "protected mode, with a GDT and an IDT of its own"
                    }
                    3 => {
                        log.interrupt_handled = true;
                        "from inside the interrupt handler"
                    }
                    4 => {
                        log.resumed = true;
                        "the handler returned and the guest carried on"
                    }
                    5 => {
                        log.ring_returned = true;
                        "the ring round trip finished"
                    }
                    6 => "and then asked for memory it does not have",
                    0xFF => {
                        log.faulted = true;
                        log.stopped = "the guest took a processor exception and said so";
                        done = true;
                        "the guest faulted — one of its own exception gates ran"
                    }
                    _ => "a hypercall this hypervisor does not know",
                };
                vmcb.save.rip = step_over(vmcb, LEN_VMMCALL);
            }
            VMEXIT_HLT => {
                halts += 1;
                vmcb.save.rip = step_over(vmcb, LEN_HLT);
                if halts == 1 {
                    // Wake it, through its own interrupt table. Injection is
                    // unconditional — it does not consult the guest's IF — so
                    // this is the hypervisor putting a vector into a guest and
                    // not merely permitting one.
                    vmcb.control.event_inject = TIMER_VECTOR | INJECT_TYPE_INTR | INJECT_VALID;
                    answer = "idle — injecting vector 0x20";
                } else {
                    answer = "idle, and nothing left to send it";
                    done = true;
                }
            }
            VMEXIT_CPUID => {
                // Nothing this guest asks for, and answered rather than left
                // to fault: a hypervisor that intercepts an instruction and has
                // no answer for it has intercepted it by accident.
                vmcb.save.rax = 0;
                regs.rbx = 0;
                regs.rcx = 0;
                regs.rdx = 0;
                vmcb.save.rip = step_over(vmcb, LEN_CPUID);
                answer = "answered with zeros";
            }
            VMEXIT_NPF => {
                answer = "a nested page fault — the guest left its own memory";
                log.stopped = "the guest took a nested page fault";
                done = true;
            }
            VMEXIT_INVALID => {
                answer = "VMRUN refused the VMCB";
                log.stopped = "VMRUN refused the VMCB; no guest instruction ran";
                done = true;
            }
            _ => {
                answer = "an exit this hypervisor has nothing to say about";
                log.stopped = "an exit the loop does not handle";
                done = true;
            }
        }

        log.exits[log.count] = Exit {
            code,
            rip,
            info,
            answer,
        };
        log.count += 1;

        if done {
            break;
        }
    }

    if log.count == MAX_EXITS {
        log.stopped = "the exit log filled up";
    }

    Outcome::Ran(log)
}

/// Print what the hypervisor and its guest said to each other.
pub fn report(log: &Transcript) {
    print("hv1   the guest's console, every byte of it an intercepted out:\n");
    print("hv1   > ");
    for byte in &log.console[..log.console_len] {
        // The guest's newlines end a line and start the next with the same
        // prefix, so a two-line console reads as two lines rather than as one
        // with a break in the middle of it.
        if *byte == b'\n' {
            print("\nhv1   > ");
        } else {
            crate::print_byte(*byte);
        }
    }
    print("\n");
}

/// A name for an exit code, for a console with no formatter.
pub fn exit_name(code: u64) -> &'static str {
    match code {
        VMEXIT_HLT => "VMEXIT_HLT     — the guest halted",
        VMEXIT_IOIO => "VMEXIT_IOIO    — the guest touched a port",
        VMEXIT_CPUID => "VMEXIT_CPUID   — the guest asked what it is running on",
        VMEXIT_VMMCALL => "VMEXIT_VMMCALL — the guest called its hypervisor",
        VMEXIT_INVALID => "VMEXIT_INVALID — VMRUN refused the VMCB; no guest instruction ran",
        0x60 => "VMEXIT_INTR",
        0x61 => "VMEXIT_NMI",
        0x40..=0x5F => "VMEXIT_EXCP    — the guest faulted",
        VMEXIT_NPF => "VMEXIT_NPF     — a nested page fault",
        0x7F => "VMEXIT_SHUTDOWN— the guest triple-faulted",
        0x64 => "VMEXIT_VINTR",
        0x65 => "VMEXIT_CR0_SEL_WRITE",
        _ => "an exit this demonstration did not expect",
    }
}
