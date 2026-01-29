# Multi-Stage Bootloader

## Overview

The multi-stage bootloader overcomes the 512-byte limitation of traditional boot sectors by implementing a two-stage loading process:

- **Stage 1**: 512-byte boot sector loaded by BIOS at `0x7C00`
- **Stage 2**: Extended loader (1KB) loaded by Stage 1 at `0x8000`

This architecture enables development of complex bootloaders and operating systems without the severe space constraints of single-stage boot sectors.

## Architecture

### Memory Layout

```
┌─────────────┬────────────────────────────────┐
│   Address   │          Description           │
├─────────────┼────────────────────────────────┤
│ 0x0000:0000 │ BIOS Interrupt Vector Table    │
│ 0x0000:0400 │ BIOS Data Area                 │
│ 0x0000:7C00 │ Stage 1 Boot Sector (512B)     │
│ 0x0000:7E00 │ Stage 2 Source (Combined Image)│
│ 0x0000:8000 │ Stage 2 Execution Location     │
│             │   (1KB - 64KB possible)        │
│ 0x000A:0000 │ VGA Text Mode Buffer           │
│ 0x000C:0000 │ BIOS Extension ROMs            │
│ 0x000F:0000 │ BIOS Code                      │
└─────────────┴────────────────────────────────┘
```

### Boot Process Flow

```
┌──────────┐
│   BIOS   │
│  Power   │
│    On    │
└────┬─────┘
     │
     ├─→ POST (Power-On Self Test)
     │
     ├─→ Load boot sector to 0x7C00
     │   (first 512 bytes from disk)
     │
     └─→ Jump to 0x7C00 (Stage 1)
         
         ┌──────────────────────────┐
         │      Stage 1 (512B)      │
         ├──────────────────────────┤
         │ 1. Initialize segments   │
         │    DS = ES = SS = 0      │
         │    SP = 0x7C00           │
         │                          │
         │ 2. Display banner        │
         │    "AetherVM Stage 1"    │
         │                          │
         │ 3. Load Stage 2          │
         │    Source: 0x7E00        │
         │    Dest:   0x8000        │
         │    Size:   1024 bytes    │
         │    Method: rep movsw     │
         │                          │
         │ 4. Jump to Stage 2       │
         │    jmp 0x0000:0x8000     │
         └────────┬─────────────────┘
                  │
                  v
         ┌──────────────────────────┐
         │      Stage 2 (1KB+)      │
         ├──────────────────────────┤
         │ 1. Re-initialize         │
         │    DS = ES = 0           │
         │                          │
         │ 2. Clear screen          │
         │                          │
         │ 3. Display menu          │
         │    [1] VGA Demo          │
         │    [2] Memory Test       │
         │    [3] System Info       │
         │    [Q] Quit              │
         │                          │
         │ 4. Wait for input        │
         │    INT 16h               │
         │                          │
         │ 5. Execute option        │
         │    Loop back to menu     │
         └──────────────────────────┘
```

## Files

### `stage1.asm` (131 lines)

**Purpose**: Minimal boot sector that loads and transfers control to Stage 2

**Key Sections**:
- **Initialization**: Set up segment registers and stack
- **Display**: Print loading messages using BIOS INT 10h
- **load_stage2**: Copy 1024 bytes from 0x7E00 to 0x8000
- **Transfer**: Far jump to Stage 2 entry point
- **Boot Signature**: 0xAA55 at offset 510-511

**Size**: Exactly 512 bytes (required for boot sector)

### `stage2.asm` (504 lines)

**Purpose**: Extended bootloader with interactive features

**Features**:
1. **VGA Demo**: Display all 16 colors using block characters
2. **Memory Test**: Write and verify 0xAA55 pattern at 0x9000
3. **System Info**: Display Stage 2 address and stack pointer
4. **Interactive Menu**: Keyboard-driven navigation

**Key Functions**:
- `clear_screen`: BIOS scroll function to clear display
- `print_string_color`: Output with embedded color attributes
- `display_memory_info`: Get low memory KB using INT 12h
- `get_keystroke`: Wait for keypress with INT 16h
- `print_decimal`: Convert and display decimal numbers
- `print_hex`: Convert and display hexadecimal numbers

**Size**: 1024 bytes (expandable to 64KB if needed)

### `build_multistage.ps1` (109 lines)

**Purpose**: Build script to assemble and combine stages

**Build Steps**:
1. Verify NASM exists at `../../tools/nasm-2.16.03/nasm.exe`
2. Assemble `stage1.asm` → `stage1.bin`
   - Verify size = 512 bytes
   - Verify boot signature = 0xAA55
