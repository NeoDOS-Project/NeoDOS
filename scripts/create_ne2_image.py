#!/usr/bin/env python3
"""Create a NeoFS v2 (NE2) filesystem image with files."""
import struct
import sys
import os
from pathlib import Path

BLOCK_SIZE = 4096
SECTOR_SIZE = 512
DIRENTRY_SIZE = 128
NAME_MAX = 48
INLINE_MAX = 16
SUPERBLOCK_MAGIC_NE2 = 0x0032454E  # "NE2\0"
MODE_DIR = 0x0040
MODE_FILE = 0x0080
PERM_R = 0x0001
PERM_W = 0x0002
PERM_X = 0x0004
PERM_S = 0x0008
PERM_D = 0x0010

def crc32(data):
    crc = 0xFFFFFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            if crc & 1:
                crc = (crc >> 1) ^ 0xEDB88320
            else:
                crc >>= 1
    return crc ^ 0xFFFFFFFF

def default_perms(name):
    u = name.upper()
    if u.endswith('.NXE') or u.endswith('.COM') or u.endswith('.EXE'): return PERM_R | PERM_X
    if u.endswith('.NEM'): return PERM_R
    if u.endswith('.NXL'): return PERM_R | PERM_X
    if u.endswith('.BAT') or u.endswith('.CMD'): return PERM_R | PERM_X
    if u.endswith('.SYS'): return PERM_R
    if u.endswith('.CFG') or u.endswith('.INI'): return PERM_R | PERM_W
    if u.endswith('.TXT') or u.endswith('.MD') or u.endswith('.LOG'): return PERM_R | PERM_W
    return PERM_R | PERM_W

def make_direntry(name, mode, size, extent_lba=0, extent_count=0, inline_data=b''):
    """Create a 128-byte DirEntryV2."""
    buf = bytearray(DIRENTRY_SIZE)
    nl = min(len(name), NAME_MAX)
    buf[0] = nl
    buf[1:1+nl] = name.encode('utf-8')[:nl]
    # inline data at offset 49
    off_inline = 1 + NAME_MAX
    il = min(len(inline_data), INLINE_MAX)
    buf[off_inline:off_inline+il] = inline_data[:il]
    # fields
    off = off_inline + INLINE_MAX  # 65
    struct.pack_into('<H', buf, off, mode); off += 2
    struct.pack_into('<Q', buf, off, size); off += 8
    struct.pack_into('<Q', buf, off, 0); off += 8   # created
    struct.pack_into('<Q', buf, off, 0); off += 8   # modified
    struct.pack_into('<I', buf, off, 0); off += 4   # checksum
    struct.pack_into('<I', buf, off, il); off += 4  # inline_len
    struct.pack_into('<Q', buf, off, extent_lba); off += 8
    struct.pack_into('<I', buf, off, extent_count); off += 4
    return bytes(buf)

def make_btree_leaf(entries_data):
    """Create a B-tree leaf node (no splitting)."""
    data = bytearray(BLOCK_SIZE)
    data[0:2] = struct.pack('<H', 1)  # Leaf
    data[2:4] = struct.pack('<H', len(entries_data))
    off = 8
    for key, value in entries_data:
        kl = len(key); vl = len(value)
        if off + 4 + kl + vl > BLOCK_SIZE: break
        data[off:off+2] = struct.pack('<H', kl)
        data[off+2:off+2+kl] = key
        data[off+2+kl:off+4+kl] = struct.pack('<H', vl)
        data[off+4+kl:off+4+kl+vl] = value
        off += 4 + kl + vl
    cksum = crc32(bytes(data[8:]))
    data[4:8] = struct.pack('<I', cksum)
    return bytes(data)

def entry_size(key_len):
    """Size of one B-tree entry in bytes."""
    return 4 + key_len + DIRENTRY_SIZE  # key_len(2) + key + val_len(2) + value(128)

ROOT = '/'  # single-character root marker for sorting

