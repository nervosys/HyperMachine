//! Getting from where a Multiboot loader leaves you to where a 64-bit guest
//! can run.
//!
//! Multiboot hands control over in 32-bit protected mode with paging off. This
//! guest is x86-64 and its first Rust instruction assumes long mode, so
//! something has to cross between the two, and nothing in Rust can express it:
//! the transition changes the width of the instruction pointer partway through
//! a function.
//!
//! The crossing is four steps, in this order and no other:
//!
//! 1. **Page tables**, because long mode requires paging. The map is the
//!    simplest one that reaches everything this guest is given: 4 GiB
//!    identity-mapped with 2 MiB pages.
//! 2. **`CR4.PAE`**, because 64-bit paging is PAE paging with another level on
//!    top. Setting it after `CR3` is fine; setting paging before either is a
//!    triple fault.
//! 3. **`EFER.LME`**, which arms long mode without entering it. The CPU is
//!    still executing 32-bit code and will be until the far jump.
//! 4. **`CR0.PG`**, which enters compatibility mode, followed immediately by a
//!    far jump through a 64-bit code descriptor. The jump is what makes the CPU
//!    64-bit; until it retires, a 64-bit instruction would decode as something
//!    else entirely.
//!
//! # Why four gigabytes rather than one
//!
//! Because of where the host puts things. `hv1-multiboot`, which this is
//! adapted from, maps 1 GiB, and that is enough for a hypervisor that only
//! touches its own image. This guest also reads a shared read-only region the
//! host maps at 3.5 GiB and talks to a virtio-mmio window at 3.25 GiB, and an
//! address that is not in the map is not slow, it is a page fault.
//!
//! The 32-bit guest needed none of this: with paging off, every address it
//! could form was already its physical address. Paging is the price of long
//! mode and the map is the smallest one that pays it — four PDPT entries and
//! 2,048 directory entries, written in a loop, in `.bss` so they cost nothing
//! in the image.

use core::arch::global_asm;

// The Multiboot header, with the address fields rather than without.
//
// Flags bit 16 says the five addresses that follow are present and
// authoritative. That is not a stylistic choice: this is an ELF64, and the
// specification's other path -- read the image's own ELF headers -- is defined
// for ELF32 only. The 32-bit version of this guest used that path and did not
// need this one.
//
// The addresses come from the linker script, so the header describes wherever
// the image was actually linked rather than wherever it was assumed to be.
global_asm!(
    r#"
    .section .multiboot, "a"
    .align 8
mb_header:
    .long 0x1BADB002                                    // magic
    .long 0x00010000                                    // flags: address fields present
    .long -(0x1BADB002 + 0x00010000)                    // checksum
    .long mb_header                                     // header_addr
    .long __load_start                                  // load_addr
    .long __load_end                                    // load_end_addr
    .long __bss_end                                     // bss_end_addr
    .long _start                                        // entry_addr
"#
);

// The 32-bit half. Everything from `_start` to the far jump runs in protected
// mode; everything after it runs in long mode, in the same section, which is
// why the assembler is told where one ends and the other begins.
global_asm!(
    r#"
    .section .boot, "ax"
    .code32
    .global _start
_start:
    // EAX holds the bootloader magic and EBX the multiboot_info address. They
    // are the only two things the loader tells us, and everything below
    // clobbers EAX. Parked in EDI and ESI, which nothing here touches and which
    // are where the 64-bit ABI wants the first two arguments anyway.
    //
    // Not pushed. The stack cannot carry them across the transition: a 32-bit
    // `push` writes four bytes and a 64-bit `pop` reads eight, so the first pop
    // would return both values glued together. Registers survive the far jump
    // untouched, and a 32-bit write zeroes the upper half of its 64-bit
    // register, so EDI and ESI arrive as correct RDI and RSI for free.
    mov edi, eax
    mov esi, ebx

    // ── 1. Page tables: 4 GiB identity, 2 MiB pages ────────────────────
    // PML4[0] -> PDPT, present + writable.
    mov eax, offset pdpt
    or eax, 0x3
    mov [pml4], eax
    mov dword ptr [pml4 + 4], 0

    // PDPT[0..4] -> the four page directories, present + writable. Four,
    // because each directory's 512 entries of 2 MiB cover one gigabyte and the
    // host maps things as high as 3.5.
    mov ecx, 0
1:
    mov eax, ecx
    shl eax, 12
    add eax, offset pd
    or eax, 0x3
    mov [pdpt + ecx * 8], eax
    mov dword ptr [pdpt + ecx * 8 + 4], 0
    inc ecx
    cmp ecx, 4
    jb 1b

    // pd[i] -> i * 2 MiB, present + writable + page-size. 2,048 of them,
    // laid out as one flat array across the four directories.
    mov ecx, 0
2:
    mov eax, 0x200000
    mul ecx                 // edx:eax = i * 2 MiB
    or eax, 0x83            // present | writable | PS
    mov [pd + ecx * 8], eax
    mov [pd + ecx * 8 + 4], edx
    inc ecx
    cmp ecx, 2048
    jb 2b

    mov eax, offset pml4
    mov cr3, eax

    // ── 2. CR4.PAE ─────────────────────────────────────────────────────
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    // ── 3. EFER.LME ────────────────────────────────────────────────────
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    // ── 4. CR0.PG, then the far jump that makes it 64-bit ──────────────
    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax

    lgdt [gdt64_pointer]
    // The loader's GDT has no 64-bit code descriptor in it, so this jump is
    // through ours. 0x08 is its first entry after the null, and it is the
    // selector every IDT gate is written with for the same reason.
    ljmp 0x08, offset long_mode

    .code64
long_mode:
    // Null the data segments. Long mode ignores their bases, but a stale
    // selector from the loader's GDT refers to a descriptor that no longer
    // exists, and something will eventually load from it.
    xor ax, ax
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    // RDI and RSI already hold the magic and the info address, put there
    // before the transition. The stack is ours from here: page-aligned, so the
    // `call` below leaves the ABI's alignment exactly as a callee expects it.
    mov rsp, offset boot_stack_top
    call kernel_main

    // kernel_main does not return.
3:
    hlt
    jmp 3b

    // ── The 64-bit GDT ─────────────────────────────────────────────────
    // Two entries: null, and a 64-bit code segment. No data descriptor,
    // because long mode does not consult one for data accesses.
    .align 16
gdt64:
    .quad 0                                 // null
    .quad 0x00AF9A000000FFFF                // code: present, ring 0, L=1
gdt64_end:
gdt64_pointer:
    .word gdt64_end - gdt64 - 1
    .quad gdt64

    // ── Zeroed space, which the loader guarantees ──────────────────────
    // In .bss, so these cost nothing in the image and arrive as zeros. Page
    // tables built on top of whatever was in RAM would be a triple fault with
    // no diagnostic at all.
    //
    // A page of nothing sits under the stack, so that an overflow faults on its
    // own address rather than quietly rewriting a page table. That ordering was
    // established the expensive way in `hv1-multiboot`, where a 4 KiB structure
    // built by value on a 16 KiB stack overflowed into the page directory and
    // the machine died several instructions later inside a print.
    .section .bss, "aw", @nobits
    .align 4096
pml4:
    .skip 4096
pdpt:
    .skip 4096
pd:
    .skip 16384
stack_guard:
    .skip 4096
boot_stack_bottom:
    .skip 131072
boot_stack_top:
"#
);
