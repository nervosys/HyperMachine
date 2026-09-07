/* A 64-bit kernel a 32-bit Multiboot loader can start.
 *
 * The unusual part is that this image describes itself twice. It is an ELF64,
 * which the Multiboot specification's ELF path cannot load -- that path is
 * ELF32 -- so the header carries its own load addresses instead, and the
 * symbols below are what fills them in. `__load_start` is the first byte the
 * loader copies, `__load_end` the last, and `__bss_end` the end of the region
 * it must zero; between them they say "put this image here" without the loader
 * having to understand the format it is in.
 *
 * Section order is load-bearing. `.multiboot` is first so the header sits at
 * the load address and the arithmetic tying file offsets to memory addresses
 * is the identity. `.boot` follows it: 32-bit code, which is the mode a
 * Multiboot loader hands control over in, and which exists only to reach
 * 64-bit mode and never runs again.
 */

ENTRY(_start)

SECTIONS
{
    . = 1M;
    __load_start = .;

    .multiboot : ALIGN(8)
    {
        KEEP(*(.multiboot))
    }

    .boot : ALIGN(16)
    {
        KEEP(*(.boot))
    }

    .text : ALIGN(4K)
    {
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
        *(.got .got.plt)
    }

    __load_end = .;

    /* Not in the file: the loader zeroes from __load_end to __bss_end. The page
     * tables and the stack live here, which is why the zeroing is not optional
     * -- a page table full of whatever was in RAM is a triple fault. */
    .bss (NOLOAD) : ALIGN(4K)
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    __bss_end = .;

    /DISCARD/ :
    {
        *(.eh_frame*)
        *(.note*)
        *(.comment)
    }
}