3. Assemble `stage2.asm` → `stage2.bin`
   - Verify size ≤ 64KB
4. Combine: `stage1.bin` + `stage2.bin` → `multiboot.img`
5. Display summary with memory layout

**Output Files**:
- `stage1.bin`: 512-byte boot sector
- `stage2.bin`: 1024-byte extended loader
- `multiboot.img`: Combined bootable image (1536 bytes)

## Building

### Prerequisites

- NASM 2.16.03 installed at `../../tools/nasm-2.16.03/`
- PowerShell 5.0 or later

### Build Commands

```powershell
# Build multi-stage bootloader
cd examples/guest_code
.\build_multistage.ps1

# Output:
#   stage1.bin      (512 bytes)
#   stage2.bin      (1024 bytes)
#   multiboot.img   (1536 bytes)
```

### Build Output

```
Building Multi-Stage Bootloader...

[1/3] Building Stage 1 (boot sector)...
  Stage 1 size: 512 bytes
  Boot signature: 0xAA55 - OK

[2/3] Building Stage 2 (extended loader)...
  Stage 2 size: 1024 bytes (1 KB)

[3/3] Creating combined boot image...
  Combined image size: 1536 bytes (1.5 KB)
  Output: multiboot.img

Build Summary:
  Stage 1: 512 bytes (boot sector)
  Stage 2: 1024 bytes (extended loader)
  Total: 1536 bytes

Memory Layout:
  0x7C00 - 0x7DFF: Stage 1 (512 bytes)
  0x8000 - 0x83FF: Stage 2 (1024 bytes)

Build successful!
```

## Testing

### Integration Tests

Three new tests in `tests/guest_code_integration.rs`:

1. **`test_multistage_bootloader`**:
   - Validates combined `multiboot.img` structure
   - Checks Stage 1 boot signature (0xAA55 at offset 510)
   - Verifies total size = 1536 bytes
   - Ensures both stages are present

2. **`test_stage1_binary`**:
   - Validates standalone `stage1.bin`
   - Checks size = 512 bytes
   - Verifies boot signature

3. **`test_stage2_binary`**:
   - Validates standalone `stage2.bin`
   - Checks size = 1024 bytes
   - Ensures NO boot signature (not a boot sector)

### Running Tests

```bash
cargo test --test guest_code_integration
```

**Expected Output**:
```
running 5 tests
test test_boot_signature ... ok
test test_stage2_binary ... ok
test test_binary_sizes ... ok
test test_multistage_bootloader ... ok
test test_stage1_binary ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured
```

## Usage with AetherVM

### Loading the Bootloader

```rust
use hv2_core::vm::{Vm, VmConfig};
use std::fs;

// Read the combined bootloader image
let boot_image = fs::read("examples/guest_code/multiboot.img")?;

// Create VM and load bootloader at 0x7C00
let mut vm = Vm::new(VmConfig::default())?;
vm.memory.write(0x7C00, &boot_image)?;

// Set initial CPU state
vm.set_registers(|regs| {
    regs.rip = 0x7C00;    // Start at Stage 1
    regs.cs = 0;
    regs.ds = 0;
    regs.es = 0;
    regs.ss = 0;
    regs.rsp = 0x7C00;    // Stack before boot sector
})?;

// Run the VM
vm.run()?;
```

### Stage 1 → Stage 2 Transition

The VM will execute Stage 1, which:
1. Displays loading messages
2. Copies Stage 2 from 0x7E00 to 0x8000
3. Jumps to 0x8000

Stage 2 then:
1. Clears the screen
2. Displays the menu
3. Waits for user input
4. Executes selected option
5. Returns to menu

## Design Rationale

### Why Two Stages?

**Single-Stage Limitations**:
- Boot sector limited to 512 bytes
- Boot signature consumes 2 bytes
- Effective space: 510 bytes
- Too small for complex bootloaders

**Multi-Stage Benefits**:
- Stage 1: Minimal loader fits in 512 bytes
- Stage 2: Unlimited size (practical limit 64KB for real mode)
- Can load even larger programs (kernels, protected mode code)
- Standard approach used by GRUB, SYSLINUX, etc.

### Memory Location Choices

**Stage 1 at 0x7C00**:
- BIOS standard boot sector location
- Non-negotiable for boot sector

**Stage 2 at 0x8000**:
- After BIOS data area (0x0000-0x0500)
- After Stage 1 (0x7C00-0x7DFF)
- Before VGA text buffer (0xA0000-0xBFFFF)
- Safe region with 608KB available
- Can be adjusted for larger Stage 2 sizes

