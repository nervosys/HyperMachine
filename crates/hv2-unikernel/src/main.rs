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
    "so that cargo reads its .cargo/config.toml:\n",
    "\n    cd crates/hv2-unikernel && cargo build --release\n\n",
    "Building it with --manifest-path from the workspace root ignores both the ",
    "target and the linker script.",
));

mod interrupts;
mod mem;
mod vsock;

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
pub(crate) fn print(text: &str) {
    for byte in text.as_bytes() {
        // SAFETY: COM1 is a serial data port; the machine model routes it to a
        // device that only records what it is given.
        unsafe { outb(COM1, *byte) };
    }
}

/// Write `value` as eight hex digits.
pub(crate) fn print_hex(value: u32) {
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

    // If a shared read-only region has been mapped, read a byte of it and say
    // so. A region that is mapped and unreadable costs the host exactly the
    // same and is worth nothing, so the host measuring one needs a guest that
    // has actually looked at it.
    //
    // Absent, the address reads as zero — this guest has no page tables of its
    // own and an unmapped guest-physical address is not a fault it can take, so
    // silence and a zero are the same thing and the marker is what tells them
    // apart.
    {
        const ROM_BASE: u32 = 0xE000_0000;
        // SAFETY: a single byte read from a guest-physical address the host
        // either mapped read-only or left unmapped; neither faults here.
        // SAFETY: a single byte read from a guest-physical address the host
        // either mapped read-only or left unmapped; neither faults here.
        let marker = unsafe { core::ptr::read_volatile(ROM_BASE as *const u8) };
        print("rom ");
        print_hex(marker as u32);

        // And then try to write it. A shared region is only safe to share if
        // the hardware refuses this: one writable copy read by a thousand
        // agents is a thousand agents able to rewrite each other's model. The
        // write is expected to be dropped and the byte to be unchanged, and
        // reporting the read-back is what turns "should be read-only" into
        // something a host can check.
        // SAFETY: the write is the thing under test; the region is either
        // read-only, in which case the hardware refuses it, or unmapped, in
        // which case it goes nowhere.
        unsafe { core::ptr::write_volatile(ROM_BASE as *mut u8, 0x00) };
        // SAFETY: as for the read above.
        let after = unsafe { core::ptr::read_volatile(ROM_BASE as *const u8) };
        if after == marker {
            print(" write refused\n");
        } else {
            print(" WRITE TOOK EFFECT\n");
        }

        // The host may also have left a working-set size in the region: how
        // many mebibytes of its own memory this agent should touch before it
        // reports for duty. It stands in for a KV cache, which is the one part
        // of an agent that cannot be shared with any other agent and is
        // therefore the thing that decides how many of them fit.
        //
        // One byte per page, not a full write. Residency is per page, so
        // touching a page is what costs it; writing the other 4,095 bytes would
        // measure memory bandwidth instead.
        if marker != 0xFF {
            // SAFETY: four bytes from the same read-only region.
            let work_mib = unsafe { core::ptr::read_volatile((ROM_BASE + 4) as *const u32) };
            if work_mib > 0 {
                const WORK_BASE: u32 = 16 * 1024 * 1024;
                const PAGE: u32 = 4096;
                let pages = work_mib * (1024 * 1024 / PAGE);
                for page in 0..pages {
                    // SAFETY: guest RAM this agent owns, above its own image
                    // and below the memory it was configured with. The host
                    // sizes the VM so that this fits.
                    unsafe {
                        core::ptr::write_volatile((WORK_BASE + page * PAGE) as *mut u8, 0xC5);
                    }
                }
                print("work ");
                print_hex(work_mib);
                print("\n");
            }
        }

        // The host may also have left a working-set size in the region: how
        // many mebibytes of its own memory this agent should touch before it
        // reports for duty. It stands in for a KV cache, which is the one part
        // of an agent that cannot be shared with any other agent and is
        // therefore the thing that decides how many of them fit.
        //
        // One byte per page, not a full write. Residency is per page, so
        // touching a page is what costs it; writing the other 4,095 bytes would
        // measure memory bandwidth instead.
        if marker != 0xFF {
            // SAFETY: four bytes from the same read-only region.
            let work_mib = unsafe { core::ptr::read_volatile((ROM_BASE + 4) as *const u32) };
            if work_mib > 0 {
                const WORK_BASE: u32 = 16 * 1024 * 1024;
                const PAGE: u32 = 4096;
                let pages = work_mib * (1024 * 1024 / PAGE);
                for page in 0..pages {
                    // SAFETY: guest RAM this agent owns, above its own image
                    // and below the memory it was configured with. The host
                    // sizes the VM so that this fits.
                    unsafe {
                        core::ptr::write_volatile((WORK_BASE + page * PAGE) as *mut u8, 0xC5);
                    }
                }
                print("work ");
                print_hex(work_mib);
                print("\n");
            }
        }

        // The host may also have left a working-set size in the region: how
        // many mebibytes of its own memory this agent should touch before it
        // reports for duty. It stands in for a KV cache, which is the one part
        // of an agent that cannot be shared with any other agent and is
        // therefore the thing that decides how many of them fit.
        //
        // One byte per page, not a full write. Residency is per page, so
        // touching a page is what costs it; writing the other 4,095 bytes would
        // measure memory bandwidth instead.
        if marker != 0xFF {
            // SAFETY: four bytes from the same read-only region.
            let work_mib = unsafe { core::ptr::read_volatile((ROM_BASE + 4) as *const u32) };
            if work_mib > 0 {
                const WORK_BASE: u32 = 16 * 1024 * 1024;
                const PAGE: u32 = 4096;
                let pages = work_mib * (1024 * 1024 / PAGE);
                for page in 0..pages {
                    // SAFETY: guest RAM this agent owns, above its own image
                    // and below the memory it was configured with. The host
                    // sizes the VM so that this fits.
                    unsafe {
                        core::ptr::write_volatile((WORK_BASE + page * PAGE) as *mut u8, 0xC5);
                    }
                }
                print("work ");
                print_hex(work_mib);
                print("\n");
            }
        }
    }

    // Everything above proves the guest was booted. Everything below is the
    // guest being an agent: a swarm message arrives over vsock, and the
    // answer goes back the same way.
    match vsock::Vsock::init() {
        Ok(mut device) => {
            print("vsock cid ");
            print_hex(device.cid() as u32);
            print("\n");

            // An IDT and a programmed PIC, so the loop below can sleep. Without
            // them any interrupt is a triple fault, which is why this driver
            // used to spin.
            // SAFETY: called once, before anything relies on being woken.
            unsafe { interrupts::init() };
            print("idle mode: hlt\n");

            serve(&mut device)
        }
        Err(e) => {
            // Not fatal. A VM with no vsock device attached is a perfectly
            // good unikernel, and saying so beats halting silently.
            print("vsock ");
            print(e.as_str());
            print("\n");
            halt()
        }
    }
}

