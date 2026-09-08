//! Run the Type-1 hypervisor, rather than compiling it.
//!
//! `hv1-core` has been graded "typed" on every honest accounting this project
//! has made of itself: EL2, stage-2, VMX, SVM, all of it compiling and none of
//! it ever executed. The reason was always the same — it is a bare-metal
//! hypervisor, and nobody had bare metal to put it on.
//!
//! This is not bare metal either, and says so. It is `hv1-core` built as a
//! Multiboot image and booted by `hv2-core`, the Type-2 hypervisor in the same
//! repository, on a host whose CPU exposes AMD-V to its guests. What that
//! proves is bounded and worth stating exactly:
//!
//! - **It proves the code runs.** `initialize()` executes, `CPUID` is read on a
//!   real CPU, and whatever it decides is reported over COM1. Every previous
//!   claim about this crate was a claim about `rustc`.
//! - **It does not prove it works on hardware.** A guest sees a CPU its
//!   hypervisor chose to show it. Nested SVM is a real feature and this is a
//!   real `VMRUN` path, but the layer underneath is KVM, not a machine.
//! - **It does not prove it can host anything.** Initialising is the first
//!   step; a hypervisor that initialises and cannot run a guest is a hypervisor
//!   that initialises.
//!
//! The distinction matters because the alternative to running it here was not
//! running it at all, and "compiles" was being carried on the front page as
//! though it were a fourth supported model.
//!
//! # Building
//!
//! From this directory, so cargo reads `.cargo/config.toml`:
//!
//! ```text
//! cd crates/hv1-multiboot && cargo build --release
//! ```

#![no_std]
#![no_main]

// Cargo reads `.cargo/config.toml` from the current directory upward rather
// than from the manifest, so `--manifest-path` from the workspace root builds
// this for the host and fails somewhere unhelpful.
#[cfg(not(target_arch = "x86_64"))]
compile_error!(concat!(
    "hv1-multiboot is a 64-bit bare-metal image and must be built from its own ",
    "directory, so that cargo reads its .cargo/config.toml:\n",
    "\n    cd crates/hv1-multiboot && cargo build --release\n\n",
    "Building it with --manifest-path from the workspace root ignores both the ",
    "target and the linker script.",
));

mod apic;
mod boot;
mod guest;
mod mem;

use core::arch::asm;
use core::panic::PanicInfo;

use linked_list_allocator::LockedHeap;

/// `hv1-core` allocates — its VMCB and host save areas, among other things —
/// so a bare-metal image of it has to say where from. On real hardware the heap
/// comes from the firmware's memory map; here it is a fixed block, because a
/// guest whose entire memory map is known at link time does not need to
/// discover one.
#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();

/// One megabyte, in `.bss`, so it costs nothing in the image and the loader
/// zeroes it. Chosen to be obviously enough rather than measured: the failure
/// mode of too little is an allocation error inside `initialize()`, reported as
/// `AllocationFailed`, which would be a confusing way to learn about a
/// constant.
const HEAP_SIZE: usize = 1024 * 1024;
static mut HEAP_SPACE: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

/// COM1's data port, which `Machine::legacy_pc` maps to an emulated 16550.
const COM1: u16 = 0x3F8;

/// What a Multiboot-compliant loader leaves in `EAX`.
const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BAD_B002;

/// Write one byte to an I/O port.
///
/// # Safety
///
/// COM1 on this machine is an emulated 16550 whose only effect is to append to
/// the host's console buffer.
unsafe fn outb(port: u16, value: u8) {
    asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags),
    );
}

fn print(text: &str) {
    for byte in text.as_bytes() {
        // SAFETY: COM1 is a serial data port routed to a device model that only
        // records what it is given.
        unsafe { outb(COM1, *byte) };
    }
}

/// Write one byte to the console, as itself.
pub(crate) fn print_byte(byte: u8) {
    // SAFETY: COM1, as in `print`.
    unsafe { outb(COM1, byte) };
}

