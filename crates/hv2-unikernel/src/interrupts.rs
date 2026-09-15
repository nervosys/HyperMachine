//! An IDT, a PIC, and one handler — so an idle agent can sleep.
//!
//! The vsock driver polled its used ring because there was nothing else it
//! could do: no IDT means any interrupt is a triple fault, so a guest that
//! wanted to be woken had to stay awake. That cost a host core per agent while
//! idle, which is the one figure in this project worse than a Linux
//! container's — a container that is doing nothing costs nothing.
//!
//! Four things are needed and each is small:
//!
//! 1. **A PIC that is programmed.** KVM's in-kernel 8259 comes up unprogrammed,
//!    and an unprogrammed 8259 delivers IRQ 0-7 as vectors 8-15, which on any
//!    x86 CPU are processor exceptions. The remap is not a nicety; without it a
//!    disk interrupt arrives as a double fault.
//! 2. **An IDT** with a gate for the vector the device raises.
//! 3. **A handler** that acknowledges the device and then the PIC, in that
//!    order. The virtio line is level-triggered and is held until
//!    `InterruptACK`, so a handler that only ends the interrupt at the PIC is
//!    re-entered immediately, forever.
//! 4. **`sti`**, and then `hlt` in the idle path rather than a spin.
//!
//! The saving is the whole cost: a halted vCPU is a thread blocked in
//! `KVM_RUN`, which is a thread the scheduler never runs.
//!
//! # What moving to 64 bits changed here
//!
//! An IDT gate is sixteen bytes rather than eight, because an offset is sixty-
//! four bits rather than thirty-two. A handler returns with `iretq`. There is
//! no `pusha`, so the nine caller-saved registers are pushed by name.
//!
//! And two of the three defects the 32-bit version of this file cost are gone
//! by construction rather than by being fixed. `iret` versus `iretd` — the
//! 16-bit form assembling silently against a 32-bit frame — has no equivalent
//! here, since `iretq` is spelled differently from both. And nothing needs to
//! enable SSE, because `x86_64-unknown-none` disables it in the target spec, so
//! there is no compiler-emitted `movaps` to fault on a misaligned stack slot.
//! The stack is still aligned before every call, because the ABI still says so
//! and because being right by accident is how the first one was found.

use core::arch::{asm, global_asm};

use crate::net;
use crate::vsock;

/// Where the master PIC is remapped to. 0x20 is the first vector above the
/// architecture's 32 reserved exception vectors, and the conventional choice
/// for the same reason.
const PIC_VECTOR_BASE: u8 = 0x20;

/// The IRQ line `VM::attach_vsock` gives the device.
const VSOCK_IRQ: u8 = 5;

/// The vector that IRQ arrives on once the PIC is remapped.
const VSOCK_VECTOR: usize = (PIC_VECTOR_BASE + VSOCK_IRQ) as usize;

/// The IRQ line `VM::attach_net` gives the device.
///
/// Not the vsock line. One line shared between two devices would have each
/// driver woken for the other's traffic and finding nothing, often enough to
/// look like a device that does not work.
const NET_IRQ: u8 = 6;

/// The vector the network IRQ arrives on once the PIC is remapped.
const NET_VECTOR: usize = (PIC_VECTOR_BASE + NET_IRQ) as usize;

/// Master PIC ports.
const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
/// Slave PIC ports.
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

/// End-of-interrupt.
const PIC_EOI: u8 = 0x20;

/// The code selector this guest is running under.
///
/// Not the loader's any more. The Multiboot loader's GDT has no 64-bit code
/// descriptor in it, so `boot.rs` loads one of its own and far-jumps through
/// entry 1 of it — selector 8, which happens to be the same number for an
/// entirely different reason than it was in 32-bit mode.
const CODE_SELECTOR: u16 = 0x08;

