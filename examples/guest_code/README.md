# Guest Code Examples for AetherVM

This directory contains example guest programs demonstrating various aspects of bare-metal programming for AetherVM.

## Overview

All examples are written in x86 assembly (NASM syntax) and compile to 512-byte boot sector images. They demonstrate device I/O, interrupt handling, and other low-level programming concepts.

## Examples

### 1. `hello.asm` - Hello World
**Difficulty**: Beginner  
**Concepts**: Basic I/O, Serial Port

The simplest possible guest program. Writes "Hello, World!" to the serial port and halts.

**Key Learning Points**:
- Boot sector structure
- Serial port I/O (OUT instruction)
- String output functions

### 2. `timer_test.asm` - Timer Programming
**Difficulty**: Intermediate  
**Concepts**: Timer (PIT), Interrupts, Decimal Output

Demonstrates programming the Programmable Interval Timer and handling timer interrupts. Prints tick counts every second.

**Key Learning Points**:
- PIT (8253/8254) programming
- Timer frequency calculation
- Interrupt handler setup
- Number-to-string conversion

### 3. `boot_sequence.asm` - Complete Boot Sequence
**Difficulty**: Intermediate  
**Concepts**: Full Initialization, Multi-Device Setup

A comprehensive boot sequence showing proper system initialization:
- CPU segment and stack setup
- Device initialization (Timer)
- Interrupt configuration
- Status reporting

**Key Learning Points**:
- Proper boot order
- Device initialization sequence
- System state management
- Comprehensive logging

### 4. `mmio_test.asm` - Memory-Mapped I/O
**Difficulty**: Advanced  
**Concepts**: MMIO, Protected Mode Concepts

Demonstrates the difference between I/O ports and memory-mapped I/O. Shows conceptual MMIO code for protected mode.

**Key Learning Points**:
- I/O ports vs MMIO
- Protected mode considerations
- Memory mapping concepts
- Device access patterns

### 5. `interrupt_demo.asm` - Interrupt Handling
**Difficulty**: Advanced  
**Concepts**: Comprehensive Interrupt Handling

A complete demonstration of interrupt handling including:
- Multiple interrupt types
- Timer interrupts (IRQ 0)
- Software interrupts (INT)
- Exception handling

**Key Learning Points**:
- Interrupt Vector Table (IVT) setup
- Multiple interrupt handlers
- Software vs hardware interrupts
- Exception handling

### 6. `keyboard_test.asm` - PS/2 Keyboard Input (NEW)
**Difficulty**: Intermediate  
**Concepts**: Keyboard Controller (8042), Scancodes, Input Handling

Demonstrates PS/2 keyboard input handling with the Intel 8042 controller. Reads scancodes and displays them on the serial port. Press ESC to exit.

**Key Learning Points**:
- 8042 keyboard controller initialization
- Reading keyboard scancodes
- Make codes (key press) vs break codes (key release)
- Controller Command Byte (CCB) configuration
- Status register polling

**Sample Output**:
```
Keyboard Test
Press keys to see scancodes (ESC to exit)
Ready!
Scancode: 0x1E
Scancode: 0x9E
Scancode: 0x01
ESC pressed, exiting...
```

### 7. `rtc_test.asm` - Real-Time Clock (NEW)
**Difficulty**: Intermediate  
**Concepts**: RTC (MC146818), CMOS RAM, Date/Time Reading

Reads the current date and time from the Real-Time Clock and displays it on the serial port. Updates continuously to show live time.

**Key Learning Points**:
- RTC register access (ports 0x70/0x71)
- CMOS RAM reading
- BCD vs binary mode
- Status registers (A, B, C, D)
- Date/time formatting

**Sample Output**:
```
RTC Test
Reading date/time from CMOS...

Date: 2024-11-04  Time: 14:23:05
Date: 2024-11-04  Time: 14:23:06
Date: 2024-11-04  Time: 14:23:07
...
```

### 8. `vga_demo.asm` - VGA Text Mode Display (NEW)
**Difficulty**: Advanced  
**Concepts**: VGA Text Mode, Color Attributes, Cursor Control

Comprehensive VGA text mode demonstration showing colored text, box drawing, and the 16-color palette. Displays a full-screen UI with title bar, color samples, and various text effects.

