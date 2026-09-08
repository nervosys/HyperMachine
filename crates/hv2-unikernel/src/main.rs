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

extern crate alloc;

mod interrupts;
mod mem;
mod vsock;

use alloc::vec::Vec;
use hv2_agent_proto::{Header, Kind, HEADER_LEN};
use linked_list_allocator::LockedHeap;

use core::arch::asm;
use core::panic::PanicInfo;

/// COM1's data port, which `Machine::legacy_pc` maps to an emulated 16550.
const COM1: u16 = 0x3F8;

/// An allocator, so an agent's messages are bounded by memory rather than by
/// what fits in a fixed array on the stack.
#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();

/// How much of one. Deliberately small.
///
/// Every byte of this is in `.bss`, and `.bss` is not free here the way it is
/// on an ordinary kernel: the Multiboot loader writes zeros across it at load
/// time, because guest RAM is only reliably zero for a freshly created VM and a
/// loader cannot assume it is being used that way. So the heap costs its full
/// size in resident memory *per agent*, and a fleet pays for it a thousand
/// times over.
///
/// 256 KiB. It was 64, chosen when a heap cost its full size in resident
/// memory per agent because the loader wrote zeros across `.bss`. It no longer
/// does — `.bss` is a range the loader skips when the guest's memory already
/// reads as zero — so a heap now costs the pages an agent actually touches, and
/// this number bounds what an agent *may* use rather than what it does.
///
/// Measured at a hundred agents: 0.242 MiB each with 64 KiB, and the same with
/// 256. The cost of the ceiling is now nearly nothing, which is the only reason
/// to raise it.
const HEAP_SIZE: usize = 256 * 1024;
static mut HEAP_SPACE: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

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
    // Align the stack before calling into Rust. The loader leaves ESP wherever
    // it likes, and the i386 SysV ABI requires ESP to be 16-byte aligned at the
    // point of a `call` — so that with the return address pushed, a callee's
    // frame is aligned. LLVM relies on that: with SSE enabled it spills to the
    // stack with `movaps`, which faults with #GP on a misaligned address.
    //
    // Nothing here needed it until this guest started doing arithmetic worth
    // vectorising. It presented as a #GP several function calls deep, long
    // after boot, in code that had been running for weeks.
    //
    // Two arguments are pushed after the alignment, so eight bytes are taken
    // off first to land back on a boundary at the `call`.
    "  and esp, -16",
    "  sub esp, 8",
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
    // Before anything at all. Everything below this line can fault, and a fault
    // with no handler is a console that stops mid-word — which is how the first
    // two attempts at an interrupt handler presented, and how the first attempt
    // at vectorised arithmetic presented after that.
    // SAFETY: called once, first.
    unsafe {
        interrupts::install_fault_handlers();
        interrupts::enable_sse();
        HEAP.lock()
            .init(core::ptr::addr_of_mut!(HEAP_SPACE) as *mut u8, HEAP_SIZE);
    }

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
        if marker == 0xFF {
            // Nothing mapped, so the write test below is skipped and this line
            // needs its own ending.
            print(
                "
",
            );
        }

        // Only if something is actually mapped there. An unmapped
        // guest-physical address reads as all ones, and *writing* one exits to
        // a host that has no device at that address — which stops the VM. The
        // write below is a test of read-only enforcement, and there is nothing
        // to enforce when there is nothing there.
        if marker != 0xFF {
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

            // And how many mebibytes of the shared region to stream through an
            // integer multiply-accumulate, which is the shape of the only loop
            // that matters in a quantised forward pass: read a weight, multiply
            // it by an activation, add it to a running sum. A token costs one
            // pass over every weight, so the rate at which a guest can do this
            // is the ceiling on how fast an agent can think.
            //
            // SAFETY: four more bytes of the same read-only region.
            let sweep_mib = unsafe { core::ptr::read_volatile((ROM_BASE + 8) as *const u32) };
            if sweep_mib > 0 {
                // Twice, reported separately. The first pass over a shared
                // region pays a nested-paging fault per page — one VM exit
                // each, 65,536 of them for 256 MiB — and that is paid once for
                // the life of the VM, not once per token. A single-pass number
                // conflates a one-off mapping cost with the steady-state rate,
                // and those answer different questions: how long an agent takes
                // to warm up, and how fast it can then think.
                let first = rdtsc();
                let a = mac_sweep(ROM_BASE, sweep_mib);
                let middle = rdtsc();
                let b = mac_sweep(ROM_BASE, sweep_mib);
                let last = rdtsc();

                print("sweep done ");
                print_hex(a ^ b);
                print(" cold ");
                print_hex((middle - first) as u32);
                print(" warm ");
                print_hex((last - middle) as u32);
                print(
                    "
",
                );
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

/// One request this agent has issued and has not been answered.
struct Pending {
    /// The id this guest put on the request.
    id: u32,
    /// What the request was for.
    ///
    /// Kept so that an answer can be reported as an answer to *this* request
    /// rather than merely as an answer. Without it, a guest pairing replies by
    /// arrival order would be indistinguishable from one reading the ids — up
    /// until the host answered out of order, which is exactly when it matters.
    what: Vec<u8>,
}

/// What the agent holds between packets.
struct Agent {
    /// The connection, remembered from the last packet that arrived on it.
    ///
    /// A frame is not a packet. Bytes left over from one packet are read as a
    /// frame while a later packet is in hand, so the header a reply takes its
    /// ports from cannot be the packet currently being processed.
    link: vsock::Header,
    /// Received bytes that are not yet a whole frame.
    inbox: Vec<u8>,
    /// Requests outstanding, in the order they were issued.
    pending: Vec<Pending>,
    /// The next id to put on a request this guest originates.
    ///
    /// This is the guest's own id space, and it is not the host's. A `Task`
    /// carries an id the host chose; a `ToolCall` carries one this counter
    /// chose. The two may collide and it is harmless, because a frame's kind
    /// says which direction it is answering — an `Error` arriving here answers
    /// something this guest asked for, and an `Error` leaving here refuses
    /// something the host sent.
    ///
    /// The guest used to answer a `Task` under the task's own id, which reads
    /// as tidy and cannot survive a task that produces two requests: they would
    /// share an id, and the first answer would settle whichever the guest
    /// happened to find first.
    next_id: u32,
}

impl Agent {
    fn new() -> Self {
        Self {
            link: vsock::Header::default(),
            inbox: Vec::new(),
            pending: Vec::new(),
            next_id: 1,
        }
    }

    /// Read every whole frame the stream now holds.
    ///
    /// A loop and not a single parse. Two frames written back to back arrive as
    /// one run of bytes — that is what a stream is — and the previous version
    /// read the first and discarded the rest along with the packet buffer. It
    /// never showed, because nothing had ever sent two.
    fn drain(&mut self, device: &mut vsock::Vsock) {
        while self.inbox.len() >= HEADER_LEN {
            let Some(header) = Header::decode(&self.inbox) else {
                // Twelve bytes are present and they do not decode, so this is a
                // kind this build does not know rather than a frame still
                // arriving. There is no resynchronising from that: the length
                // that would say where the next frame starts is part of what
                // could not be read.
                self.say(Header::new(0, Kind::Error, 0), b"unreadable frame", device);
                self.inbox.clear();
                return;
            };
            let end = HEADER_LEN + header.len as usize;
            if self.inbox.len() < end {
                // The ordinary case on a stream, and not an error.
                return;
            }
            let body: Vec<u8> = self.inbox[HEADER_LEN..end].to_vec();
            self.inbox.drain(..end);
            self.act(header, &body, device);
        }
    }

    /// Act on one frame.
    fn act(&mut self, header: Header, body: &[u8], device: &mut vsock::Vsock) {
        match header.kind {
            Kind::Task => {
                print("agent task ");
                print_id(header.id);
                print(" \"");
                print_slice(body);
                print("\"\n");
                self.take_on(body, device);
            }
            Kind::ToolResult => match self.settle(header.id) {
                Some(what) => {
                    print("agent done ");
                    print_id(header.id);
                    print(" \"");
                    print_slice(&what);
                    print("\" = \"");
                    print_slice(body);
                    print("\"\n");
                }
                None => self.stray(header.id),
            },
            Kind::Error => match self.settle(header.id) {
                Some(what) => {
                    // The first half of this line is what it always was, so a
                    // reader looking for a refusal still finds one. The tail is
                    // the new half: which of several outstanding requests was
                    // refused.
                    print("agent denied ");
                    print_id(header.id);
                    print(" \"");
                    print_slice(body);
                    print("\" was \"");
                    print_slice(&what);
                    print("\"\n");
                }
                None => self.stray(header.id),
            },
            Kind::Deliver => {
                // Received and not answered. The guest used to reply to
                // everything, which is a convenient property for a test and a
                // strange one for an agent: being reached by a peer is not a
                // question.
                print("agent recv ");
                print_id(header.id);
                print(" \"");
                print_slice(body);
                print("\"\n");
            }
            // A guest never receives these; it sends them.
            Kind::ToolCall | Kind::Send => {
                self.say(
                    Header::new(header.id, Kind::Error, 0),
                    b"not for a guest",
                    device,
                );
            }
        }
    }

    /// Turn a task into requests.
    ///
    /// Three rules, each one line, standing in for a model deciding what to do.
    /// What is under test is the frame, the id and the permission around them,
    /// none of which care how the decision was reached.
    ///
    /// ```text
    ///   send <to>:<text>     reach another agent
    ///   tools <a> <b> ...    call several tools, all at once
    ///   anything else        call one tool, named by the whole task
    /// ```
    fn take_on(&mut self, body: &[u8], device: &mut vsock::Vsock) {
        if let Some(rest) = strip(body, b"send ") {
            let id = self.issue(rest);
            self.say(Header::new(id, Kind::Send, 0), rest, device);
            print("agent send ");
            print_id(id);
            print("\n");
        } else if let Some(rest) = strip(body, b"tools ") {
            for name in rest.split(|byte| *byte == b' ').filter(|n| !n.is_empty()) {
                let id = self.issue(name);
                self.say(Header::new(id, Kind::ToolCall, 0), name, device);
                print("agent call ");
                print_id(id);
                print(" \"");
                print_slice(name);
                print("\"\n");
            }
        } else {
            let id = self.issue(body);
            self.say(Header::new(id, Kind::ToolCall, 0), body, device);
            print("agent call ");
            print_id(id);
            print(" \"");
            print_slice(body);
            print("\"\n");
        }

        // How many things this agent is holding at once. Printed because it is
        // the whole claim: a guest that reports two outstanding requests and
        // goes back to reading is not waiting for either of them.
        print("agent holds ");
        print_hex(self.pending.len() as u32);
        print("\n");
    }

    /// Record a request this guest is about to send, and give it an id.
    fn issue(&mut self, what: &[u8]) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.pending.push(Pending {
            id,
            what: what.to_vec(),
        });
        id
    }

    /// Take the outstanding request that `id` answers, if there is one.
    fn settle(&mut self, id: u32) -> Option<Vec<u8>> {
        let at = self.pending.iter().position(|p| p.id == id)?;
        Some(self.pending.remove(at).what)
    }

    /// An answer to nothing this guest asked for.
    ///
    /// Reported rather than ignored. Under the old scheme, where a reply was
    /// matched by being the only one outstanding, this could not be detected at
    /// all: whatever arrived was the answer.
    fn stray(&self, id: u32) {
        print("agent stray ");
        print_id(id);
        print("\n");
    }

    /// Write one frame to the host, filling in the length from the payload.
    fn say(&self, header: Header, payload: &[u8], device: &mut vsock::Vsock) {
        let header = Header::new(header.id, header.kind, payload.len() as u32);
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        let mut bytes = [0u8; HEADER_LEN];
        header.encode(&mut bytes);
        out.extend_from_slice(&bytes);
        out.extend_from_slice(payload);
        device.reply(&self.link, vsock::op::RW, &out);
    }
}

/// Answer the host until the VM is stopped.
///
/// The packet-level conversation:
///
/// ```text
///   REQUEST  -> RESPONSE      the host opened a connection
///   RW       -> RW            frames, in both directions
///   SHUTDOWN -> RST           the host is done
/// ```
///
/// Nothing here waits for an answer. A request is issued, recorded, and the
/// loop goes back to reading — which is why a tool call and a peer's message
/// can be outstanding at the same time, and why the host may answer them in
/// whichever order it likes.
fn serve(device: &mut vsock::Vsock) -> ! {
    let mut agent = Agent::new();

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
                agent.link = packet.header;
                agent.inbox.extend_from_slice(&payload_bytes(&packet));
                agent.drain(device);
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

/// `body` with `prefix` removed, if it starts with it.
fn strip<'a>(body: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if body.len() >= prefix.len() && &body[..prefix.len()] == prefix {
        Some(&body[prefix.len()..])
    } else {
        None
    }
}