/// Answer the host until the VM is stopped.
///
/// The whole protocol this agent speaks:
///
///   REQUEST  -> RESPONSE      the host opened a connection
///   RW       -> RW            a message arrived; the answer is its echo
///   SHUTDOWN -> RST           the host is done
///
/// An echo rather than anything cleverer, because what is being demonstrated
/// is that the bytes crossed the boundary in both directions. A guest that
/// receives a message and answers with something unrelated proves only the
/// first half.
fn serve(device: &mut vsock::Vsock) -> ! {
    loop {
        device.ack_interrupt();

        // Interrupts off across the check. A device that raises its line
        // between the ring being found empty and the CPU halting would
        // otherwise be missed, and the guest would sleep until something else
        // happened to wake it — the kind of bug that presents as occasional
        // seconds of latency and nothing else.
        // SAFETY: re-enabled below, on both paths.
        unsafe { interrupts::disable() };

        let Some(packet) = device.recv() else {
            // Nothing waiting, so stop asking. A halted vCPU is a thread
            // blocked in `KVM_RUN`, which is a thread the host never schedules:
            // an idle agent costs a parked thread and no CPU at all.
            //
            // `sti; hlt` as one pair, never separated — `sti` does not take
            // effect until after the instruction following it, so an interrupt
            // cannot arrive between enabling them and halting.
            // SAFETY: `interrupts::init` ran before this loop, so something can
            // wake this CPU again.
            unsafe { interrupts::wait_for_interrupt() };
            continue;
        };

        // SAFETY: there is work in hand and the handler is not needed to find
        // it; interrupts stay on while it is processed.
        unsafe { interrupts::enable() };

        match packet.header.op {
            vsock::op::REQUEST => {
                print("vsock connect\n");
                device.reply(&packet.header, vsock::op::RESPONSE, &[]);
            }
            vsock::op::RW => {
                print("agent recv \"");
                print_bytes(packet.payload_at, packet.payload_len);
                print("\"\n");

                // Echo the payload straight back out of the receive buffer.
                // Nothing is copied because there is nowhere to copy to: this
                // guest has no allocator.
                echo(device, &packet);
            }
            vsock::op::SHUTDOWN => {
                device.reply(&packet.header, vsock::op::RST, &[]);
            }
            // A credit request wants a report, and every packet this driver
            // sends carries one.
            vsock::op::CREDIT_REQUEST => {
                device.reply(&packet.header, vsock::op::CREDIT_UPDATE, &[]);
            }
            _ => {}
        }

        device.release(&packet);
    }
}

/// Send a packet's payload back to the host, reading it from the receive
/// buffer a byte at a time.
fn echo(device: &mut vsock::Vsock, packet: &vsock::Packet) {
    // Bounded by the transmit buffer, which is one page: a longer message is
    // answered with as much of itself as fits rather than corrupting memory
    // past the buffer. Nothing in this swarm sends one that long.
    const MAX: u32 = 4096 - vsock::HEADER_SIZE as u32;
    let len = packet.payload_len.min(MAX) as usize;

    let mut scratch = [0u8; 256];
    let len = len.min(scratch.len());
    for (i, byte) in scratch.iter_mut().enumerate().take(len) {
        // SAFETY: `payload_at + i` is inside the receive buffer the device
        // wrote, bounded by `payload_len` above.
        *byte = unsafe { core::ptr::read_volatile((packet.payload_at + i as u32) as *const u8) };
    }
    device.reply(&packet.header, vsock::op::RW, &scratch[..len]);
}

/// Write `len` bytes of guest memory at `at` to the console.
fn print_bytes(at: u32, len: u32) {
    for i in 0..len {
        // SAFETY: the caller passes a receive buffer and the length the device
        // reported writing into it.
        let byte = unsafe { core::ptr::read_volatile((at + i) as *const u8) };
        // SAFETY: COM1, as in `print`.
        unsafe { outb(COM1, byte) };
    }
}

/// Stop, for good.
fn halt() -> ! {
    loop {
        // SAFETY: `hlt` with interrupts disabled parks this vCPU, which is the
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