/// Present, ring 0, 64-bit interrupt gate. An *interrupt* gate rather than a
/// trap gate: it clears IF on entry, so a handler cannot be interrupted by the
/// line it is in the middle of acknowledging.
const GATE_INTERRUPT_64: u8 = 0x8E;

/// One IDT entry, in the layout the CPU reads.
///
/// Sixteen bytes in long mode, against eight in protected mode. The extra eight
/// are the top half of the offset and a reserved word — a table of the 32-bit
/// shape is not a smaller table, it is a table whose second entry the CPU reads
/// as the first one's high bits.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct Gate {
    offset_low: u16,
    selector: u16,
    /// Interrupt-stack-table index, or zero for "keep using the current stack".
    ///
    /// Zero here, and correct because this guest never changes privilege level
    /// and its stack is large and guarded. A kernel that took a fault on a bad
    /// stack would need one of these; this one has nowhere else to go anyway.
    ist: u8,
    kind: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl Gate {
    /// A gate pointing at `handler`.
    fn to(handler: u64) -> Self {
        Self {
            offset_low: handler as u16,
            selector: CODE_SELECTOR,
            ist: 0,
            kind: GATE_INTERRUPT_64,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

/// What `lidt` takes.
#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

/// The table. 256 entries so that any vector at all lands somewhere defined —
/// an entry the CPU finds absent is a general protection fault, and a fault
/// while handling a fault is how a guest triple-faults with no diagnostic.
static mut IDT: [Gate; 256] = [Gate {
    offset_low: 0,
    selector: 0,
    ist: 0,
    kind: 0,
    offset_mid: 0,
    offset_high: 0,
    reserved: 0,
}; 256];

// The handler's outer half. Written in assembly because a Rust `extern "C"`
// function returns with `ret` and an interrupt handler must return with
// `iretq`, which also restores RFLAGS and re-enables interrupts.
//
// There is no `pusha` in long mode, so the caller-saved registers are pushed by
// name: the interrupted code is entitled to find them as it left them, and the
// Rust function called below is entitled to clobber every one of them.
//
// The alignment is not decoration. The SysV AMD64 ABI wants RSP 16-byte aligned
// at a `call`, and a long-mode interrupt frame is five eight-byte words, so the
// stack here is aligned only by luck. Nothing in this build emits `movaps`
// against a stack slot — the target disables SSE — but the 32-bit version of
// this file faulted on exactly that, and an alignment that holds because of a
// target flag is one that stops holding when the flag changes.
global_asm!(
    r#"
    .section .text
    .global vsock_isr
vsock_isr:
    push rax
    push rcx
    push rdx
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push rbp
    mov rbp, rsp
    and rsp, -16
    call vsock_interrupt
    mov rsp, rbp
    pop rbp
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rax
    iretq
"#
);

global_asm!(
    r#"
    .section .text
    .global net_isr
net_isr:
    push rax
    push rcx
    push rdx
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push rbp
    mov rbp, rsp
    and rsp, -16
    call net_interrupt
    mov rsp, rbp
    pop rbp
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rax
    iretq
"#
);

// A handler for each of the processor's own exception vectors.
//
// Without one, a fault has no gate, which is itself a general protection fault,
// which has no gate either — and a fault while handling a fault triple-faults
// the CPU. The host sees a shutdown exit; anyone reading the guest's console
// sees output that stops mid-word with no indication that anything went wrong.
// Every debugging session that begins "it just stops" begins here.
//
// One stub per vector, because the vector is the single most useful thing to
// know and the CPU does not push it. Each pushes its own number and falls into
// a common tail, which is the standard shape and the reason every kernel has
// thirty-two nearly identical stubs in it.
global_asm!(
    r#"
    .section .text
    .global fault_stub_0
fault_stub_0:
    push 0
    jmp fault_common
    .global fault_stub_1
fault_stub_1:
    push 1
    jmp fault_common
    .global fault_stub_2
fault_stub_2:
    push 2
    jmp fault_common
    .global fault_stub_3
fault_stub_3:
    push 3
    jmp fault_common
    .global fault_stub_4
fault_stub_4:
    push 4
    jmp fault_common
    .global fault_stub_5
fault_stub_5:
    push 5
    jmp fault_common
    .global fault_stub_6
fault_stub_6:
    push 6
    jmp fault_common
    .global fault_stub_7
fault_stub_7:
    push 7
    jmp fault_common
    .global fault_stub_8
fault_stub_8:
    push 8
    jmp fault_common
    .global fault_stub_9
fault_stub_9:
    push 9
    jmp fault_common
    .global fault_stub_10
fault_stub_10:
    push 10
    jmp fault_common
    .global fault_stub_11
fault_stub_11:
    push 11
    jmp fault_common
    .global fault_stub_12
fault_stub_12:
    push 12
    jmp fault_common
    .global fault_stub_13
fault_stub_13:
    push 13
    jmp fault_common
    .global fault_stub_14
fault_stub_14:
    push 14
    jmp fault_common
    .global fault_stub_15
fault_stub_15:
    push 15
    jmp fault_common
    .global fault_stub_16
fault_stub_16:
    push 16
    jmp fault_common
    .global fault_stub_17
fault_stub_17:
    push 17
    jmp fault_common
    .global fault_stub_18
fault_stub_18:
    push 18
    jmp fault_common
    .global fault_stub_19
fault_stub_19:
    push 19
    jmp fault_common
    .global fault_stub_20
fault_stub_20:
    push 20
    jmp fault_common
    .global fault_stub_21
fault_stub_21:
    push 21
    jmp fault_common
    .global fault_stub_22
fault_stub_22:
    push 22
    jmp fault_common
    .global fault_stub_23
fault_stub_23:
    push 23
    jmp fault_common
    .global fault_stub_24
fault_stub_24:
    push 24
    jmp fault_common
    .global fault_stub_25
fault_stub_25:
    push 25
    jmp fault_common
    .global fault_stub_26
fault_stub_26:
    push 26
    jmp fault_common
    .global fault_stub_27
fault_stub_27:
    push 27
    jmp fault_common
    .global fault_stub_28
fault_stub_28:
    push 28
    jmp fault_common
    .global fault_stub_29
fault_stub_29:
    push 29
    jmp fault_common
    .global fault_stub_30
fault_stub_30:
    push 30
    jmp fault_common
    .global fault_stub_31
fault_stub_31:
    push 31
    jmp fault_common

fault_common:
    // [rsp] is the vector this stub pushed. Above it is what the CPU pushed:
    // RIP, CS, RFLAGS, RSP, SS — or, for the vectors that have one, an error
    // code first. The first three words above the vector are reported rather
    // than interpreted, because guessing which layout applies is how a
    // diagnostic misleads.
    mov rdi, [rsp]
    mov rsi, [rsp + 8]
    mov rdx, [rsp + 16]
    mov rcx, [rsp + 24]
    and rsp, -16
    call fault_report
1:
    hlt
    jmp 1b

    .section .rodata
    .global fault_stub_table
fault_stub_table:
    .quad fault_stub_0
    .quad fault_stub_1
    .quad fault_stub_2
    .quad fault_stub_3
    .quad fault_stub_4
    .quad fault_stub_5
    .quad fault_stub_6
    .quad fault_stub_7
    .quad fault_stub_8
    .quad fault_stub_9
    .quad fault_stub_10
    .quad fault_stub_11
    .quad fault_stub_12
    .quad fault_stub_13
    .quad fault_stub_14
    .quad fault_stub_15
    .quad fault_stub_16
    .quad fault_stub_17
    .quad fault_stub_18
    .quad fault_stub_19
    .quad fault_stub_20
    .quad fault_stub_21
    .quad fault_stub_22
    .quad fault_stub_23
    .quad fault_stub_24
    .quad fault_stub_25
    .quad fault_stub_26
    .quad fault_stub_27
    .quad fault_stub_28
    .quad fault_stub_29
    .quad fault_stub_30
    .quad fault_stub_31
"#
);

extern "C" {
    fn vsock_isr();
    fn net_isr();
    /// Thirty-two stub addresses, indexed by vector.
    static fault_stub_table: [u64; 32];
}

/// Report a fault and stop. Never returns to the faulting instruction, because
/// nothing here could put right whatever caused it.
#[no_mangle]
pub extern "C" fn fault_report(vector: u64, w0: u64, w1: u64, w2: u64) {
    crate::print("\nFAULT vector ");
    crate::print_hex(vector as u32);
    crate::print(" frame ");
    crate::print_hex64(w0);
    crate::print(" ");
    crate::print_hex64(w1);
    crate::print(" ");
    crate::print_hex64(w2);
    crate::print("\n");
}

/// What the vsock interrupt actually does.
///
/// Both acknowledgements, in this order. The virtio line is level-triggered and
/// stays asserted until `InterruptACK` is written, so ending the interrupt at
/// the PIC first would return into the same interrupt immediately.
#[no_mangle]
pub extern "C" fn vsock_interrupt() {
    vsock::ack_interrupt_raw();

    // SAFETY: writing EOI to the master PIC command port, which ends an
    // interrupt from IRQ 0-7 and has no other effect.
    unsafe { outb(PIC1_COMMAND, PIC_EOI) };
}

/// What the network interrupt actually does.
///
/// The same two acknowledgements in the same order and for the same reason as
/// [`vsock_interrupt`]. Nothing is drained here: a handler that walked a ring
/// would be doing it with the main loop's view of that ring half-formed.
#[no_mangle]
pub extern "C" fn net_interrupt() {
    net::ack_interrupt_raw();

    // SAFETY: as in `vsock_interrupt`.
    unsafe { outb(PIC1_COMMAND, PIC_EOI) };
}

/// Write one byte to an I/O port.
///
/// # Safety
///
/// The caller must be writing to a port whose device tolerates it.
unsafe fn outb(port: u16, value: u8) {
    asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags),
    );
}

/// Program the PIC and unmask the lines this guest has handlers for.
///
/// The initialisation sequence is four control words to each chip, in a fixed
/// order, and the pair has to be done together: the master is told which of its
/// lines the slave is cascaded on and the slave is told which line it is.
///
/// # Safety
///
/// Reprograms the interrupt controller. Correct exactly once, at boot.
unsafe fn init_pic() {
    // ICW1: begin initialisation, expect ICW4.
    outb(PIC1_COMMAND, 0x11);
    outb(PIC2_COMMAND, 0x11);

    // ICW2: the vector each chip's lines are mapped to.
    outb(PIC1_DATA, PIC_VECTOR_BASE);
    outb(PIC2_DATA, PIC_VECTOR_BASE + 8);

    // ICW3: the cascade. The slave hangs off the master's line 2.
    outb(PIC1_DATA, 1 << 2);
    outb(PIC2_DATA, 2);

    // ICW4: 8086 mode.
    outb(PIC1_DATA, 0x01);
    outb(PIC2_DATA, 0x01);

    // Masks. Everything off except the lines this guest has handlers for: an
    // unmasked line with no handler is a fault, and this guest has two
    // devices, either of which may be absent -- a line no device asserts
    // costs nothing.
    outb(PIC1_DATA, !((1 << VSOCK_IRQ) | (1 << NET_IRQ)));
    outb(PIC2_DATA, 0xFF);
}

/// Install the fault handlers, and nothing else.
///
/// Called first, before anything that could fault — which is everything. A
/// fault with no gate is a general protection fault, which also has no gate,
/// which triple-faults the CPU and stops the console mid-word with no
/// indication that anything went wrong.
///
/// That ordering is the lesson this guest has already learned once, and it was
/// still wrong the second time: the IDT went in with the device interrupts,
/// after the code that reads the shared region, and a fault three functions
/// earlier had no reporter to reach.
///
/// # Safety
///
/// Called once, before anything else.
pub unsafe fn install_fault_handlers() {
    let idt = core::ptr::addr_of_mut!(IDT);

    for (vector, &stub) in fault_stub_table.iter().enumerate() {
        (*idt)[vector] = Gate::to(stub);
    }

    let idtr = Idtr {
        limit: (core::mem::size_of::<[Gate; 256]>() - 1) as u16,
        base: idt as u64,
    };
    asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack, preserves_flags));
}

