#!/usr/bin/env python3
"""
generate_driver.py - Demo driver NDM v1 module.

Produces driver.ndm with:
  [NDM header (64 bytes)]
  [code section (x86-64, Ring 3)]
  [data section (string constants)]

The module registers as device handler 0, then polls for ioctl commands.
"""

import struct

BASE = 0x400000  # code is loaded at user slot base

# ── helper: x86-64 instruction encoding ──────────────────────────────

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

# ── code section (placeholders for RBX/RCX) ──────────────────────────

code = b''

# sys_write(MSG_INIT)    — eax=1, rbx=ptr, rcx=len
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()
# sys_register_device(0) — eax=15, rbx=device_num
code += mov_eax(15) + mov_rbx(0) + int80()
# sys_write(MSG_REG)
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()
# sys_write(MSG_WAIT)
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()
# sys_ioctl(0,0,0,0)    — eax=14, rbx=device, rcx=cmd, rdx=buf
code += mov_eax(14) + mov_rbx(0) + mov_rcx(0) + mov_rdx(0) + int80()
# sys_write(MSG_CMD)
code += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()
# sys_exit(0)            — eax=0
code += mov_eax(0) + int80()

CODE_SIZE = len(code)
assert CODE_SIZE < 64 * 1024, "code too large"

# ── layout ────────────────────────────────────────────────────────────
# The module is loaded at slot.code_base (BASE + slot_idx * 0x20000).
# code section goes first, then data section immediately after.

HEADER_SIZE = 64
DATA_OFFSET = HEADER_SIZE + CODE_SIZE
DATA_SIZE   = len(MSG_INIT) + len(MSG_REG) + len(MSG_WAIT) + len(MSG_CMD)

# Build data section
data  = MSG_INIT
data += MSG_REG
data += MSG_WAIT
data += MSG_CMD

# ── patch code with calculated addresses ─────────────────────────────

# Strings are at known offsets within the data section:
OFF_INIT = DATA_OFFSET
OFF_REG  = DATA_OFFSET + len(MSG_INIT)
OFF_WAIT = DATA_OFFSET + len(MSG_INIT) + len(MSG_REG)
OFF_CMD  = DATA_OFFSET + len(MSG_INIT) + len(MSG_REG) + len(MSG_WAIT)

# Code layout: each syscall has a specific pattern of mov_rbx / mov_rcx
# mov_rbx occurs before every syscall except sys_exit.
# mov_rcx occurs only before sys_write calls.
# We find them by scanning for the opcode bytes.

def find_offsets(blob, opcode_2bytes):
    """Return list of file-offsets of the 8-byte immediate after `opcode_2bytes`."""
    results = []
    i = 0
    while i < len(blob) - 9:
        if blob[i] == opcode_2bytes[0] and blob[i+1] == opcode_2bytes[1]:
            results.append(i + 2)  # skip opcode, point to imm64
            i += 10
        else:
            i += 1
    return results

rbx_offsets = find_offsets(code, (0x48, 0xBB))
rcx_offsets = find_offsets(code, (0x48, 0xB9))

def patch_imm64(blob, offset, value):
    for j in range(8):
        blob[offset + j] = struct.pack('<Q', value)[j]

code_arr = bytearray(code)

# sys_write RBX: msg ptrs
patch_imm64(code_arr, rbx_offsets[0], BASE + OFF_INIT)   # MSG_INIT
patch_imm64(code_arr, rbx_offsets[2], BASE + OFF_REG)    # MSG_REG
patch_imm64(code_arr, rbx_offsets[3], BASE + OFF_WAIT)   # MSG_WAIT
patch_imm64(code_arr, rbx_offsets[5], BASE + OFF_CMD)    # MSG_CMD

# sys_write RCX: msg lengths (excluding NUL terminator)
patch_imm64(code_arr, rcx_offsets[0], len(MSG_INIT) - 1)  # MSG_INIT
patch_imm64(code_arr, rcx_offsets[1], len(MSG_REG) - 1)   # MSG_REG
patch_imm64(code_arr, rcx_offsets[2], len(MSG_WAIT) - 1)  # MSG_WAIT
patch_imm64(code_arr, rcx_offsets[4], len(MSG_CMD) - 1)   # MSG_CMD

# ── NDM v1 header ────────────────────────────────────────────────────

NAME = b"DRIVER  "
assert len(NAME) <= 15

hdr = struct.pack('<I', 0x004D444E)       # magic          "NDM\0"
hdr += struct.pack('<I', 1)                # version        NDM_ABI_VERSION
hdr += struct.pack('<B', 0)                # module_type    Driver
hdr += struct.pack('<B', 0)                # reserved1
hdr += struct.pack('<H', HEADER_SIZE)      # header_size    64
hdr += struct.pack('<I', HEADER_SIZE)      # entry_offset   = code_offset (start of code = entry)
hdr += struct.pack('<I', HEADER_SIZE)      # code_offset    right after header
hdr += struct.pack('<I', CODE_SIZE)        # code_size
hdr += struct.pack('<I', DATA_OFFSET)      # data_offset
hdr += struct.pack('<I', DATA_SIZE)        # data_size
hdr += struct.pack('<I', 1)                # api_version
hdr += struct.pack('<I', 0)                # _reserved2
hdr += NAME.ljust(16, b'\x00')[:16]        # name
hdr += struct.pack('<B', 0)                # compat_flags
hdr += b'\x00' * 7                        # _padding

assert len(hdr) == HEADER_SIZE, f"header size: {len(hdr)} != {HEADER_SIZE}"

# ── write binary ─────────────────────────────────────────────────────

bin_data = hdr + bytes(code_arr) + data

with open('driver.ndm', 'wb') as f:
    f.write(bin_data)

print(f"driver.ndm: {len(bin_data)} bytes")
print(f"  header: {len(hdr)} bytes")
print(f"  code:   {CODE_SIZE} bytes @ offset {HEADER_SIZE}")
print(f"  data:   {DATA_SIZE} bytes @ offset {DATA_OFFSET}")
print(f"  name:   {NAME.decode()}")