**Key Learning Points**:
- VGA text buffer (0xB8000-0xBFFFF)
- Character + attribute format
- 16-color palette (foreground/background)
- CRTC registers for cursor control
- Box-drawing characters (IBM extended ASCII)
- Screen positioning calculations

**Visual Features**:
- Title bar with gray background
- Complete 16-color palette display
- Gradient patterns
- Bordered box with text
- Colored text samples (green, red, yellow)

### 9. `device_combo.asm` - Multi-Device Integration (NEW)
**Difficulty**: Advanced  
**Concepts**: RTC + Keyboard + VGA + Serial Integration

A comprehensive demonstration that uses all devices together. Shows a live dashboard on VGA with:
- Real-time clock display (updates automatically)
- Keyboard input display (shows scancodes as you type)
- Event log window (scrolling)
- Serial port logging (parallel output)

**Key Learning Points**:
- Multi-device coordination
- Real-time UI updates
- Event-driven programming
- Device state synchronization
- Efficient screen updates (partial redraws)

**UI Layout**:
```
╔════════════════════════════════════════════════════════╗
║           DEVICE DEMONSTRATION                         ║
╠════════════════════════════════════════════════════════╣
║ TIME:     2024-11-04 14:23:05                         ║
║                                                        ║
║ KEYBOARD: 0x1E (KEY)                                  ║
║                                                        ║
║ EVENT LOG:                                            ║
║   Key: 0x1E                                           ║
║   Key: 0x9E                                           ║
║   ...                                                  ║
╠════════════════════════════════════════════════════════╣
║ Press keys to see scancodes | Time updates auto       ║
╚════════════════════════════════════════════════════════╝
```

## Building the Examples

### Prerequisites

Install NASM (Netwide Assembler):

**Windows**:
```powershell
choco install nasm
# or download from https://www.nasm.us/
```

**Linux**:
```bash
sudo apt install nasm      # Debian/Ubuntu
sudo dnf install nasm      # Fedora
sudo pacman -S nasm        # Arch
```

**macOS**:
```bash
brew install nasm
```

### Build Instructions

Each example can be built with:

```bash
nasm -f bin example.asm -o example.bin
```

Build all examples:

```bash
# Windows (PowerShell)
Get-ChildItem *.asm | ForEach-Object { nasm -f bin $_.Name -o "$($_.BaseName).bin" }

# Linux/macOS (bash)
for f in *.asm; do nasm -f bin "$f" -o "${f%.asm}.bin"; done
```

### Build with Debug Symbols

```bash
nasm -f bin -g program.asm -o program.bin
```

## Running in AetherVM

### Option 1: Using the Test Framework

The examples can be tested using AetherVM's MockHypervisorBackend:

```rust
use hv2_core::*;

#[tokio::test]
async fn test_guest_hello() {
    // Load guest binary
    let guest_code = std::fs::read("examples/guest_code/hello.bin").unwrap();
    
    // Create VM and load guest code
    let vm = VM::new(VMConfig::default()).unwrap();
    vm.memory().write_bytes(0x7C00, &guest_code).unwrap();
    
    // Setup devices
    let serial = Arc::new(RwLock::new(SerialDevice::new("serial".into(), 0x3F8)));
    vm.devices().register_device("serial".into(), serial.clone()).unwrap();
    vm.devices().register_io_port_range("serial".into(), 0x3F8, 0x3FF).unwrap();
    
    // Run VM
    vm.start().await.unwrap();
    
    // Check output
    assert!(serial.read().output_string().contains("Hello, World!"));
}
```

### Option 2: Using Real Hypervisor Backend

```rust
// Load and run with KVM/WHPX
let vm = VM::new(VMConfig::default()).unwrap();
vm.memory().write_bytes(0x7C00, &guest_code).unwrap();
vm.run().await.unwrap();
```

## Understanding the Output

All examples write to the serial port (COM1 at 0x3F8). In AetherVM, this output can be captured by:

1. **Test Framework**: Check `SerialDevice::output_string()`
2. **Real Hypervisor**: Serial output appears in VM console or log file
3. **Debugging**: Use `-serial stdio` flag to see output in terminal

