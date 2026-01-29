; ==============================================================================
; interrupt_demo.asm - Comprehensive Interrupt Handling Demonstration
; ==============================================================================
;
; Description:
;   A complete demonstration of interrupt handling in AetherVM including:
;   - Setting up the Interrupt Vector Table (IVT)
;   - Handling multiple interrupt types
;   - Timer interrupts (IRQ 0)
;   - Software interrupts (INT instruction)
;   - Exception handling
;
; Build:
;   nasm -f bin interrupt_demo.asm -o interrupt_demo.bin
;
; Expected Output:
;   Demonstrates various interrupt scenarios with detailed logging
;
; ==============================================================================

[BITS 16]
[ORG 0x7C00]

start:
    ; ====== Initialize System ======
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    cld
    
    ; ====== Print Header ======
    mov si, msg_header
    call print
    
    ; ====== Setup Interrupt Vectors ======
    mov si, msg_setup_ivt
    call print
    call setup_ivt
    
    mov si, msg_ivt_done
    call print
    
    ; ====== Initialize Timer ======
    mov si, msg_init_timer
    call print
    call init_timer
    
    ; ====== Enable Interrupts ======
    sti
    
    ; ====== Demo 1: Software Interrupt ======
    mov si, msg_demo_sw_int
    call print
    
    ; Call our custom software interrupt
    int 0x80                    ; Custom interrupt
    
    ; ====== Demo 2: Timer Interrupts ======
    mov si, msg_demo_timer
    call print
    
    ; Wait for some timer interrupts
    mov cx, 50                  ; Wait for ~50 timer ticks
.wait_loop:
    hlt
    cmp word [timer_ticks], cx
    jl .wait_loop
    
    ; Print timer statistics
    mov si, msg_timer_stats
    call print
    mov ax, [timer_ticks]
    call print_number
    mov si, msg_newline
    call print
    
    ; ====== Demo 3: Divide by Zero Exception ======
    mov si, msg_demo_div_zero
    call print
    
    ; This will trigger INT 0x00 (divide error)
    ; We've set up a handler that will catch it
    xor dx, dx
    mov ax, 100
    mov bx, 0
    ; div bx                    ; Uncomment to test (will crash if handler not working)
    
    ; Instead, manually trigger the exception
    int 0x00
    
    ; ====== Demo 4: Chain of Interrupts ======
    mov si, msg_demo_chain
    call print
    
    int 0x81                    ; Custom interrupt 2
    int 0x82                    ; Custom interrupt 3
    
    ; ====== Shutdown ======
    mov si, msg_shutdown
    call print
    
    cli
    hlt

; ==============================================================================
; setup_ivt
;
; Setup Interrupt Vector Table with our handlers
; ==============================================================================
setup_ivt:
    push ax
    push bx
    push es
    
    xor ax, ax
    mov es, ax                  ; ES = 0 (IVT base)
    
    ; ===== INT 0x00: Divide Error =====
    mov bx, 0x00 * 4
    mov word [es:bx], int_divide_error
    mov word [es:bx+2], 0
    
    ; ===== INT 0x08: Timer (IRQ 0) =====
    mov bx, 0x08 * 4
    mov word [es:bx], int_timer
    mov word [es:bx+2], 0
    
    ; ===== INT 0x80: Custom Software Interrupt =====
    mov bx, 0x80 * 4
    mov word [es:bx], int_custom_80
    mov word [es:bx+2], 0
    
    ; ===== INT 0x81: Custom Software Interrupt 2 =====
    mov bx, 0x81 * 4
    mov word [es:bx], int_custom_81
    mov word [es:bx+2], 0
    
    ; ===== INT 0x82: Custom Software Interrupt 3 =====
    mov bx, 0x82 * 4
    mov word [es:bx], int_custom_82
    mov word [es:bx+2], 0
    
    pop es
    pop bx
    pop ax
    ret

