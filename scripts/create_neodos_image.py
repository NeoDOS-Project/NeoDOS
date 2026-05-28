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
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--label', default='NEO-DISK')
    parser.add_argument('--output', default='neodos_image.img')
    parser.add_argument('--minimal', action='store_true',
        help='Create minimal image with only test.txt')
    parser.add_argument('--readme', default='''Welcome to NeoDOS!
This is a DOS-like operating system.
Built with NeoDOS FS v1.0 (inodes, not FAT).
Cluster: Block 1, Inode 1, Size: 1024 bytes.
Happy hacking!
''')
    args = parser.parse_args()
    vol_label = args.label[:11]
    readme_text = args.readme

    # Crear imagen vacía (10 MB)
    image_size = 10 * 1024 * 1024
    image = bytearray(image_size)
    
    # 1. Superbloque @ sector 0
    print(f"[*] Writing superblock (label={vol_label})...")
    superblock = create_superblock(vol_label)
    image[0:512] = superblock
    
    # 2. Inode table @ sectors 1-63 (125 inodes max)
    print("[*] Writing inode table...")

    # .nem test driver files
    nem_dir = os.environ.get('NEM_DIR', '/tmp/nem_drivers')

    if args.minimal:
        # --- Minimal disk: only test.txt ---
        root_inode = create_inode(0, 0x40, 256, [ROOT_DIR_BLOCK, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512:512+256] = root_inode
        txt_inode = create_inode(1, 0x80, 56, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+256:512+512] = txt_inode

        # Root dir
        offset = (DATA_START_SECTOR + ROOT_DIR_BLOCK * 8) * 512
        entry = create_dir_entry(1, 1, "test.txt")
        image[offset:offset+256] = entry

        # Data block 1
        offset = (200 + 8) * 512
        txt_content = b"This is the secondary disk (D:). Only has this file.\r\n"
        image[offset:offset+len(txt_content)] = txt_content
    else:
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
        system_dir_inode = create_inode(3, 0x40, 768, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+768:512+1024] = system_dir_inode

        # Inode 4: CONFIG.SYS in SYSTEM (points to block 4)
        config_inode = create_inode(4, 0x80, 512, [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+1024:512+1280] = config_inode

        # Inode 5: AUTOEXEC.BAT (root) (points to block 5)
        autoexec_inode = create_inode(5, 0x80, 512, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+1280:512+1536] = autoexec_inode

        # Read all user binary data
        userbin_dir = os.path.join(os.path.dirname(__file__), '..', 'userbin')
        bin_files = {}
        for name in ['hello', 'systest', 'filetest', 'alltest']:
            fpath = os.path.join(userbin_dir, f'{name}.bin')
            data = b''
            if os.path.exists(fpath):
                with open(fpath, 'rb') as f:
                    data = f.read()
                print(f"[*] Including {name}.bin ({len(data)} bytes)")
            else:
                print(f"[!] {name}.bin not found — skipping")
            bin_files[name] = data

        # Allocate data blocks dynamically from block 6 onwards
        next_block = 6
        block_allocs = {}  # inode_num -> list of block numbers

        def alloc_blocks(inode_num, data_size):
            nonlocal next_block
            if data_size == 0:
                block_allocs[inode_num] = []
                return []
            blocks_needed = max(1, (data_size + BLOCK_SIZE - 1) // BLOCK_SIZE)
            if blocks_needed > 12:
                print(f"[!] Warning: {inode_num} needs {blocks_needed} blocks (max 12)")
                blocks_needed = 12
            blks = list(range(next_block, next_block + blocks_needed))
            next_block += blocks_needed
            block_allocs[inode_num] = blks
            return blks

        # Assign blocks for user binaries
        hello_blocks = alloc_blocks(6, len(bin_files['hello']))
        systest_blocks = alloc_blocks(7, len(bin_files['systest']))
        filetest_blocks = alloc_blocks(8, len(bin_files['filetest']))
        alltest_blocks = alloc_blocks(9, len(bin_files['alltest']))
        dir_blocks = alloc_blocks(15, BLOCK_SIZE)   # DRIVERS dir
        testdir_blocks = alloc_blocks(16, 256 * 5)  # TEST dir
        bootdir_blocks = alloc_blocks(19, 256 * 2)  # BOOT dir
        sys2dir_blocks = alloc_blocks(20, 512)      # SYSTEM dir (DRIVERS)

        # Build inodes with correct block lists
        def pad_blocks(blks):
            """Pad block list to 12 entries with zeros."""
            return (blks + [0] * 12)[:12]

        inodes_data = {
            6: (0x80, len(bin_files['hello']), pad_blocks(hello_blocks)),
            7: (0x80, len(bin_files['systest']), pad_blocks(systest_blocks)),
            8: (0x80, len(bin_files['filetest']), pad_blocks(filetest_blocks)),
            9: (0x80, len(bin_files['alltest']), pad_blocks(alltest_blocks)),
            15: (0x40, BLOCK_SIZE, pad_blocks(dir_blocks)),
            16: (0x40, 256 * 5, pad_blocks(testdir_blocks)),
            19: (0x40, 256 * 2, pad_blocks(bootdir_blocks)),
            20: (0x40, 512, pad_blocks(sys2dir_blocks)),
        }

        # Write inodes to inode table
        for inum, (mode, size, blks) in inodes_data.items():
            inode = create_inode(inum, mode, size, blks)
            offset = 512 + inum * 256
            image[offset:offset+256] = inode

        # Boot .nem driver inodes (BOOT category)
        boot_nem_data = {}
        boot_nem_files = [
            (21, "ps2kbd.nem"),
            (22, "serial.nem"),
            (23, "rtc.nem"),
        ]
        for inum, fname in boot_nem_files:
            fpath = os.path.join(nem_dir, "BOOT", fname)
            data = b''
            if os.path.exists(fpath):
                with open(fpath, 'rb') as nf:
                    data = nf.read()
                print(f"[*] Including BOOT/{fname} ({len(data)} bytes)")
            boot_nem_data[inum] = data
            blks = alloc_blocks(inum, len(data))
            inode = create_inode(inum, 0x80, len(data), pad_blocks(blks))
            offset = 512 + inum * 256
            image[offset:offset+256] = inode

        # System .nem driver inodes (SYSTEM category)
        system_nem_data = {}
        system_nem_files = [
            (24, "acpi.nem"),
            (25, "pci.nem"),
        ]
        for inum, fname in system_nem_files:
            fpath = os.path.join(nem_dir, "SYSTEM", fname)
            data = b''
            if os.path.exists(fpath):
                with open(fpath, 'rb') as nf:
                    data = nf.read()
                print(f"[*] Including SYSTEM/{fname} ({len(data)} bytes)")
            system_nem_data[inum] = data
            blks = alloc_blocks(inum, len(data))
            inode = create_inode(inum, 0x80, len(data), pad_blocks(blks))
            offset = 512 + inum * 256
            image[offset:offset+256] = inode

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

        # Entry 4: SYSTEST.BIN (syscall test binary)
        entry = create_dir_entry(7, 1, "SYSTEST.BIN")
        image[offset+1024:offset+1280] = entry

        # Entry 5: FILETEST.BIN (file I/O test binary)
        entry = create_dir_entry(8, 1, "FILETEST.BIN")
        image[offset+1280:offset+1536] = entry

        # Entry 6: ALLTEST.BIN (comprehensive syscall test)
        entry = create_dir_entry(9, 1, "ALLTEST.BIN")
        image[offset+1536:offset+1792] = entry

        # 4. Data blocks
        # Block 1 = sector 208 (readme.txt)
        print("[*] Writing readme.txt content...")
        offset = (200 + 8) * 512
        readme_content = readme_text.encode('utf-8')
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
        # Entry 3: DRIVERS subdirectory
        entry3 = create_dir_entry(15, 2, "DRIVERS")
        image[offset+512:offset+768] = entry3

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
        autoexec_content = b"""ECHO Welcome to NeoDOS 0.16
ECHO System Configuration Loaded.
VER
"""
        image[offset:offset+len(autoexec_content)] = autoexec_content

        # Write user binary data across their allocated blocks
        bin_data_map = {
            6: ('HELLO.BIN', bin_files['hello']),
            7: ('SYSTEST.BIN', bin_files['systest']),
            8: ('FILETEST.BIN', bin_files['filetest']),
            9: ('ALLTEST.BIN', bin_files['alltest']),
        }
        for inum, (name, data) in bin_data_map.items():
            if not data:
                continue
            blks = block_allocs.get(inum, [])
            print(f"[*] Writing {name} content ({len(data)} bytes across {len(blks)} blocks)...")
            for bi, blk in enumerate(blks):
                chunk = data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (200 + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # Write directory data blocks
        print("[*] Writing DRIVERS directory...")
        off = (200 + dir_blocks[0] * 8) * 512
        entry_boot = create_dir_entry(19, 2, "BOOT")
        image[off+256:off+512] = entry_boot
        entry_sys2 = create_dir_entry(20, 2, "SYSTEM")
        image[off+512:off+768] = entry_sys2

        print("[*] Writing TEST directory...")
        off = (200 + testdir_blocks[0] * 8) * 512
        entry_null = create_dir_entry(10, 1, "null.nem")
        image[off:off+256] = entry_null
        entry_echo = create_dir_entry(11, 1, "echo.nem")
        image[off+256:off+512] = entry_echo
        entry_stress = create_dir_entry(12, 1, "stress_lifecycle.nem")
        image[off+512:off+768] = entry_stress
        entry_fault = create_dir_entry(13, 1, "fault.nem")
        image[off+768:off+1024] = entry_fault
        entry_burst = create_dir_entry(14, 1, "burst.nem")
        image[off+1024:off+1280] = entry_burst

        # BOOT directory (uses dynamically allocated blocks)
        print("[*] Writing BOOT directory...")
        for bi, blk in enumerate(bootdir_blocks):
            if bi == 0:
                offset = (200 + blk * 8) * 512
                entry_ps2kbd = create_dir_entry(21, 1, "ps2kbd.nem")
                image[offset:offset+256] = entry_ps2kbd
                entry_serial = create_dir_entry(22, 1, "serial.nem")
                image[offset+256:offset+512] = entry_serial
                entry_rtc = create_dir_entry(23, 1, "rtc.nem")
                image[offset+512:offset+768] = entry_rtc


        # Boot driver data blocks
        for (inum, fname) in boot_nem_files:
            data = boot_nem_data.get(inum, b'')
            if data:
                blks = block_allocs.get(inum, [])
                for bi, blk in enumerate(blks):
                    chunk = data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                    offset = (200 + blk * 8) * 512
                    image[offset:offset+len(chunk)] = chunk
                print(f"[*] Writing BOOT/{fname} content...")

        # SYSTEM directory (DRIVERS) - uses dynamically allocated block
        print("[*] Writing SYSTEM directory (DRIVERS)...")
        for bi, blk in enumerate(sys2dir_blocks):
            if bi == 0:
                offset = (200 + blk * 8) * 512
                entry_acpi = create_dir_entry(24, 1, "acpi.nem")
                image[offset:offset+256] = entry_acpi
                entry_pci = create_dir_entry(25, 1, "pci.nem")
                image[offset+256:offset+512] = entry_pci

        # System driver data blocks
        for (inum, fname) in system_nem_files:
            data = system_nem_data.get(inum, b'')
            if data:
                blks = block_allocs.get(inum, [])
                for bi, blk in enumerate(blks):
                    chunk = data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                    offset = (200 + blk * 8) * 512
                    image[offset:offset+len(chunk)] = chunk
                print(f"[*] Writing SYSTEM/{fname} content...")
    
    # Escribir imagen a disco
    output_file = args.output
    print(f"[*] Writing image to {output_file}...")
    with open(output_file, 'wb') as f:
        f.write(image)
    
    print(f"[+] Image created: {output_file} ({len(image)} bytes)")

if __name__ == '__main__':
    main()