/// Write a request id, so a console line can be matched to a frame.
fn print_id(id: u32) {
    print("#");
    print_hex(id);
}

/// Write bytes to the console.
fn print_slice(bytes: &[u8]) {
    for byte in bytes {
        // SAFETY: COM1, as in `print`.
        unsafe { outb(COM1, *byte) };
    }
}

/// Copy a packet's payload out of the receive buffer.
///
/// The buffer belongs to the device and is handed back to it as soon as the
/// packet is released, so anything that outlives the handler has to be copied.
/// With a heap that is a `Vec`; without one it was whatever fitted on the
/// stack.
fn payload_bytes(packet: &vsock::Packet) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.payload_len as usize);
    for i in 0..packet.payload_len {
        // SAFETY: inside the receive buffer the device wrote, bounded by the
        // length it reported.
        out.push(unsafe { core::ptr::read_volatile((packet.payload_at + i) as *const u8) });
    }
    out
}

/// The time-stamp counter, for splitting one measurement into two.
///
/// Only ever used as a ratio. Turning cycles into seconds needs the TSC
/// frequency, which this guest has no way to learn and does not need: the host
/// knows how long both passes took together, and the ratio says how to divide
/// it between them.
fn rdtsc() -> u64 {
    let (low, high): (u32, u32);
    // SAFETY: `rdtsc` has no operands and no side effects.
    unsafe {
        asm!("rdtsc", out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
    }
    (u64::from(high) << 32) | u64::from(low)
}

/// Multiply-accumulate over `mib` mebibytes starting at `base`.
///
/// The inner loop of a quantised forward pass, with the parts that do not
/// affect its cost left out: each byte is a weight, it is multiplied by a
/// varying activation, and the products are summed. Nothing here is a real
/// model — there is no matrix shape, no attention and no softmax — but the
/// memory traffic and the arithmetic per byte are the same, and those are what
/// decide how long a token takes.
///
/// Deliberately not optimised into a memcmp: the accumulator is returned and
/// printed, so the compiler cannot drop the loop, and the multiplier changes
/// each iteration so it cannot fold it into a shift.
fn mac_sweep(base: u32, mib: u32) -> u32 {
    let bytes = mib.saturating_mul(1024 * 1024) as usize;

    // A slice and an ordinary loop, not `read_volatile` per byte. The volatile
    // version measured 155 MiB/s, which is a number about load-store
    // serialisation rather than about inference: it forbids the compiler from
    // using a wide load, and a real kernel would use the widest it has. The
    // region does not change under us, so an ordinary read is also the correct
    // one.
    // SAFETY: inside the shared read-only region, whose length the host chose
    // to cover this sweep. It is mapped for the life of the VM and no guest can
    // write it.
    let weights = unsafe { core::slice::from_raw_parts(base as *const u8, bytes) };

    let mut sum: u32 = 0;
    for (i, &w) in weights.iter().enumerate() {
        sum = sum.wrapping_add(u32::from(w).wrapping_mul(i as u32 & 0xFF));
    }
    sum
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
