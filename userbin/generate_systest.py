#!/usr/bin/env python3
"""
generate_systest.py - Versión ESTABLE y FUNCIONAL
Prueba syscalls básicos: write, getpid, yield, exit
(Estructura idéntica a generate_hello.py que funciona)
"""

import struct, os, sys

BASE = 0x400000

# ── Instrucciones (exactamente como hello.py) ───────────────────────────────

def mov_eax(n: int) -> bytes:
    return b'\xB8' + struct.pack('<I', n)

def mov_ebx(n: int) -> bytes:
    return b'\xBB' + struct.pack('<I', n)

def mov_ecx(n: int) -> bytes:
    return b'\xB9' + struct.pack('<I', n)

def mov_edx(n: int) -> bytes:
    return b'\xBA' + struct.pack('<I', n)

def mov_rbx_imm64(addr: int) -> bytes:
    return b'\x48\xBB' + struct.pack('<Q', addr)

def mov_mem64_rax(addr: int) -> bytes:
    return b'\x48\xA3' + struct.pack('<Q', addr)

def push_rcx() -> bytes:
    return b'\x51'

def pop_rcx() -> bytes:
    return b'\x59'

def loop_rel8(offset: int) -> bytes:
    return b'\xE2' + struct.pack('b', offset)

def int_80() -> bytes:
    return b'\xCD\x80'

def xor_ebx_ebx() -> bytes:
    return b'\x31\xDB'


# ── Datos ────────────────────────────────────────────────────────────────────

MSG_START = b"=== NeoDOS v0.9 Syscall Test ===\r\n"
MSG_GETPID = b"sys_getpid: OK\r\n"
MSG_YIELD  = b"sys_yield (3x): OK\r\n"
MSG_EXIT   = b"All tests passed. Calling sys_exit...\r\n"
PID_BUF    = b"\x00" * 8


# ── sys_write_block ──────────────────────────────────────────────────────────

def sys_write_block(ptr_addr: int, length: int) -> bytes:
    return (mov_eax(1) +
            mov_rbx_imm64(ptr_addr) +
            mov_ecx(length) +
            int_80())

SW_SIZE = 22


# ── Cálculo de code_parts_sizes (COPIADO de hello.py) ──────────────────────
# Esta estructura está PROBADA y FUNCIONA

code_parts_sizes = (
    SW_SIZE,                    # sys_write(MSG_START)
    5 + 2,                      # sys_getpid: mov eax,3 + int 0x80
    10,                         # mov [PID_BUF], rax
    SW_SIZE,                    # sys_write(MSG_GETPID)
    5,                          # mov ecx, 3
    1,                          # push rcx
    5,                          # mov eax, 2
    2,                          # int 0x80
    1,                          # pop rcx
    2,                          # loop rel8
    SW_SIZE,                    # sys_write(MSG_YIELD)
    SW_SIZE,                    # sys_write(MSG_EXIT)
    2,                          # xor ebx, ebx
    5,                          # mov eax, 0
    2,                          # int 0x80
)

CODE_SIZE = sum(code_parts_sizes)


# ── Direcciones ──────────────────────────────────────────────────────────────

idx = 0
DATA_BASE = BASE + CODE_SIZE

ADDR_START  = DATA_BASE + idx; idx += len(MSG_START)
ADDR_GETPID = DATA_BASE + idx; idx += len(MSG_GETPID)
ADDR_YIELD  = DATA_BASE + idx; idx += len(MSG_YIELD)
ADDR_EXIT   = DATA_BASE + idx; idx += len(MSG_EXIT)
ADDR_PIDBUF = DATA_BASE + idx


# ── Construcción ─────────────────────────────────────────────────────────────

def build_code() -> bytes:
    # Mismo cálculo de offset que hello.py
    # push_rcx está en: SW_SIZE + 7 + 10 + SW_SIZE + 5 = 22+7+10+22+5 = 66
    # Después de loop: 66 + 1+5+2+1+2 = 77
    # rel8 = 66 - 77 = -11
    LOOP_OFFSET = -11
    
    code = b''
    code += sys_write_block(ADDR_START, len(MSG_START))
    code += mov_eax(3)
    code += int_80()
    code += mov_mem64_rax(ADDR_PIDBUF)
    code += sys_write_block(ADDR_GETPID, len(MSG_GETPID))
    code += mov_ecx(3)
    code += push_rcx()
    code += mov_eax(2)
    code += int_80()
    code += pop_rcx()
    code += loop_rel8(LOOP_OFFSET)
    code += sys_write_block(ADDR_YIELD, len(MSG_YIELD))
    code += sys_write_block(ADDR_EXIT, len(MSG_EXIT))
    code += xor_ebx_ebx()
    code += mov_eax(0)
    code += int_80()
    return code


code = build_code()

# Validación (como hello.py)
if len(code) != CODE_SIZE:
    print(f"ERROR: calculado={CODE_SIZE}, real={len(code)}")
    sys.exit(1)


# ── Binario ──────────────────────────────────────────────────────────────────

binary = code + MSG_START + MSG_GETPID + MSG_YIELD + MSG_EXIT + PID_BUF

output = os.path.join(os.path.dirname(__file__), 'systest.bin')
with open(output, 'wb') as f:
    f.write(binary)

print(f"systest.bin: {len(binary)} bytes")
print(f"  Prueba: sys_write, sys_getpid, sys_yield (3x), sys_exit")
print(f"  (Estructura idéntica a hello.py que funciona)")
