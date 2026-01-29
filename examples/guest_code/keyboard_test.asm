; keyboard_test.asm - PS/2 Keyboard Input Test
; Tests the Intel 8042 keyboard controller
;
; This program:
; 1. Initializes the keyboard controller
; 2. Waits for keyboard input
; 3. Reads scancodes and displays them on serial port
; 4. Shows make codes (key press) and break codes (key release)
;
; Build: nasm -f bin keyboard_test.asm -o keyboard_test.bin
; Or use: ./build.sh keyboard_test

[BITS 16]
[ORG 0x7C00]

start:
    ; Set up segments
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00

    ; Initialize serial port (COM1)
    call init_serial

    ; Display welcome message
    mov si, msg_welcome
    call print_serial

    ; Initialize keyboard controller
    call init_keyboard

    ; Display ready message
    mov si, msg_ready
    call print_serial

main_loop:
    ; Check if keyboard data available
    call keyboard_available
    jnc main_loop           ; No data, loop

    ; Read scancode from keyboard
    call read_keyboard
    mov bl, al              ; Save scancode in BL

    ; Display scancode in hex
    mov si, msg_scancode
    call print_serial

    mov al, bl              ; Restore scancode
    call print_hex_byte

    ; Print newline
    mov si, msg_newline
    call print_serial

    ; Check for ESC key (scancode 0x01)
    cmp bl, 0x01
    je exit_program

    jmp main_loop

exit_program:
    mov si, msg_exit
    call print_serial
    hlt

;------------------------------------------------------------------------------
; Serial Port Functions (COM1 @ 0x3F8)
;------------------------------------------------------------------------------

init_serial:
    ; Disable interrupts
    mov dx, 0x3F9           ; IER (Interrupt Enable Register)
    xor al, al
    out dx, al

    ; Set baud rate divisor (115200 baud)
    mov dx, 0x3FB           ; LCR (Line Control Register)
    mov al, 0x80            ; Enable DLAB
    out dx, al

    mov dx, 0x3F8           ; DLL (Divisor Latch Low)
    mov al, 0x01            ; Divisor = 1 (115200 baud)
    out dx, al

    mov dx, 0x3F9           ; DLH (Divisor Latch High)
    xor al, al
    out dx, al

    ; 8N1 mode
    mov dx, 0x3FB           ; LCR
    mov al, 0x03            ; 8 data bits, no parity, 1 stop bit
    out dx, al

    ; Enable FIFO
    mov dx, 0x3FA           ; FCR (FIFO Control Register)
    mov al, 0xC7            ; Enable FIFO, clear, 14-byte threshold
    out dx, al

    ; Enable RTS/DTR
    mov dx, 0x3FC           ; MCR (Modem Control Register)
    mov al, 0x03            ; RTS + DTR
    out dx, al

    ret

print_serial:
    ; Print null-terminated string (DS:SI) to serial port
.loop:
    lodsb                   ; Load byte from DS:SI into AL
    test al, al             ; Check for null terminator
    jz .done
    call write_serial_char
    jmp .loop
.done:
    ret

write_serial_char:
    ; Write character in AL to serial port
    push dx
    mov dx, 0x3FD           ; LSR (Line Status Register)
.wait:
    in al, dx
    test al, 0x20           ; Check THRE (Transmit Holding Register Empty)
    jz .wait
    pop dx

    mov dx, 0x3F8           ; THR (Transmit Holding Register)
    out dx, al
    ret

;------------------------------------------------------------------------------
; Keyboard Controller Functions (8042 @ 0x60, 0x64)
;------------------------------------------------------------------------------

init_keyboard:
    ; Disable keyboard
    mov al, 0xAD            ; Disable keyboard command
    out 0x64, al
    call kbd_wait_input

    ; Read Controller Command Byte
    mov al, 0x20            ; Read CCB command
    out 0x64, al
    call kbd_wait_output
    in al, 0x60
    mov ah, al              ; Save CCB in AH

    ; Modify CCB: enable interrupts and translation
    or ah, 0x01             ; Enable keyboard interrupt
    or ah, 0x40             ; Enable translation (Set 1)
    and ah, 0xEF            ; Clear disable bit

    ; Write Controller Command Byte
    mov al, 0x60            ; Write CCB command
    out 0x64, al
    call kbd_wait_input
    mov al, ah              ; CCB value
    out 0x60, al
    call kbd_wait_input

    ; Enable keyboard
    mov al, 0xAE            ; Enable keyboard command
    out 0x64, al
    call kbd_wait_input

    ; Reset keyboard
    mov al, 0xFF            ; Reset command
    out 0x60, al
    call kbd_wait_output

    ; Read ACK
    in al, 0x60
    cmp al, 0xFA            ; Check for ACK
    jne .reset_fail

    ; Read self-test result
    call kbd_wait_output
    in al, 0x60
    cmp al, 0xAA            ; Check for pass (0xAA)
    jne .reset_fail

    ret

.reset_fail:
    ; Reset failed, but continue anyway
    ret

kbd_wait_input:
    ; Wait for input buffer to be empty
    push ax
.wait:
    in al, 0x64             ; Read status register
    test al, 0x02           ; Check IBF (Input Buffer Full)
    jnz .wait
    pop ax
    ret

kbd_wait_output:
    ; Wait for output buffer to have data
    push ax
.wait:
    in al, 0x64             ; Read status register
    test al, 0x01           ; Check OBF (Output Buffer Full)
    jz .wait
    pop ax
    ret

keyboard_available:
    ; Check if keyboard data is available
    ; Returns: CF=1 if data available, CF=0 if not
    in al, 0x64             ; Read status register
    test al, 0x01           ; Check OBF
    jz .no_data
    stc                     ; Set carry flag
    ret
.no_data:
    clc                     ; Clear carry flag
    ret

read_keyboard:
    ; Read scancode from keyboard
    ; Returns: AL = scancode
    call kbd_wait_output
    in al, 0x60             ; Read data port
    ret

;------------------------------------------------------------------------------
; Utility Functions
;------------------------------------------------------------------------------

print_hex_byte:
    ; Print byte in AL as hex (e.g., "A5")
    push ax
    push cx

    mov cl, al              ; Save AL in CL

    ; Print high nibble
    shr al, 4               ; Get high nibble
    call print_hex_digit

    ; Print low nibble
    mov al, cl              ; Restore AL
    and al, 0x0F            ; Get low nibble
    call print_hex_digit

    pop cx
    pop ax
    ret

print_hex_digit:
    ; Print single hex digit (0-F) in AL
    cmp al, 9
    jle .digit
    add al, 'A' - 10        ; Convert to A-F
    jmp .print
.digit:
    add al, '0'             ; Convert to 0-9
.print:
    call write_serial_char
    ret

;------------------------------------------------------------------------------
; Data Section
;------------------------------------------------------------------------------

msg_welcome:
    db 'Keyboard Test', 13, 10
    db 'Press keys to see scancodes (ESC to exit)', 13, 10, 0

msg_ready:
    db 'Ready!', 13, 10, 0

msg_scancode:
    db 'Scancode: 0x', 0

msg_newline:
    db 13, 10, 0

msg_exit:
    db 'ESC pressed, exiting...', 13, 10, 0

; Boot sector signature
times 510-($-$$) db 0
dw 0xAA55
