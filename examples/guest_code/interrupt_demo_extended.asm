; ==============================================================================
; interrupt_demo_extended.asm - Comprehensive Interrupt Handling (Multi-Stage)
; ==============================================================================
;
; Description:
;   A complete demonstration of interrupt handling in AetherVM using the
;   multi-stage bootloader. This is Stage 2 code loaded at 0x8000.
;
;   Features:
;   - Setting up the Interrupt Vector Table (IVT)
;   - Handling multiple interrupt types
;   - Timer interrupts (IRQ 0)
;   - Software interrupts (INT instruction)
;   - Exception handling
;
; Build:
;   Use build_interrupt_demo.ps1 script
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
    cld
    
    ; ====== Clear Screen ======
    call clear_screen
    
    ; ====== Print Header ======
    mov si, msg_header
    call print_screen
    
    ; ====== Setup Interrupt Vectors ======
    mov si, msg_setup_ivt
    call print_screen
    call setup_ivt
    
    mov si, msg_ivt_done
    call print_screen
    
    ; ====== Initialize PIC ======
    mov si, msg_init_pic
    call print_screen
    call init_pic
    
    ; ====== Initialize Timer ======
    mov si, msg_init_timer
    call print_screen
    call init_timer
    
    ; ====== Enable Interrupts ======
    sti
    
    ; ====== Demo 1: Software Interrupt ======
    mov si, msg_demo_sw_int
    call print_screen
    
    ; Call our custom software interrupt
    int 0x80                    ; Custom interrupt
    
    ; Small delay
    call delay_short
    
    ; ====== Demo 2: Timer Interrupts ======
    mov si, msg_demo_timer
    call print_screen
    
    ; Wait for some timer interrupts
    mov cx, 20                  ; Wait for ~20 timer ticks
.wait_loop:
    hlt                         ; Halt until next interrupt
    cmp word [timer_ticks], cx
    jl .wait_loop
    
    ; Print timer statistics
    mov si, msg_timer_stats
    call print_screen
    mov ax, [timer_ticks]
    call print_number
    mov si, msg_newline
    call print_screen
    
    call delay_short
    
    ; ====== Demo 3: Divide by Zero Exception ======
    mov si, msg_demo_div_zero
    call print_screen
    
    ; Manually trigger the divide error exception
    int 0x00
    
    call delay_short
    
    ; ====== Demo 4: Chain of Interrupts ======
    mov si, msg_demo_chain
    call print_screen
    
    int 0x81                    ; Custom interrupt 2
    int 0x82                    ; Custom interrupt 3
    
    call delay_short
    
    ; ====== Print Statistics ======
    mov si, msg_statistics
    call print_screen
    
    mov si, msg_stat_timer
    call print_screen
    mov ax, [timer_ticks]
    call print_number
    mov si, msg_newline
    call print_screen
    
    mov si, msg_stat_div
    call print_screen
    mov ax, [div_error_count]
    call print_number
    mov si, msg_newline
    call print_screen
    
    mov si, msg_stat_custom
    call print_screen
    mov ax, [custom_int_count]
    call print_number
    mov si, msg_newline
    call print_screen
    
    ; ====== Shutdown ======
    mov si, msg_shutdown
    call print_screen
    
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
; init_pic
;
; Initialize Programmable Interrupt Controller (PIC)
; ==============================================================================
init_pic:
    push ax
    
    ; Initialize master PIC
    mov al, 0x11                ; ICW1: Init with ICW4
    out 0x20, al
    
    mov al, 0x08                ; ICW2: Vector offset (IRQ 0 -> INT 0x08)
    out 0x21, al
    
    mov al, 0x04                ; ICW3: Slave on IRQ2
    out 0x21, al
    
    mov al, 0x01                ; ICW4: 8086 mode
    out 0x21, al
    
    ; Unmask IRQ 0 (timer) only
    mov al, 0xFE                ; Mask all except IRQ 0
    out 0x21, al
    
    pop ax
    ret

; ==============================================================================
; init_timer
;
; Initialize PIT to generate interrupts at 18.2 Hz (default BIOS rate)
; ==============================================================================
init_timer:
    push ax
    
    mov al, 0x36                ; Channel 0, LSB+MSB, Mode 3
    out 0x43, al
    
    ; 1193182 Hz / 65536 ≈ 18.2 Hz
    xor al, al
    out 0x40, al
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
    call print_screen
    
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
    call print_screen
    
    pop si
    pop ax
    iret

; ----- INT 0x81: Custom Software Interrupt 2 -----
int_custom_81:
    push ax
    push si
    
    mov si, msg_int_81
    call print_screen
    
    pop si
    pop ax
    iret

; ----- INT 0x82: Custom Software Interrupt 3 -----
int_custom_82:
    push ax
    push si
    
    mov si, msg_int_82
    call print_screen
    
    pop si
    pop ax
    iret

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

; ----- print_number: Print 16-bit number in decimal -----
print_number:
    pusha
    
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
    pop ax
    mov ah, 0x0E
    xor bx, bx
    int 0x10
    loop .print
    
    popa
    ret

; ----- delay_short: Short delay for readability -----
delay_short:
    push cx
    mov cx, 3
.loop:
    hlt
    loop .loop
    pop cx
    ret

; ==============================================================================
; Data Section
; ==============================================================================

; Messages
msg_header:
    db '========================================', 13, 10
    db '  Interrupt Handling Demonstration', 13, 10
    db '  (Multi-Stage Extended)', 13, 10
    db '========================================', 13, 10, 13, 10, 0

msg_setup_ivt:
    db '[1] Setting up Interrupt Vector Table...', 13, 10, 0
msg_ivt_done:
    db '    IVT configured successfully', 13, 10, 13, 10, 0

msg_init_pic:
    db '[2] Initializing PIC...', 13, 10, 0

msg_init_timer:
    db '[3] Initializing timer (18.2 Hz)...', 13, 10, 13, 10, 0

msg_demo_sw_int:
    db '[4] Demonstrating software interrupt (INT 0x80)...', 13, 10, 0

msg_demo_timer:
    db '[5] Testing timer interrupts (waiting for 20 ticks)...', 13, 10, 0

msg_timer_stats:
    db '    Timer ticks received: ', 0

msg_demo_div_zero:
    db '[6] Testing divide error exception (INT 0x00)...', 13, 10, 0

msg_demo_chain:
    db '[7] Testing interrupt chain...', 13, 10, 0

msg_statistics:
    db 13, 10, '========================================', 13, 10
    db '  Final Statistics', 13, 10
    db '========================================', 13, 10, 0

msg_stat_timer:
    db 'Timer interrupts: ', 0
msg_stat_div:
    db 'Divide errors handled: ', 0
msg_stat_custom:
    db 'Custom interrupts: ', 0

msg_shutdown:
    db 13, 10, '[8] All tests complete!', 13, 10
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

; Pad to 4KB (can be adjusted)
times 4096-($-$$) db 0
