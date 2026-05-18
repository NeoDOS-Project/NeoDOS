#!/usr/bin/env python3
"""
generate_driver.py - Demo driver NDM v1 module using shared ndm_builder.

Produces driver.ndm:
  [NDM header (64 bytes)]
  [code section (x86-64, Ring 3)]
  [data section (string constants)]

The module registers as device handler 0, then polls for ioctl commands.
"""

import struct
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ndm_builder import NdmHeader, build_ndm

BASE = 0x400000

# ── x86-64 instruction encoding ──────────────────────────────────────

def mov_eax(v): return b'\xB8' + struct.pack('<I', v)
def mov_rbx(v): return b'\x48\xBB' + struct.pack('<Q', v)
def mov_rcx(v): return b'\x48\xB9' + struct.pack('<Q', v)
def mov_rdx(v): return b'\x48\xBA' + struct.pack('<Q', v)
def int80():    return b'\xCD\x80'

# ── data section (NUL-terminated strings) ─────────────────────────────

MSG_INIT  = b"[Driver] Initializing...\r\n\0"
MSG_REG   = b"[Driver] Registered as device handler\r\n\0"
MSG_WAIT  = b"[Driver] Waiting for ioctl...\r\n\0"
MSG_CMD   = b"[Driver] Got command: \r\n\0"

# ── code section ─────────────────────────────────────────────────────

code = b''
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()   # sys_write(MSG_INIT)
code += mov_eax(15) + mov_rbx(0) + int80()                 # sys_register_device(0)
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()    # sys_write(MSG_REG)
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()    # sys_write(MSG_WAIT)
code += mov_eax(14) + mov_rbx(0) + mov_rcx(0) + mov_rdx(0) + int80()  # sys_ioctl
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()    # sys_write(MSG_CMD)
code += mov_eax(0) + int80()                               # sys_exit(0)

CODE_SIZE = len(code)
assert CODE_SIZE < 64 * 1024

# ── data layout ──────────────────────────────────────────────────────

data  = MSG_INIT
data += MSG_REG
data += MSG_WAIT
data += MSG_CMD

DATA_SIZE = len(data)

# ── patch code with calculated addresses ─────────────────────────────

DATA_OFFSET = 64 + CODE_SIZE
OFF_INIT = DATA_OFFSET
OFF_REG  = DATA_OFFSET + len(MSG_INIT)
OFF_WAIT = DATA_OFFSET + len(MSG_INIT) + len(MSG_REG)
OFF_CMD  = DATA_OFFSET + len(MSG_INIT) + len(MSG_REG) + len(MSG_WAIT)

def find_offsets(blob, b1, b2):
    results = []
    i = 0
    while i < len(blob) - 9:
        if blob[i] == b1 and blob[i+1] == b2:
            results.append(i + 2)
            i += 10
        else:
            i += 1
    return results

rbx_offsets = find_offsets(code, 0x48, 0xBB)
rcx_offsets = find_offsets(code, 0x48, 0xB9)

def patch_imm64(blob, offset, value):
    for j in range(8):
        blob[offset + j] = struct.pack('<Q', value)[j]

code_arr = bytearray(code)
patch_imm64(code_arr, rbx_offsets[0], BASE + OFF_INIT)
patch_imm64(code_arr, rbx_offsets[2], BASE + OFF_REG)
patch_imm64(code_arr, rbx_offsets[3], BASE + OFF_WAIT)
patch_imm64(code_arr, rbx_offsets[5], BASE + OFF_CMD)
patch_imm64(code_arr, rcx_offsets[0], len(MSG_INIT) - 1)
patch_imm64(code_arr, rcx_offsets[1], len(MSG_REG) - 1)
patch_imm64(code_arr, rcx_offsets[2], len(MSG_WAIT) - 1)
patch_imm64(code_arr, rcx_offsets[4], len(MSG_CMD) - 1)

# ── build NDM binary ─────────────────────────────────────────────────

header = NdmHeader(name="DRIVER", module_type=0)
ndm = build_ndm(header, bytes(code_arr), data)

with open('driver.ndm', 'wb') as f:
    f.write(ndm)

print(f"driver.ndm: {len(ndm)} bytes")
print(f"  header: 64 bytes")
print(f"  code:   {CODE_SIZE} bytes")
print(f"  data:   {DATA_SIZE} bytes")
print(f"  name:   DRIVER")