**Source at 0x7E00**:
- Immediately after Stage 1 in combined image
- BIOS loads entire sector including Stage 2
- Stage 1 copies to 0x8000 for execution

### Stage 2 Size Choice

Current: **1024 bytes (1KB)**
- Large enough for demonstration features
- Small enough for quick testing
- Easily expandable to 64KB if needed

**Expandability**:
```nasm
; In stage2.asm, change:
times 1024-($-$$) db 0     ; Current: 1KB
; To:
times 8192-($-$$) db 0     ; Larger: 8KB
times 65536-($-$$) db 0    ; Maximum: 64KB
```

## Technical Details

### Segment vs Linear Addressing

**Segment:Offset Notation**:
- `0x07E0:0x0000` = (0x07E0 × 16) + 0x0000 = 0x7E00
- Used in Stage 1 for clarity

**Linear Addressing**:
- `0x7E00` = Direct memory address
- Easier to understand for modern programmers

### Copy Method: `movsw` vs `movsb`

**Current (Word Copy)**:
```nasm
mov cx, 512        ; 512 words = 1024 bytes
rep movsw          ; Copy word-by-word
```

**Alternative (Byte Copy)**:
```nasm
mov cx, 1024       ; 1024 bytes
rep movsb          ; Copy byte-by-byte
```

**Why `movsw`?**:
- Faster: Copies 2 bytes per instruction
- Alignment: Stage 2 starts at word boundary

### Color Codes

Stage 2 uses BIOS color attributes (4-bit):

```
┌─────────────┬─────────────┬──────────┐
│   Value     │    Color    │  Usage   │
├─────────────┼─────────────┼──────────┤
│    0x07     │  Light Gray │  Normal  │
│    0x09     │  Light Blue │  Info    │
│    0x0A     │  Light Green│  Success │
│    0x0E     │  Yellow     │  Banner  │
│    0x0F     │  White      │  Menu    │
└─────────────┴─────────────┴──────────┘
```

## Future Enhancements

### Session 19: Protected Mode

Stage 2 can be expanded to:
1. **Set up GDT** (Global Descriptor Table)
   - Code segment descriptor
   - Data segment descriptor
   - 32-bit flat memory model

2. **Set up IDT** (Interrupt Descriptor Table)
   - Exception handlers
   - Interrupt handlers

3. **Enable A20 Line**
   - Access memory above 1MB
   - Required for protected mode

4. **Switch to Protected Mode**
   - Set CR0.PE bit
   - Far jump to 32-bit code

5. **Load Kernel**
   - Read from disk to higher memory
   - Parse ELF/PE headers
   - Transfer control to kernel

### Possible Improvements

1. **Disk I/O**:
   - Load Stage 2 from disk (INT 13h)
   - Support larger Stage 2 sizes
   - Enable loading Stage 3 (kernel)

2. **Error Handling**:
   - Validate Stage 2 signature
   - Checksum verification
   - Retry on load failure

3. **Configuration**:
   - Boot options menu
   - Read config file
   - Multiple boot targets

4. **Filesystem Support**:
   - FAT12/16/32 parsing
   - Load files by name
   - Directory traversal

## References

### BIOS Interrupts Used

- **INT 10h**: Video Services
  - AH=0x00: Set video mode
  - AH=0x01: Set cursor shape
  - AH=0x02: Set cursor position
  - AH=0x06: Scroll up window
  - AH=0x0E: Teletype output

- **INT 12h**: Get Low Memory Size
  - Returns KB in AX

- **INT 16h**: Keyboard Services
  - AH=0x00: Read keystroke (blocking)
  - AH=0x01: Check for keystroke

### Related Documentation

- Intel 8086 Family User's Manual
- BIOS Boot Specification v1.01
- Ralf Brown's Interrupt List
- OSDev Wiki: Bootloader

## Troubleshooting

### Build Errors

**Error**: `NASM not found`
```
Solution: Install NASM 2.16.03 to tools/nasm-2.16.03/
```

**Error**: `Stage 1 size mismatch`
```
Solution: Check stage1.asm padding directive:
  times 510-($-$$) db 0
```

**Error**: `Invalid boot signature`
```
Solution: Verify last 2 bytes of stage1.asm:
  dw 0xAA55
```

### Runtime Issues

**Issue**: Stage 2 doesn't load
```
Check:
1. Stage 2 source address (0x7E00)
2. Stage 2 destination (0x8000)
3. Copy size (1024 bytes)
4. Combined image includes both stages
```

**Issue**: Garbage on screen
```
Check:
1. Segment registers initialized
2. Color codes valid (0x00-0x0F)
3. Strings null-terminated
```

## License

Part of AetherVM project. See root LICENSE file.
