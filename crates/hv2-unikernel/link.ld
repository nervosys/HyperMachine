/* Where a Multiboot unikernel goes, and in what order.
 *
 * Two things here are load-bearing rather than conventional:
 *
 *   - `.multiboot` is first. The specification requires the header in the
 *     first 8 KB of the file and 4-byte aligned; putting it at the start of
 *     the first loaded section is the only placement that stays true however
 *     large the rest of the image grows.
 *   - `.text.entry` follows it, so `_start` is the first instruction after the
 *     header. ENTRY() is what the loader actually reads, so this is belt and
 *     braces -- but it means the image also works under a loader that enters
 *     at the load address, and being wrong about that is silent.
 *
 * 1 MB is the conventional load address for a Multiboot kernel: below it is
 * the BIOS's, and a kernel that overlaps the loader's own boot information
 * destroys the structure EBX points at.
 */

ENTRY(_start)

SECTIONS
{
    . = 1M;

    .text : ALIGN(4K)
    {
        KEEP(*(.multiboot))
        *(.text.entry)
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(4K)
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    /* Nothing here is loaded or executed, and some of it confuses a loader
     * that walks program headers. A unikernel ships no symbols for anyone. */
    /DISCARD/ :
    {
        *(.eh_frame*)
        *(.note*)
        *(.comment)
    }
}
