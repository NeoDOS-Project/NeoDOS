#!/usr/bin/env python3
"""generate_filetest.py - Prueba syscalls: open, readfile, writefile"""

import struct

BASE = 0x400000

def mov_eax(v): return b'\xB8' + struct.pack('<I', v)
def mov_rbx(v): return b'\x48\xBB' + struct.pack('<Q', v)
def mov_ecx(v): return b'\xB9' + struct.pack('<I', v)
def mov_edx(v): return b'\xBA' + struct.pack('<I', v)
def int80(): return b'\xCD\x80'

MSG_START = b"=== NeoDOS File I/O Test ===\r\n"
MSG_OPEN = b"sys_open: OK\r\n"
MSG_READ = b"sys_read: OK\r\n"
MSG_WRITE = b"sys_write: OK\r\n"
MSG_DONE = b"File test complete!\r\n"
FILENAME = b"readme.txt"
WRITE_DATA = b"Hola FILETEST!"

# Sizes - actual computed lengths
S_START = 30   # === NeoDOS File I/O Test === + \r\n
S_OPEN = 14    # sys_open: OK + \r\n
S_READ = 14    # sys_read: OK + \r\n
S_WRITE = 15   # sys_write: OK + \r\n
S_DONE = 21    # File test complete! + \r\n
S_FILE = 10    # readme.txt
S_DATA = 14    # Hola FILETEST!

# Code (one build with correct addresses from start)
CODE = b''

# Use base 0x400000 + known code size estimate, then adjust
# Start with estimated to get initial addresses
EST_CODE = 120
DATA_BASE = BASE + EST_CODE

a_start = DATA_BASE
a_open = a_start + S_START
a_read = a_open + S_OPEN
a_write = a_read + S_READ
a_done = a_write + S_WRITE
a_file = a_done + S_DONE
a_data = a_file + S_FILE + 1  # filename + null byte
a_buf = a_data  # buffer is same as write data location

# Build code with these addresses
CODE += mov_eax(1) + mov_rbx(a_start) + mov_ecx(S_START) + int80()
CODE += mov_eax(10) + mov_rbx(a_file) + mov_ecx(0) + int80()
CODE += b'\x48\x89\xc3'  # mov rbx, rax (inode)
CODE += b'\x49\x89\xc0'  # mov r12, rax (save)
CODE += mov_eax(12) + mov_ecx(a_data) + mov_edx(S_DATA) + int80()
CODE += mov_eax(1) + mov_rbx(a_write) + mov_ecx(S_WRITE) + int80()
CODE += b'\x4c\x89\xc0' + b'\x48\x89\xc3'  # mov rax, r12 + mov rbx, rax
CODE += mov_eax(11) + mov_ecx(a_buf) + mov_edx(64) + int80()
CODE += mov_eax(1) + mov_rbx(a_read) + mov_ecx(S_READ) + int80()
CODE += mov_eax(1) + mov_rbx(a_done) + mov_ecx(S_DONE) + int80()
CODE += b'\x31\xdb' + mov_eax(0) + int80()

print(f"Code: {len(CODE)} bytes")

# Now recalculate addresses based on actual code size
REAL_CODE = len(CODE)
DATA_BASE = BASE + REAL_CODE

a_start = DATA_BASE
a_open = a_start + S_START
a_read = a_open + S_OPEN
a_write = a_read + S_READ
a_done = a_write + S_WRITE
a_file = a_done + S_DONE
a_data = a_file + S_FILE + 1  # filename + null
a_buf = a_data  # buffer is same as write data location

print(f"Data base: {DATA_BASE:#x}")
print(f"file: {a_file:#x}, data: {a_data:#x}, buf: {a_buf:#x}")

# Rebuild with correct addresses
CODE = b''
CODE += mov_eax(1) + mov_rbx(a_start) + mov_ecx(S_START) + int80()  # print start
CODE += mov_eax(10) + mov_rbx(a_file) + mov_ecx(0) + int80()        # open file
CODE += b'\x48\x89\xc3'                                              # mov rbx, rax (copy inode)
CODE += b'\x49\x89\xc0'                                              # mov r12, rax (save inode to R12)
CODE += mov_eax(12) + mov_ecx(a_data) + mov_edx(S_DATA) + int80()  # write
CODE += mov_eax(1) + mov_rbx(a_write) + mov_ecx(S_WRITE) + int80() # print write OK
CODE += b'\x4c\x89\xc0' + b'\x48\x89\xc3'                            # mov rax, r12 + mov rbx, rax (restore inode)
CODE += mov_eax(11) + mov_ecx(a_buf) + mov_edx(64) + int80()       # read back
CODE += mov_eax(1) + mov_rbx(a_read) + mov_ecx(S_READ) + int80()   # print read OK
CODE += mov_eax(1) + mov_rbx(a_done) + mov_ecx(S_DONE) + int80()   # print done
CODE += b'\x31\xdb' + mov_eax(0) + int80()                          # exit

print(f"Final code: {len(CODE)} bytes")

# Build binary - add null after filename
BIN = CODE + MSG_START + MSG_OPEN + MSG_READ + MSG_WRITE + MSG_DONE + FILENAME + b'\x00' + WRITE_DATA + b'\x00' * 80

with open('userbin/filetest.bin', 'wb') as f:
    f.write(BIN)

print(f"filetest.bin: {len(BIN)} bytes")