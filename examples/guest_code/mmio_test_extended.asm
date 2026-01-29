; ==============================================================================
; mmio_test_extended.asm - Memory-Mapped I/O Demo (Multi-Stage)
; ==============================================================================
;
; Description:
;   Demonstrates memory-mapped I/O concepts using the multi-stage bootloader.
;   This is Stage 2 code loaded at 0x8000.
;
;   Shows the difference between:
;   - I/O Port access (IN/OUT instructions)
;   - Memory-mapped I/O (MOV instructions to memory addresses)
;
;   Note: True MMIO requires protected mode with paging for high addresses.
;   This demo shows both concepts and includes protected mode MMIO examples.
;
; Build:
;   Use build_mmio_test.ps1 script
;
; Memory Layout:
;   0x7C00: Stage 1 (boot sector, 512 bytes)
;   0x8000: Stage 2 (this code, ~4KB)
;
; ==============================================================================

[BITS 16]
[ORG 0x8000]

stage2_start:
    ; ====== Initialize System ======
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti
    
    ; ====== Clear Screen ======
    call clear_screen
    
    ; ====== Print Header ======
    mov si, msg_header
    call print_screen
    
    ; ====== Demo 1: I/O Port Access ======
    mov si, msg_demo1_header
    call print_screen
    
    ; Demonstrate writing to COM1 serial port using OUT
    mov si, msg_io_method
    call print_screen
    
    ; Write test string using I/O ports
    mov si, test_string_io
    call write_serial_io
    
    mov si, msg_io_success
    call print_screen
    
    call delay
    
    ; ====== Demo 2: Low Memory MMIO (Real Mode) ======
    mov si, msg_demo2_header
    call print_screen
    
    ; VGA text buffer is memory-mapped at 0xB8000
    mov si, msg_vga_mmio
    call print_screen
    
    ; Write directly to VGA memory
    call demo_vga_mmio
    
    mov si, msg_vga_success
    call print_screen
    
    call delay
    
    ; ====== Demo 3: Conceptual Protected Mode MMIO ======
    mov si, msg_demo3_header
    call print_screen
    
    mov si, msg_pmode_concept
    call print_screen
    
    ; Show code example (conceptual)
    mov si, msg_pmode_code
    call print_screen
    
    call delay
    
    ; ====== Demo 4: Comparison Table ======
    mov si, msg_demo4_header
    call print_screen
    
    mov si, msg_comparison
    call print_screen
    
    ; ====== Demo 5: Practical Examples ======
    mov si, msg_demo5_header
    call print_screen
    
    mov si, msg_examples
    call print_screen
    
    ; ====== Shutdown ======
    mov si, msg_shutdown
    call print_screen
    
    cli
    hlt

; ==============================================================================
; write_serial_io
;
; Write string to serial port using I/O port instructions
;
; Input: SI = pointer to null-terminated string
; ==============================================================================
write_serial_io:
    push ax
    push dx
    push si
    
    mov dx, 0x3F8               ; COM1 I/O port address
    
.loop:
    lodsb                       ; Load byte from [SI]
    test al, al
    jz .done
    out dx, al                  ; OUT instruction - write to I/O port
    jmp .loop
    
.done:
    pop si
    pop dx
    pop ax
    ret

; ==============================================================================
; demo_vga_mmio
;
; Demonstrate memory-mapped I/O using VGA text buffer
; Writes colored text directly to video memory at 0xB8000
; ==============================================================================
demo_vga_mmio:
    push ax
    push bx
    push cx
    push es
    push di
    
    ; Set ES to VGA text buffer segment
    mov ax, 0xB800
    mov es, ax
    xor di, di
    
    ; Calculate position: row 10, col 20
    mov ax, 10                  ; Row 10
    mov cx, 80                  ; 80 columns per row
    mul cx                      ; AX = row * 80
    add ax, 20                  ; Add column offset
    shl ax, 1                   ; Multiply by 2 (char + attribute)
    mov di, ax
    
    ; Write string with attributes directly to memory
    mov si, mmio_demo_string
    mov ah, 0x0E                ; Yellow text on black background
    
.loop:
    lodsb
    test al, al
    jz .done
    
    ; MOV instruction - write directly to memory
    mov [es:di], ax             ; This is MMIO!
    add di, 2
    jmp .loop
    
.done:
    pop di
    pop es
    pop cx
    pop bx
    pop ax
    ret

; ==============================================================================
; Utility Functions
; ==============================================================================

; ----- clear_screen: Clear screen using BIOS -----
clear_screen:
    pusha
    mov ah, 0x06
    xor al, al
    xor cx, cx
    mov dx, 0x184F
    mov bh, 0x07
    int 0x10
    
    ; Reset cursor
    mov ah, 0x02
    xor bh, bh
    xor dx, dx
    int 0x10
    popa
    ret