def create_image(output_path, num_blocks, label, file_data):
    # Organize files into directory tree
    dir_tree = {ROOT: []}  # ensure root exists
    for path, content, mode in file_data:
        parts = path.strip('/').split('/')
        filename = parts[-1]
        parent = ROOT + '/'.join(parts[:-1]) if len(parts) > 1 else ROOT
        if parent not in dir_tree: dir_tree[parent] = []
        dir_tree[parent].append((filename, content, mode, False))
        for i in range(1, len(parts)):
            dirpath = ROOT + '/'.join(parts[:i])
            dirname = parts[i-1]
            parent_of_dir = ROOT + '/'.join(parts[:i-1]) if i > 1 else ROOT
            if dirpath not in dir_tree: dir_tree[dirpath] = []
            if parent_of_dir not in dir_tree: dir_tree[parent_of_dir] = []
            already = any(d == dirname and is_d for d,_,_,is_d in dir_tree[parent_of_dir])
            if not already:
                dir_tree[parent_of_dir].append((dirname, b'', MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D, True))
    
    next_lba = 2
    dir_nodes = {}
    dir_lba_map = {}
    
    # First pass: allocate LBAs for dir B-tree nodes
    for dirpath in sorted(dir_tree.keys(), key=lambda x: x.count(ROOT)):
        dir_lba_map[dirpath] = next_lba
        next_lba += 1
    
    # Build B-tree entries per directory
    for dirpath in sorted(dir_tree.keys(), key=lambda x: x.count(ROOT)):
        node_entries = []
        for name, content, mode, is_dir in dir_tree[dirpath]:
            if is_dir:
                subdir_path = dirpath + ROOT + name if dirpath != ROOT else ROOT + name
                subdir_lba = dir_lba_map.get(subdir_path, 0)
                entry = make_direntry(name, mode, 0, extent_lba=subdir_lba, extent_count=0)
            elif len(content) <= INLINE_MAX:
                entry = make_direntry(name, mode, len(content), inline_data=content)
            else:
                extent_lba = next_lba
                blocks = (len(content) + BLOCK_SIZE - 1) // BLOCK_SIZE
                entry = make_direntry(name, mode, len(content),
                                      extent_lba=extent_lba, extent_count=blocks)
                next_lba += blocks
            node_entries.append((name.encode('utf-8'), entry))
        # Sort by key for binary search
        node_entries.sort(key=lambda x: x[0])
        dir_nodes[dirpath] = node_entries
    
    total_blocks = max(next_lba, num_blocks)
    image_size = total_blocks * BLOCK_SIZE
    image = bytearray(image_size)
    
    # 1. Superblock
    root_lba = dir_lba_map.get(ROOT, 1)
    label_bytes = label.encode('utf-8')[:32]
    sb = bytearray(SECTOR_SIZE)
    struct.pack_into('<I', sb, 0, SUPERBLOCK_MAGIC_NE2)
    struct.pack_into('<I', sb, 4, 2)
    struct.pack_into('<Q', sb, 8, root_lba)
    struct.pack_into('<Q', sb, 16, 1)
    struct.pack_into('<Q', sb, 24, 0)
    struct.pack_into('<Q', sb, 32, total_blocks)
    struct.pack_into('<Q', sb, 40, next_lba)
    struct.pack_into('<Q', sb, 48, total_blocks - next_lba)
    sb[56] = len(label_bytes)
    sb[57:57+len(label_bytes)] = label_bytes
    cksum = crc32(bytes(sb[:72]))
    sb[109:113] = struct.pack('<I', cksum)
    image[0:SECTOR_SIZE] = bytes(sb)
    
    # 2. Write all B-tree nodes (one per directory)
    for dirpath, node_entries in dir_nodes.items():
        lba = dir_lba_map[dirpath]
        node_data = make_btree_leaf(node_entries)
        image[lba * BLOCK_SIZE : lba * BLOCK_SIZE + BLOCK_SIZE] = node_data
    
    # 3. Write file data blocks
    for dirpath, entries in dir_tree.items():
        for name, content, mode, is_dir in entries:
            if is_dir or len(content) <= INLINE_MAX:
                continue
            # Find extent_lba from the entry we built
            for ename, ebytes in dir_nodes[dirpath]:
                if ename == name.encode('utf-8'):
                    # Parse extent_lba from the serialized entry (offset 99)
                    extent_lba = struct.unpack_from('<Q', ebytes[99:107])[0]
                    blocks = (len(content) + BLOCK_SIZE - 1) // BLOCK_SIZE
                    block_start = extent_lba * BLOCK_SIZE
                    for i in range(blocks):
                        chunk = content[i*BLOCK_SIZE:(i+1)*BLOCK_SIZE]
                        image[block_start + i*BLOCK_SIZE : block_start + i*BLOCK_SIZE + len(chunk)] = chunk
                    break
    
    total_file_entries = sum(len(nodes) for nodes in dir_nodes.values())
    with open(output_path, 'wb') as f:
        f.write(bytes(image))
    
    actual = os.path.getsize(output_path)
    print(f"[+] NE2 image: {output_path}")
    print(f"    Size: {actual} bytes ({total_blocks} blocks)")
    print(f"    Entries in tree: {total_file_entries}")


