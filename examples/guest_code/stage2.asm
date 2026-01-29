; Stage 2 Bootloader - Extended Functionality
; ===========================================
; This is the second stage bootloader that can be much larger than 512 bytes.
; It's loaded by Stage 1 at address 0x8000 and provides extended functionality.
;
; Features demonstrated:
;   - VGA color text display
;   - Memory detection
;   - Simple menu system
;   - Preparation for protected mode transition
;
; Size: Up to 64KB (but we'll keep it reasonable for testing)

[BITS 16]
[ORG 0x8000]

; Entry point (jumped to from Stage 1)
stage2_start:
    ; Re-initialize segments (safety)
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00          ; Same stack as Stage 1

    ; Clear screen
    call clear_screen

    ; Display Stage 2 banner
    mov si, msg_banner
    call print_string_color

    ; Display memory info
    call display_memory_info

    ; Display features menu
    call display_menu

    ; Main loop - wait for keypress
.main_loop:
    call get_keystroke
    
    ; Check for option selection
    cmp al, '1'
    je .option_vga_demo
    cmp al, '2'
    je .option_memory_test
    cmp al, '3'
    je .option_system_info
    cmp al, 'q'
    je .quit
    cmp al, 'Q'
    je .quit
    
    jmp .main_loop

.option_vga_demo:
    call vga_demo
    call display_menu
    jmp .main_loop

.option_memory_test:
    call memory_test
    call display_menu
    jmp .main_loop

.option_system_info:
    call system_info
    call display_menu
    jmp .main_loop

.quit:
    ; Display shutdown message
    mov si, msg_shutdown
    call print_string_color
    
    ; Halt system
    cli
    hlt

; Clear screen using BIOS
clear_screen:
    push ax
    push bx
    push cx
    push dx
    
    mov ah, 0x06            ; Scroll up function
    mov al, 0               ; Clear entire screen
    mov bh, 0x07            ; White on black
    xor cx, cx              ; Upper left (0,0)
    mov dx, 0x184F          ; Lower right (24,79)
    int 0x10
    
    ; Reset cursor to top-left
    mov ah, 0x02            ; Set cursor position
    xor bh, bh              ; Page 0
    xor dx, dx              ; Row 0, Column 0
    int 0x10
    
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; Print colored string at DS:SI
; Color format: byte before string contains color (high nibble = bg, low = fg)
print_string_color:
    push ax
    push bx
    push si
    
    ; Load color attribute
    lodsb
    mov bl, al              ; Store color in BL
    
.next_char:
    lodsb                   ; Load character
    test al, al             ; Check null terminator
    jz .done
    
    mov ah, 0x0E            ; Teletype output
    mov bh, 0               ; Page 0
    int 0x10
    
    jmp .next_char
    
.done:
    pop si
    pop bx
    pop ax
    ret

; Display memory information
display_memory_info:
    push ax
    push bx
    
    mov si, msg_memory
    call print_string_color
    
    ; Get low memory (KB) via BIOS
    int 0x12                ; Returns KB in AX
    mov bx, ax
    
    ; Display as decimal
    call print_decimal
    mov si, msg_kb
    call print_string_color
    
    pop bx
    pop ax
    ret

; Display main menu
display_menu:
    push si
    
    mov si, msg_menu_title
    call print_string_color
    
    mov si, msg_menu_1
    call print_string_color
    
    mov si, msg_menu_2
    call print_string_color
    
    mov si, msg_menu_3
    call print_string_color
    
    mov si, msg_menu_quit
    call print_string_color
    
    mov si, msg_prompt
    call print_string_color
    
    pop si
    ret

; Wait for keystroke and return in AL
get_keystroke:
    push bx
    
    mov ah, 0x00            ; Wait for keystroke
    int 0x16                ; Keyboard interrupt
    ; AL contains ASCII code
    
    pop bx
    ret

; VGA Demo - Display color palette
vga_demo:
    call clear_screen
    
    mov si, msg_vga_title
    call print_string_color
    
    ; Display 16 colors
    mov cx, 16
    mov bl, 0               ; Start with color 0
    
.color_loop:
    push cx
    
    ; Set color attribute (background = 0, foreground = BL)
    mov ah, 0x0E
    mov al, 219             ; Block character █
    mov bh, 0
    
    ; Print 4 blocks of this color
    push bx
    mov cx, 4
.block_loop:
    int 0x10
    loop .block_loop
    pop bx
    
    ; Print space
    mov al, ' '
    int 0x10
    
    inc bl
    pop cx
    loop .color_loop
    
    ; New line
    mov al, 13
    int 0x10
    mov al, 10
    int 0x10
    
    ; Wait for keypress
    mov si, msg_press_key
    call print_string_color
    call get_keystroke
    
    call clear_screen
    ret

; Memory test - Simple pattern test
memory_test:
    call clear_screen
    
    mov si, msg_mem_test
    call print_string_color
    
    ; Test 1KB at 0x9000
    mov ax, 0x9000
    mov es, ax
    xor di, di
    
    ; Write pattern
    mov cx, 512             ; 512 words = 1KB
    mov ax, 0xAA55
.write_loop:
    stosw
    loop .write_loop
    
    ; Read back and verify
    mov ax, 0x9000
    mov es, ax
    xor di, di
    mov cx, 512
    mov dx, 0xAA55
    
.verify_loop:
    mov ax, [es:di]
    cmp ax, dx
    jne .test_failed
    add di, 2
    loop .verify_loop
    
    ; Success
    mov si, msg_mem_pass
    call print_string_color
    jmp .mem_test_done
    
.test_failed:
    mov si, msg_mem_fail
    call print_string_color
    
.mem_test_done:
    mov si, msg_press_key
    call print_string_color
    call get_keystroke
    
    call clear_screen
    ret

; System information display
system_info:
    call clear_screen
    
    mov si, msg_sys_info
    call print_string_color
    
    ; Display Stage 2 address
    mov si, msg_stage2_addr
    call print_string_color
    mov ax, 0x8000
    call print_hex
    
    ; New line
    mov ah, 0x0E
    mov al, 13
    int 0x10
    mov al, 10
    int 0x10
    
    ; Display stack pointer
    mov si, msg_stack_ptr
    call print_string_color
    mov ax, sp
    call print_hex
    
    ; New line
    mov ah, 0x0E
    mov al, 13
    int 0x10
    mov al, 10
    int 0x10
    
    ; Wait for keypress
    mov si, msg_press_key
    call print_string_color
    call get_keystroke
    
    call clear_screen
    ret

; Print decimal number in BX
print_decimal:
    push ax
    push bx
    push cx
    push dx
    
    mov ax, bx
    mov cx, 0               ; Digit counter
    mov bx, 10
    
.divide_loop:
    xor dx, dx
    div bx                  ; AX = AX / 10, DX = remainder
    push dx                 ; Save digit
    inc cx
    test ax, ax
    jnz .divide_loop
    
.print_loop:
    pop dx
    add dl, '0'
    mov ah, 0x0E
    mov al, dl
    mov bh, 0
    int 0x10
    loop .print_loop
    
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; Print hexadecimal number in AX
print_hex:
    push ax
    push bx
    push cx
    push dx
    
    mov cx, 4               ; 4 hex digits
    
.digit_loop:
    rol ax, 4               ; Rotate left 4 bits
    mov dl, al
    and dl, 0x0F            ; Mask low nibble
    
    ; Convert to ASCII
    cmp dl, 9
    jle .is_digit
    add dl, 7               ; A-F
.is_digit:
    add dl, '0'
    
    ; Print character
    push ax
    mov ah, 0x0E
    mov al, dl
    mov bh, 0
    int 0x10
    pop ax
    
    loop .digit_loop
    
    pop dx
    pop cx
    pop bx
    pop ax
    ret

; Data section - Messages with embedded color codes
; Color byte: high nibble = background, low nibble = foreground
; 0x0F = black background, white foreground

msg_banner:     db 0x0E, '=== Stage 2 Bootloader ===', 13, 10, 0
msg_memory:     db 0x0B, 'System Memory: ', 0
msg_kb:         db 0x0B, ' KB', 13, 10, 10, 0

msg_menu_title: db 0x0F, 'Main Menu:', 13, 10, 0
msg_menu_1:     db 0x07, '  1. VGA Color Demo', 13, 10, 0
msg_menu_2:     db 0x07, '  2. Memory Test', 13, 10, 0
msg_menu_3:     db 0x07, '  3. System Information', 13, 10, 0
msg_menu_quit:  db 0x07, '  Q. Quit (Halt)', 13, 10, 10, 0
msg_prompt:     db 0x0E, 'Select option: ', 0

msg_shutdown:   db 0x0C, 13, 10, 'System halted. Goodbye!', 13, 10, 0

msg_vga_title:  db 0x0F, 'VGA 16-Color Palette:', 13, 10, 0
msg_press_key:  db 0x07, 13, 10, 'Press any key to continue...', 0

msg_mem_test:   db 0x0F, 'Memory Test (1KB at 0x9000):', 13, 10, 0
msg_mem_pass:   db 0x0A, 'PASSED: Memory test successful!', 13, 10, 0
msg_mem_fail:   db 0x0C, 'FAILED: Memory test error!', 13, 10, 0

msg_sys_info:   db 0x0F, 'System Information:', 13, 10, 10, 0
msg_stage2_addr: db 0x07, '  Stage 2 Address: 0x', 0
msg_stack_ptr:  db 0x07, '  Stack Pointer: 0x', 0

; Pad to make Stage 2 exactly 1KB for testing
times 1024-($-$$) db 0
