#!/usr/bin/env python3
"""
generate_alltest.py — Comprehensive syscall test for NeoDOS
Tests: getpid, yield, open, readfile, close, chdir, getcwd, brk, exit
"""

import struct, os, sys

BASE = 0x400000

def mov_eax(n):      return b'\xB8' + struct.pack('<I', n)
def mov_ebx(n):      return b'\xBB' + struct.pack('<I', n)
def mov_ecx(n):      return b'\xB9' + struct.pack('<I', n)
def mov_edx(n):      return b'\xBA' + struct.pack('<I', n)
def mov_rbx_imm64(a): return b'\x48\xBB' + struct.pack('<Q', a)
def mov_rcx_imm64(a): return b'\x48\xB9' + struct.pack('<Q', a)
def int80():         return b'\xCD\x80'
def xor_ebx_ebx():   return b'\x31\xDB'
def test_rax_rax():  return b'\x48\x85\xC0'
def cmp_rax_neg1():  return b'\x48\x3D\xFF\xFF\xFF\xFF'
def jz_rel8(o):      return b'\x74' + struct.pack('b', o)
def jmp_rel8(o):     return b'\xEB' + struct.pack('b', o)
def mov_mem64_rax(a): return b'\x48\xA3' + struct.pack('<Q', a)
def mov_rax_mem64(a): return b'\x48\xA1' + struct.pack('<Q', a)
def mov_rbx_rax():   return b'\x48\x89\xC3'
def add_rbx_imm32(n): return b'\x48\x81\xC3' + struct.pack('<I', n)
def sub_rbx_1():     return b'\x48\x83\xEB\x01'
def mov_byte_rbx(bv): return b'\xC6\x03' + bytes([bv])
def push_rcx(): return b'\x51'
def pop_rcx():  return b'\x59'
def loop_rel8(o): return b'\xE2' + struct.pack('b', o)

def sys_write_block(addr, length):
    return mov_eax(1) + mov_rbx_imm64(addr) + mov_ecx(length) + int80()

SW = 22  # sys_write block size

# ── Strings and buffers ────────────────────────────────
STR = {}
STR['start']     = b"=== ALL Syscall Test ===\r\n"
STR['yield']     = b"sys_yield: OK\r\n"
STR['pid_ok']    = b"sys_getpid: OK\r\n"
STR['pid_fail']  = b"sys_getpid: FAIL\r\n"
STR['open_ok']   = b"sys_open: OK\r\n"
STR['open_fail'] = b"sys_open: FAIL\r\n"
STR['read_ok']   = b"sys_readfile: OK\r\n"
STR['read_fail'] = b"sys_readfile: FAIL\r\n"
STR['close_msg'] = b"sys_close: OK\r\n"
STR['chdir_ok']  = b"sys_chdir: OK\r\n"
STR['chdir_fail']= b"sys_chdir: FAIL\r\n"
STR['cwd_ok']    = b"sys_getcwd: OK\r\n"
STR['cwd_fail']  = b"sys_getcwd: FAIL\r\n"
STR['brk_ok']    = b"sys_brk: OK\r\n"
STR['brk_fail']  = b"sys_brk: FAIL\r\n"
STR['done']      = b"ALL_TESTS_PASSED\r\n"
STR['path']      = b"C:\\HELLO.BIN\0"
STR['cwdpath']   = b"C:\\\0"

STR_ORDER = ['start', 'yield', 'pid_ok', 'pid_fail', 'open_ok', 'open_fail',
             'read_ok', 'read_fail', 'close_msg', 'chdir_ok', 'chdir_fail',
             'cwd_ok', 'cwd_fail', 'brk_ok', 'brk_fail', 'done', 'path', 'cwdpath']

# ── First pass: emit code with dummy data addrs to measure size ──
DUMMY = 0x400000  # any valid-looking address within user range

code = bytearray()

def emit(data):
    code.extend(data)

def emit_je():
    pos = len(code)
    code.extend(b'\x74\x00')
    return pos

def emit_jmp():
    pos = len(code)
    code.extend(b'\xEB\x00')
    return pos

def patch_je(pos):
    target = len(code)
    code[pos + 1] = (target - (pos + 2)) & 0xFF

def patch_jmp(pos):
    target = len(code)
    code[pos + 1] = (target - (pos + 2)) & 0xFF

# -- start --
emit(sys_write_block(DUMMY, 10))

# -- yield --
emit(mov_ecx(3))
emit(push_rcx())
emit(mov_eax(2))
emit(int80())
emit(pop_rcx())
emit(loop_rel8(-11))
emit(sys_write_block(DUMMY, 10))

