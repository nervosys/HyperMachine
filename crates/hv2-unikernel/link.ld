/* Where a 64-bit Multiboot unikernel goes, and in what order.
 *
 * The image describes itself twice, and has to. It is an ELF64, and the
 * Multiboot specification's "read the image's own headers" path is defined for
 * ELF32 only -- so the header carries its own load addresses instead, and the
 * symbols here are what fills them in. `__load_start` is the first byte the
 * loader copies, `__load_end` the last, and `__bss_end` the end of the region
 * that must read as zero. Between them they say "put this image here" without
 * the loader having to understand the format it is in.
 *
 * Three placements are load-bearing rather than conventional:
 *
 *   - `.multiboot` is first, so the header sits at the load address itself.
 *     The specification requires it in the first 8 KB of the file and 4-byte
 *     aligned; first is the only placement that stays true however large the
 *     rest of the image grows.
 *   - `.boot` follows it. That is the 32-bit half: the mode a Multiboot loader
 *     hands control over in, and code that exists only to reach long mode and
 *     is never entered again. It cannot be mixed in with 64-bit code.
 *   - `.text` and everything after it are 64-bit.
 *
 * 1 MB is the conventional load address for a Multiboot kernel: below it is the
 * BIOS's, and a kernel that overlaps the loader's own boot information destroys
 * the structure EBX points at.
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

    /* Not in the file: the loader guarantees that everything from __load_end to
     * __bss_end reads as zero. The page tables and the boot stack live here,
     * and page tables built on whatever happened to be in RAM are a triple
     * fault with no diagnostic at all. */
    .bss (NOLOAD) : ALIGN(4K)
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    __bss_end = .;

    /* Nothing here is loaded or executed, and some of it confuses a loader
     * that walks program headers. A unikernel ships no symbols for anyone. */
    /DISCARD/ :
    {
        *(.eh_frame*)
        *(.note*)
        *(.comment)
    }
}
