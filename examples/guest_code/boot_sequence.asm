; ==============================================================================
; boot_sequence.asm - Complete Boot Sequence with Device Initialization
; ==============================================================================
;
; Description:
;   A comprehensive boot sequence demonstrating proper device initialization:
;   1. Initialize CPU segments and stack
;   2. Setup Programmable Interval Timer (PIT)
;   3. Setup interrupt handlers
;   4. Print boot messages via serial port
;   5. Enter idle loop with interrupt handling
;
; Build:
;   nasm -f bin boot_sequence.asm -o boot_sequence.bin
;
; Expected Output:
;   [BOOT] AetherVM Starting...
;   [BOOT] Initializing Timer...
;   [BOOT] Timer initialized at 100 Hz
;   [BOOT] Setting up interrupts...
;   [BOOT] Interrupts configured
;   [BOOT] Boot sequence complete!
;   [TICK] 10 [TICK] 20 [TICK] 30 ...
;
; ==============================================================================

[BITS 16]
[ORG 0x7C00]

; ==============================================================================
; Boot Entry Point
; ==============================================================================
start:
    ; ====== Stage 1: CPU Initialization ======
    cli                         ; Disable interrupts
    
    ; Clear segment registers
    xor ax, ax
    mov ds, ax                  ; Data segment = 0
    mov es, ax                  ; Extra segment = 0
    mov fs, ax                  ; FS = 0
    mov gs, ax                  ; GS = 0
    
    ; Setup stack
    mov ss, ax                  ; Stack segment = 0
    mov sp, 0x7C00              ; Stack grows down from boot sector
    
    ; Clear direction flag (string operations increment)
    cld
    
    ; Print startup message
    mov si, msg_starting
    call serial_write_string
    
    ; ====== Stage 2: Initialize Timer ======
    mov si, msg_init_timer
    call serial_write_string
    
    call init_timer
    
    mov si, msg_timer_ok
    call serial_write_string
    
    ; ====== Stage 3: Setup Interrupts ======
    mov si, msg_init_interrupts
    call serial_write_string
    
    call setup_interrupts
    
    mov si, msg_interrupts_ok
    call serial_write_string
    
    ; ====== Stage 4: Boot Complete ======
    mov si, msg_boot_complete
    call serial_write_string
    
    ; Enable interrupts
    sti
    
    ; ====== Stage 5: Main Loop ======
main_loop:
    hlt                         ; Wait for interrupt
    
    ; Check if we should print status
    mov ax, [tick_count]
    mov bx, ax
    sub bx, [last_print]
    cmp bx, 10                  ; Print every 10 ticks (1 second)
    jl main_loop
    
    ; Update last print time
    mov [last_print], ax
    
    ; Print tick marker
    mov si, msg_tick
    call serial_write_string
    
    ; Print tick count
    call print_decimal
    
    ; Print space
    mov al, ' '
    mov dx, 0x3F8
    out dx, al
    
    jmp main_loop

; ==============================================================================
; init_timer
;
; Initialize the Programmable Interval Timer (PIT)
; Sets Channel 0 to ~100 Hz (10ms intervals)
; ==============================================================================
init_timer:
    push ax
    
    ; Control Word:
    ;   Bits 7-6: 00 = Channel 0
    ;   Bits 5-4: 11 = Access mode: LSB followed by MSB
    ;   Bits 3-1: 011 = Mode 3 (Square Wave Generator)
    ;   Bit 0:    0 = Binary mode (not BCD)
    ; Result: 0x36
    mov al, 0x36
    out 0x43, al                ; Write to command register
    
    ; Calculate divisor for 100 Hz
    ; Base frequency: 1.193182 MHz
    ; Divisor: 1193182 / 100 = 11932 (0x2E9C)
    mov ax, 11932
    
    ; Write divisor (LSB first, then MSB)
    out 0x40, al                ; Write low byte
    mov al, ah
    out 0x40, al                ; Write high byte
    
    pop ax
    ret

