#!/usr/bin/env python3
"""
generate_hello_elf.py — Genera HELLO.ELF (NeoDOS v0.7 user-mode test binary)
en formato ELF64, cargado en 0x400000.

Syscall ABI (INT 0x80):
  RAX = número de syscall, RBX = arg0, RCX = arg1
  0  sys_exit(code)
  1  sys_write(ptr, len)
  2  sys_yield
  3  sys_getpid → RAX

Equivale funcionalmente a hello.bin, pero con cabecera ELF64.
"""

import struct, os, sys

BASE = 0x400000   # Dirección de carga

# ── Constructores de instrucciones x86-64 ──

def mov_eax(n: int) -> bytes:
    return b'\xB8' + struct.pack('<I', n)

def mov_rbx_imm64(addr: int) -> bytes:
    return b'\x48\xBB' + struct.pack('<Q', addr)

def mov_ecx(n: int) -> bytes:
    return b'\xB9' + struct.pack('<I', n)

def int_80() -> bytes:
    return b'\xCD\x80'

def mov_mem64_rax(addr: int) -> bytes:
    return b'\x48\xA3' + struct.pack('<Q', addr)

def push_rcx() -> bytes:
    return b'\x51'

def pop_rcx() -> bytes:
    return b'\x59'

def loop_rel8(offset: int) -> bytes:
    return b'\xE2' + struct.pack('b', offset)

def xor_ebx_ebx() -> bytes:
    return b'\x31\xDB'

def hlt() -> bytes:
    return b'\xF4'


# ── Datos ──

MSG_HELLO = b"Hello from Ring 3! (NeoDOS v0.7)\r\n"  # 34 bytes
MSG_PID   = b"sys_getpid returned successfully.\r\n"   # 35 bytes
MSG_BYE   = b"Goodbye from user space! Calling sys_exit...\r\n"  # 46 bytes


# ── Código (misma lógica que hello.bin) ──

def sys_write_block(ptr_addr: int, length: int) -> bytes:
    return (mov_eax(1) +
            mov_rbx_imm64(ptr_addr) +
            mov_ecx(length) +
            int_80())

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

ADDR_MSG_HELLO = BASE + CODE_SIZE                        # 0x40006D
ADDR_MSG_PID   = ADDR_MSG_HELLO + len(MSG_HELLO)        # 0x40008F
ADDR_MSG_BYE   = ADDR_MSG_PID   + len(MSG_PID)          # 0x4000B2
ADDR_PID_BUF   = ADDR_MSG_BYE   + len(MSG_BYE)          # 0x4000E0

def _build_code() -> bytes:
    LOOP_OFFSET = -11
    code = b''
    code += sys_write_block(ADDR_MSG_HELLO, len(MSG_HELLO))
    code += mov_eax(3)
    code += int_80()
    code += mov_mem64_rax(ADDR_PID_BUF)
    code += sys_write_block(ADDR_MSG_PID, len(MSG_PID))
    code += mov_ecx(3)
    code += push_rcx()
    code += mov_eax(2)
    code += int_80()
    code += pop_rcx()
    code += loop_rel8(LOOP_OFFSET)
    code += sys_write_block(ADDR_MSG_BYE, len(MSG_BYE))
    code += xor_ebx_ebx()
    code += mov_eax(0)
    code += int_80()
    code += hlt()
    return code

code = _build_code()
assert len(code) == CODE_SIZE, f"code size mismatch: {len(code)} != {CODE_SIZE}"

# ── Datos (appended al código) ──

data = MSG_HELLO + MSG_PID + MSG_BYE + struct.pack('<Q', 0)
CODE_AND_DATA = code + data

# ── Construir ELF64 ──
# Layout:
#   [0..64)     ELF header
#   [64..120)   Program header (1× PT_LOAD)
#   [120..)     Code + data

FILE_OFFSET = 120  # code starts at file offset 120
TOTAL_SIZE = len(CODE_AND_DATA)

def build_elf_header() -> bytes:
    ei_class = 2        # ELFCLASS64
    ei_data = 1         # ELFDATA2LSB
    ei_version = 1
    e_type = 2          # ET_EXEC
    e_machine = 62      # EM_X86_64
    e_version = 1
    e_entry = BASE
    e_phoff = 64
    e_shoff = 0
    e_flags = 0
    e_ehsize = 64
    e_phentsize = 56
    e_phnum = 1
    e_shentsize = 0
    e_shnum = 0
    e_shstrndx = 0

    hdr = b''
    hdr += b'\x7fELF'
    hdr += struct.pack('BBBB', ei_class, ei_data, ei_version, 0)  # ident[4..8]
    hdr += b'\x00' * 8  # padding (ident[8..16])
    hdr += struct.pack('<HHI', e_type, e_machine, e_version)
    hdr += struct.pack('<Q', e_entry)
    hdr += struct.pack('<Q', e_phoff)
    hdr += struct.pack('<Q', e_shoff)
    hdr += struct.pack('<I', e_flags)
    hdr += struct.pack('<HHHHHH', e_ehsize, e_phentsize, e_phnum, e_shentsize, e_shnum, e_shstrndx)
    return hdr

def build_program_header() -> bytes:
    p_type = 1          # PT_LOAD
    p_flags = 7         # PF_R | PF_W | PF_X
    p_offset = FILE_OFFSET
    p_vaddr = BASE
    p_paddr = BASE
    p_filesz = TOTAL_SIZE
    p_memsz = TOTAL_SIZE
    p_align = 1

    ph = b''
    ph += struct.pack('<II', p_type, p_flags)
    ph += struct.pack('<Q', p_offset)
    ph += struct.pack('<Q', p_vaddr)
    ph += struct.pack('<Q', p_paddr)
    ph += struct.pack('<Q', p_filesz)
    ph += struct.pack('<Q', p_memsz)
    ph += struct.pack('<Q', p_align)
    return ph

elf_header = build_elf_header()
assert len(elf_header) == 64, f"ELF header size: {len(elf_header)}"

prog_header = build_program_header()
assert len(prog_header) == 56, f"Program header size: {len(prog_header)}"

binary = elf_header + prog_header + CODE_AND_DATA

# ── Verificación ──
# ELF magic
assert binary[:4] == b'\x7fELF', "Bad magic"
# Entry point at BASE
entry = struct.unpack('<Q', binary[24:32])[0]
assert entry == BASE, f"Entry point {entry:#x} != {BASE:#x}"
# PT_LOAD at correct offset
phdr_type = struct.unpack('<I', binary[64:68])[0]
assert phdr_type == 1, f"phdr type {phdr_type} != PT_LOAD"
phdr_vaddr = struct.unpack('<Q', binary[80:88])[0]
assert phdr_vaddr == BASE, f"phdr vaddr {phdr_vaddr:#x} != {BASE:#x}"

# ── Escribir ──

output = os.path.join(os.path.dirname(__file__), 'hello.elf')
with open(output, 'wb') as f:
    f.write(binary)

print(f"[✓] hello.elf generado: {len(binary)} bytes")
print(f"    Entry point: 0x{BASE:08X}")
print(f"    Total size:  {len(binary)} bytes")
