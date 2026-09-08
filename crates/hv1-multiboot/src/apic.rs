//! This kernel's own local APIC, and the interrupt table to receive it.
//!
//! Everything else in this crate is about the *guest's* APIC — a page hv1
//! emulates. This is the other one: the real device on the processor hv1 is
//! running on, which it needs for exactly one reason. Its guest can arm a timer
//! and then spin, and a spinning guest never exits, so there is no moment at
//! which a hypervisor that only looks at exits could notice the deadline pass.
//! The way out is the way every hypervisor takes it: arm a timer of your own,
//! let the interrupt arrive while the guest runs, and let the interception of
//! it be the exit.
//!
//! # Three things had to be true first, and two of them were not
//!
//! **The page has to be mapped.** The trampoline identity-mapped one gigabyte,
//! and the local APIC is at `0xFEE00000` — just under four. It maps four now,
//! with that one page marked uncacheable, because a write-back mapping of a
//! device register is a write that may never arrive.
//!
//! **The APIC has to be on.** It was not, and the reason was in the *other*
//! hypervisor: `hv2` initialised each vCPU's segments from a zeroed
//! `kvm_sregs`, and `kvm_sregs` carries `apic_base` next to the segments. KVM
//! had put `0xFEE00900` there — the architectural base, the enable bit, and the
//! bootstrap-processor bit — and writing the struct back turned it off. This
//! kernel reported `apic 0x0 DISABLED, application processor, id 255` on a vCPU
//! that was none of those things. Fixed there; the reading is what found it.
//!
//! **`GIF` has to be set.** `#VMEXIT` clears the global interrupt flag, so
//! after the first `VMRUN` a hypervisor that never executes `STGI` has
//! interrupts masked at a level `STI` cannot reach. Nothing had noticed,
//! because nothing had ever wanted an interrupt.

use core::arch::global_asm;

/// Where the architecture puts the local APIC.
pub const MMIO: usize = 0xFEE0_0000;

/// This processor's identity, in the top byte.
pub const ID: usize = 0x020;
/// End of interrupt. Written by the handler, ignored value.
pub const EOI: usize = 0x0B0;
/// The spurious-interrupt vector register. Bit 8 is the software enable.
pub const SPURIOUS: usize = 0x0F0;
/// The timer's local vector table entry.
pub const LVT_TIMER: usize = 0x320;
/// Reload value; writing it arms the timer.
pub const INITIAL_COUNT: usize = 0x380;
/// What is left.
pub const CURRENT_COUNT: usize = 0x390;
/// Bus cycles per tick.
pub const DIVIDE: usize = 0x3E0;
/// The performance-monitor counter's entry in the local vector table. Unlike
/// the timer's, this one takes a delivery mode -- which is the whole reason it
/// is here, because one of the modes is NMI.
pub const LVT_PMC: usize = 0x340;

/// Bit 16 of an LVT entry: counts, delivers nothing.
const LVT_MASKED: u32 = 1 << 16;
/// Divide by one.
const DIVIDE_BY_1: u32 = 0x0B;

/// The vector this kernel gives its own timer. Nothing else uses it, and it is
/// well clear of the exception range the processor reserves.
pub const TIMER_VECTOR: u8 = 0xE0;
/// The vector for interrupts that arrive with nobody claiming them.
const SPURIOUS_VECTOR: u32 = 0xFF;

/// Delivery mode 100 in an LVT entry: deliver as an NMI, which goes to vector 2
/// and is not blocked by the interrupt flag.
const LVT_NMI: u32 = 0b100 << 8;

/// AMD's first legacy performance-counter pair.
const PERF_CTL0: u32 = 0xC001_0000;
const PERF_CTR0: u32 = 0xC001_0004;
/// Event 0x76, CPU clocks not halted, counted in both user and supervisor, with
/// the counter enabled and told to raise an APIC interrupt when it overflows.
const PERF_CYCLES: u64 = 0x0063_0076;
/// The counters are 48 bits wide, so a count of `n` is armed by starting `n`
/// short of the wrap.
const PERF_WIDTH: u64 = 1 << 48;

