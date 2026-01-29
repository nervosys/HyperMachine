; vga_demo.asm - VGA Text Mode Demo (Optimized)
; Demonstrates VGA 80x25 text mode with colors
;
; Build: nasm -f bin vga_demo.asm -o vga_demo.bin

[BITS 16]
[ORG 0x7C00]

start:
    ; Setup
    xor ax, ax
    mov ds, ax
    mov ss, ax
    mov sp, 0x7C00
    
    ; VGA segment
    mov ax, 0xB800
    mov es, ax

    ; Clear screen (blue bg)
    xor di, di
    mov cx, 2000
    mov ax, 0x1720      ; Blue bg, white space
.cls:
    stosw
    loop .cls

    ; Title at row 0, centered
    mov di, 30          ; Col 15
    mov si, title
    mov ah, 0x1F        ; Blue bg, bright white
.title:
    lodsb
    test al, al
    jz .colors
    stosw
    jmp .title

.colors:
    ; Color palette at row 2
    mov di, 320         ; Row 2
    mov si, colors_msg
    mov ah, 0x17
.clbl:
    lodsb
    test al, al
    jz .clrs
    stosw
    jmp .clbl

.clrs:
    ; 16 color blocks
    xor bl, bl
.cloop:
    ; Hex digit
    mov al, bl
    call hex_char
    mov ah, 0x17
    stosw
    
    ; Colon + color block
    mov ax, 0x173A      ; ':'
    stosw
    
    mov al, 0xDB        ; █
    mov ah, bl
    or ah, 0x10         ; Blue bg
    stosw
    stosw
    stosw
    
    ; Space
    mov ax, 0x1720
    stosw
    
    inc bl
    cmp bl, 16
    jb .cloop

    ; Gradient at row 5
    mov di, 800         ; Row 5
    mov cx, 80
    xor bl, bl
.grad:
    mov al, 0xB0        ; ░
    mov ah, bl
    and ah, 0x0F
    or ah, 0x10
    stosw
    inc bl
    loop .grad

    ; Text samples
    mov di, 1124        ; Row 7, col 2
    mov si, txt1
    mov ah, 0x1A        ; Green
    call pvga
    
    mov di, 1444        ; Row 9
    mov si, txt2
    mov ah, 0x1C        ; Red
    call pvga
    
    mov di, 1764        ; Row 11
    mov si, txt3
    mov ah, 0x1E        ; Yellow
    call pvga

    ; Box at row 13
    mov di, 2100        ; Row 13, col 10
    mov ax, 0x17DA      ; ┌
    stosw
    mov cx, 30
    mov ax, 0x17C4      ; ─
.bt:
    stosw
    loop .bt
    mov ax, 0x17BF      ; ┐
    stosw
    
    ; Box sides (6 rows)
    mov cx, 6
.sl:
    push cx
    add di, 160
    sub di, 64
    mov ax, 0x17B3      ; │
    stosw
    add di, 60
    stosw
    pop cx
    loop .sl
    
    ; Box bottom
    add di, 160
    sub di, 64
    mov ax, 0x17C0      ; └
    stosw
    mov cx, 30
    mov ax, 0x17C4
.bb:
    stosw
    loop .bb
    mov ax, 0x17D9      ; ┘
    stosw
    
    ; Box text
    mov di, 2420        ; Row 15, col 10
    mov si, btxt
    mov ah, 0x17
    call pvga

    ; Instructions at row 23
    mov di, 3684
    mov si, inst
    mov ah, 0x1F
    call pvga

.halt:
    hlt
    jmp .halt

; Print VGA string
pvga:
.l:
    lodsb
    test al, al
    jz .d
    stosw
    jmp .l
.d:
    ret

; Hex to ASCII
hex_char:
    and al, 0x0F
    cmp al, 9
    jle .d
    add al, 7
.d:
    add al, '0'
    ret

; Data
title:      db ' VGA TEXT MODE ', 0
colors_msg: db 'Colors: ', 0
txt1:       db 'GREEN text', 0
txt2:       db 'RED text', 0
txt3:       db 'YELLOW text', 0
btxt:       db ' Box ', 0
inst:       db 'Press Reset', 0

times 510-($-$$) db 0
dw 0xAA55


