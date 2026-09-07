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

    // Initialising is not hosting. This is the first time anything has
    // run *under* hv1: a VMCB the hardware accepts, a guest that
    // executes, an exit that says why, and a second entry afterwards.
    // One entry proves a guest ran; two prove a loop.
    //
    // The nested page tables are the ones the trampoline built, read
    // back out of CR3 rather than passed down from it: this image is
    // identity mapped, so the guest's physical address space is the
    // host's, and CR3 is where that map already lives.
    // SAFETY: `initialize()` returned Ok above, so EFER.SVME is set;
    // this is ring 0 and the address space is identity mapped.
    match unsafe { guest::run() } {
        guest::Outcome::NotEnabled => {
            print(
                "hv1   guest: SVM reports disabled, so no guest was run
",
            );
        }
        guest::Outcome::Ran { first, second } => {
            report_exit(1, &first);
            report_exit(2, &second);
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

/// Print one guest exit.
fn report_exit(n: u32, exit: &guest::Exit) {
    print("hv1   guest exit ");
    // SAFETY: COM1, as in `print`.
    unsafe { outb(COM1, b'0' + n as u8) };
    print(" ");
    print_hex(exit.code);
    print(" ");
    print(guest::exit_name(exit.code));
    if exit.code == guest::VMEXIT_NPF {
        print(" at ");
        print_hex(exit.fault_addr);
    }
    print(
        "
",
    );
}

/// Read a model-specific register.
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
