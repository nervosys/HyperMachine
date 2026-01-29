# Guest Programming Guide for AetherVM

## Overview

This guide explains how to write guest code that runs inside AetherVM. Guest code can interact with emulated devices through I/O ports (x86 IN/OUT instructions) and memory-mapped I/O (MMIO). Understanding these interfaces is essential for writing operating systems, bootloaders, and bare-metal applications.

## Table of Contents

1. [Device I/O Overview](#device-io-overview)
2. [Serial Port Programming](#serial-port-programming)
3. [Timer Programming (PIT)](#timer-programming-pit)
4. [Interrupt Handling](#interrupt-handling)
5. [Memory-Mapped I/O](#memory-mapped-io)
6. [Boot Sequence Example](#boot-sequence-example)
7. [Assembly Reference](#assembly-reference)

---

## Device I/O Overview

AetherVM provides two primary methods for device communication:

### I/O Port Access (x86 IN/OUT)

- **OUT instruction**: Write data to an I/O port
- **IN instruction**: Read data from an I/O port
- Port addresses: 16-bit (0x0000 - 0xFFFF)
- Data sizes: 8-bit (AL), 16-bit (AX), 32-bit (EAX)

**Example:**
```asm
; Write byte 'A' to serial port 0x3F8
mov al, 'A'
mov dx, 0x3F8
out dx, al

; Read byte from port 0x3F8
mov dx, 0x3F8
in al, dx
```

### Memory-Mapped I/O (MMIO)

- Devices mapped to specific memory addresses
- Use standard MOV instructions
- Common for modern devices (GPUs, network cards)

**Example:**
```asm
; Write to MMIO device at 0x10000000
mov dword [0x10000000], 0x12345678

; Read from MMIO device
mov eax, [0x10000000]
```

---

## Serial Port Programming

The serial port (COM1) is at I/O port base **0x3F8** and provides 8 registers:

| Offset | Port  | Register | Description                       |
| ------ | ----- | -------- | --------------------------------- |
| 0      | 0x3F8 | Data     | Transmit/Receive Buffer           |
| 1      | 0x3F9 | IER      | Interrupt Enable Register         |
| 2      | 0x3FA | IIR      | Interrupt Identification Register |
| 3      | 0x3FB | LCR      | Line Control Register             |
| 4      | 0x3FC | MCR      | Modem Control Register            |
| 5      | 0x3FD | LSR      | Line Status Register              |
| 6      | 0x3FE | MSR      | Modem Status Register             |
| 7      | 0x3FF | SCR      | Scratch Register                  |

### Writing a Character

```asm
; Function: Write character in AL to serial port
serial_write:
    mov dx, 0x3F8           ; COM1 data port
    out dx, al              ; Write character
    ret
```

### Writing a String

```asm
; Function: Write null-terminated string
; Input: SI = pointer to string
serial_write_string:
.loop:
    lodsb                   ; Load byte from [SI] into AL, increment SI
    test al, al             ; Check for null terminator
    jz .done                ; If zero, we're done
    mov dx, 0x3F8           ; COM1 data port
    out dx, al              ; Write character
    jmp .loop               ; Continue
.done:
    ret
```

### Complete Example: "Hello, World!"

```asm
[BITS 16]
[ORG 0x7C00]

start:
    ; Initialize
    cli                     ; Disable interrupts
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti                     ; Enable interrupts

    ; Print message
    mov si, message
    call serial_write_string

    ; Halt
    cli
    hlt

serial_write_string:
.loop:
    lodsb
    test al, al
    jz .done
    mov dx, 0x3F8
    out dx, al
    jmp .loop
.done:
    ret

message: db 'Hello, World!', 0

; Boot sector signature
times 510-($-$$) db 0
dw 0xAA55
```

**Assemble with NASM:**
```bash
nasm -f bin hello.asm -o hello.bin
```

---

## Timer Programming (PIT)

The Programmable Interval Timer (8253/8254 PIT) has **4 I/O ports**:

| Port | Description                                |
| ---- | ------------------------------------------ |
| 0x40 | Channel 0 Data Port (System Timer)         |
| 0x41 | Channel 1 Data Port (RAM Refresh - legacy) |
| 0x42 | Channel 2 Data Port (PC Speaker)           |
| 0x43 | Mode/Command Register                      |

### Control Word Format (Port 0x43)

```
Bits 7-6: Channel Select (00=Channel 0, 01=Channel 1, 10=Channel 2)
Bits 5-4: Access Mode (01=LSB only, 10=MSB only, 11=LSB then MSB)
Bits 3-1: Operating Mode (0-5, different timer modes)
Bit 0:    BCD Mode (0=Binary, 1=BCD)
```

### Setting Timer Frequency

The PIT base frequency is **1.193182 MHz** (1193182 Hz).

**Formula**: `divisor = 1193182 / desired_frequency`

**Example: 1000 Hz (1ms) timer:**
```asm
; Set Channel 0 to 1000 Hz
mov al, 0x36                ; Channel 0, LSB+MSB, Mode 3, Binary
out 0x43, al                ; Write control word

mov ax, 1193                ; Divisor for 1000 Hz (1193182 / 1000 ≈ 1193)
out 0x40, al                ; Write low byte
mov al, ah
out 0x40, al                ; Write high byte
```

### Complete Timer Setup

```asm
; Function: Initialize PIT to 100 Hz (10ms intervals)
init_timer:
    ; Control word: Channel 0, LSB/MSB, Mode 3, Binary
    mov al, 0x36
    out 0x43, al

    ; Calculate divisor: 1193182 / 100 ≈ 11932 (0x2E9C)
    mov ax, 11932
    out 0x40, al            ; Write low byte
    mov al, ah
    out 0x40, al            ; Write high byte
    
    ret
```

---

## Interrupt Handling

To handle timer interrupts and other IRQs, you need to set up the Interrupt Descriptor Table (IDT) and program the Programmable Interrupt Controller (PIC).

### Setting up IDT (Real Mode)

```asm
; Set interrupt vector (Real Mode)
; Input: AX = segment, BX = offset, CL = interrupt number
set_interrupt_vector:
    push es
    xor dx, dx
    mov es, dx              ; ES = 0 (IVT at 0x0000)
    
    movzx di, cl            ; DI = interrupt number
    shl di, 2               ; Multiply by 4 (each entry is 4 bytes)
    
    mov [es:di], bx         ; Offset
    mov [es:di+2], ax       ; Segment
    
    pop es
    ret
```

### Timer Interrupt Handler

```asm
; Timer interrupt handler (IRQ 0 = INT 0x08)
timer_interrupt:
    push ax
    push dx
    
    ; Increment tick counter
    inc word [tick_count]
    
    ; Send End-Of-Interrupt (EOI) to PIC
    mov al, 0x20
    out 0x20, al
    
    pop dx
    pop ax
    iret

tick_count: dw 0
```

---

## Memory-Mapped I/O

MMIO devices are accessed through memory addresses rather than I/O ports.

### Example: MMIO Device at 0x10000000

```asm
[BITS 32]                   ; 32-bit protected mode

; Write to MMIO device
mov dword [0x10000000], 0x12345678

; Read from MMIO device
mov eax, [0x10000000]

; Write string to MMIO serial device
mov esi, message
mov edi, 0x10000000
.loop:
    lodsb
    test al, al
    jz .done
    mov [edi], al
    add edi, 1
    jmp .loop
.done:
```

### MMIO with Paging (Protected Mode)

In protected mode with paging enabled, you need to map the physical MMIO addresses:

```asm
; Map MMIO region (example for 0x10000000)
; Assumes page directory and page tables are set up

; Calculate page directory entry
mov eax, 0x10000000
shr eax, 22                 ; Get PDE index (bits 31-22)
; Set present, writable, user, cache-disable bits
mov dword [page_dir + eax*4], page_table | 0x13

; Calculate page table entry
mov eax, 0x10000000
shr eax, 12                 ; Get page number
and eax, 0x3FF              ; Get PTE index (bits 21-12)
mov dword [page_table + eax*4], 0x10000000 | 0x13
```

---

## Boot Sequence Example

A complete boot sequence demonstrating device initialization:

```asm
[BITS 16]
[ORG 0x7C00]

start:
    ; === Step 1: Initialize Segments ===
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    ; === Step 2: Initialize Timer (100 Hz) ===
    call init_timer

    ; === Step 3: Print Boot Message ===
    mov si, boot_msg
    call serial_write_string

    ; === Step 4: Setup Interrupt Handler ===
    ; (Simplified - in real code, set up full IDT)
    
    ; === Step 5: Enable Interrupts and Enter Main Loop ===
    sti
    
main_loop:
    hlt                     ; Wait for interrupt
    jmp main_loop

; ===== Functions =====

init_timer:
    mov al, 0x36            ; Channel 0, LSB/MSB, Mode 3
    out 0x43, al
    mov ax, 11932           ; 100 Hz
    out 0x40, al
    mov al, ah
    out 0x40, al
    ret

serial_write_string:
.loop:
    lodsb
    test al, al
    jz .done
    mov dx, 0x3F8
    out dx, al
    jmp .loop
.done:
    ret

; ===== Data =====
boot_msg: db 'AetherVM Boot...', 13, 10, 0

; ===== Boot Signature =====
times 510-($-$$) db 0
dw 0xAA55
```

---

## Assembly Reference

### x86 I/O Instructions

| Instruction     | Description             | Example          |
| --------------- | ----------------------- | ---------------- |
| `OUT port, AL`  | Write byte to port      | `out 0x3F8, al`  |
| `OUT port, AX`  | Write word to port      | `out 0x3F8, ax`  |
| `OUT port, EAX` | Write dword to port     | `out 0x3F8, eax` |
| `IN AL, port`   | Read byte from port     | `in al, 0x3F8`   |
| `IN AX, port`   | Read word from port     | `in ax, 0x3F8`   |
| `IN EAX, port`  | Read dword from port    | `in eax, 0x3F8`  |
| `OUT DX, AL`    | Write byte (port in DX) | `out dx, al`     |
| `IN AL, DX`     | Read byte (port in DX)  | `in al, dx`      |

### Common Port Addresses

| Device        | Base Port | Ports Used  | Description                   |
| ------------- | --------- | ----------- | ----------------------------- |
| COM1 (Serial) | 0x3F8     | 0x3F8-0x3FF | First serial port             |
| COM2 (Serial) | 0x2F8     | 0x2F8-0x2FF | Second serial port            |
| PIT (Timer)   | 0x40      | 0x40-0x43   | Programmable Interval Timer   |
| PIC Master    | 0x20      | 0x20-0x21   | Interrupt Controller (Master) |
| PIC Slave     | 0xA0      | 0xA0-0xA1   | Interrupt Controller (Slave)  |
| Keyboard      | 0x60      | 0x60-0x64   | PS/2 Keyboard Controller      |
| RTC           | 0x70      | 0x70-0x71   | Real-Time Clock / CMOS        |

### Register Sizes

| Register           | Size         | Example Usage         |
| ------------------ | ------------ | --------------------- |
| AL, BL, CL, DL     | 8-bit        | `mov al, 0x41`        |
| AH, BH, CH, DH     | 8-bit (high) | `mov ah, 0x42`        |
| AX, BX, CX, DX     | 16-bit       | `mov ax, 0x1234`      |
| EAX, EBX, ECX, EDX | 32-bit       | `mov eax, 0x12345678` |

### Useful Macros

```asm
; Macro to print a character
%macro PRINT_CHAR 1
    mov al, %1
    mov dx, 0x3F8
    out dx, al
%endmacro

; Usage:
PRINT_CHAR 'H'
PRINT_CHAR 'i'

; Macro to write string
%macro PRINT_STRING 1
    mov si, %1
    call serial_write_string
%endmacro

; Usage:
PRINT_STRING message1
```

---

## Debugging Tips

### 1. Serial Port is Your Friend

The serial port is the easiest way to debug guest code:

```asm
; Debug macro - print character and value
debug_print:
    mov dx, 0x3F8
    out dx, al              ; Print character
    ret

; Print hex byte
print_hex_byte:
    push ax
    shr al, 4               ; High nibble
    call .print_nibble
    pop ax
    and al, 0x0F            ; Low nibble
    call .print_nibble
    ret
.print_nibble:
    add al, '0'
    cmp al, '9'
    jbe .digit
    add al, 7               ; Convert to A-F
.digit:
    mov dx, 0x3F8
    out dx, al
    ret
```

### 2. Use Distinctive Patterns

Print distinctive characters to identify code execution points:

```asm
PRINT_CHAR '1'              ; Checkpoint 1
; ... code ...
PRINT_CHAR '2'              ; Checkpoint 2
; ... code ...
PRINT_CHAR '3'              ; Checkpoint 3
```

### 3. Infinite Loop Safety

Always include a way to break out of loops:

```asm
mov cx, 1000                ; Max 1000 iterations
.loop:
    ; ... loop body ...
    loop .loop              ; Auto-decrements CX
```

---

## Best Practices

1. **Always disable interrupts** during critical sections:
   ```asm
   cli                      ; Disable interrupts
   ; ... critical code ...
   sti                      ; Re-enable interrupts
   ```

2. **Initialize segments** at startup:
   ```asm
   xor ax, ax
   mov ds, ax
   mov es, ax
   mov ss, ax
   ```

3. **Set up stack** before calling functions:
   ```asm
   mov sp, 0x7C00          ; Stack grows down from boot sector
   ```

4. **Send EOI to PIC** after handling interrupts:
   ```asm
   mov al, 0x20
   out 0x20, al            ; Send EOI to master PIC
   ```

5. **Test device readiness** before I/O:
   ```asm
   ; Wait for serial port ready
   .wait:
       mov dx, 0x3FD       ; Line Status Register
       in al, dx
       test al, 0x20       ; Transmit Holding Register Empty
       jz .wait            ; Wait if not ready
   ```

---

## Example Programs Included

AetherVM includes several example guest programs:

1. **`hello.asm`** - Simple "Hello, World!" via serial port
2. **`timer_test.asm`** - Timer setup and interrupt handling
3. **`boot_sequence.asm`** - Complete boot sequence with device init
4. **`mmio_test.asm`** - Memory-mapped I/O demonstration
5. **`interrupt_demo.asm`** - Comprehensive interrupt handling example

See the `examples/guest_code/` directory for complete source code.

---

## Building Guest Code

### Using NASM

```bash
# 16-bit real mode binary
nasm -f bin program.asm -o program.bin

# 32-bit protected mode
nasm -f elf32 program.asm -o program.o

# With debugging symbols
nasm -f bin -g program.asm -o program.bin
```

### Using GCC (for C code)

```bash
# Compile without standard library
gcc -m32 -ffreestanding -nostdlib -c program.c -o program.o

# Link at specific address
ld -m elf_i386 -T linker.ld program.o -o program.elf

# Extract binary
objcopy -O binary program.elf program.bin
```

---

## Further Reading

- [OSDev Wiki - Serial Ports](https://wiki.osdev.org/Serial_Ports)
- [OSDev Wiki - Programmable Interval Timer](https://wiki.osdev.org/Programmable_Interval_Timer)
- [Intel Software Developer Manuals](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [x86 Assembly Language Reference](https://www.felixcloutier.com/x86/)

---

## Support

For questions or issues with guest programming in AetherVM:
- Check the examples in `examples/guest_code/`
- Review test cases in `tests/end_to_end_vm.rs`
- Open an issue on the AetherVM repository

---

**Happy Guest Programming!** 🚀