## Example Output

### hello.bin
```
Hello, World!
```

### timer_test.bin
```
Timer Test Starting...
Tick: 10
Tick: 20
Tick: 30
...
```

### boot_sequence.bin
```
[BOOT] AetherVM Starting...
[BOOT] Initializing Timer...
[BOOT] Timer initialized at 100 Hz
[BOOT] Setting up interrupts...
[BOOT] Interrupts configured
[BOOT] Boot sequence complete!
[TICK] 10 [TICK] 20 [TICK] 30 ...
```

### interrupt_demo.bin
```
========================================
Interrupt Handling Demonstration
========================================
[1] Setting up Interrupt Vector Table...
    IVT configured successfully
[2] Initializing timer (100 Hz)...
[3] Demonstrating software interrupt (INT 0x80)...
    [INT 0x80] Custom interrupt handler called
[4] Testing timer interrupts...
    Timer ticks received: 50
[5] Testing divide error exception (INT 0x00)...
    [INT 0x00] Divide error caught!
[6] Testing interrupt chain...
    [INT 0x81] Custom interrupt 2
    [INT 0x82] Custom interrupt 3
[7] All tests complete!
========================================
```

## Code Structure

Each example follows this structure:

```asm
[BITS 16]           ; 16-bit real mode
[ORG 0x7C00]        ; Loaded at 0x7C00

start:
    ; 1. Initialize segments and stack
    cli
    xor ax, ax
    mov ds, ax
    mov ss, ax
    mov sp, 0x7C00
    sti
    
    ; 2. Main program logic
    ; ...
    
    ; 3. Halt
    hlt

; Functions
; ...

; Data
; ...

; Boot sector signature
times 510-($-$$) db 0
dw 0xAA55
```

## Common Patterns

### Writing to Serial Port

```asm
mov al, 'A'         ; Character to write
mov dx, 0x3F8       ; COM1 data port
out dx, al          ; Write character
```

### String Output

```asm
mov si, message     ; Pointer to string
call print_string

print_string:
    lodsb           ; Load byte from [SI]
    test al, al     ; Check for null
    jz .done
    mov dx, 0x3F8
    out dx, al
    jmp print_string
.done:
    ret
```

### Timer Setup

```asm
mov al, 0x36        ; Control word
out 0x43, al
mov ax, 11932       ; Divisor for 100 Hz
out 0x40, al        ; Low byte
mov al, ah
out 0x40, al        ; High byte
```

### Interrupt Handler

```asm
timer_interrupt:
    push ax         ; Save registers
    push dx
    
    ; Handler code
    inc word [tick_count]
    
    ; Send EOI to PIC
    mov al, 0x20
    out 0x20, al
    
    pop dx          ; Restore registers
    pop ax
    iret            ; Return from interrupt
```

## Debugging Tips

1. **Add Checkpoints**: Print characters at key points
   ```asm
   mov al, '1'
   mov dx, 0x3F8
   out dx, al      ; Checkpoint 1
   ```

2. **Print Hex Values**: Debug register contents
   ```asm
   ; Print AL in hex
   push ax
   shr al, 4
   call print_hex_nibble
   pop ax
   and al, 0x0F
   call print_hex_nibble
   ```

3. **Infinite Loop Safety**: Add loop counters
   ```asm
   mov cx, 1000
   .loop:
       ; ...
       loop .loop
   ```

4. **State Dumps**: Print important variables
   ```asm
   mov ax, [tick_count]
   call print_number
   ```

## Further Learning

For more information, see:
- **[Guest Programming Guide](../../docs/GUEST_PROGRAMMING_GUIDE.md)** - Comprehensive programming reference
- **[AetherVM Documentation](../../docs/)** - VM architecture and API docs
- **[Test Suite](../../crates/hv2-core/tests/)** - More examples of VM usage

## Contributing

To add new guest code examples:

1. Follow the existing naming convention (`example_name.asm`)
2. Include comprehensive comments
3. Add example to this README
4. Create corresponding test in `tests/end_to_end_vm.rs`
5. Document any new patterns or techniques

## License

These examples are part of the AetherVM project and are provided as educational material for learning bare-metal x86 programming.
