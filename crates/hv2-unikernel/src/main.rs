//! A unikernel in Rust: the whole guest, in one binary, with no kernel under it.
//!
//! Every guest that has run in this repository until now was hand-assembled
//! bytes. That proves the hypervisor works and proves nothing about whether
//! anyone can *write* a guest for it, which is the only thing that matters for
//! an agent payload. This is compiled by the ordinary Rust toolchain, linked to
//! a Multiboot entry, and loaded by [`BootSource::Multiboot`].
//!
//! It is not a kernel plus an application. There is no scheduler, no init, no
//! allocator, no syscall boundary, because there is nothing on the other side
//! of one: `kernel_main` *is* the program, and when it halts the machine is
//! done. That is the security argument for an agent sandbox — a guest with no
//! kernel has no kernel attack surface — and it is why there is no second of
//! boot time to make faster.
//!
//! # What it proves
//!
//! Writing a byte to COM1 proves the image was loaded at the right address and
//! entered at the right instruction. Checking `EAX` proves more than that: the
//! Multiboot specification says a bootloader hands the kernel `0x2BADB002` in
//! `EAX` and its info structure in `EBX`, and nothing but the loader can put it
//! there. A guest that reports the magic has been booted *by the protocol*
//! rather than merely jumped to.
//!
//! # Building
//!
//! Not a workspace member: it targets 32-bit bare metal and cannot be built for
//! the host. `examples/rust_unikernel` in `hv2-core` builds and boots it, or by
//! hand:
//!
//! ```text
//! cargo build --release --target i686-unknown-linux-musl
//! ```
//!
//! The target is a Linux one and nothing here is Linux: it is used because it
//! is the 32-bit x86 target for which stable Rust ships a prebuilt `core`, and
//! `-nostdlib` with this crate's linker script leaves nothing of it in the
//! output. The alternative is a custom target JSON and `-Z build-std`, which
//! needs nightly for a binary that is otherwise entirely stable.

#![no_std]
#![no_main]

// Cargo reads `.cargo/config.toml` from the current directory upward, not from
// the manifest -- so `cargo build --manifest-path crates/hv2-unikernel/...`
// from the workspace root silently ignores this crate's target *and* its linker
// script, and builds a 64-bit host binary whose first error is
// "instruction requires: Not 64-bit mode" pointing at inline assembly. Which is
// true, and says nothing about the cause.
#[cfg(not(target_arch = "x86"))]
compile_error!(concat!(
    "hv2-unikernel is a 32-bit guest and must be built from its own directory, ",
    "so that cargo reads its .cargo/config.toml:
",
    "
    cd crates/hv2-unikernel && cargo build --release

",
    "Building it with --manifest-path from the workspace root ignores both the ",
    "target and the linker script.",
));

use core::arch::asm;
use core::panic::PanicInfo;

/// COM1's data port, which `Machine::legacy_pc` maps to an emulated 16550.
const COM1: u16 = 0x3F8;

/// What a Multiboot-compliant loader leaves in `EAX` before entering a kernel.
const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BAD_B002;

// The Multiboot header. Assembled here rather than declared as a Rust static
// because it has to be the first thing in the image, and a `#[link_section]`
// static is subject to whatever order the compiler feels like emitting statics
// in. The linker script places this section first; between them the header is
// at the image's first byte, well inside the 8 KB the specification allows.
//
//   magic     0x1BADB002, which is how a loader recognises the image at all
//   flags     0, so no alignment or memory-map requests
//   checksum  -(magic + flags), which the loader verifies sums to zero
core::arch::global_asm!(
    ".section .multiboot",
    ".align 4",
    ".long 0x1BADB002",
    ".long 0",
    ".long -(0x1BADB002)",
);

// The entry point. Written in assembly rather than Rust because Multiboot
// hands its two arguments over in registers and the C ABI this target uses
// expects them on the stack -- there is no way to spell "read EAX" in Rust
// before the first Rust statement has already clobbered it.
//
// The stack pointer the loader set is used as it is. `.text.entry` is what the
// linker script places first, so this is the image's first byte after the
// Multiboot header.
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "  push ebx", // second argument: the multiboot_info address
    "  push eax", // first argument:  the bootloader magic
    "  call kernel_main",
    // kernel_main does not return. If it somehow does, stop rather than
    // execute whatever follows in memory.
    "1:",
    "  hlt",
    "  jmp 1b",
);

/// Write one byte to an I/O port.
///
/// # Safety
///
/// Writing to an arbitrary port can do anything the device behind it does.
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

/// Write a string to COM1, one byte at a time.
fn print(text: &str) {
    for byte in text.as_bytes() {
        // SAFETY: COM1 is a serial data port; the machine model routes it to a
        // device that only records what it is given.
        unsafe { outb(COM1, *byte) };
    }
}

/// Write `value` as eight hex digits.
fn print_hex(value: u32) {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    print("0x");
    for shift in (0..8).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as usize;
        // SAFETY: as above.
        unsafe { outb(COM1, DIGITS[nibble]) };
    }
}

/// The whole program.
///
/// `magic` and `info` are what the bootloader left in `EAX` and `EBX`. Reported
/// rather than assumed: a guest that prints a greeting proves it was entered,
/// and a guest that prints the magic proves it was *booted*.
#[no_mangle]
pub extern "C" fn kernel_main(magic: u32, info: u32) -> ! {
    print("HYPERMACHINE RUST UNIKERNEL\n");

    print("magic ");
    print_hex(magic);
    if magic == MULTIBOOT_BOOTLOADER_MAGIC {
        print(" OK\n");
    } else {
        print(" WRONG\n");
    }

    print("info  ");
    print_hex(info);
    print("\n");

    // A unikernel has nowhere to return to. Halting is how it says it is done;
    // the vCPU takes a HLT exit and stops asking the host for time.
    loop {
        // SAFETY: `hlt` with interrupts disabled parks this vCPU for good,
        // which is the intended end of the program.
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// There is no unwinder and nowhere to report to but the console.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("PANIC\n");
    loop {
        // SAFETY: as in `kernel_main`.
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}