/// Turn on SSE, which this build's compiler assumes it may use.
///
/// `x86_64-unknown-none` disables SSE in its own target spec and this crate
/// turns it back on, because the loop that decides how fast an agent can think
/// is 2.5 times slower without it. Having asked for vector instructions, the
/// guest has to make them legal.
///
/// Four bits, and every one of them is required:
///
/// - `CR0.EM` clear: there is a real FPU, do not trap to emulate one.
/// - `CR0.MP` set: `WAIT`/`FWAIT` respects `TS`, which is the pair `EM` belongs to.
/// - `CR4.OSFXSR` set: this guest can save and restore the SSE register file,
///   which is true because it never context-switches.
/// - `CR4.OSXMMEXCPT` set: unmasked SIMD exceptions arrive as #XM rather than
///   as #UD, so a numeric fault reports itself as one.
///
/// On a CPU straight out of reset none of them hold, so the first
/// compiler-emitted `movdqa` raises #UD or #NM. In the 32-bit guest that
/// presented as the console stopping mid-word, from a fault three functions
/// before the reporter that would have named it.
///
/// # Safety
///
/// Called once, before any code the compiler may have vectorised — which in
/// practice means immediately after the fault handlers and before anything
/// else.
pub unsafe fn enable_sse() {
    const CR0_MP: u64 = 1 << 1;
    const CR0_EM: u64 = 1 << 2;
    const CR4_OSFXSR: u64 = 1 << 9;
    const CR4_OSXMMEXCPT: u64 = 1 << 10;

    let mut cr0: u64;
    asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
    cr0 &= !CR0_EM;
    cr0 |= CR0_MP;
    asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack, preserves_flags));

    let mut cr4: u64;
    asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    cr4 |= CR4_OSFXSR | CR4_OSXMMEXCPT;
    asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
}

