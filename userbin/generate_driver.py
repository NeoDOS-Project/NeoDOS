#!/usr/bin/env python3
"""
generate_driver.py - Demo driver que se registra como handler de device 0
"""

import struct

BASE = 0x400000

def mov_eax(v): return b'\xB8' + struct.pack('<I', v)
def mov_rbx(v): return b'\x48\xBB' + struct.pack('<Q', v)
def mov_rcx(v): return b'\x48\xB9' + struct.pack('<Q', v)
def mov_rdx(v): return b'\x48\xBA' + struct.pack('<Q', v)
def int80(): return b'\xCD\x80'

# Messages
MSG_INIT = b"[Driver] Initializing...\r\n"
MSG_REG = b"[Driver] Registered as device handler\r\n"
MSG_WAIT = b"[Driver] Waiting for ioctl...\r\n"
MSG_CMD = b"[Driver] Got command: \r\n"

S_INIT = len(MSG_INIT)   # 24
S_REG = len(MSG_REG)    # 28
S_WAIT = len(MSG_WAIT)   # 24
S_CMD = len(MSG_CMD)     # 23

# Build code with placeholders - we'll fix them after knowing code size
CODE = b''

# Syscall 1: sys_write(MSG_INIT) - placeholders
CODE += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()

# Syscall 2: sys_register_device(0) 
CODE += mov_eax(15) + mov_rbx(0) + int80()

# Syscall 3: sys_write(MSG_REG)
CODE += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()

# Syscall 4: sys_write(MSG_WAIT)
CODE += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()

# Syscall 5: sys_ioctl(0, 0, 0, 0)
CODE += mov_eax(14) + mov_rbx(0) + mov_rcx(0) + mov_rdx(0) + int80()

# Syscall 6: sys_write(MSG_CMD)
CODE += mov_eax(1) + mov_rbx(0) + mov_rcx(0) + int80()

# Syscall 7: sys_exit(0)
CODE += mov_eax(0) + int80()

print(f"Code: {len(CODE)} bytes")

# Calculate data addresses
CODE_SIZE = len(CODE)
DATA_BASE = BASE + CODE_SIZE

print(f"DATA_BASE = 0x{DATA_BASE:x}")
print(f"S_INIT = {S_INIT}, S_REG = {S_REG}, S_WAIT = {S_WAIT}, S_CMD = {S_CMD}")

# Find all mov_rbx and mov_rcx positions in the code
code_list = bytearray(CODE)
rbx_addrs = []
rcx_lens = []

i = 0
while i < len(code_list) - 2:
    if code_list[i] == 0x48 and code_list[i+1] == 0xbb:
        # Found mov_rbx - imm64 at offset+2
        rbx_addrs.append(i + 2)
        i += 10  # 48 bb + 8 bytes = 10
    elif code_list[i] == 0x48 and code_list[i+1] == 0xb9:
        # Found mov_rcx - imm64 at offset+2
        rcx_lens.append(i + 2)
        i += 10  # 48 b9 + 8 bytes = 10
    else:
        i += 1

print(f"Found {len(rbx_addrs)} mov_rbx at {rbx_addrs}")
print(f"Found {len(rcx_lens)} mov_rcx at {rcx_lens}")

# Now fix them in order
# Positions: RBX[0]=sys_write1, RBX[1]=sys_register, RBX[2]=sys_write2, RBX[3]=sys_write3,
#            RBX[4]=sys_ioctl, RBX[5]=sys_write4
# RCX[0]=sys_write1, RCX[1]=sys_write2, RCX[2]=sys_write3, RCX[3]=sys_ioctl, RCX[4]=sys_write4

# sys_write(MSG_INIT) - RBX[0], RCX[0]
for j in range(8):
    code_list[rbx_addrs[0] + j] = (struct.pack('<Q', DATA_BASE))[j]
for j in range(8):
    code_list[rcx_lens[0] + j] = (struct.pack('<Q', S_INIT))[j]

# sys_write(MSG_REG) - RBX[2], RCX[1]
for j in range(8):
    code_list[rbx_addrs[2] + j] = (struct.pack('<Q', DATA_BASE + S_INIT))[j]
for j in range(8):
    code_list[rcx_lens[1] + j] = (struct.pack('<Q', S_REG))[j]

# sys_write(MSG_WAIT) - RBX[3], RCX[2]
for j in range(8):
    code_list[rbx_addrs[3] + j] = (struct.pack('<Q', DATA_BASE + S_INIT + S_REG))[j]
for j in range(8):
    code_list[rcx_lens[2] + j] = (struct.pack('<Q', S_WAIT))[j]

# sys_write(MSG_CMD) - RBX[5], RCX[4]
for j in range(8):
    code_list[rbx_addrs[5] + j] = (struct.pack('<Q', DATA_BASE + S_INIT + S_REG + S_WAIT))[j]
for j in range(8):
    code_list[rcx_lens[4] + j] = (struct.pack('<Q', S_CMD))[j]

# Build binary
BIN = bytes(code_list) + MSG_INIT + MSG_REG + MSG_WAIT + MSG_CMD

with open('driver.ndm', 'wb') as f:
    f.write(BIN)

print(f"driver.ndm: {len(BIN)} bytes")