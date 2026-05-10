; systest.asm - NeoDOS v0.9 user-mode syscall test
; Flat binary, loaded at 0x400000
; nasm -f bin -o systest.bin systest.asm

BITS 64
ORG 0x400000

section .text

start:
    ; sys_write(MSG1)
    mov eax, 1
    mov rbx, msg1
    mov ecx, msg1_len
    int 0x80

    ; sys_write(MSG2)
    mov eax, 1
    mov rbx, msg2
    mov ecx, msg2_len
    int 0x80

    ; sys_getpid
    mov eax, 3
    int 0x80

    ; sys_write(MSG3)
    mov eax, 1
    mov rbx, msg3
    mov ecx, msg3_len
    int 0x80

    ; sys_write(MSG4)
    mov eax, 1
    mov rbx, msg4
    mov ecx, msg4_len
    int 0x80

    ; sys_yield x3
    mov ecx, 3
yield_loop:
    mov eax, 2
    int 0x80
    loop yield_loop

    ; sys_write(MSG5)
    mov eax, 1
    mov rbx, msg5
    mov ecx, msg5_len
    int 0x80

    ; sys_write(MSG6)
    mov eax, 1
    mov rbx, msg6
    mov ecx, msg6_len
    int 0x80

    ; sys_open(filename, 0)
    mov eax, 10
    mov rbx, filename
    mov ecx, 0
    int 0x80

    ; Check if failed (rax == 0xFFFFFFFF)
    cmp eax, 0xFFFFFFFF
    je open_failed

    ; sys_readfile(inode, buf, 256)
    ; rax = inode from sys_open
    push rax          ; save inode
    mov eax, 11
    pop rbx           ; rbx = inode
    mov rcx, file_buf
    mov edx, 256
    int 0x80

    ; sys_write(MSG7)
    mov eax, 1
    mov rbx, msg7
    mov ecx, msg7_len
    int 0x80

    ; sys_write(MSG8)
    mov eax, 1
    mov rbx, msg8
    mov ecx, msg8_len
    int 0x80

    jmp done

open_failed:
    ; sys_write(MSG9)
    mov eax, 1
    mov rbx, msg9
    mov ecx, msg9_len
    int 0x80

done:
    ; sys_write(MSG10)
    mov eax, 1
    mov rbx, msg10
    mov ecx, msg10_len
    int 0x80

    ; sys_exit(0)
    xor ebx, ebx
    mov eax, 0
    int 0x80

    hlt

section .data

msg1:     db "=== NeoDOS v0.9 Syscall Test ===", 13, 10
msg1_len  equ $ - msg1

msg2:     db "Testing sys_getpid... "
msg2_len  equ $ - msg2

msg3:     db "OK", 13, 10
msg3_len  equ $ - msg3

msg4:     db "Testing sys_yield (3x)... "
msg4_len  equ $ - msg4

msg5:     db "OK", 13, 10
msg5_len  equ $ - msg5

msg6:     db "Testing file I/O (sys_open, sys_readfile)... "
msg6_len  equ $ - msg6

msg7:     db "File content: "
msg7_len  equ $ - msg7

msg8:     db "OK", 13, 10
msg8_len  equ $ - msg8

msg9:     db "FAIL", 13, 10
msg9_len  equ $ - msg9

msg10:    db "All tests passed. Calling sys_exit...", 13, 10
msg10_len equ $ - msg10

filename: db "readme.txt", 0

section .bss
file_buf: resb 256