; ----- print_screen: Write null-terminated string to screen -----
print_screen:
    pusha
.loop:
    lodsb
    test al, al
    jz .done
    mov ah, 0x0E
    xor bx, bx
    int 0x10
    jmp .loop
.done:
    popa
    ret

; ----- delay: Short delay for readability -----
delay:
    push cx
    mov cx, 0xFFFF
.loop:
    nop
    loop .loop
    pop cx
    ret

; ==============================================================================
; Data Section
; ==============================================================================

msg_header:
    db '========================================', 13, 10
    db '  Memory-Mapped I/O Demonstration', 13, 10
    db '  (Multi-Stage Extended)', 13, 10
    db '========================================', 13, 10, 13, 10, 0

msg_demo1_header:
    db '[Demo 1] I/O Port Access', 13, 10
    db '--------------------------', 13, 10, 0

msg_io_method:
    db 'Method: OUT instruction', 13, 10
    db 'Port: 0x3F8 (COM1)', 13, 10
    db 'Writing to serial...', 0

test_string_io:
    db ' SUCCESS!', 13, 10, 0

msg_io_success:
    db 13, 10, 'I/O port write completed', 13, 10, 13, 10, 0

msg_demo2_header:
    db '[Demo 2] Memory-Mapped I/O (VGA)', 13, 10
    db '----------------------------------', 13, 10, 0

msg_vga_mmio:
    db 'Method: MOV instruction to 0xB8000', 13, 10
    db 'Writing to VGA buffer...', 13, 10, 0

mmio_demo_string:
    db '<<< MMIO DEMO >>>', 0

msg_vga_success:
    db 'VGA MMIO write completed', 13, 10
    db '(See row 10 for colored text)', 13, 10, 13, 10, 0

msg_demo3_header:
    db '[Demo 3] Protected Mode MMIO Concept', 13, 10
    db '--------------------------------------', 13, 10, 0

msg_pmode_concept:
    db 'In 32-bit protected mode with paging:', 13, 10
    db '  - Can map any physical address', 13, 10
    db '  - Access high memory (>1MB)', 13, 10
    db '  - Example: GPU at 0xE0000000', 13, 10, 13, 10, 0

msg_pmode_code:
    db 'Code example (32-bit):', 13, 10
    db '  mov esi, message', 13, 10
    db '  mov edi, 0x10000000  ; MMIO device', 13, 10
    db '  .loop:', 13, 10
    db '    lodsb', 13, 10
    db '    test al, al', 13, 10
    db '    jz .done', 13, 10
    db '    mov [edi], al      ; Write to MMIO', 13, 10
    db '    inc edi', 13, 10
    db '    jmp .loop', 13, 10, 13, 10, 0

msg_demo4_header:
    db '[Demo 4] Comparison: I/O Ports vs MMIO', 13, 10
    db '----------------------------------------', 13, 10, 0

msg_comparison:
    db 'I/O Ports:', 13, 10
    db '  + Separate address space (0-65535)', 13, 10
    db '  + Special instructions (IN/OUT)', 13, 10
    db '  + x86-specific', 13, 10
    db '  + Legacy devices (serial, parallel)', 13, 10
    db '  - Limited to 64K ports', 13, 10
    db '  - Slower than memory access', 13, 10, 13, 10
    db 'MMIO:', 13, 10
    db '  + Uses normal memory instructions', 13, 10
    db '  + Same address space as RAM', 13, 10
    db '  + Portable across architectures', 13, 10
    db '  + Modern devices (GPU, NIC, etc.)', 13, 10
    db '  + Can use caching/optimization', 13, 10
    db '  - Requires paging for high addresses', 13, 10
    db '  - Can conflict with RAM if not mapped', 13, 10, 13, 10, 0

msg_demo5_header:
    db '[Demo 5] Common MMIO Devices', 13, 10
    db '-----------------------------', 13, 10, 0

msg_examples:
    db 'Real-world MMIO examples:', 13, 10
    db '  0xB8000: VGA text buffer (80x25)', 13, 10
    db '  0xA0000: VGA graphics buffer', 13, 10
    db '  0xFEC00000: LAPIC (local APIC)', 13, 10
    db '  0xFEE00000: I/O APIC', 13, 10
    db '  0xE0000000+: PCI device BARs', 13, 10
    db '  Custom: Emulator-specific devices', 13, 10, 13, 10, 0

msg_shutdown:
    db '========================================', 13, 10
    db '  All demos complete!', 13, 10
    db '========================================', 13, 10, 0

; Pad to 4KB
times 4096-($-$$) db 0
