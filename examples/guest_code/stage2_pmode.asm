; AetherVM Stage 2 Protected Mode Bootloader
; Size: Up to 64KB (currently ~2KB)
; Load Address: 0x8000
; Purpose: Transition from 16-bit real mode to 32-bit protected mode

[BITS 16]           ; Start in 16-bit real mode
[ORG 0x8000]        ; Stage 2 is loaded at 0x8000

section .text

; Entry point - called by Stage 1
stage2_start:
    ; Re-initialize segments
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    
    ; Clear screen
    call clear_screen
    
    ; Display banner
    mov si, msg_banner
    call print_string_color
    
    ; Display memory info
    call display_memory_info
    
    ; Enable A20 line
    mov si, msg_a20
    call print_string_color
    call enable_a20
    
    ; Check A20 status
    call check_a20
    test ax, ax
    jz .a20_failed
    
    mov si, msg_a20_ok
    call print_string_color
    jmp .a20_done
    
.a20_failed:
    mov si, msg_a20_fail
    call print_string_color
    jmp hang
    
.a20_done:
    ; Load GDT
    mov si, msg_gdt
    call print_string_color
    lgdt [gdt_descriptor]
    mov si, msg_gdt_ok
    call print_string_color
    
    ; Disable interrupts before mode switch
    cli
    
    ; Display transition message
    mov si, msg_switching
    call print_string_color
    
    ; Enable protected mode
    mov eax, cr0
    or eax, 1           ; Set PE (Protection Enable) bit
    mov cr0, eax
    
    ; Far jump to flush prefetch queue and load CS with protected mode selector
    jmp 0x08:protected_mode_entry

; Hang if something goes wrong
hang:
    hlt
    jmp hang

;=============================================================================
; 16-bit Real Mode Functions
;=============================================================================

; Clear screen using BIOS
clear_screen:
    pusha
    mov ah, 0x06        ; Scroll up function
    xor al, al          ; Clear entire window
    xor cx, cx          ; Upper left (0,0)
    mov dx, 0x184F      ; Lower right (24,79)
    mov bh, 0x07        ; White on black
    int 0x10
    
    ; Reset cursor position
    mov ah, 0x02
    xor bh, bh
    xor dx, dx
    int 0x10
    popa
    ret

; Print null-terminated string with embedded color
; First byte of string is color attribute
; SI = pointer to string
print_string_color:
    pusha
    lodsb               ; Load color byte
    mov bl, al          ; BL = color
.loop:
    lodsb
    test al, al
    jz .done
    mov ah, 0x0E
    int 0x10
    jmp .loop
.done:
    popa
    ret

; Display memory information
display_memory_info:
    pusha
    mov si, msg_memory
    call print_string_color
    
    ; Get low memory (KB) using INT 12h
    int 0x12
    mov bx, ax
    call print_decimal
    
    mov si, msg_kb
    call print_string_color
    popa
    ret

; Print decimal number in BX
print_decimal:
    pusha
    mov cx, 0           ; Digit counter
    mov ax, bx
    
.push_digits:
    xor dx, dx
    mov bx, 10
    div bx              ; AX = quotient, DX = remainder
    push dx
    inc cx
    test ax, ax
    jnz .push_digits
    
.print_digits:
    pop ax
    add al, '0'
    mov ah, 0x0E
    xor bx, bx
    int 0x10
    loop .print_digits
    
    popa
    ret

; Enable A20 line using multiple methods
enable_a20:
    pusha
    
    ; Method 1: Try BIOS INT 15h
    mov ax, 0x2401
    int 0x15
    
    ; Method 2: Try keyboard controller
    call enable_a20_keyboard
    
    ; Method 3: Try fast A20 gate
    in al, 0x92
    or al, 2
    out 0x92, al
    
    popa
    ret

; Enable A20 via keyboard controller
enable_a20_keyboard:
    cli
    
    ; Wait for input buffer to be empty
    call wait_input_empty
    
    ; Send command to read output port
    mov al, 0xD0
    out 0x64, al
    
    ; Wait for output buffer to be full
    call wait_output_full
    
    ; Read output port value
    in al, 0x60
    push ax
    
    ; Wait for input buffer to be empty
    call wait_input_empty
    
    ; Send command to write output port
    mov al, 0xD1
    out 0x64, al
    
    ; Wait for input buffer to be empty
    call wait_input_empty
    
    ; Write output port value with A20 enabled
    pop ax
    or al, 2            ; Set A20 bit
    out 0x60, al
    
    ; Wait for input buffer to be empty
    call wait_input_empty
    
    sti
    ret

; Wait for keyboard controller input buffer to be empty
wait_input_empty:
    in al, 0x64
    test al, 2
    jnz wait_input_empty
    ret

; Wait for keyboard controller output buffer to be full
wait_output_full:
    in al, 0x64
    test al, 1
    jz wait_output_full
    ret

