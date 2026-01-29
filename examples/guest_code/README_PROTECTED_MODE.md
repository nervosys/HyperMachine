# Protected Mode Bootloader

## Overview

This is a complete **32-bit protected mode bootloader** that transitions from 16-bit real mode to 32-bit protected mode. It demonstrates fundamental OS development concepts including:

- **A20 Line Enabling**: Access memory above 1MB
- **Global Descriptor Table (GDT)**: Memory segmentation for protected mode
- **Mode Transition**: Switching from real mode to protected mode
- **Direct VGA Access**: Writing to video memory in protected mode
- **Flat Memory Model**: 4GB addressable space

## Architecture

### Boot Process

```
BIOS → Stage 1 (Real Mode) → Stage 2 (Real Mode Setup) → Protected Mode
```

1. **BIOS**: Loads Stage 1 to 0x7C00
2. **Stage 1**: Loads Stage 2 to 0x8000
3. **Stage 2 (16-bit)**:
   - Enable A20 line
   - Set up GDT
   - Disable interrupts
   - Switch to protected mode
4. **Protected Mode (32-bit)**:
   - Load segment selectors
   - Set up protected mode stack
   - Execute 32-bit code

### Memory Layout

```
┌─────────────┬────────────────────────────────┐
│   Address   │          Description           │
├─────────────┼────────────────────────────────┤
│ 0x00000000  │ BIOS Data / IDT (not used)     │
│ 0x00007C00  │ Stage 1 Boot Sector (512B)     │
│ 0x00007E00  │ Stage 2 Source (in image)      │
│ 0x00008000  │ Stage 2 Code (16-bit + 32-bit) │
│             │   ~2KB protected mode loader   │
│ 0x00090000  │ Protected Mode Stack (576KB)   │
│ 0x000B8000  │ VGA Text Buffer (80x25)        │
│ 0x00100000  │ Extended Memory (1MB+)         │
│ 0xFFFFFFFF  │ End of 32-bit address space    │
└─────────────┴────────────────────────────────┘
```

### Segment Layout

Protected mode uses a flat memory model with the following segments:

| Selector | Base | Limit | Type | Description           |
| -------- | ---- | ----- | ---- | --------------------- |
| 0x00     | -    | -     | Null | Required null segment |
| 0x08     | 0x0  | 4GB   | Code | 32-bit code segment   |
| 0x10     | 0x0  | 4GB   | Data | 32-bit data segment   |

## Files

### `stage2_pmode.asm` (463 lines, ~2KB)

Complete protected mode bootloader with:

**16-bit Real Mode Section**:
- Screen clearing and colored text output
- Memory detection (INT 12h)
- A20 line enabling (multiple methods)
- A20 status verification
- GDT loading
- Protected mode transition

**32-bit Protected Mode Section**:
- Segment selector initialization
- Stack setup at 0x90000
- Direct VGA text buffer access (0xB8000)
- Color palette demo with 16 colors

**GDT Structure**:
- Null descriptor (required)
- Code segment (32-bit, execute/read, 4GB limit)
- Data segment (32-bit, read/write, 4GB limit)

### `build_pmode.ps1` (118 lines)

Build script that:
1. Assembles Stage 1 (boot sector)
2. Assembles Stage 2 (protected mode loader)
3. Combines into `pmode.img`
4. Validates sizes and signatures
5. Displays memory layout and features

## Building

### Prerequisites

- NASM 2.16.03 at `../../tools/nasm-2.16.03/`
- PowerShell 5.0+

### Build Commands

```powershell
cd examples/guest_code
.\build_pmode.ps1
```

### Build Output

```
Building Protected Mode Multi-Stage Bootloader...

[1/3] Building Stage 1 (boot sector)...
  Stage 1 size: 512 bytes
  Boot signature: 0xAA55 - OK

[2/3] Building Stage 2 (protected mode loader)...
  Stage 2 size: 2048 bytes (2 KB)

[3/3] Creating combined boot image...
  Combined image size: 2560 bytes (2.5 KB)
  Output: pmode.img

Build Summary:
  Stage 1: 512 bytes (boot sector)
  Stage 2: 2048 bytes (protected mode loader)
  Total: 2560 bytes

Memory Layout:
  0x7C00 - 0x7DFF: Stage 1 (512 bytes)
  0x8000 - 0x87FF: Stage 2 (2048 bytes)
  0xB8000: VGA text buffer (protected mode)
  0x90000: Protected mode stack

Protected Mode Features:
  [x] A20 line enabled
  [x] GDT configured (code + data segments)
  [x] 32-bit protected mode active
  [x] Flat memory model (4GB addressable)
  [x] VGA direct access
```

