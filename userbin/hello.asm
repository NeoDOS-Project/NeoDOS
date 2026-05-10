; hello.asm — NeoDOS v0.7 user-mode test binary
;
; Flat binary, loaded at physical address 0x400000 (USER_BASE).
; All code and data must be position-independent relative to that load address,
; OR use absolute addresses within the 0x400000..0x800000 window.
;
; This binary is assembled with:
;   nasm -f bin -o hello.bin hello.asm
;
; Syscall ABI (INT 0x80):
;   RAX = syscall number
;   RBX = arg0    (sys_write: pointer to buffer)
;   RCX = arg1    (sys_write: length)
;   RAX ← return value
;
;   0  sys_exit(code)
;   1  sys_write(ptr, len)
;   2  sys_yield
;   3  sys_getpid → RAX

BITS 64
ORG 0x400000        ; Tell NASM our load address so absolute refs work

_start:
    ; ── print greeting ──────────────────────────────────────────────────────
    mov     rax, 1              ; sys_write
    mov     rbx, msg_hello      ; ptr
    mov     rcx, msg_hello_len  ; len
    int     0x80

    ; ── ask for our PID ─────────────────────────────────────────────────────
    mov     rax, 3              ; sys_getpid
    int     0x80
    ; RAX now holds our PID — store it for later (not printed in this version)
    mov     [pid_buf], rax

    ; ── print PID line ──────────────────────────────────────────────────────
    mov     rax, 1
    mov     rbx, msg_pid
    mov     rcx, msg_pid_len
    int     0x80

    ; ── loop 3 times yielding ───────────────────────────────────────────────
    mov     rcx, 3
.yield_loop:
    push    rcx
    mov     rax, 2              ; sys_yield
    int     0x80
    pop     rcx
    loop    .yield_loop

    ; ── goodbye ─────────────────────────────────────────────────────────────
    mov     rax, 1
    mov     rbx, msg_bye
    mov     rcx, msg_bye_len
    int     0x80

    ; ── sys_exit(0) ─────────────────────────────────────────────────────────
    mov     rax, 0
    xor     rbx, rbx
    int     0x80

    ; Should never reach here — halt just in case.
    hlt

; ── Data ─────────────────────────────────────────────────────────────────────
msg_hello:      db  "Hello from Ring 3! (NeoDOS v0.7)", 0x0D, 0x0A
msg_hello_len   equ $ - msg_hello

msg_pid:        db  "sys_getpid returned successfully.", 0x0D, 0x0A
msg_pid_len     equ $ - msg_pid

msg_bye:        db  "Goodbye from user space! Calling sys_exit...", 0x0D, 0x0A
msg_bye_len     equ $ - msg_bye

pid_buf:        dq  0           ; storage for the returned PID