/// Read one of the local APIC's registers.
///
/// # Safety
///
/// The page must be mapped and uncached, which the trampoline arranges.
/// Volatile because these are device registers: reading one twice is not the
/// same as reading it once.
pub unsafe fn read(reg: usize) -> u32 {
    core::ptr::read_volatile((MMIO + reg) as *const u32)
}

/// Write one of the local APIC's registers.
///
/// # Safety
///
/// As [`read`].
pub unsafe fn write(reg: usize, value: u32) {
    core::ptr::write_volatile((MMIO + reg) as *mut u32, value);
}

/// How many times this kernel's own timer interrupt has been taken.
///
/// Incremented by the handler below, which is assembly, which is why it is
/// `#[no_mangle]` and not an `AtomicU64`.
#[no_mangle]
pub static mut HOST_TICKS: u64 = 0;

/// How many non-maskable interrupts this kernel has taken.
#[no_mangle]
pub static mut HOST_NMIS: u64 = 0;

global_asm!(
    r#"
    .section .text, "ax"
    .code64

    // The timer. It counts, acknowledges, and returns -- and it must not
    // disturb a single register, because it interrupts whatever the host was
    // doing between two entries into a guest.
    .global isr_timer
isr_timer:
    push rax
    push rdx
    inc qword ptr [rip + HOST_TICKS]
    // End of interrupt. Without this the APIC leaves the vector in service and
    // never delivers another.
    mov rdx, 0xFEE000B0
    xor eax, eax
    mov [rdx], eax
    pop rdx
    pop rax
    iretq

    // The one interrupt a guest cannot turn off. It carries no end-of-interrupt
    // -- an NMI is not delivered through the APIC's priority machinery -- so
    // all it does is count and return. Further NMIs are blocked until the
    // `iretq`, which is the architecture's doing and not this handler's.
    .global isr_nmi
isr_nmi:
    push rax
    inc qword ptr [rip + HOST_NMIS]
    pop rax
    iretq

    // Everything else. A kernel with no interrupt table triple-faults on its
    // first exception with no diagnostic at all; a kernel with a table that
    // returns quietly loops forever on it. This one says so and stops, one
    // character at a time, because a serial port is the only output it has.
    .global isr_unexpected
isr_unexpected:
    mov dx, 0x3F8
    mov al, 63          // '?'
    out dx, al
    mov al, 33          // '!'
    out dx, al
    mov al, 10
    out dx, al
1:
    hlt
    jmp 1b
"#
);

extern "C" {
    fn isr_timer();
    fn isr_nmi();
    fn isr_unexpected();
}