## Technical Details

### A20 Line

The A20 line is the 21st address line (bit 20) that allows access to memory above 1MB. Historical x86 systems had this line disabled for compatibility with 8086 (which had 20-bit addressing).

**Enabling Methods** (tried in order):

1. **BIOS INT 15h** (AX=0x2401):
   ```asm
   mov ax, 0x2401
   int 0x15
   ```

2. **Keyboard Controller** (most reliable):
   ```asm
   ; Read output port
   mov al, 0xD0
   out 0x64, al
   in al, 0x60
   
   ; Set A20 bit and write back
   or al, 2
   mov al, 0xD1
   out 0x64, al
   out 0x60, al
   ```

3. **Fast A20 Gate** (port 0x92):
   ```asm
   in al, 0x92
   or al, 2
   out 0x92, al
   ```

**Verification**:
The A20 check writes different values to 0x0000:0x0500 and 0xFFFF:0x0510. If A20 is disabled, these addresses wrap and point to the same location. If enabled, they're different locations.

### Global Descriptor Table (GDT)

The GDT defines memory segments in protected mode. Each descriptor is 8 bytes:

```
Descriptor Format (64 bits):
┌────────────┬─────────┬───────┬──────────┬─────────┐
│ Limit      │ Base    │ Access│ Flags+   │ Base    │
│ (bits 0-15)│(0-15)   │ Byte  │ Limit    │ (24-31) │
│  2 bytes   │ 2 bytes │1 byte │  1 byte  │ 1 byte  │
└────────────┴─────────┴───────┴──────────┴─────────┘
```

**Access Byte** (for code segment):
```
Bit 7: Present (1 = valid)
Bit 6-5: Privilege level (0 = kernel)
Bit 4: Descriptor type (1 = code/data)
Bit 3: Executable (1 = code)
Bit 2: Direction/Conforming
Bit 1: Readable (1 = readable code)
Bit 0: Accessed (set by CPU)

Code: 10011010b (0x9A)
Data: 10010010b (0x92)
```

**Flags Byte**:
```
Bit 7: Granularity (1 = 4KB, 0 = 1B)
Bit 6: Size (1 = 32-bit, 0 = 16-bit)
Bit 5: Long mode (0 for 32-bit)
Bit 4: Reserved (0)
Bits 3-0: Limit bits 16-19

Value: 11001111b (0xCF) - 4KB granularity, 32-bit
```

**Our GDT**:
```asm
gdt_start:
    ; Null descriptor (offset 0x00)
    dd 0x0, 0x0

    ; Code segment (offset 0x08)
    dw 0xFFFF       ; Limit 0-15
    dw 0x0000       ; Base 0-15
    db 0x00         ; Base 16-23
    db 0x9A         ; Access: Present, Ring 0, Code, Exec/Read
    db 0xCF         ; Flags: 4KB gran, 32-bit + Limit 16-19
    db 0x00         ; Base 24-31

    ; Data segment (offset 0x10)
    dw 0xFFFF       ; Limit 0-15
    dw 0x0000       ; Base 0-15
    db 0x00         ; Base 16-23
    db 0x92         ; Access: Present, Ring 0, Data, Read/Write
    db 0xCF         ; Flags: 4KB gran, 32-bit + Limit 16-19
    db 0x00         ; Base 24-31
```

With 4KB granularity and limit 0xFFFFF, each segment covers:
- 0xFFFFF × 4KB = 4GB (entire 32-bit address space)

### Protected Mode Transition

Critical steps to switch modes:

```asm
; 1. Load GDT
lgdt [gdt_descriptor]

; 2. Disable interrupts (real mode IVT invalid in pmode)
cli

; 3. Set PE bit in CR0
mov eax, cr0
or eax, 1           ; Set bit 0 (Protection Enable)
mov cr0, eax

; 4. Far jump to flush prefetch queue
jmp 0x08:protected_mode_entry  ; 0x08 = code segment selector
```

**Why Far Jump?**
- Flushes the CPU's instruction prefetch queue
- Loads CS with the protected mode code selector
- Forces CPU to start fetching instructions in protected mode

### Protected Mode Code

Once in protected mode:

```asm
[BITS 32]
protected_mode_entry:
    ; Load all segment registers with data selector
    mov ax, 0x10        ; Data segment (offset 0x10 in GDT)
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    
    ; Set up stack
    mov esp, 0x90000    ; 576KB location
    
    ; Now can use 32-bit instructions and full 4GB address space
```

