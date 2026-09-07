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
//!    32-bit CPU are processor exceptions. The remap is not a nicety; without
//!    it a disk interrupt arrives as a double fault.
//! 2. **An IDT** with a gate for the vector the device raises.
//! 3. **A handler** that acknowledges the device and then the PIC, in that
//!    order. The virtio line is level-triggered and is held until
//!    `InterruptACK`, so a handler that only ends the interrupt at the PIC is
//!    re-entered immediately, forever.
//! 4. **`sti`**, and then `hlt` in the idle path rather than a spin.
//!
//! The saving is the whole cost: a halted vCPU is a thread blocked in
//! `KVM_RUN`, which is a thread the scheduler never runs.

use core::arch::{asm, global_asm};

use crate::vsock;

/// Where the master PIC is remapped to. 0x20 is the first vector above the
/// architecture's 32 reserved exception vectors, and the conventional choice
/// for the same reason.
const PIC_VECTOR_BASE: u8 = 0x20;

/// The IRQ line `VM::attach_vsock` gives the device.
const VSOCK_IRQ: u8 = 5;

/// The vector that IRQ arrives on once the PIC is remapped.
const VSOCK_VECTOR: usize = (PIC_VECTOR_BASE + VSOCK_IRQ) as usize;

/// Master PIC ports.
const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
/// Slave PIC ports.
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

/// End-of-interrupt.
const PIC_EOI: u8 = 0x20;

/// The code selector the Multiboot loader's GDT puts our code in. Flat 32-bit
/// code at entry 1, so selector 8.
const CODE_SELECTOR: u16 = 0x08;

/// Present, ring 0, 32-bit interrupt gate. An *interrupt* gate rather than a
/// trap gate: it clears IF on entry, so a handler cannot be interrupted by the
/// line it is in the middle of acknowledging.
const GATE_INTERRUPT_32: u8 = 0x8E;

/// One IDT entry, in the layout the CPU reads.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct Gate {
    offset_low: u16,
    selector: u16,
    zero: u8,
    kind: u8,
    offset_high: u16,
}

/// What `lidt` takes.
#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u32,
}

/// The table. 256 entries so that any vector at all lands somewhere defined —
/// an entry the CPU finds absent is a general protection fault, and a fault
/// while handling a fault is how a guest triple-faults with no diagnostic.
static mut IDT: [Gate; 256] = [Gate {
    offset_low: 0,
    selector: 0,
    zero: 0,
    kind: 0,
    offset_high: 0,
}; 256];

