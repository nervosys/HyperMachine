; rtc_test.asm - Real-Time Clock Test
; Tests the MC146818 RTC (Real-Time Clock)
;
; This program:
; 1. Initializes the RTC
; 2. Reads date and time from CMOS
; 3. Displays the current date/time on serial port
; 4. Reads CMOS configuration bytes
; 5. Loops and updates time every second
;
; Build: nasm -f bin rtc_test.asm -o rtc_test.bin
; Or use: ./build.sh rtc_test

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

    ; Initialize RTC
    call init_rtc

    ; Display date/time loop
main_loop:
    ; Read and display date/time
    call display_datetime

    ; Wait a bit (crude delay)
    mov cx, 0xFFFF
.delay_outer:
    mov dx, 0x0010
.delay_inner:
    dec dx
    jnz .delay_inner
    loop .delay_outer

    jmp main_loop

halt:
    hlt
    jmp halt

;------------------------------------------------------------------------------
; Serial Port Functions (COM1 @ 0x3F8)
;------------------------------------------------------------------------------

init_serial:
    ; Disable interrupts
    mov dx, 0x3F9           ; IER
    xor al, al
    out dx, al

    ; Set baud rate divisor (115200 baud)
    mov dx, 0x3FB           ; LCR
    mov al, 0x80            ; Enable DLAB
    out dx, al

    mov dx, 0x3F8           ; DLL
    mov al, 0x01            ; Divisor = 1
    out dx, al

    mov dx, 0x3F9           ; DLH
    xor al, al
    out dx, al

    ; 8N1 mode
    mov dx, 0x3FB           ; LCR
    mov al, 0x03            ; 8N1
    out dx, al

    ; Enable FIFO
    mov dx, 0x3FA           ; FCR
    mov al, 0xC7
    out dx, al

    ; Enable RTS/DTR
    mov dx, 0x3FC           ; MCR
    mov al, 0x03
    out dx, al

    ret

print_serial:
    ; Print null-terminated string (DS:SI) to serial port
.loop:
    lodsb
    test al, al
    jz .done
    call write_serial_char
    jmp .loop
.done:
    ret

write_serial_char:
    ; Write character in AL to serial port
    push dx
    mov dx, 0x3FD           ; LSR
.wait:
    in al, dx
    test al, 0x20           ; Check THRE
    jz .wait
    pop dx

    mov dx, 0x3F8           ; THR
    out dx, al
    ret

;------------------------------------------------------------------------------
; RTC Functions (MC146818 @ 0x70, 0x71)
;------------------------------------------------------------------------------

init_rtc:
    ; Set RTC to 24-hour binary mode
    ; Read Status Register B
    mov al, 0x0B            ; Status B register
    out 0x70, al
    jmp short $+2           ; I/O delay
    in al, 0x71

    ; Set 24-hour mode and binary mode
    or al, 0x02             ; 24-hour mode
    and al, 0xFB            ; Binary mode (clear BCD bit)

    ; Write back
    mov ah, al
    mov al, 0x0B
    out 0x70, al
    jmp short $+2
    mov al, ah
    out 0x71, al

    ret

read_rtc_register:
    ; Read RTC register (AL = register index)
    ; Returns: AL = register value
    out 0x70, al            ; Select register
    jmp short $+2           ; I/O delay
    in al, 0x71             ; Read data
    ret

display_datetime:
    ; Display current date and time

    ; Read time values
    mov al, 0x04            ; Hours
    call read_rtc_register
    mov [rtc_hours], al

    mov al, 0x02            ; Minutes
    call read_rtc_register
    mov [rtc_minutes], al

    mov al, 0x00            ; Seconds
    call read_rtc_register
    mov [rtc_seconds], al

    ; Read date values
    mov al, 0x09            ; Year (2-digit)
    call read_rtc_register
    mov [rtc_year], al

    mov al, 0x08            ; Month
    call read_rtc_register
    mov [rtc_month], al

    mov al, 0x07            ; Day of month
    call read_rtc_register
    mov [rtc_day], al

    mov al, 0x06            ; Day of week
    call read_rtc_register
    mov [rtc_weekday], al

    ; Display date
    mov si, msg_date
    call print_serial

    ; Display year (20xx format)
    mov si, msg_20
    call print_serial

    mov al, [rtc_year]
    call print_decimal_byte

    mov al, '-'
    call write_serial_char

    ; Display month
    mov al, [rtc_month]
    call print_decimal_byte

    mov al, '-'
    call write_serial_char

    ; Display day
    mov al, [rtc_day]
    call print_decimal_byte

    ; Display time
    mov si, msg_time
    call print_serial

    ; Display hours
    mov al, [rtc_hours]
    call print_decimal_byte

    mov al, ':'
    call write_serial_char

    ; Display minutes
    mov al, [rtc_minutes]
    call print_decimal_byte

    mov al, ':'
    call write_serial_char

    ; Display seconds
    mov al, [rtc_seconds]
    call print_decimal_byte

    ; Newline
    mov si, msg_newline
    call print_serial

    ret

;------------------------------------------------------------------------------
; Utility Functions
;------------------------------------------------------------------------------

print_decimal_byte:
    ; Print byte in AL as decimal (00-99)
    push ax
    push cx
    push dx

    xor ah, ah              ; Clear AH
    mov cl, 10
    div cl                  ; AL = quotient, AH = remainder

    ; Print tens digit
    push ax
    add al, '0'
    call write_serial_char
    pop ax

    ; Print ones digit
    mov al, ah
    add al, '0'
    call write_serial_char

    pop dx
    pop cx
    pop ax
    ret

;------------------------------------------------------------------------------
; Data Section
;------------------------------------------------------------------------------

msg_welcome:
    db 'RTC Test', 13, 10
    db 'Reading date/time from CMOS...', 13, 10, 13, 10, 0

msg_date:
    db 'Date: ', 0

msg_20:
    db '20', 0

msg_time:
    db '  Time: ', 0

msg_newline:
    db 13, 10, 0

; RTC data storage
rtc_seconds: db 0
rtc_minutes: db 0
rtc_hours: db 0
rtc_day: db 0
rtc_month: db 0
rtc_year: db 0
rtc_weekday: db 0

; Boot sector signature
times 510-($-$$) db 0
dw 0xAA55