### VGA Direct Access

In protected mode, we directly access the VGA text buffer at 0xB8000:

```asm
; Format: Each character = 2 bytes (ASCII + Attribute)
mov esi, message        ; Source string
mov edi, 0xB8000        ; VGA buffer
mov ah, 0x0A            ; Attribute (light green on black)

.loop:
    lodsb               ; Load character
    test al, al
    jz .done
    stosw               ; Write char + attribute
    jmp .loop
```

**VGA Attributes** (4-bit background, 4-bit foreground):
```
Background (bits 4-7):
  0 = Black, 1 = Blue, 2 = Green, 3 = Cyan
  4 = Red, 5 = Magenta, 6 = Brown, 7 = Light Gray
  
Foreground (bits 0-3):
  0-7 = Same as background
  8-15 = Bright versions (add 8)
  
Example: 0x0A = Light green text on black background
```

### Color Demo

The protected mode demo displays 16 color bars:

```asm
pmode_demo:
    mov edi, 0xB8000 + (2 * 80 * 2)  ; Start at row 2
    mov ecx, 16                       ; 16 colors
    
.color_loop:
    ; Set attribute: background = foreground = current color
    mov ah, cl
    shl ah, 4           ; Background in high nibble
    or ah, cl           ; Foreground in low nibble
    
    ; Print block character
    mov al, 0xDB        ; '█' character
    mov ecx, 8
    rep stosw           ; Write 8 blocks
    
    ; Next line
    add edi, (80 - 8) * 2
    loop .color_loop
```

## Testing

### Integration Tests

Three new tests in `guest_code_integration.rs`:

1. **`test_protected_mode_bootloader`**:
   - Validates `pmode.img` structure
   - Checks Stage 1 boot signature
   - Verifies minimum size (2KB+)

2. **`test_stage2_pmode_binary`**:
   - Validates `stage2_pmode.bin` size
   - Ensures 1KB-64KB range
   - Verifies no boot signature

3. **`test_protected_mode_gdt`**:
   - Validates binary size for GDT code
   - Basic sanity checks

### Running Tests

```bash
cargo test --test guest_code_integration
```

**Results**:
```
running 8 tests
test test_protected_mode_gdt ... ok
test test_stage2_pmode_binary ... ok
test test_protected_mode_bootloader ... ok
test test_multistage_bootloader ... ok
test test_binary_sizes ... ok
test test_stage1_binary ... ok
test test_stage2_binary ... ok
test test_boot_signature ... ok

test result: ok. 8 passed; 0 failed
```

## Usage with AetherVM

### Loading and Running

```rust
use hv2_core::vm::{Vm, VmConfig};
use std::fs;

// Read protected mode bootloader
let boot_image = fs::read("examples/guest_code/pmode.img")?;

// Create VM
let mut vm = Vm::new(VmConfig::default())?;

// Load bootloader at 0x7C00
vm.memory.write(0x7C00, &boot_image)?;

// Set initial CPU state (16-bit real mode)
vm.set_registers(|regs| {
    regs.rip = 0x7C00;
    regs.cs = 0;
    regs.ds = 0;
    regs.es = 0;
    regs.ss = 0;
    regs.rsp = 0x7C00;
    regs.rflags = 0x2;  // Reserved bit always set
})?;

// Run VM - will transition to protected mode
vm.run()?;
```

### Expected Behavior

1. Stage 1 loads Stage 2 to 0x8000
2. Stage 2 displays:
   ```
   ==============================================
     AetherVM Stage 2 - Protected Mode Boot
   ==============================================
   
   Detecting Memory: 640 KB
   
   [1/3] Enabling A20 line... OK
   [2/3] Loading GDT... OK
   [3/3] Switching to protected mode...
   
   *** PROTECTED MODE ACTIVE ***
   ```
3. Displays 16 color bars showing all VGA colors
4. Halts in protected mode

## Comparison: Real Mode vs Protected Mode

| Feature               | Real Mode (16-bit) | Protected Mode (32-bit) |
| --------------------- | ------------------ | ----------------------- |
| **Address Space**     | 1MB (20-bit)       | 4GB (32-bit)            |
| **Segmentation**      | 64KB segments      | Up to 4GB per segment   |
| **Memory Protection** | None               | Ring-based protection   |
| **Instructions**      | 16-bit only        | 32-bit + 16-bit         |
| **BIOS**              | Available (INT)    | Not available           |
| **Direct Hardware**   | Limited            | Full access             |
| **Multitasking**      | No support         | Hardware support        |

