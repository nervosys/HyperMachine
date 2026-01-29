; Stage 1 Bootloader - Multi-Stage Boot System
; ===========================================
; This is the first stage bootloader that fits in 512 bytes (boot sector).
; It loads Stage 2 from a predefined memory location and transfers control to it.
;
; Memory Layout:
;   0x7C00 - 0x7DFF: Stage 1 (this boot sector, 512 bytes)
;   0x8000 - 0xFFFF: Stage 2 (loaded here, up to 32KB)
;
; Boot Process:
;   1. BIOS loads Stage 1 at 0x7C00
;   2. Stage 1 initializes segments and display
;   3. Stage 1 copies Stage 2 from embedded location to 0x8000
;   4. Stage 1 jumps to Stage 2 at 0x8000
;
; For AetherVM testing, Stage 2 is embedded after the boot signature.

[BITS 16]
[ORG 0x7C00]

; Entry point
start:
    ; Initialize segments
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00          ; Stack grows down from boot sector

    ; Clear direction flag (string operations increment)
    cld

    ; Display Stage 1 banner
    mov si, msg_stage1
    call print_string

    ; Display loading message
    mov si, msg_loading
    call print_string

    ; Load Stage 2 from embedded location
    ; In a real bootloader, this would read from disk
    ; For AetherVM, we'll copy from a known memory location
    call load_stage2

    ; Display success message
    mov si, msg_success
    call print_string

    ; Display jump message
    mov si, msg_jump
    call print_string

    ; Small delay to see messages
    mov cx, 0x1000
.delay:
    loop .delay

    ; Jump to Stage 2
    jmp 0x0000:0x8000       ; Far jump to Stage 2 at 0x8000

; Load Stage 2 into memory at 0x8000
; In a real bootloader, this would use INT 13h to read from disk
; For AetherVM, Stage 2 is loaded right after boot sector at 0x7E00
load_stage2:
    push ax
    push cx
    push si
    push di
    push ds
    push es

    ; Set up source (DS:SI = 0x07E0:0x0000, which is 0x7E00)
    mov ax, 0x07E0
    mov ds, ax
    xor si, si

    ; Set up destination (ES:DI = 0x0000:0x8000)
    xor ax, ax
    mov es, ax
    mov di, 0x8000

    ; Copy 1024 bytes (Stage 2 size)
    mov cx, 512             ; 512 words = 1024 bytes
    rep movsw               ; Copy word by word

    pop es
    pop ds
    pop di
    pop si
    pop cx
    pop ax
    ret

; Print null-terminated string at DS:SI
print_string:
    push ax
    push bx
    push si
.next_char:
    lodsb                   ; Load byte from DS:SI into AL, increment SI
    test al, al             ; Check if null terminator
    jz .done
    
    ; BIOS teletype output
    mov ah, 0x0E            ; BIOS function: Teletype output
    mov bh, 0               ; Page number
    mov bl, 0x07            ; Light gray on black
    int 0x10                ; BIOS video interrupt
    
    jmp .next_char
.done:
    pop si
    pop bx
    pop ax
    ret

; Data section
msg_stage1:  db 'AetherVM Multi-Stage Boot', 13, 10, 0
msg_loading: db 'Stage 1: Loading Stage 2...', 13, 10, 0
msg_success: db 'Stage 1: Stage 2 loaded OK', 13, 10, 0
msg_jump:    db 'Stage 1: Jumping to Stage 2', 13, 10, 0

; Padding to fill boot sector
times 510-($-$$) db 0

; Boot signature
dw 0xAA55
