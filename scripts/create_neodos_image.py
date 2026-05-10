#!/usr/bin/env python3
import struct
import sys
import os

BLOCK_SIZE = 4096
SECTOR_SIZE = 512
SUPERBLOCK_MAGIC = 0x4F444F4E  # "NEOD"
DATA_START_SECTOR = 200
ROOT_DIR_BLOCK = 0

def create_superblock(volume_label=""):
    """Crear superbloque (512 bytes)"""
    data = bytearray(512)
    
    # Magic
    data[0:4] = struct.pack('<I', SUPERBLOCK_MAGIC)
    
    # Block size
    data[4:8] = struct.pack('<I', BLOCK_SIZE)
    
    # Num blocks (10 MB / 4KB = 2560 blocks)
    data[8:12] = struct.pack('<I', 2560)
    
    # Num inodes (max 256)
    data[12:16] = struct.pack('<I', 256)
    
    # Created timestamp
    data[16:24] = struct.pack('<Q', 0)
    
    # Volume label
    label_bytes = volume_label.encode('utf-8')[:11]
    data[24] = len(label_bytes)
    data[25:25+len(label_bytes)] = label_bytes
    # Fill rest with spaces
    for i in range(25+len(label_bytes), 36):
        data[i] = ord(' ')
    
    return bytes(data)

def create_inode(inode_num, mode, size, direct_blocks):
    """Crear inode (256 bytes)"""
    data = bytearray(256)
    
    # Inode number
    data[0:4] = struct.pack('<I', inode_num)
    
    # Mode
    data[4:6] = struct.pack('<H', mode)
    
    # Size
    data[6:10] = struct.pack('<I', size)
    
    # Times (atime, mtime, ctime)
    data[10:18] = struct.pack('<Q', 0)  # atime
    data[18:26] = struct.pack('<Q', 0)  # mtime
    data[26:34] = struct.pack('<Q', 0)  # ctime
    
    # Link count
    data[34:36] = struct.pack('<H', 1)
    
    # UID/GID
    data[36:40] = struct.pack('<I', 0)  # uid
    data[40:44] = struct.pack('<I', 0)  # gid
    
    # Direct blocks (12 × u32)
    for i, block in enumerate(direct_blocks):
        data[44 + i*4:44 + (i+1)*4] = struct.pack('<I', block)
    
    # Indirect block (0 for now)
    data[44+12*4:44+12*4+4] = struct.pack('<I', 0)
    
    return bytes(data)

def create_dir_entry(inode_num, entry_type, name, attributes=0):
    """Crear directory entry (256 bytes)"""
    data = bytearray(256)
    
    # Inode number
    data[0:4] = struct.pack('<I', inode_num)
    
    # Name length
    data[4] = len(name)
    
    # Entry type (1=file, 2=dir)
    data[5] = entry_type
    
    # Attributes (default: Archive for files, Dir for dirs)
    if attributes == 0:
        attributes = 0x10 if entry_type == 2 else 0x20  # DIR or ARCHIVE
    data[6] = attributes
    
    # Name (starts at offset 7)
    data[7:7+len(name)] = name.encode('utf-8')
    
    return bytes(data)