## Design Rationale

### Why Protected Mode?

Protected mode is essential for:
- **Accessing >1MB memory**: Real mode limited to 1MB
- **Memory protection**: Prevent programs from corrupting each other
- **Hardware support**: Modern CPUs designed for protected mode
- **32-bit computing**: Full 32-bit registers and instructions
- **OS development**: All modern OSes use protected mode (or long mode)

### Why Flat Memory Model?

Alternatives:
1. **Real Mode Segmentation**: Limited to 1MB, complex addressing
2. **Protected Mode Segmentation**: Complex segment management
3. **Flat Model** (chosen): Simple, entire memory as one space

Advantages:
- Simpler programming (no segment calculations)
- Compatible with modern OS design
- Easy transition to paging
- Matches protected mode kernels (Linux, Windows)

### Why A20 Must Be Enabled?

Without A20:
- Address bit 20 is always 0
- Memory wraps at 1MB boundary
- Cannot access extended memory
- Protected mode GDT/IDT might be inaccessible

With A20:
- Full 32-bit addressing works
- Can access all 4GB
- Required for protected mode operation

## Future Enhancements

### Interrupt Handling

Add Interrupt Descriptor Table (IDT):
```asm
; IDT structure similar to GDT
idt_start:
    ; 256 interrupt descriptors
    ; Each 8 bytes
    
lidt [idt_descriptor]
```

Required for:
- Exception handling (divide by zero, page faults)
- Hardware interrupts (keyboard, timer)
- System calls (software interrupts)

### Paging

Enable virtual memory:
```asm
; Set up page directory and tables
mov eax, page_directory
mov cr3, eax        ; Load page directory base

; Enable paging
mov eax, cr0
or eax, 0x80000000  ; Set PG bit
mov cr0, eax
```

Benefits:
- Virtual memory management
- Memory isolation between processes
- Demand paging
- Memory-mapped files

### Loading ELF Kernel

Stage 2 can load a larger kernel:
1. Parse ELF header
2. Load program segments to memory
3. Set up entry point
4. Jump to kernel

### Long Mode (64-bit)

Transition to 64-bit:
1. Enable PAE (Physical Address Extension)
2. Set up 64-bit page tables
3. Enable long mode in EFER MSR
4. Enable paging
5. Jump to 64-bit code

## Troubleshooting

### A20 Enable Fails

**Symptoms**: Bootloader hangs after "Enabling A20 line"

**Solutions**:
- Some emulators may already have A20 enabled
- Try commenting out A20 check for testing
- Verify keyboard controller timing

### Triple Fault

**Symptoms**: System resets after mode switch

**Causes**:
- Invalid GDT (wrong base/limit)
- Wrong selector in far jump
- Interrupts not disabled
- Stack not set up properly

**Debug**:
```asm
; Add breakpoint before mode switch
xchg bx, bx     ; Bochs magic breakpoint

; Verify GDT descriptor
lgdt [gdt_descriptor]

; Check CR0 before and after
mov eax, cr0
; Examine value
```

### Garbage on Screen

**Symptoms**: Random characters in protected mode

**Causes**:
- Wrong VGA buffer address
- Incorrect attribute bytes
- String not null-terminated

**Fix**:
```asm
; Verify VGA buffer
mov edi, 0xB8000    ; Must be physical address

; Check string format
pmode_msg: db "Text", 0  ; MUST be null-terminated
```

## References

### Intel Manuals

- Intel® 64 and IA-32 Architectures Software Developer's Manual
  - Volume 3A: System Programming Guide, Part 1
  - Chapter 3: Protected-Mode Memory Management
  - Chapter 9: Processor Management and Initialization

### External Resources

- [OSDev Wiki - Protected Mode](https://wiki.osdev.org/Protected_Mode)
- [OSDev Wiki - GDT](https://wiki.osdev.org/GDT)
- [OSDev Wiki - A20 Line](https://wiki.osdev.org/A20_Line)
- [Writing a Simple Operating System from Scratch](https://www.cs.bham.ac.uk/~exr/lectures/opsys/10_11/lectures/os-dev.pdf)

### Related Projects

- **GRUB**: Multi-stage bootloader with protected mode
- **SYSLINUX**: Lightweight bootloader collection
- **SeaBIOS**: Open-source x86 BIOS implementation

## License

Part of AetherVM project. See root LICENSE file.