; ==============================================================================
; setup_interrupts
;
; Configure interrupt vectors in the Interrupt Vector Table (IVT)
; IVT is at 0x0000-0x03FF (256 entries × 4 bytes)
; ==============================================================================
setup_interrupts:
    push ax
    push bx
    push es
    
    ; Point ES to IVT (segment 0x0000)
    xor ax, ax
    mov es, ax
    
    ; ===== IRQ 0 (Timer) = INT 0x08 =====
    ; IVT entry at 0x0000:0x0020 (0x08 × 4)
    mov bx, 0x08 * 4
    mov word [es:bx], timer_interrupt    ; Offset
    mov word [es:bx+2], 0                ; Segment 0
    
    ; ===== Initialize PIC (8259) =====
    ; For now, we'll use default PIC configuration
    ; In a full implementation, you would reprogram the PIC here
    
    pop es
    pop bx
    pop ax
    ret

; ==============================================================================
; timer_interrupt
;
; Interrupt Service Routine (ISR) for IRQ 0 (Timer)
; Called ~100 times per second
; ==============================================================================
timer_interrupt:
    ; Save registers
    push ax
    push dx
    
    ; Increment tick counter
    inc word [tick_count]
    
    ; Send End-Of-Interrupt (EOI) to PIC Master
    ; This tells the PIC we're done handling the interrupt
    mov al, 0x20                ; EOI command
    out 0x20, al                ; Send to PIC command port
    
    ; Restore registers
    pop dx
    pop ax
    
    ; Return from interrupt
    iret

; ==============================================================================
; serial_write_string
;
; Write null-terminated string to serial port (COM1)
;
; Input: SI = pointer to string
; Clobbers: AL, DX
; ==============================================================================
serial_write_string:
    push ax
    push dx
    push si
    
.loop:
    lodsb                       ; Load byte from [SI], increment SI
    test al, al                 ; Check for null terminator
    jz .done
    
    mov dx, 0x3F8               ; COM1 data port
    out dx, al                  ; Write character
    
    jmp .loop
    
.done:
    pop si
    pop dx
    pop ax
    ret

; ==============================================================================
; print_decimal
;
; Print 16-bit unsigned number in decimal
;
; Input: AX = number to print
; Clobbers: AX, BX, CX, DX
; ==============================================================================
print_decimal:
    push ax
    push bx
    push cx
    push dx
    
    mov cx, 0                   ; Digit counter
    mov bx, 10                  ; Divisor
    
    ; Handle zero special case
    test ax, ax
    jnz .divide_loop
    mov al, '0'
    mov dx, 0x3F8
    out dx, al
    jmp .done
    
.divide_loop:
    xor dx, dx                  ; Clear DX for division
    div bx                      ; AX = AX / 10, DX = remainder
    add dl, '0'                 ; Convert digit to ASCII
    push dx                     ; Save digit on stack
    inc cx                      ; Increment digit count
    test ax, ax                 ; Check if quotient is 0
    jnz .divide_loop
    
.print_loop:
    pop dx                      ; Get digit from stack
    mov al, dl
    mov dx, 0x3F8
    out dx, al                  ; Print digit
    loop .print_loop            ; Repeat for all digits
    
.done:
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; ==============================================================================
; Data Section
; ==============================================================================

; Boot messages
msg_starting:       db '[BOOT] AetherVM Starting...', 13, 10, 0
msg_init_timer:     db '[BOOT] Initializing Timer...', 13, 10, 0
msg_timer_ok:       db '[BOOT] Timer initialized at 100 Hz', 13, 10, 0
msg_init_interrupts: db '[BOOT] Setting up interrupts...', 13, 10, 0
msg_interrupts_ok:  db '[BOOT] Interrupts configured', 13, 10, 0
msg_boot_complete:  db '[BOOT] Boot sequence complete!', 13, 10, 0
msg_tick:           db '[TICK] ', 0

; Runtime data
tick_count:         dw 0        ; Number of timer ticks
last_print:         dw 0        ; Last tick when we printed status

; ==============================================================================
; Boot Sector Padding and Signature
; ==============================================================================
times 510-($-$$) db 0           ; Pad to 510 bytes
dw 0xAA55                       ; Boot sector signature (0x55 0xAA in little-endian)
