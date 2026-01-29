; ==============================================================================
; mmio_test.asm - Memory-Mapped I/O Demonstration
; ==============================================================================
;
; Description:
;   Demonstrates using memory-mapped I/O (MMIO) to communicate with devices.
;   This example writes to an MMIO serial device at address 0x10000000.
;
;   Note: This requires 32-bit protected mode with paging to access addresses
;   above 1MB. For simplicity, this demo uses 16-bit real mode with segment
;   tricks, but in a real OS you'd use protected mode.
;
; Build:
;   nasm -f bin mmio_test.asm -o mmio_test.bin
;
; Memory Layout:
;   0x10000000: MMIO Serial Device (write characters here)
;
; ==============================================================================

[BITS 16]
[ORG 0x7C00]

start:
    ; ====== Initialize ======
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti
    
    ; ====== Print Startup Message (via I/O port) ======
    mov si, msg_start
    call serial_write_string_io
    
    ; ====== Demonstrate I/O Port vs MMIO ======
    
    ; Write to I/O port serial (COM1)
    mov si, msg_io_demo
    call serial_write_string_io
    
    ; In a real 32-bit protected mode OS, you would:
    ; 1. Setup paging
    ; 2. Map physical address 0x10000000 to virtual address
    ; 3. Write to that virtual address
    ;
    ; For this 16-bit demo, we'll show the concept and use I/O ports
    
    mov si, msg_mmio_note
    call serial_write_string_io
    
    ; ====== 32-bit Protected Mode MMIO Example (Conceptual) ======
    ; This section shows what MMIO code would look like in protected mode
    ; (This won't execute in 16-bit real mode, it's for reference)
    
    jmp real_mode_demo
    
    ; === Protected Mode MMIO Code (32-bit) ===
protected_mode_mmio:
    [BITS 32]
    
    ; Write string to MMIO device at 0x10000000
    mov esi, mmio_message       ; Source string
    mov edi, 0x10000000         ; MMIO device address
    
.mmio_loop:
    lodsb                       ; Load byte from [ESI]
    test al, al
    jz .mmio_done
    mov [edi], al               ; Write to MMIO address
    add edi, 1                  ; Next MMIO address
    jmp .mmio_loop
    
.mmio_done:
    ; Continue...
    
    [BITS 16]                   ; Back to 16-bit mode
    
real_mode_demo:
    ; ====== Show Conceptual Difference ======
    mov si, msg_comparison
    call serial_write_string_io
    
    ; ====== Halt ======
    cli
    hlt

; ==============================================================================
; serial_write_string_io
;
; Write string using I/O port (traditional method)
;
; Input: SI = pointer to string
; ==============================================================================
serial_write_string_io:
    push ax
    push dx
    push si
    
.loop:
    lodsb
    test al, al
    jz .done
    mov dx, 0x3F8               ; COM1 I/O port
    out dx, al                  ; OUT instruction
    jmp .loop
    
.done:
    pop si
    pop dx
    pop ax
    ret

; ==============================================================================
; Data Section
; ==============================================================================

msg_start:
    db 'MMIO Test Starting...', 13, 10, 0

msg_io_demo:
    db 'I/O Port Method: ', 13, 10
    db '  - Uses OUT instruction', 13, 10
    db '  - Port address: 0x3F8', 13, 10
    db '  - Example: OUT 0x3F8, AL', 13, 10, 13, 10, 0

msg_mmio_note:
    db 'MMIO Method (Protected Mode): ', 13, 10
    db '  - Uses MOV instruction to memory', 13, 10
    db '  - Memory address: 0x10000000', 13, 10
    db '  - Example: MOV [0x10000000], AL', 13, 10, 13, 10, 0

msg_comparison:
    db 'Key Differences:', 13, 10
    db '  I/O Ports:', 13, 10
    db '    + Separate address space (0-65535)', 13, 10
    db '    + Special instructions (IN/OUT)', 13, 10
    db '    + Legacy x86 devices', 13, 10
    db '  MMIO:', 13, 10
    db '    + Same address space as RAM', 13, 10
    db '    + Normal MOV instructions', 13, 10
    db '    + Modern devices (GPU, NIC)', 13, 10
    db '    + Requires paging in protected mode', 13, 10, 13, 10, 0

mmio_message:
    db 'Hello from MMIO!', 0

; ==============================================================================
; Boot Sector Signature
; ==============================================================================
times 510-($-$$) db 0
dw 0xAA55
