#!/usr/bin/env python3
"""
generate_hello.py — Genera hello.bin (NeoDOS v0.7 user-mode test binary)
sin necesitar nasm. El binario es un flat binary de 64 bits cargado en 0x400000.

Syscall ABI (INT 0x80):
  RAX = número de syscall, RBX = arg0, RCX = arg1
  0  sys_exit(code)
  1  sys_write(ptr, len)
  2  sys_yield
  3  sys_getpid → RAX

Equivale al hello.asm de este directorio.
"""

import struct, os, sys

BASE = 0x400000   # Dirección de carga del binario


# ── Constructores de instrucciones x86-64 ─────────────────────────────────────

def mov_eax(n: int) -> bytes:
    """mov eax, imm32  (5 bytes; zero-extends a RAX)"""
    return b'\xB8' + struct.pack('<I', n)

def mov_rbx_imm64(addr: int) -> bytes:
    """mov rbx, imm64  (10 bytes)"""
    return b'\x48\xBB' + struct.pack('<Q', addr)

def mov_ecx(n: int) -> bytes:
    """mov ecx, imm32  (5 bytes)"""
    return b'\xB9' + struct.pack('<I', n)

def int_80() -> bytes:
    return b'\xCD\x80'

def mov_mem64_rax(addr: int) -> bytes:
    """MOV [addr64], RAX  — dirección absoluta de 64 bits (10 bytes)
       Codificación: REX.W (48) + A3 + addr64
    """
    return b'\x48\xA3' + struct.pack('<Q', addr)

def push_rcx() -> bytes:
    return b'\x51'

def pop_rcx() -> bytes:
    return b'\x59'

def loop_rel8(offset: int) -> bytes:
    """LOOP rel8  (E2 + signed byte offset from next instruction)"""
    return b'\xE2' + struct.pack('b', offset)

def xor_ebx_ebx() -> bytes:
    return b'\x31\xDB'

def hlt() -> bytes:
    return b'\xF4'


# ── Datos ────────────────────────────────────────────────────────────────────

MSG_HELLO = b"Hello from Ring 3! (NeoDOS v0.7)\r\n"  # 34 bytes
MSG_PID   = b"sys_getpid returned successfully.\r\n"   # 35 bytes
MSG_BYE   = b"Goodbye from user space! Calling sys_exit...\r\n"  # 46 bytes


# ── Cálculo de offsets ────────────────────────────────────────────────────────
# Calculamos el tamaño del bloque de código para poder saber dónde
# caerán las strings en memoria.

def sys_write_block(ptr_addr: int, length: int) -> bytes:
    return (mov_eax(1) +        # 5
            mov_rbx_imm64(ptr_addr) +  # 10
            mov_ecx(length) +   # 5
            int_80())           # 2   → 22 bytes total

# Código parcial para medir su tamaño (las direcciones exactas las ponemos después)
_DUMMY = 0xDEADBEEF_DEADBEEF

code_parts_sizes = (
    22,   # sys_write(hello)
    7,    # sys_getpid: mov eax,3 + int 0x80
    10,   # mov [pid_buf], rax
    22,   # sys_write(pid_msg)
    5,    # mov ecx, 3
    1,    # push rcx
    5,    # mov eax, 2
    2,    # int 0x80
    1,    # pop rcx
    2,    # loop rel8
    22,   # sys_write(bye)
    2,    # xor ebx, ebx
    5,    # mov eax, 0
    2,    # int 0x80
    1,    # hlt
)

CODE_SIZE = sum(code_parts_sizes)  # 109 bytes

# Direcciones absolutas de los datos:
ADDR_MSG_HELLO = BASE + CODE_SIZE                        # 0x40006D
ADDR_MSG_PID   = ADDR_MSG_HELLO + len(MSG_HELLO)        # 0x40008F
ADDR_MSG_BYE   = ADDR_MSG_PID   + len(MSG_PID)          # 0x4000B2
ADDR_PID_BUF   = ADDR_MSG_BYE   + len(MSG_BYE)          # 0x4000E0

# ── Verificación del tamaño del código ───────────────────────────────────────
# (aborta si la lógica de arriba no cuadra con las instrucciones reales)

def _build_code() -> bytes:
    """Construye el bloque de código con las direcciones correctas."""

    # Offset del loop: push_rcx está en la posición:
    # 22 + 7 + 10 + 22 + 5 = 66 desde el inicio del código.
    # La instrucción loop está en:
    # 22 + 7 + 10 + 22 + 5 + 1 + 5 + 2 + 1 = 75, y ocupa 2 bytes → siguiente en 77.
    # rel8 = 66 - 77 = -11 = 0xF5
    LOOP_OFFSET = -11

    code = b''
    code += sys_write_block(ADDR_MSG_HELLO, len(MSG_HELLO))  # 22
    code += mov_eax(3)                                        #  5
    code += int_80()                                          #  2  → sys_getpid (7 total)
    code += mov_mem64_rax(ADDR_PID_BUF)                      # 10
    code += sys_write_block(ADDR_MSG_PID, len(MSG_PID))      # 22
    code += mov_ecx(3)                                        #  5
    code += push_rcx()                                        #  1
    code += mov_eax(2)                                        #  5
    code += int_80()                                          #  2  → sys_yield
    code += pop_rcx()                                         #  1
    code += loop_rel8(LOOP_OFFSET)                            #  2
    code += sys_write_block(ADDR_MSG_BYE, len(MSG_BYE))      # 22
    code += xor_ebx_ebx()                                     #  2
    code += mov_eax(0)                                        #  5
    code += int_80()                                          #  2  → sys_exit
    code += hlt()                                             #  1
    return code


code = _build_code()

if len(code) != CODE_SIZE:
    print(f"ERROR: tamaño de código calculado={CODE_SIZE} real={len(code)}")
    sys.exit(1)

# ── Ensamblar el binario ──────────────────────────────────────────────────────

binary = code + MSG_HELLO + MSG_PID + MSG_BYE + struct.pack('<Q', 0)  # pid_buf

output = os.path.join(os.path.dirname(__file__), 'hello.bin')
with open(output, 'wb') as f:
    f.write(binary)

print(f"[✓] hello.bin generado: {len(binary)} bytes")
print(f"    Carga en:   0x{BASE:08X}")
print(f"    msg_hello:  0x{ADDR_MSG_HELLO:08X}")
print(f"    msg_pid:    0x{ADDR_MSG_PID:08X}")
print(f"    msg_bye:    0x{ADDR_MSG_BYE:08X}")
print(f"    pid_buf:    0x{ADDR_PID_BUF:08X}")
print(f"    Total:      {len(binary)} bytes")
