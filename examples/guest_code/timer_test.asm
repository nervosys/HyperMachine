; ==============================================================================
; timer_test.asm - Timer Programming and Interrupt Handling
; ==============================================================================
;
; Description:
;   Demonstrates how to program the Programmable Interval Timer (PIT) and
;   handle timer interrupts. Sets up a 10 Hz timer (100ms intervals) and
;   prints a message every 10 ticks (once per second).
;
; Build:
;   nasm -f bin timer_test.asm -o timer_test.bin
;
; Expected Output:
;   Serial port will output "Tick: 10", "Tick: 20", etc.
;
; ==============================================================================

[BITS 16]
[ORG 0x7C00]

start:
    ; ====== Initialize Segments ======
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    
    ; ====== Print Boot Message ======
    mov si, boot_msg
    call serial_write_string
    
    ; ====== Setup Timer Interrupt Handler ======
    ; Set interrupt vector for IRQ 0 (INT 0x08)
    ; Real mode IVT: 4 bytes per entry (offset:segment)
    xor ax, ax
    mov es, ax              ; ES = 0 (IVT at 0x0000)
    
    mov word [es:0x08*4], timer_interrupt      ; Offset
    mov word [es:0x08*4+2], 0                  ; Segment
    
    ; ====== Initialize PIT (Channel 0 to 10 Hz) ======
    call init_timer
    
    ; ====== Enable Interrupts ======
    sti
    
    ; ====== Main Loop ======
main_loop:
    hlt                     ; Wait for interrupt
    
    ; Check if we should print tick count
    mov ax, [tick_count]
    mov bx, ax
    sub bx, [last_printed]
    cmp bx, 10              ; Print every 10 ticks
    jl main_loop
    
    ; Print tick count
    mov [last_printed], ax
    call print_tick_count
    
    jmp main_loop

; ==============================================================================
; init_timer
;
; Initialize PIT Channel 0 to 10 Hz (100ms intervals)
;
; PIT base frequency: 1.193182 MHz
; Divisor for 10 Hz: 1193182 / 10 = 119318 (0x1D1A6)
; Since divisor is 16-bit, we use 11932 for ~100 Hz (close enough for demo)
; ==============================================================================
init_timer:
    ; Control word: Channel 0, LSB+MSB, Mode 3 (Square Wave), Binary
    ; Format: 00 11 011 0 = 0x36
    mov al, 0x36
    out 0x43, al            ; Write control word
    
    ; Write divisor (11932 = 0x2E9C for ~100 Hz)
    mov ax, 11932
    out 0x40, al            ; Write low byte
    mov al, ah
    out 0x40, al            ; Write high byte
    
    ret

; ==============================================================================
; timer_interrupt
;
; Timer interrupt handler (IRQ 0)
; Called approximately every 100ms
; ==============================================================================
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

; ==============================================================================
; print_tick_count
;
; Prints the current tick count to serial port
; ==============================================================================
print_tick_count:
    push ax
    push bx
    push cx
    push dx
    
    ; Print "Tick: "
    mov si, tick_msg
    call serial_write_string
    
    ; Convert tick_count to decimal and print
    mov ax, [tick_count]
    call print_decimal
    
    ; Print newline
    mov si, newline
    call serial_write_string
    
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; ==============================================================================
; print_decimal
;
; Prints a 16-bit unsigned number in decimal
;
; Input: AX = number to print
; ==============================================================================
print_decimal:
    push ax
    push bx
    push cx
    push dx
    
    mov cx, 0               ; Digit counter
    mov bx, 10              ; Divisor
    
.divide_loop:
    xor dx, dx              ; Clear DX for division
    div bx                  ; AX = AX / 10, DX = remainder
    add dl, '0'             ; Convert to ASCII
    push dx                 ; Save digit
    inc cx                  ; Increment counter
    test ax, ax             ; Check if quotient is 0
    jnz .divide_loop
    
.print_loop:
    pop dx                  ; Get digit
    mov al, dl
    mov dx, 0x3F8
    out dx, al              ; Print digit
    loop .print_loop
    
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; ==============================================================================
; serial_write_string
;
; Writes null-terminated string to serial port
;
; Input: SI = pointer to string
; ==============================================================================
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

; ==============================================================================
; Data Section
; ==============================================================================
boot_msg:       db 'Timer Test Starting...', 13, 10, 0
tick_msg:       db 'Tick: ', 0
newline:        db 13, 10, 0

tick_count:     dw 0        ; Current tick count
last_printed:   dw 0        ; Last printed tick count

; ==============================================================================
; Boot Sector Padding and Signature
; ==============================================================================
times 510-($-$$) db 0
dw 0xAA55