/// One entry of a 64-bit interrupt descriptor table.
///
/// Sixteen bytes, not eight: long mode splits the handler's address across
/// three fields and adds an interrupt-stack-table index that this kernel leaves
/// at zero, because it never changes privilege level and so never switches
/// stacks.
#[repr(C)]
#[derive(Clone, Copy)]
struct Gate {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl Gate {
    const fn empty() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    fn to(handler: unsafe extern "C" fn()) -> Self {
        let address = handler as usize as u64;
        Self {
            offset_low: address as u16,
            // The 64-bit code descriptor the trampoline built, which is the
            // only one this kernel has.
            selector: 0x08,
            ist: 0,
            // Present, ring 0, 64-bit interrupt gate. An interrupt gate rather
            // than a trap gate, so the handler runs with interrupts off.
            attributes: 0x8E,
            offset_mid: (address >> 16) as u16,
            offset_high: (address >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

static mut IDT: [Gate; 256] = [Gate::empty(); 256];

/// Build an interrupt table and load it.
///
/// Every vector points somewhere: the timer at its own, and all 255 others at a
/// handler that reports and stops. The alternative is a table with holes in it,
/// where an unexpected vector is a fault during delivery, which is a double
/// fault, which is a triple fault with nothing on the console.
///
/// # Safety
///
/// Writes a static and loads `IDTR`. Call once, before enabling interrupts.
pub unsafe fn load_idt() {
    let idt = core::ptr::addr_of_mut!(IDT);
    for slot in 0..256 {
        (*idt)[slot] = Gate::to(isr_unexpected);
    }
    (*idt)[TIMER_VECTOR as usize] = Gate::to(isr_timer);
    // Vector 2 is the NMI, and the architecture chooses it rather than us.
    (*idt)[2] = Gate::to(isr_nmi);

    let idtr = Idtr {
        limit: (core::mem::size_of::<[Gate; 256]>() - 1) as u16,
        base: idt as u64,
    };
    core::arch::asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack, preserves_flags));
}

/// Turn the APIC on and stop the timer.
///
/// The software enable is bit 8 of the spurious-interrupt vector register, and
/// an APIC without it is present, addressable, and silent.
///
/// # Safety
///
/// The APIC page must be mapped and the APIC enabled in `IA32_APIC_BASE`.
pub unsafe fn enable() {
    write(SPURIOUS, SPURIOUS_VECTOR | (1 << 8));
    write(DIVIDE, DIVIDE_BY_1);
    write(LVT_TIMER, u32::from(TIMER_VECTOR) | LVT_MASKED);
    write(INITIAL_COUNT, 0);
    LVT_IS_ARMED = false;
}

/// How many timestamp-counter ticks one APIC tick takes.
///
/// The APIC counts a bus clock and the rest of this kernel counts `RDTSC`, and
/// nothing tells you the ratio — so it is measured, once, by running the two
/// against each other. Returned as a numerator and denominator rather than a
/// float, because there is no floating point in this kernel and a rounded
/// integer ratio of a number near one is a factor-of-two error.
///
/// # Safety
///
/// As [`enable`], and it spins for the sampling window.
pub unsafe fn calibrate() -> (u64, u64) {
    // Masked, so the sampling cannot deliver anything, and counting from the
    // top so it cannot run out during the window.
    write(LVT_TIMER, u32::from(TIMER_VECTOR) | LVT_MASKED);
    write(DIVIDE, DIVIDE_BY_1);
    write(INITIAL_COUNT, u32::MAX);

    let apic_start = read(CURRENT_COUNT);
    let tsc_start = core::arch::x86_64::_rdtsc();
    // Long enough that the read overheads at each end are noise against it.
    while core::arch::x86_64::_rdtsc().wrapping_sub(tsc_start) < 2_000_000 {
        core::hint::spin_loop();
    }
    let tsc_end = core::arch::x86_64::_rdtsc();
    let apic_end = read(CURRENT_COUNT);

    write(INITIAL_COUNT, 0);

    let apic_ticks = u64::from(apic_start.saturating_sub(apic_end));
    let tsc_ticks = tsc_end.wrapping_sub(tsc_start);
    if apic_ticks == 0 || tsc_ticks == 0 {
        // The counter did not move. Say one to one rather than divide by zero;
        // the caller reports the ratio, so a suspicious 1:1 is visible.
        return (1, 1);
    }
    (tsc_ticks, apic_ticks)
}

/// Whether the timer's vector table entry currently says "one-shot, unmasked".
///
/// Every write here is an uncached store to a device the layer below emulates
/// and costs an exit of hv1's own, so the one that says nothing new is worth
/// not making: the entry only has to change when the timer goes from stopped to
/// running or back.
static mut LVT_IS_ARMED: bool = false;

/// Arm the timer to fire once, `count` ticks from now.
///
/// # Safety
///
/// As [`enable`]. The vector must have a gate in the loaded table.
pub unsafe fn arm_oneshot(count: u32) {
    // Unmasked, one-shot: bits 17 and 18 clear. Only when it is not already
    // saying that -- writing the count is what arms it, and the mode has not
    // changed since the last time.
    if !LVT_IS_ARMED {
        write(LVT_TIMER, u32::from(TIMER_VECTOR));
        LVT_IS_ARMED = true;
    }
    write(INITIAL_COUNT, count.max(1));
}

/// Stop it.
///
/// # Safety
///
/// As [`enable`].
pub unsafe fn disarm() {
    write(INITIAL_COUNT, 0);
    write(LVT_TIMER, u32::from(TIMER_VECTOR) | LVT_MASKED);
    LVT_IS_ARMED = false;
}

/// Arm a performance counter to raise an NMI after `cycles` unhalted cycles.
///
/// This is the only preemption source that works on a guest which has cleared
/// its interrupt flag. The timer cannot: an external-interrupt intercept
/// produces an exit when the interrupt *would be delivered*, and to a guest with
/// `IF` clear it never would be. An NMI is not maskable, so it arrives anyway.
///
/// The local vector table's timer entry has no delivery mode -- fixed is all it
/// supports -- and the performance-counter entry does. That is why this is a
/// counter of cycles rather than a second timer.
///
/// # Safety
///
/// Ring 0, an APIC page that is mapped, and a processor with AMD's legacy
/// performance counters.
pub unsafe fn nmi_after(cycles: u64) {
    write(LVT_PMC, LVT_NMI);
    // Stop it before moving the count, so an overflow cannot land in between.
    wrmsr(PERF_CTL0, 0);
    wrmsr(PERF_CTR0, PERF_WIDTH.wrapping_sub(cycles));
    wrmsr(PERF_CTL0, PERF_CYCLES);
}

/// Stop the counter.
///
/// # Safety
///
/// As [`nmi_after`].
pub unsafe fn nmi_off() {
    wrmsr(PERF_CTL0, 0);
    write(LVT_PMC, LVT_NMI | LVT_MASKED);
}

/// # Safety
///
/// Ring 0, and the MSR must exist.
unsafe fn wrmsr(msr: u32, value: u64) {
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") value as u32,
        in("edx") (value >> 32) as u32,
        options(nomem, nostack, preserves_flags),
    );
}

/// Whether an NMI arrived while the interrupt flag was clear.
///
/// Asked at boot rather than assumed, because the answer is a property of the
/// layer underneath -- this kernel is itself a guest, and whether its counters
/// and its APIC's delivery modes are virtualised is not something to find out
/// halfway through a scheduling decision.
///
/// # Safety
///
/// As [`nmi_after`], and it spins for the sampling window.
pub unsafe fn probe_nmi() -> (bool, u64, u64) {
    let before = core::ptr::read_volatile(core::ptr::addr_of!(HOST_NMIS));
    nmi_after(200_000);
    let armed = rdmsr(PERF_CTR0);
    let start = core::arch::x86_64::_rdtsc();
    // Interrupts are already off in this kernel and stay off: if this returns
    // true, it returned true with `IF` clear, which is the whole question.
    while core::arch::x86_64::_rdtsc().wrapping_sub(start) < 20_000_000 {
        if core::ptr::read_volatile(core::ptr::addr_of!(HOST_NMIS)) != before {
            let ended = rdmsr(PERF_CTR0);
            nmi_off();
            return (true, armed, ended);
        }
        core::hint::spin_loop();
    }
    let ended = rdmsr(PERF_CTR0);
    nmi_off();
    (false, armed, ended)
}

/// # Safety
///
/// Ring 0, and the MSR must exist.
unsafe fn rdmsr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack, preserves_flags),
    );
    (u64::from(high) << 32) | u64::from(low)
}

/// Let one pending interrupt in, and shut the door again.
///
/// Two flags stand between a pending interrupt and its handler, and this opens
/// both. `STGI` because `#VMEXIT` cleared the global interrupt flag — a level
/// `STI` cannot reach, and the reason a hypervisor which never executes it can
/// wait forever for an interrupt that has already arrived. Then a one
/// instruction window with `IF` set, which is where the handler runs.
///
/// # Safety
///
/// Requires `EFER.SVME` for `STGI`, and a loaded interrupt table for whatever
/// arrives.
pub unsafe fn take_pending() {
    core::arch::asm!("stgi", "sti", "nop", "cli", options(nostack));
}