/// Write a small count in decimal.
///
/// `print_hex` is for addresses. Using it on "how many times" gives sixteen
/// digits of leading zeros in front of a two, which is a formatter chosen for
/// one job doing another.
fn print_dec(value: u64) {
    if value >= 10 {
        print_dec(value / 10);
    }
    print_byte(b'0' + (value % 10) as u8);
}

fn print_hex(value: u64) {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    print("0x");
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as usize;
        // SAFETY: as in `print`.
        unsafe { outb(COM1, DIGITS[nibble]) };
    }
}

/// Long mode, at last.
///
/// `magic` and `info` are what the loader left in `EAX` and `EBX`, carried
/// across the 32-bit half by `boot.rs`. Reported first, because if they are
/// wrong then nothing below means what it appears to.
///
/// # Safety
///
/// Called once, by the trampoline, with a valid stack and nothing else set up.
#[no_mangle]
pub extern "C" fn kernel_main(magic: u32, info: u32) -> ! {
    // Before anything that might allocate.
    // SAFETY: called once, on the only CPU running, before any allocation.
    // `HEAP_SPACE` is a static this image alone owns.
    unsafe {
        HEAP.lock()
            .init(core::ptr::addr_of_mut!(HEAP_SPACE) as *mut u8, HEAP_SIZE);
    }

    print("HV1 MULTIBOOT\n");

    print("magic ");
    print_hex(u64::from(magic));
    if magic == MULTIBOOT_BOOTLOADER_MAGIC {
        print(" OK\n");
    } else {
        print(" WRONG\n");
    }
    print("info  ");
    print_hex(u64::from(info));
    print("\n");

    // Long mode is only claimed once it has been checked. `EFER.LMA` is set by
    // the CPU, not by the trampoline, so reading it back distinguishes "we
    // asked for long mode" from "we are in it".
    let efer = read_msr(0xC000_0080);
    print("efer  ");
    print_hex(efer);
    print(if efer & (1 << 10) != 0 {
        " LMA — 64-bit\n"
    } else {
        " no LMA\n"
    });

    // This kernel's own local APIC, which it needs for the same reason its
    // guest does: it is the only clock a processor has that it can set. The
    // base address is in a model-specific register rather than being assumed,
    // because it is relocatable and because the enable bit is in the same
    // register -- and a kernel that reads 0xFEE00000 without checking either
    // is reading whatever the last thing to own that page left behind.
    let apic_base = read_msr(IA32_APIC_BASE);
    print("apic  ");
    print_hex(apic_base & 0xFFFF_F000);
    print(if apic_base & APIC_BASE_ENABLE != 0 {
        " enabled"
    } else {
        " DISABLED"
    });
    print(if apic_base & APIC_BASE_BSP != 0 {
        ", bootstrap processor, id "
    } else {
        ", application processor, id "
    });
    // SAFETY: the page is identity-mapped uncached by the trampoline, and this
    // is a read of an architectural register.
    let id = unsafe { apic::read(apic::ID) } >> 24;
    print_dec(u64::from(id));
    print("
");

    // An interrupt table before anything can interrupt, then the APIC on, then
    // the one measurement that cannot be looked up: how fast its counter runs
    // against the timestamp counter the rest of this kernel uses.
    //
    // SAFETY: called once, on the only processor running, before interrupts
    // are enabled anywhere and before any guest runs.
    let ratio = unsafe {
        apic::load_idt();
        apic::enable();
        apic::calibrate()
    };
    // SAFETY: written once here, before any guest runs and before any other
    // code reads it.
    unsafe { CLOCK_RATIO = ratio };
    print("clock ");
    print_dec(ratio.0);
    print(" timestamp ticks per ");
    print_dec(ratio.1);
    print(" APIC ticks
");

    // What the CPU says it can do, before asking hv1 what it made of it. These
    // two lines are the difference between "hv1 refused" and "there was nothing
    // to accept".
    let (_, _, ecx, _) = cpuid(0x8000_0001);
    print("svm   ");
    print(if ecx & (1 << 2) != 0 {
        "available\n"
    } else {
        "absent\n"
    });
    let (_, _, ecx1, _) = cpuid(1);
    print("vmx   ");
    print(if ecx1 & (1 << 5) != 0 {
        "available\n"
    } else {
        "absent\n"
    });

    // ── The thing this image exists for ─────────────────────────────────
    print("hv1   version ");
    print(hv1_core::version());
    print("\n");

    match hv1_core::initialize() {
        Ok(()) => {
            print("hv1   initialize() OK\n");
            print(if hv1_core::is_initialized() {
                "hv1   is_initialized() true\n"
            } else {
                "hv1   is_initialized() FALSE after an Ok — inconsistent\n"
            });

            // The verdict is the easy half. On an AMD CPU `initialize()`
            // ends in `svm::initialize()`, whose whole architectural effect
            // is setting `EFER.SVME` — so reading the MSR back is the
            // difference between a function that returned `Ok` and a CPU
            // that is now in a state it was not in before. Without this the
            // interesting failure is invisible: an `initialize()` that
            // succeeds, changes nothing, and fails at the first `VMRUN`.
            let after = read_msr(0xC000_0080);
            print("hv1   efer now ");
            print_hex(after);
            print(if after & (1 << 12) != 0 {
                " SVME set — the CPU accepted SVM\n"
            } else {
                " SVME CLEAR — initialize() succeeded and enabled nothing\n"
            });
        }
        Err(e) => {
            // Named, not swallowed. "It failed" is the same sentence for a CPU
            // with no virtualisation and for a hypervisor that cannot read
            // CPUID, and those want different fixes.
            print("hv1   initialize() failed: ");
            print(error_name(e));
            print("\n");
        }
    }

    // Initialising is not hosting, and neither is entering a guest twice.
    // What makes this a hypervisor is the loop below: an emulated serial
    // port the guest writes a line to, hypercalls answered and stepped
    // over, and an interrupt injected into a halted guest that its own
    // handler receives and returns from.
    //
    // The nested page tables are the guest's own now, and they translate:
    // guest-physical zero is the base of a 2 MiB region this image owns,
    // and everything above it is absent. They used to identity-map the
    // first gigabyte, which was survivable only because a four-byte guest
    // touches nothing -- this one has a stack and an interrupt table at
    // its address zero.
    // SAFETY: `initialize()` returned Ok above, so EFER.SVME is set;
    // this is ring 0 and the address space is identity mapped.
    match unsafe { guest::run() } {
        guest::Outcome::NotEnabled => {
            print(
                "hv1   guest: SVM reports disabled, so no guest was run
",
            );
        }
        guest::Outcome::BaseMismatch { asm, rust } => {
            print("hv1   guest: the guest's assembly was built for ");
            print_hex(asm);
            print(" and this module loads it at ");
            print_hex(rust);
            print(" — a far jump into nothing, refused before it happened
");
        }
        guest::Outcome::Ran(log) => {
            // Runs of identical exits are collapsed. A serial port written a
            // byte at a time produces sixty consecutive lines that say the same
            // thing, which is not a log, it is a wall — and it overran the
            // console buffer on the way, truncating the summary that comes
            // after it. Sixty identical lines and "x60" carry the same
            // information.
            let mut n = 0;
            while n < log.count {
                let mut run = 1;
                while n + run < log.count
                    && log.exits[n + run].code == log.exits[n].code
                    && log.exits[n + run].answer.as_ptr() == log.exits[n].answer.as_ptr()
                {
                    run += 1;
                }
                report_exit(n + 1, &log.exits[n], run);
                n += run;
            }
            print("hv1   stopped: ");
            print(log.stopped);
            print("
");
            guest::report(&log);

            // The three claims, each separately checkable, in the order they
            // have to happen in.
            print("hv1   device    : ");
            print(if log.console_len > 0 {
                "the guest wrote to a port and this hypervisor was what answered"
            } else {
                "FAILED — the guest never reached its serial port"
            });
            print("
");
            print("hv1   protected : ");
            print(if log.protected_mode {
                "the guest built a GDT and an IDT and crossed into 32-bit mode"
            } else {
                "FAILED — the guest never reported reaching protected mode"
            });
            print("
");
            if log.faulted {
                print("hv1   fault     : the guest took a processor exception of its own
");
            }
            print("hv1   ring      : ");
            if log.ring_len > 0 {
                print("the guest sent \"");
                for byte in &log.ring[..log.ring_len] {
                    print_byte(*byte);
                }
                print("\" through a descriptor it filled in
");
            } else {
                print("FAILED — nothing arrived through the ring
");
            }
            print("hv1   returned  : ");
            print(if log.ring_returned {
                "the guest read the reply back out of the buffer it named"
            } else {
                "FAILED — the guest never came back from the ring"
            });
            print("
");
            print("hv1   refused   : ");
            print(if log.ring_refused {
                "a descriptor pointing outside the guest's own memory was not followed"
            } else {
                "FAILED — an out-of-range descriptor was followed, which is a guest reading its hypervisor"
            });
            print("
");
            print("hv1   apic      : ");
            print(if log.apic_enabled {
                "software-enabled by the guest, "
            } else {
                "FAILED — never turned on, "
            });
            print_dec(log.eois as u64);
            print(" end-of-interrupt");
            print(if log.identified[0] && log.identified[1] {
                ", and both processors read their own identity from one instruction
"
            } else {
                ", and FAILED — a processor never read its identity
"
            });
            print("hv1   timer     : ");
            if log.timer_counts && log.timer_served && log.ap_timer_served {
                print_dec(log.ticks[0] as u64);
                print(" ticks on the first processor and ");
                print_dec(log.ticks[1] as u64);
                print(" on the second, each on the vector it chose, both while spinning
");
                // What used to be reported here -- the drop in the guest's
                // own count between arming its timer and reading it back --
                // was described as the cost of two nested exits, and stopped
                // being that the moment there were two processors to schedule.
                // The other one now runs in the middle of it, so the number
                // includes a slice of somebody else's work and grew from
                // ~380,000 to ~20,000,000 without anything getting slower.
                //
                // A measurement whose name stopped being true is worse than no
                // measurement, so it is gone. What is left is the one taken
                // directly, with a timestamp on either side of the writes.
                print("hv1   arm cost  : ");
                print_dec(log.arm_cost);
                print(" timestamp ticks for two uncached writes to hv1's own timer
");
            } else {
                print("FAILED — the guest armed a timer and did not get what it asked for
");
            }
            print("hv1   half a start: ");
            print(if log.ap_refused {
                "a startup that skipped the reset was refused
"
            } else {
                "FAILED — a startup that skipped the reset was obeyed
"
            });
            print("hv1   second cpu: ");
            if log.ap_started && log.ap_ran && log.ap_seen {
                print("INIT and STARTUP through the APIC page, up through real mode, and the first processor saw its work in shared memory
");
            } else if log.ap_started {
                print("FAILED — started and did not get where it was going
");
            } else {
                print("FAILED — never started
");
            }
            print("hv1   switches  : ");
            print_dec(log.switches as u64);
            print(" times the hypervisor moved from one processor to the other
");
            print("hv1   interrupt : ");
            print(if log.interrupt_handled {
                "injected 0x20 while halted, and the guest's own handler ran"
            } else {
                "FAILED — the vector was injected and no handler ran"
            });
            print("
");
            print("hv1   resumed   : ");
            print(if log.resumed {
                "the handler's iret returned, and the guest carried on"
            } else {
                "FAILED — the guest never came back from its handler"
            });
            print("
");
        }
    }

    print("hv1   done\n");
    halt()
}

/// A name for an `hv1_core::Error`.
///
/// `Display` needs a formatter and there is nothing here to format into, so the
/// four errors `initialize()` can actually return are named and the rest are
/// grouped. Naming them matters: "it failed" is the same sentence for a CPU
/// with no virtualisation and for a `VMXON` that was refused, and those want
/// entirely different fixes.
fn error_name(e: hv1_core::Error) -> &'static str {
    use hv1_core::Error;
    match e {
        Error::AlreadyInitialized => "AlreadyInitialized",
        Error::NoHardwareSupport => "NoHardwareSupport — CPUID reports neither VMX nor SVM",
        Error::UnsupportedCpu => "UnsupportedCpu — the vendor is neither Intel nor AMD",
        Error::VmxInitFailed => "VmxInitFailed — VMX was present and would not start",
        Error::SvmInitFailed => "SvmInitFailed — SVM was present and would not start",
        Error::VmxonFailed => "VmxonFailed",
        Error::AllocationFailed => "AllocationFailed",
        Error::OutOfMemory => "OutOfMemory",
        Error::NotSupported => "NotSupported",
        _ => "an error initialize() was not expected to return",
    }
}

/// Print one guest exit, and what the hypervisor did about it.
///
/// The answer is the half that matters. An exit log is a list of things that
/// happened to a guest; a hypervisor is the column next to it.
fn report_exit(n: usize, exit: &guest::Exit, run: usize) {
    print("hv1   exit ");
    // Three digits, because the log holds a hundred and sixty. Two of them
    // printed exit 100 as `;0`, which is what `b'0' + 10` is — a counter that
    // outgrew its formatter, and unreadable in exactly the place a reader goes
    // looking when something has gone wrong.
    print_byte(b'0' + (n / 100) as u8);
    print_byte(b'0' + ((n / 10) % 10) as u8);
    print_byte(b'0' + (n % 10) as u8);
    print("  ");
    print_hex(exit.code);
    print(" ");
    print(guest::exit_name(exit.code));
    print("  rip ");
    print_hex(exit.rip);
    print("  info ");
    print_hex(exit.info);
    print("  -> ");
    print(exit.answer);
    if run > 1 {
        print("  x");
        print_dec(run as u64);
    }
    print("
");
}

/// Read a model-specific register.
/// The measured ratio between the two clocks, as numerator and denominator.
/// Written once by `kernel_main` before any guest runs, read by the exit loop.
static mut CLOCK_RATIO: (u64, u64) = (1, 1);

/// Where the local APIC's base address and enable bit live.
const IA32_APIC_BASE: u32 = 0x1B;
/// Bit 11: the APIC is on at all. Bit 8: this is the bootstrap processor.
const APIC_BASE_ENABLE: u64 = 1 << 11;
const APIC_BASE_BSP: u64 = 1 << 8;

fn read_msr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    // SAFETY: `rdmsr` at ring 0 on an MSR the architecture defines. `EFER`
    // exists on every CPU that reached long mode, which this one has.
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

/// Execute `cpuid` and return `(eax, ebx, ecx, edx)`.
fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let (a, b, c, d): (u32, u32, u32, u32);
    // SAFETY: `cpuid` has no memory operands and is valid in any mode. `rbx` is
    // callee-saved and reserved by LLVM, so it is preserved by hand.
    unsafe {
        asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            ebx_out = out(reg) b,
            inout("eax") leaf => a,
            inout("ecx") 0u32 => c,
            out("edx") d,
            options(nomem, preserves_flags),
        );
    }
    (a, b, c, d)
}

/// Stop, for good.
fn halt() -> ! {
    loop {
        // SAFETY: `hlt` with interrupts disabled parks this CPU, which is the
        // intended end of the program.
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// There is no unwinder and nowhere to report to but the console.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("PANIC\n");
    halt()
}