// The handler's outer half. Written in assembly because a Rust `extern "C"`
// function returns with `ret` and an interrupt handler must return with `iret`,
// which also restores EFLAGS and re-enables interrupts. `pusha`/`popa` save the
// eight general-purpose registers, since the interrupted code is entitled to
// find them as it left them.
//
// The alignment is not decoration. The i386 SysV ABI requires ESP to be
// 16-byte aligned at a `call`, and this target's `core` is compiled with SSE2,
// so a compiler-emitted `movaps` against a stack slot faults with #GP on a
// misaligned one. An interrupt frame is twelve bytes and `pusha` is another
// thirty-two, so the stack at this point is aligned only by luck. Without the
// `and`, this handler faulted the first time it ran — which presented as the
// console stopping mid-word, three frames away from anything to do with
// interrupts.
global_asm!(
    r#"
    .section .text
    .global vsock_isr
vsock_isr:
    pusha
    mov ebp, esp
    and esp, -16
    call vsock_interrupt
    mov esp, ebp
    popa
    // `iretd`, not `iret`. In Intel syntax -- which is what `asm!` and
    // `global_asm!` use -- bare `iret` is the 16-bit form and assembles with a
    // 0x66 prefix, so it pops a 16-bit IP/CS/FLAGS off a 32-bit interrupt
    // frame and returns to a garbage selector. That presented as #GP with
    // error code 0x10 at the `iret` itself, which is to say: the handler ran
    // perfectly and could not get back.
    iretd
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
    // [esp] is the vector this stub pushed. Above it is what the CPU pushed:
    // EIP, CS, EFLAGS — or, for the vectors that have one, an error code first.
    // All three are reported rather than interpreted, because guessing which
    // layout applies is how a diagnostic misleads.
    mov eax, [esp]
    mov ecx, [esp + 4]
    mov edx, [esp + 8]
    mov ebx, [esp + 12]
    and esp, -16
    push ebx
    push edx
    push ecx
    push eax
    call fault_report
1:
    hlt
    jmp 1b

    .section .rodata
    .global fault_stub_table
fault_stub_table:
    .long fault_stub_0
    .long fault_stub_1
    .long fault_stub_2
    .long fault_stub_3
    .long fault_stub_4
    .long fault_stub_5
    .long fault_stub_6
    .long fault_stub_7
    .long fault_stub_8
    .long fault_stub_9
    .long fault_stub_10
    .long fault_stub_11
    .long fault_stub_12
    .long fault_stub_13
    .long fault_stub_14
    .long fault_stub_15
    .long fault_stub_16
    .long fault_stub_17
    .long fault_stub_18
    .long fault_stub_19
    .long fault_stub_20
    .long fault_stub_21
    .long fault_stub_22
    .long fault_stub_23
    .long fault_stub_24
    .long fault_stub_25
    .long fault_stub_26
    .long fault_stub_27
    .long fault_stub_28
    .long fault_stub_29
    .long fault_stub_30
    .long fault_stub_31
"#
);

extern "C" {
    fn vsock_isr();
    /// Thirty-two stub addresses, indexed by vector.
    static fault_stub_table: [u32; 32];
}

/// Report a fault and stop. Never returns to the faulting instruction, because
/// nothing here could put right whatever caused it.
#[no_mangle]
pub extern "C" fn fault_report(vector: u32, w0: u32, w1: u32, w2: u32) {
    crate::print("\nFAULT vector ");
    crate::print_hex(vector);
    crate::print(" frame ");
    crate::print_hex(w0);
    crate::print(" ");
    crate::print_hex(w1);
    crate::print(" ");
    crate::print_hex(w2);
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

/// Program the PIC and unmask only the vsock line.
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

    // Masks. Everything off except the one line this guest has a handler for:
    // an unmasked line with no handler is a fault, and this guest has exactly
    // one device.
    outb(PIC1_DATA, !(1 << VSOCK_IRQ));
    outb(PIC2_DATA, 0xFF);
}

/// Install the IDT, program the PIC, and enable interrupts.
///
/// After this the guest can `hlt` and expect to be woken.
///
/// # Safety
///
/// Called once, before anything relies on being interrupted.
pub unsafe fn init() {
    let handler = vsock_isr as *const () as u32;
    let idt = core::ptr::addr_of_mut!(IDT);

    // Every processor exception gets the reporting handler. Not because this
    // guest can recover from any of them, but because a fault that says which
    // fault and where is a diagnosis, and a fault with no gate at all is a
    // triple fault and a console that stops mid-word.
    for (vector, &stub) in fault_stub_table.iter().enumerate() {
        (*idt)[vector] = Gate {
            offset_low: stub as u16,
            selector: CODE_SELECTOR,
            zero: 0,
            kind: GATE_INTERRUPT_32,
            offset_high: (stub >> 16) as u16,
        };
    }

    (*idt)[VSOCK_VECTOR] = Gate {
        offset_low: handler as u16,
        selector: CODE_SELECTOR,
        zero: 0,
        kind: GATE_INTERRUPT_32,
        offset_high: (handler >> 16) as u16,
    };

    let idtr = Idtr {
        limit: (core::mem::size_of::<[Gate; 256]>() - 1) as u16,
        base: idt as u32,
    };
    asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack, preserves_flags));

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