def main():
    # Crear imagen vacía (10 MB)
    image_size = 10 * 1024 * 1024
    image = bytearray(image_size)
    
    # 1. Superbloque @ sector 0
    print("[*] Writing superblock...")
    superblock = create_superblock("NEO-DISK")
    image[0:512] = superblock
    
    # 2. Inode table @ sectors 1-63 (125 inodes max)
    print("[*] Writing inode table...")
    
    # Inode 0: root directory (block 0 is valid and reserved for root directory data)
    # Directory logical size must cover all directory entries (3 × 256 = 768; use full block).
    root_inode = create_inode(0, 0x40, BLOCK_SIZE, [ROOT_DIR_BLOCK, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    image[512:512+256] = root_inode
    
    # Inode 1: readme.txt (points to block 1)
    readme_inode = create_inode(1, 0x80, 1024, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    image[512+256:512+512] = readme_inode
    
    # Inode 2: test.bat (points to block 2)
    testbat_inode = create_inode(2, 0x80, 512, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    image[512+512:512+768] = testbat_inode
    
    # Inode 3: SYSTEM directory (points to block 3)
    system_dir_inode = create_inode(3, 0x40, 512, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    image[512+768:512+1024] = system_dir_inode

    # Inode 4: CONFIG.SYS in SYSTEM (points to block 4)
    config_inode = create_inode(4, 0x80, 512, [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    image[512+1024:512+1280] = config_inode

    # Inode 5: AUTOEXEC.BAT (root) (points to block 5)
    autoexec_inode = create_inode(5, 0x80, 512, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    image[512+1280:512+1536] = autoexec_inode

    # Inode 6: HELLO.BIN — user-mode test binary (flat, loaded at 0x400000)
    # Size is determined at build time; default to 64 KB slot (1 block = 4KB is enough)
    hello_bin_path = os.path.join(os.path.dirname(__file__), '..', 'userbin', 'hello.bin')
    hello_bin_data = b''
    if os.path.exists(hello_bin_path):
        with open(hello_bin_path, 'rb') as hf:
            hello_bin_data = hf.read()
        print(f"[*] Including hello.bin ({len(hello_bin_data)} bytes)")
    else:
        print("[!] hello.bin not found — skipping (run 'nasm -f bin -o userbin/hello.bin userbin/hello.asm')")

    hello_inode = create_inode(6, 0x80, len(hello_bin_data), [6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    image[512+1536:512+1792] = hello_inode
    
    # 3. Root directory block (block 0) @ first data sector
    print("[*] Writing root directory...")
    offset = (DATA_START_SECTOR + ROOT_DIR_BLOCK * 8) * 512
    
    # Entry 0: readme.txt
    entry = create_dir_entry(1, 1, "readme.txt")  # type=1 (file)
    image[offset:offset+256] = entry
    
    # Entry 1: test.bat
    entry = create_dir_entry(2, 1, "test.bat")
    image[offset+256:offset+512] = entry

    # Entry 2: SYSTEM dir
    entry = create_dir_entry(3, 2, "SYSTEM") # type=2 (dir)
    image[offset+512:offset+768] = entry

    # Entry 3: HELLO.BIN (user-mode binary)
    entry = create_dir_entry(6, 1, "HELLO.BIN")
    image[offset+768:offset+1024] = entry
    
    # 4. Data blocks
    # Block 1 = sector 208 (readme.txt)
    print("[*] Writing readme.txt content...")
    offset = (200 + 8) * 512
    readme_content = b"""Welcome to NeoDOS!
This is a DOS-like operating system.
Built with NeoDOS FS v1.0 (inodes, not FAT).
Cluster: Block 1, Inode 1, Size: 1024 bytes.
Happy hacking!
"""
    image[offset:offset+len(readme_content)] = readme_content
    
    # Block 2 = sector 216 (test.bat)
    print("[*] Writing test.bat content...")
    offset = (200 + 16) * 512
    testbat_content = b"""ECHO Running batch test...
ECHO Hello from NeoDOS Batch!
SET TESTVAR=Success
ECHO Variable TESTVAR is %TESTVAR%
VER
ECHO Done.
"""
    image[offset:offset+len(testbat_content)] = testbat_content

    # Block 3 = sector 224 (SYSTEM directory)
    print("[*] Writing SYSTEM directory...")
    offset = (200 + 24) * 512
    entry1 = create_dir_entry(4, 1, "CONFIG.SYS")
    image[offset:offset+256] = entry1
    entry2 = create_dir_entry(5, 1, "AUTOEXEC.BAT")
    image[offset+256:offset+512] = entry2

    # Block 4 = sector 232 (CONFIG.SYS)
    print("[*] Writing CONFIG.SYS...")
    offset = (200 + 32) * 512
    config_content = b"""FILES=20
BUFFERS=10
COUNTRY=034
CURSOR=10
"""
    image[offset:offset+len(config_content)] = config_content

    # Block 5 = sector 240 (AUTOEXEC.BAT)
    print("[*] Writing AUTOEXEC.BAT...")
    offset = (200 + 40) * 512
    autoexec_content = b"""ECHO Welcome to NeoDOS 0.7
ECHO System Configuration Loaded.
VER
"""
    image[offset:offset+len(autoexec_content)] = autoexec_content

    # Block 6 = sector 248 (HELLO.BIN — user-mode flat binary)
    if hello_bin_data:
        print("[*] Writing HELLO.BIN content...")
        offset = (200 + 48) * 512
        image[offset:offset+len(hello_bin_data)] = hello_bin_data

    
    # Escribir imagen a disco
    output_file = "neodos_image.img"
    print(f"[*] Writing image to {output_file}...")
    with open(output_file, 'wb') as f:
        f.write(image)
    
    print(f"[+] Image created: {output_file} ({len(image)} bytes)")

if __name__ == '__main__':
    main()
