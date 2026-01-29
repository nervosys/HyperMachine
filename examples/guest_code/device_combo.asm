; device_combo.asm - Multi-Device Demo (Optimized)
; RTC, Keyboard, VGA, Serial integration
;
; Build: nasm -f bin device_combo.asm -o device_combo.bin

[BITS 16]
[ORG 0x7C00]

start:
    ; Setup
    xor ax, ax
    mov ds, ax
    mov ss, ax
    mov sp, 0x7C00
    mov ax, 0xB800
    mov es, ax

    ; Init devices
    call init_serial
    call init_kbd

    ; Clear screen
    xor di, di
    mov cx, 2000
    mov ax, 0x1720
.cls:
    stosw
    loop .cls

    ; Title
    mov di, 50
    mov si, title
    mov ah, 0x71
.t:
    lodsb
    test al, al
    jz .main
    stosw
    jmp .t

.main:
    ; Update time
    call upd_time
    
    ; Check keyboard
    in al, 0x64
    test al, 1
    jz .dl
    
    in al, 0x60
    mov [scan], al
    call show_key

.dl:
    mov cx, 0x800
.d:
    loop .d
    jmp .main

; Time update
upd_time:
    mov al, 4
    call rd_rtc
    mov [h], al
    mov al, 2
    call rd_rtc
    mov [m], al
    mov al, 0
    call rd_rtc
    mov [s], al

    ; Display HH:MM:SS at row 2
    mov di, 340
    mov al, [h]
    call dec2
    mov ax, 0x1E3A
    stosw
    mov al, [m]
    call dec2
    mov ax, 0x1E3A
    stosw
    mov al, [s]
    call dec2
    ret

; Show keyboard
show_key:
    mov di, 644
    mov al, [scan]
    push ax
    shr al, 4
    call hx
    mov ah, 0x1A
    stosw
    pop ax
    and al, 0x0F
    call hx
    mov ah, 0x1A
    stosw
    ret

; Decimal print (2 digits)
dec2:
    push ax
    xor ah, ah
    mov cl, 10
    div cl
    add al, '0'
    mov ah, 0x1E
    stosw
    pop ax
    and al, 0x0F
    add al, '0'
    mov ah, 0x1E
    stosw
    ret

; Hex digit
hx:
    and al, 0x0F
    cmp al, 9
    jle .d
    add al, 7
.d:
    add al, '0'
    ret

; Init serial
init_serial:
    mov dx, 0x3FB
    mov al, 0x80
    out dx, al
    mov dx, 0x3F8
    mov al, 1
    out dx, al
    mov dx, 0x3F9
    xor al, al
    out dx, al
    mov dx, 0x3FB
    mov al, 3
    out dx, al
    ret

; Init keyboard
init_kbd:
    mov al, 0xAE
    out 0x64, al
    ret

; Read RTC register (AL)
rd_rtc:
    out 0x70, al
    in al, 0x71
    ret

; Data
title: db ' MULTI-DEVICE ', 0
h: db 0
m: db 0
s: db 0
scan: db 0

times 510-($-$$) db 0
dw 0xAA55