# -- getpid --
emit(mov_eax(3))
emit(int80())
emit(test_rax_rax())
p1 = emit_je()
emit(sys_write_block(DUMMY, 10))
p1j = emit_jmp()
patch_je(p1)
emit(sys_write_block(DUMMY, 10))
emit(mov_ebx(1))
emit(mov_eax(0))
emit(int80())
patch_jmp(p1j)

# -- open --
emit(mov_eax(10))
emit(mov_rbx_imm64(DUMMY))
emit(mov_ecx(0))
emit(int80())
emit(cmp_rax_neg1())
p2 = emit_je()
emit(mov_mem64_rax(DUMMY))
emit(sys_write_block(DUMMY, 10))
p2j = emit_jmp()
patch_je(p2)
emit(sys_write_block(DUMMY, 10))
emit(mov_ebx(1))
emit(mov_eax(0))
emit(int80())
patch_jmp(p2j)

# -- readfile --
emit(mov_rax_mem64(DUMMY))
emit(mov_rbx_rax())
emit(mov_rcx_imm64(DUMMY))
emit(mov_edx(16))
emit(mov_eax(11))
emit(int80())
emit(cmp_rax_neg1())
p3 = emit_je()
emit(sys_write_block(DUMMY, 10))
p3j = emit_jmp()
patch_je(p3)
emit(sys_write_block(DUMMY, 10))
emit(mov_ebx(1))
emit(mov_eax(0))
emit(int80())
patch_jmp(p3j)

# -- close --
emit(mov_rax_mem64(DUMMY))
emit(mov_rbx_rax())
emit(mov_eax(13))
emit(int80())
emit(sys_write_block(DUMMY, 10))

# -- chdir --
emit(mov_eax(16))
emit(mov_rbx_imm64(DUMMY))
emit(int80())
emit(cmp_rax_neg1())
p5 = emit_je()
emit(sys_write_block(DUMMY, 10))
p5j = emit_jmp()
patch_je(p5)
emit(sys_write_block(DUMMY, 10))
emit(mov_ebx(1))
emit(mov_eax(0))
emit(int80())
patch_jmp(p5j)

# -- getcwd --
emit(mov_eax(17))
emit(mov_rbx_imm64(DUMMY))
emit(mov_ecx(64))
emit(int80())
emit(cmp_rax_neg1())
p6 = emit_je()
emit(sys_write_block(DUMMY, 10))
p6j = emit_jmp()
patch_je(p6)
emit(sys_write_block(DUMMY, 10))
emit(mov_ebx(1))
emit(mov_eax(0))
emit(int80())
patch_jmp(p6j)

# -- brk --
emit(mov_eax(18))
emit(xor_ebx_ebx())
emit(int80())
emit(cmp_rax_neg1())
p7a = emit_je()
emit(mov_rbx_rax())
emit(add_rbx_imm32(4096))
emit(mov_eax(18))
emit(int80())
emit(cmp_rax_neg1())
p7b = emit_je()
emit(sub_rbx_1())
emit(mov_byte_rbx(0x42))
emit(sys_write_block(DUMMY, 10))
p7j = emit_jmp()
patch_je(p7a)
patch_je(p7b)
emit(sys_write_block(DUMMY, 10))
emit(mov_ebx(1))
emit(mov_eax(0))
emit(int80())
patch_jmp(p7j)

# -- done --
emit(sys_write_block(DUMMY, 10))

# -- exit --
emit(xor_ebx_ebx())
emit(mov_eax(0))
emit(int80())

CODE_SIZE = len(code)

# ── Compute real data addresses ────────────────────────
idx = 0
DATA_BASE = BASE + CODE_SIZE
ADDR = {}
for key in STR_ORDER:
    ADDR[key] = DATA_BASE + idx
    idx += len(STR[key])
ADDR['inode'] = DATA_BASE + idx
idx += 8
ADDR['scratch'] = DATA_BASE + idx
idx += 128

# ── Second pass: rebuild code with real addresses ──────
code = bytearray()

def emit2(data):
    code.extend(data)

def emit2_je():
    pos = len(code)
    code.extend(b'\x74\x00')
    return pos

def emit2_jmp():
    pos = len(code)
    code.extend(b'\xEB\x00')
    return pos

def patch2_je(pos):
    target = len(code)
    code[pos + 1] = (target - (pos + 2)) & 0xFF

def patch2_jmp(pos):
    target = len(code)
    code[pos + 1] = (target - (pos + 2)) & 0xFF

# -- start --
emit2(sys_write_block(ADDR['start'], len(STR['start'])))

# -- yield --
emit2(mov_ecx(3))
emit2(push_rcx())
emit2(mov_eax(2))
emit2(int80())
emit2(pop_rcx())
emit2(loop_rel8(-11))
emit2(sys_write_block(ADDR['yield'], len(STR['yield'])))

