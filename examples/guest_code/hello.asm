; ==============================================================================
; hello.asm - Simple "Hello, World!" Guest Program for AetherVM
; ==============================================================================
;
; Description:
;   This is the simplest possible guest program for AetherVM. It writes
;   "Hello, World!" to the serial port (COM1 at 0x3F8) and then halts.
;
; Build:
;   nasm -f bin hello.asm -o hello.bin
;
; Run in AetherVM:
;   (Load hello.bin as guest memory and start VM)
;
; Expected Output:
;   Serial port will output: "Hello, World!"
;
; ==============================================================================

[BITS 16]                   ; 16-bit real mode
[ORG 0x7C00]                ; Boot sector loads at 0x7C00

start:
    ; ====== Initialize Segments ======
    cli                     ; Disable interrupts during setup
    xor ax, ax              ; AX = 0
    mov ds, ax              ; Data segment = 0
    mov es, ax              ; Extra segment = 0
    mov ss, ax              ; Stack segment = 0
    mov sp, 0x7C00          ; Stack pointer (grows downward from boot sector)
    sti                     ; Re-enable interrupts

    ; ====== Print Message ======
    mov si, message         ; SI points to message string
    call serial_write_string

    ; ====== Halt ======
    cli                     ; Disable interrupts
    hlt                     ; Halt the CPU

; ==============================================================================
; serial_write_string
;
; Writes a null-terminated string to the serial port (COM1)
;
; Input:  SI = pointer to null-terminated string
; Output: None
; Clobbers: AL, DX, SI
; ==============================================================================
serial_write_string:
.loop:
    lodsb                   ; Load byte from [SI] into AL, increment SI
    test al, al             ; Check if AL == 0 (null terminator)
    jz .done                ; If zero, we're done
    
    ; Write character to serial port
    mov dx, 0x3F8           ; COM1 data port
    out dx, al              ; Output character
    
    jmp .loop               ; Continue with next character
.done:
    ret

; ==============================================================================
; Data Section
; ==============================================================================
message:
    db 'Hello, World!', 13, 10, 0   ; Message with CR, LF, and null terminator

; ==============================================================================
; Boot Sector Signature
;
; The last 2 bytes of a boot sector must be 0x55 0xAA to be recognized
; as bootable by the BIOS.
; ==============================================================================
times 510-($-$$) db 0       ; Fill remainder with zeros
dw 0xAA55                   ; Boot sector signature