; Check if A20 line is enabled
; Returns: AX = 1 if enabled, 0 if disabled
check_a20:
    pushf
    push ds
    push es
    push di
    push si
    
    cli
    
    xor ax, ax
    mov es, ax          ; ES = 0x0000
    
    not ax
    mov ds, ax          ; DS = 0xFFFF
    
    mov di, 0x0500      ; ES:DI = 0x0000:0x0500
    mov si, 0x0510      ; DS:SI = 0xFFFF:0x0510 (wraps to 0x0000:0x0500 if A20 disabled)
    
    mov al, byte [es:di]
    push ax             ; Save original value
    
    mov al, byte [ds:si]
    push ax             ; Save original value
    
    mov byte [es:di], 0x00
    mov byte [ds:si], 0xFF
    
    cmp byte [es:di], 0xFF
    
    pop ax
    mov byte [ds:si], al    ; Restore original value
    
    pop ax
    mov byte [es:di], al    ; Restore original value
    
    mov ax, 0
    je .disabled
    mov ax, 1
    
.disabled:
    pop si
    pop di
    pop es
    pop ds
    popf
    ret

;=============================================================================
; Protected Mode Code (32-bit)
;=============================================================================

[BITS 32]

protected_mode_entry:
    ; Load segment selectors
    mov ax, 0x10        ; Data segment selector (offset 0x10 in GDT)
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    
    ; Setup stack in protected mode
    mov esp, 0x90000    ; Stack at 576KB
    
    ; Display success message in protected mode
    mov esi, pmode_msg
    mov edi, 0xB8000    ; VGA text buffer
    mov ah, 0x0A        ; Light green on black
    
.print_loop:
    lodsb
    test al, al
    jz .done
    stosw               ; Write character and attribute
    jmp .print_loop
    
.done:
    ; Simple kernel - display colored blocks to show we're in protected mode
    call pmode_demo
    
    ; Halt
    cli
    hlt
    jmp $

; Protected mode demo - display colored pattern
pmode_demo:
    mov edi, 0xB8000 + (2 * 80 * 2)  ; Start at row 2
    mov ecx, 16         ; 16 colors
    
.color_loop:
    push ecx
    
    ; Set color (background = foreground = current color)
    mov ah, cl
    shl ah, 4
    or ah, cl
    
    ; Print 8 blocks
    mov al, 0xDB        ; Block character
    mov ecx, 8
    rep stosw
    
    ; Skip to next line (80 chars per line, 2 bytes per char)
    add edi, (80 - 8) * 2
    
    pop ecx
    loop .color_loop
    
    ret

;=============================================================================
; Global Descriptor Table (GDT)
;=============================================================================

align 8
gdt_start:

; Null descriptor (required)
gdt_null:
    dd 0x0
    dd 0x0

; Code segment descriptor
gdt_code:
    dw 0xFFFF           ; Limit (bits 0-15)
    dw 0x0000           ; Base (bits 0-15)
    db 0x00             ; Base (bits 16-23)
    db 10011010b        ; Access byte: Present, Ring 0, Code, Execute/Read
    db 11001111b        ; Flags + Limit (bits 16-19): 4KB granularity, 32-bit
    db 0x00             ; Base (bits 24-31)

; Data segment descriptor
gdt_data:
    dw 0xFFFF           ; Limit (bits 0-15)
    dw 0x0000           ; Base (bits 0-15)
    db 0x00             ; Base (bits 16-23)
    db 10010010b        ; Access byte: Present, Ring 0, Data, Read/Write
    db 11001111b        ; Flags + Limit (bits 16-19): 4KB granularity, 32-bit
    db 0x00             ; Base (bits 24-31)

gdt_end:

; GDT descriptor
gdt_descriptor:
    dw gdt_end - gdt_start - 1  ; Size of GDT - 1
    dd gdt_start                ; Base address of GDT

;=============================================================================
; Data Section (16-bit strings)
;=============================================================================

section .data

; Color codes: 0x07=gray, 0x09=blue, 0x0A=green, 0x0E=yellow, 0x0F=white

msg_banner:
    db 0x0E
    db 13, 10
    db "==============================================", 13, 10
    db "  AetherVM Stage 2 - Protected Mode Boot", 13, 10
    db "==============================================", 13, 10, 13, 10, 0

msg_memory:
    db 0x09, "Detecting Memory: ", 0

msg_kb:
    db 0x09, " KB", 13, 10, 13, 10, 0

msg_a20:
    db 0x0F, "[1/3] Enabling A20 line...", 0

msg_a20_ok:
    db 0x0A, " OK", 13, 10, 0

msg_a20_fail:
    db 0x0C, " FAILED", 13, 10
    db "Cannot enable A20 line. System halted.", 13, 10, 0

msg_gdt:
    db 0x0F, "[2/3] Loading GDT...", 0

msg_gdt_ok:
    db 0x0A, " OK", 13, 10, 0

msg_switching:
    db 0x0F, "[3/3] Switching to protected mode...", 13, 10, 13, 10, 0

; Protected mode message (32-bit)
pmode_msg:
    db "*** PROTECTED MODE ACTIVE ***", 0

; Pad to 2KB (can be expanded up to 64KB)
times 2048-($-$$) db 0
