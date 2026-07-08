#!/usr/bin/env python3
"""Create a NeoFS v2 (NE2) filesystem image."""
import struct
import sys
import os

BLOCK_SIZE = 4096
SECTOR_SIZE = 512
SUPERBLOCK_MAGIC_NE2 = 0x0032454E  # "NE2\0"


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


def create_superblock(num_blocks, label=""):
    """Create NE2 superblock (512 bytes)"""
    data = bytearray(512)

    # magic (4), version (4)
    data[0:4] = struct.pack('<I', SUPERBLOCK_MAGIC_NE2)
    data[4:8] = struct.pack('<I', 2)  # version

    # root_btree_lba = 1 (right after superblock)
    data[8:16] = struct.pack('<Q', 1)
    # root_version = 1
    data[16:24] = struct.pack('<Q', 1)
    # root_timestamp = 0
    data[24:32] = struct.pack('<Q', 0)
    # num_blocks
    data[32:40] = struct.pack('<Q', num_blocks)
    # num_used = 1 (just the root B-tree node)
    data[40:48] = struct.pack('<Q', 1)
    # num_free = num_blocks - 2
    data[48:56] = struct.pack('<Q', num_blocks - 2)
    # label_len
    label_bytes = label.encode('utf-8')[:32]
    data[56] = len(label_bytes)
    data[57:57 + len(label_bytes)] = label_bytes
    # flags = 0 (offset 89)
    data[89:93] = struct.pack('<I', 0)
    # freelist_lba = 0 (implicit: from LBA 2 to num_blocks-1)
    data[93:101] = struct.pack('<Q', 0)
    # snapshot_table_lba = 0
    data[101:109] = struct.pack('<Q', 0)
    # reserved[403..407] = CRC32 (computed below)

    # Compute CRC32 of bytes 0..72
    cksum = crc32(bytes(data[:72]))
    data[109:113] = struct.pack('<I', cksum)

    return bytes(data)


def create_empty_btree_node():
    """Create an empty B-tree leaf node (4096 bytes)"""
    data = bytearray(BLOCK_SIZE)
    # node_type = 1 (Leaf), num_entries = 0
    data[0:2] = struct.pack('<H', 1)  # Leaf
    data[2:4] = struct.pack('<H', 0)  # 0 entries
    # CRC32 of payload (offset 8..4096)
    cksum = crc32(bytes(data[8:]))
    data[4:8] = struct.pack('<I', cksum)
    return bytes(data)


def create_image(output_path, num_blocks, label=""):
    with open(output_path, 'wb') as f:
        # LBA 0: Superblock (512 bytes)
        sb = create_superblock(num_blocks, label)
        f.write(sb)
        # Pad to 512 bytes (already 512)
        assert len(sb) == 512

        # LBA 1: Empty B-tree root (4096 bytes = 8 sectors)
        root_node = create_empty_btree_node()
        f.write(root_node)
        assert len(root_node) == 4096

        # Rest of the disk: empty (free space)
        remaining = (num_blocks - 2) * BLOCK_SIZE
        f.write(b'\x00' * remaining)

    actual_size = os.path.getsize(output_path)
    expected = num_blocks * BLOCK_SIZE
    print(f"[+] NE2 image created: {output_path}")
    print(f"    Size: {actual_size} bytes ({num_blocks} blocks)")
    print(f"    Label: '{label}'")


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description='Create NE2 filesystem image')
    parser.add_argument('--label', default='NEODOS')
    parser.add_argument('--blocks', type=int, default=2560, help='Number of 4KB blocks')
    parser.add_argument('--size-mb', type=int, help='Size in MB (overrides --blocks)')
    parser.add_argument('--output', required=True)
    args = parser.parse_args()

    if args.size_mb:
        total_bytes = args.size_mb * 1024 * 1024
        num_blocks = total_bytes // BLOCK_SIZE
    else:
        num_blocks = args.blocks

    create_image(args.output, num_blocks, args.label)