# -- getpid --
emit2(mov_eax(3))
emit2(int80())
emit2(test_rax_rax())
p1 = emit2_je()
emit2(sys_write_block(ADDR['pid_ok'], len(STR['pid_ok'])))
p1j = emit2_jmp()
patch2_je(p1)
emit2(sys_write_block(ADDR['pid_fail'], len(STR['pid_fail'])))
emit2(mov_ebx(1))
emit2(mov_eax(0))
emit2(int80())
patch2_jmp(p1j)

# -- open --
emit2(mov_eax(10))
emit2(mov_rbx_imm64(ADDR['path']))
emit2(mov_ecx(0))
emit2(int80())
emit2(cmp_rax_neg1())
p2 = emit2_je()
emit2(mov_mem64_rax(ADDR['inode']))
emit2(sys_write_block(ADDR['open_ok'], len(STR['open_ok'])))
p2j = emit2_jmp()
patch2_je(p2)
emit2(sys_write_block(ADDR['open_fail'], len(STR['open_fail'])))
emit2(mov_ebx(1))
emit2(mov_eax(0))
emit2(int80())
patch2_jmp(p2j)

# -- readfile --
emit2(mov_rax_mem64(ADDR['inode']))
emit2(mov_rbx_rax())
emit2(mov_rcx_imm64(ADDR['scratch']))
emit2(mov_edx(16))
emit2(mov_eax(11))
emit2(int80())
emit2(cmp_rax_neg1())
p3 = emit2_je()
emit2(sys_write_block(ADDR['read_ok'], len(STR['read_ok'])))
p3j = emit2_jmp()
patch2_je(p3)
emit2(sys_write_block(ADDR['read_fail'], len(STR['read_fail'])))
emit2(mov_ebx(1))
emit2(mov_eax(0))
emit2(int80())
patch2_jmp(p3j)

# -- close --
emit2(mov_rax_mem64(ADDR['inode']))
emit2(mov_rbx_rax())
emit2(mov_eax(13))
emit2(int80())
emit2(sys_write_block(ADDR['close_msg'], len(STR['close_msg'])))

# -- chdir --
emit2(mov_eax(16))
emit2(mov_rbx_imm64(ADDR['cwdpath']))
emit2(int80())
emit2(cmp_rax_neg1())
p5 = emit2_je()
emit2(sys_write_block(ADDR['chdir_ok'], len(STR['chdir_ok'])))
p5j = emit2_jmp()
patch2_je(p5)
emit2(sys_write_block(ADDR['chdir_fail'], len(STR['chdir_fail'])))
emit2(mov_ebx(1))
emit2(mov_eax(0))
emit2(int80())
patch2_jmp(p5j)

# -- getcwd --
emit2(mov_eax(17))
emit2(mov_rbx_imm64(ADDR['scratch']))
emit2(mov_ecx(64))
emit2(int80())
emit2(cmp_rax_neg1())
p6 = emit2_je()
emit2(sys_write_block(ADDR['cwd_ok'], len(STR['cwd_ok'])))
p6j = emit2_jmp()
patch2_je(p6)
emit2(sys_write_block(ADDR['cwd_fail'], len(STR['cwd_fail'])))
emit2(mov_ebx(1))
emit2(mov_eax(0))
emit2(int80())
patch2_jmp(p6j)

# -- brk --
emit2(mov_eax(18))
emit2(xor_ebx_ebx())
emit2(int80())
emit2(cmp_rax_neg1())
p7a = emit2_je()
emit2(mov_rbx_rax())
emit2(add_rbx_imm32(4096))
emit2(mov_eax(18))
emit2(int80())
emit2(cmp_rax_neg1())
p7b = emit2_je()
emit2(sub_rbx_1())
emit2(mov_byte_rbx(0x42))
emit2(sys_write_block(ADDR['brk_ok'], len(STR['brk_ok'])))
p7j = emit2_jmp()
patch2_je(p7a)
patch2_je(p7b)
emit2(sys_write_block(ADDR['brk_fail'], len(STR['brk_fail'])))
emit2(mov_ebx(1))
emit2(mov_eax(0))
emit2(int80())
patch2_jmp(p7j)

# -- done --
emit2(sys_write_block(ADDR['done'], len(STR['done'])))

# -- exit --
emit2(xor_ebx_ebx())
emit2(mov_eax(0))
emit2(int80())

assert len(code) == CODE_SIZE, f"pass2 size {len(code)} != pass1 size {CODE_SIZE}"

# ── Assemble final binary ──────────────────────────────
data = b''.join(STR[k] for k in STR_ORDER)
data += b'\x00' * 8   # inode buf
data += b'\x00' * 128 # scratch buf

binary = bytes(code) + data

output = os.path.join(os.path.dirname(__file__), 'alltest.bin')
with open(output, 'wb') as f:
    f.write(binary)

print(f"alltest.bin: {len(binary)} bytes (code: {len(code)}, data: {len(data)})")