/// Program the PIC, point the device vector at its handler, and enable
/// interrupts.
///
/// After this the guest can `hlt` and expect to be woken.
///
/// # Safety
///
/// Requires [`install_fault_handlers`] to have run.
pub unsafe fn init() {
    let idt = core::ptr::addr_of_mut!(IDT);

    (*idt)[VSOCK_VECTOR] = Gate::to(vsock_isr as *const () as u64);
    (*idt)[NET_VECTOR] = Gate::to(net_isr as *const () as u64);

    init_pic();

    asm!("sti", options(nomem, nostack));
}

/// Sleep until the next interrupt.
///
/// `sti` immediately before `hlt`, and never separated: `sti` does not take
/// effect until after the following instruction, so the pair cannot be
/// interrupted between them. Written the other way round — check, then enable,
/// then halt — a device that raised its line in the gap is missed and the guest
/// sleeps until something else happens to wake it.
///
/// # Safety
///
/// Requires [`init`] to have run, or this halts a CPU that nothing can wake.
pub unsafe fn wait_for_interrupt() {
    asm!("sti; hlt", options(nomem, nostack));
}

/// Enable interrupts.
///
/// # Safety
///
/// Requires [`init`], or an interrupt arrives with no table to dispatch it.
pub unsafe fn enable() {
    asm!("sti", options(nomem, nostack));
}

/// Disable interrupts.
///
/// # Safety
///
/// Leaves them disabled until something enables them again.
pub unsafe fn disable() {
    asm!("cli", options(nomem, nostack));
}