def collect_files():
    """Collect all files to include in the image."""
    files = []
    
    # Config files (inline, small)
    base_path = os.path.join(os.path.dirname(__file__), "..", "preferences")
    for cfg in ['boot.cfg', 'system.cfg', 'input.cfg']:
        p = os.path.join(base_path, cfg)
        if os.path.exists(p):
            with open(p, 'rb') as f:
                files.append(('/System/' + cfg, f.read(), MODE_FILE | PERM_R))
    
    # README
    readme = b"Welcome to NeoDOS v2!\r\n"
    files.append(("/README.TXT", readme, MODE_FILE | PERM_R | PERM_W))

    # Temp directory marker (needed by tests)
    files.append(("/Temp/.empty", b"", MODE_FILE | PERM_R | PERM_W))
    
    # NXE binaries — split into \Programs\ (essential) and \System\Tools\ (extra)
    userbin_dir = os.path.join(os.path.dirname(__file__), '..', 'userbin')
    programs_nxe = ['neoshell', 'neoinit', 'cmdtest', 'cd', 'corehelp',
                    'datetime', 'ver', 'neomem', 'vol', 'echo', 'label',
                    'coretype', 'tree', 'corecls', 'corecopy', 'coredel',
                    'coreren', 'coremd', 'corerd', 'drives', 'ps', 'keyb', 'coredir']
    tools_nxe = ['kill', 'pri', 'fsck', 'ndreg', 'loadnem', 'progress',
                 'neotop', 'dhcpd', 'netcfg', 'ipconfig', 'coredir', 'ping', 'cpuinfo']
    for name in programs_nxe + tools_nxe:
        subdir = 'Programs' if name in programs_nxe else 'System/Tools'
        p = os.path.join(userbin_dir, f'{name}.nxe')
        if os.path.exists(p):
            with open(p, 'rb') as f:
                data = f.read()
            perms = default_perms(f'{name}.NXE')
            files.append(('/' + subdir + '/' + name + '.nxe', data, MODE_FILE | perms))
            print(f"  [+] {name}.nxe (in {subdir}) ({len(data)} bytes)")
        else:
            print(f"  [!] {name}.nxe not found")

    # NXL libraries
    nxl_map = [
        ('libneodos.nxl', 'fs.nxl'),
        ('libmath.nxl', 'math.nxl'),
        ('console.nxl', 'console.nxl'),
        ('net.nxl', 'net.nxl'),
    ]
    for src_name, dst_name in nxl_map:
        nxl_path = os.path.join(os.path.dirname(__file__), '..', src_name)
        if not os.path.exists(nxl_path):
            libname = src_name.replace('lib', '').replace('.nxl', '')
            nxl_path = os.path.join(os.path.dirname(__file__), '..', f'lib{libname}-nxl',
                                    'target', 'x86_64-unknown-none', 'release', src_name)
        if os.path.exists(nxl_path):
            with open(nxl_path, 'rb') as f:
                data = f.read()
            files.append(('/System/Libraries/' + dst_name, data, MODE_FILE | PERM_R | PERM_X))
            print(f"  [+] {src_name} -> {dst_name} ({len(data)} bytes)")
        else:
            print(f"  [!] {src_name} not found")
            files.append(('/System/Libraries/' + dst_name, b'', MODE_FILE | PERM_R))

    # NEM drivers
    nem_dir = os.environ.get('NEM_DIR', '/tmp/nem_drivers_0')
    for nem_name in ['ps2kbd', 'ps2mouse', 'rtc', 'serial', 'acpi', 'ahci', 'ata', 'e1000', 'pci', 'virtio-blk']:
        p = os.path.join(nem_dir, nem_name, f'{nem_name}.nem') if os.path.exists(os.path.join(nem_dir, nem_name)) else '/tmp/nem_drivers/' + nem_name + '.nem'
        if os.path.exists(p):
            with open(p, 'rb') as f:
                data = f.read()
            files.append(('/System/Drivers/' + nem_name + '.nem', data, MODE_FILE | PERM_R))
            print(f"  [+] {nem_name}.nem ({len(data)} bytes)")
        else:
            print(f"  [!] {nem_name}.nem not found")
    
    return files


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description='Create NE2 filesystem image')
    parser.add_argument('--label', default='NEODOS')
    parser.add_argument('--blocks', type=int, default=2560)
    parser.add_argument('--output', required=True)
    parser.add_argument('--no-files', action='store_true', help='Create empty image')
    args = parser.parse_args()
    
    if args.no_files:
        create_image(args.output, args.blocks, args.label, [])
    else:
        files = collect_files()
        create_image(args.output, args.blocks, args.label, files)