; ==============================================================================
; init_timer
;
; Initialize PIT to generate interrupts at 100 Hz
; ==============================================================================
init_timer:
    push ax
    
    mov al, 0x36                ; Channel 0, LSB+MSB, Mode 3
    out 0x43, al
    
    mov ax, 11932               ; 100 Hz
    out 0x40, al
    mov al, ah
    out 0x40, al
    
    pop ax
    ret

; ==============================================================================
; Interrupt Handlers
; ==============================================================================

; ----- INT 0x00: Divide Error -----
int_divide_error:
    push ax
    push si
    
    inc word [div_error_count]
    
    mov si, msg_int_div
    call print
    
    pop si
    pop ax
    iret

; ----- INT 0x08: Timer (IRQ 0) -----
int_timer:
    push ax
    push dx
    
    inc word [timer_ticks]
    
    ; Send EOI to PIC
    mov al, 0x20
    out 0x20, al
    
    pop dx
    pop ax
    iret

; ----- INT 0x80: Custom Software Interrupt -----
int_custom_80:
    push ax
    push si
    
    inc word [custom_int_count]
    
    mov si, msg_int_80
    call print
    
    pop si
    pop ax
    iret

; ----- INT 0x81: Custom Software Interrupt 2 -----
int_custom_81:
    push ax
    push si
    
    mov si, msg_int_81
    call print
    
    pop si
    pop ax
    iret

; ----- INT 0x82: Custom Software Interrupt 3 -----
int_custom_82:
    push ax
    push si
    
    mov si, msg_int_82
    call print
    
    pop si
    pop ax
    iret

; ==============================================================================
; Utility Functions
; ==============================================================================

; ----- print: Write null-terminated string to serial port -----
print:
    push ax
    push dx
    push si
.loop:
    lodsb
    test al, al
    jz .done
    mov dx, 0x3F8
    out dx, al
    jmp .loop
.done:
    pop si
    pop dx
    pop ax
    ret

; ----- print_number: Print 16-bit number in decimal -----
print_number:
    push ax
    push bx
    push cx
    push dx
    
    mov cx, 0
    mov bx, 10
    
.divide:
    xor dx, dx
    div bx
    add dl, '0'
    push dx
    inc cx
    test ax, ax
    jnz .divide
    
.print:
    pop dx
    mov al, dl
    mov dx, 0x3F8
    out dx, al
    loop .print
    
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; ==============================================================================
; Data Section
; ==============================================================================

; Messages
msg_header:
    db '========================================', 13, 10
    db 'Interrupt Handling Demonstration', 13, 10
    db '========================================', 13, 10, 0

msg_setup_ivt:
    db '[1] Setting up Interrupt Vector Table...', 13, 10, 0
msg_ivt_done:
    db '    IVT configured successfully', 13, 10, 0

msg_init_timer:
    db '[2] Initializing timer (100 Hz)...', 13, 10, 0

msg_demo_sw_int:
    db '[3] Demonstrating software interrupt (INT 0x80)...', 13, 10, 0

msg_demo_timer:
    db '[4] Testing timer interrupts...', 13, 10, 0

msg_timer_stats:
    db '    Timer ticks received: ', 0

msg_demo_div_zero:
    db '[5] Testing divide error exception (INT 0x00)...', 13, 10, 0

msg_demo_chain:
    db '[6] Testing interrupt chain...', 13, 10, 0

msg_shutdown:
    db '[7] All tests complete!', 13, 10
    db '========================================', 13, 10, 0

msg_int_div:
    db '    [INT 0x00] Divide error caught!', 13, 10, 0
msg_int_80:
    db '    [INT 0x80] Custom interrupt handler called', 13, 10, 0
msg_int_81:
    db '    [INT 0x81] Custom interrupt 2', 13, 10, 0
msg_int_82:
    db '    [INT 0x82] Custom interrupt 3', 13, 10, 0

msg_newline:
    db 13, 10, 0

; Runtime data
timer_ticks:        dw 0
div_error_count:    dw 0
custom_int_count:   dw 0

; ==============================================================================
; Boot Sector Signature
; ==============================================================================
times 510-($-$$) db 0
dw 0xAA55
