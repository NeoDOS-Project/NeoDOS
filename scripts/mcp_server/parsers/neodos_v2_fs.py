"""
neodos_v2_fs.py — Offline NeoFS v2 (NE2) parser for MCP tools.

Provides read-only access to NeoFS v2 filesystem images:
  - Superblock dump
  - Directory tree listing
  - File content read
  - Path resolution
  - Inode/stat info
  - Recursive directory tree
"""

import struct
from dataclasses import dataclass, field
from typing import Optional

SECTOR_SIZE = 512
BLOCK_SIZE = 4096
NODE_SIZE = 4096
DIRENTRY_SIZE = 128
NAME_MAX = 48
INLINE_MAX = 16
HEADER_SIZE = 8

SUPERBLOCK_MAGIC = 0x0032454E

NODE_INTERNAL = 0
NODE_LEAF = 1
NODE_FREELIST = 3
NODE_SNAPSHOT = 4

MODE_DIR = 0x0040
MODE_FILE = 0x0080

PERM_R = 0x0001
PERM_W = 0x0002
PERM_X = 0x0004
PERM_S = 0x0008
PERM_D = 0x0010


def crc32(data: bytes) -> int:
    crc = 0xFFFFFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            if crc & 1:
                crc = (crc >> 1) ^ 0xEDB88320
            else:
                crc >>= 1
    return crc ^ 0xFFFFFFFF


def mode_str(mode: int) -> str:
    parts = []
    if mode & MODE_DIR:
        parts.append("d")
    elif mode & MODE_FILE:
        parts.append("f")
    else:
        parts.append("?")
    parts.append("r" if mode & PERM_R else "-")
    parts.append("w" if mode & PERM_W else "-")
    parts.append("x" if mode & PERM_X else "-")
    parts.append("s" if mode & PERM_S else "-")
    parts.append("d" if mode & PERM_D else "-")
    return "".join(parts)


@dataclass
class DirEntryV2:
    name: str = ""
    mode: int = 0
    size: int = 0
    created: int = 0
    modified: int = 0
    checksum: int = 0
    inline_len: int = 0
    inline_data: bytes = b""
    extent_lba: int = 0
    extent_count: int = 0

    @property
    def is_dir(self) -> bool:
        return bool(self.mode & MODE_DIR)

    @property
    def is_file(self) -> bool:
        return bool(self.mode & MODE_FILE)

    def serialize(self) -> bytes:
        buf = bytearray(DIRENTRY_SIZE)
        nl = min(len(self.name), NAME_MAX)
        buf[0] = nl
        buf[1:1 + nl] = self.name.encode("utf-8", errors="replace")[:nl]
        il = min(self.inline_len, INLINE_MAX)
        buf[49:49 + il] = self.inline_data[:il]
        struct.pack_into("<H", buf, 65, self.mode)
        struct.pack_into("<Q", buf, 67, self.size)
        struct.pack_into("<Q", buf, 75, self.created)
        struct.pack_into("<Q", buf, 83, self.modified)
        struct.pack_into("<I", buf, 91, self.checksum)
        struct.pack_into("<I", buf, 95, self.inline_len)
        struct.pack_into("<Q", buf, 99, self.extent_lba)
        struct.pack_into("<I", buf, 107, self.extent_count)
        return bytes(buf)

    @classmethod
    def deserialize(cls, data: bytes) -> "DirEntryV2":
        if len(data) < DIRENTRY_SIZE:
            raise ValueError(f"DirEntry data too short: {len(data)}")
        nl = data[0]
        raw_name = data[1:1 + NAME_MAX]
        name = raw_name[:nl].decode("utf-8", errors="replace") if nl > 0 else ""
        inline_data = data[49:49 + INLINE_MAX]
        mode = struct.unpack_from("<H", data, 65)[0]
        size = struct.unpack_from("<Q", data, 67)[0]
        created = struct.unpack_from("<Q", data, 75)[0]
        modified = struct.unpack_from("<Q", data, 83)[0]
        checksum = struct.unpack_from("<I", data, 91)[0]
        inline_len = struct.unpack_from("<I", data, 95)[0]
        extent_lba = struct.unpack_from("<Q", data, 99)[0]
        extent_count = struct.unpack_from("<I", data, 107)[0]
        return cls(name=name, mode=mode, size=size, created=created,
                   modified=modified, checksum=checksum,
                   inline_len=inline_len, inline_data=inline_data,
                   extent_lba=extent_lba, extent_count=extent_count)


@dataclass
class BTreeEntry:
    key: bytes = b""
    value: bytes = b""


@dataclass
class BTreeNode:
    node_type: int = NODE_LEAF
    entries: list[BTreeEntry] = field(default_factory=list)

    @classmethod
    def deserialize(cls, data: bytes) -> Optional["BTreeNode"]:
        if len(data) < HEADER_SIZE:
            return None
        node_type = struct.unpack_from("<H", data, 0)[0]
        entry_count = struct.unpack_from("<H", data, 2)[0]
        stored_csum = struct.unpack_from("<I", data, 4)[0]
        if stored_csum != 0:
            actual = crc32(data[8:])
            if actual != stored_csum:
                pass
        entries = []
        off = HEADER_SIZE
        for _ in range(entry_count):
            if off + 4 > len(data):
                break
            key_len = struct.unpack_from("<H", data, off)[0]
            if off + 2 + key_len + 2 > len(data):
                break
            key = data[off + 2:off + 2 + key_len]
            value_len = struct.unpack_from("<H", data, off + 2 + key_len)[0]
            val_start = off + 4 + key_len
            if val_start + value_len > len(data):
                break
            value = data[val_start:val_start + value_len]
            entries.append(BTreeEntry(key=key, value=value))
            off = val_start + value_len
        return cls(node_type=node_type, entries=entries)

    @property
    def is_leaf(self) -> bool:
        return self.node_type == NODE_LEAF


@dataclass
class SuperblockNE2:
    magic: int = 0
    version: int = 0
    root_btree_lba: int = 0
    root_version: int = 0
    root_timestamp: int = 0
    num_blocks: int = 0
    num_used: int = 0
    num_free: int = 0
    label: str = ""
    flags: int = 0
    freelist_lba: int = 0
    snapshot_table_lba: int = 0

    @classmethod
    def deserialize(cls, data: bytes) -> "SuperblockNE2":
        magic = struct.unpack_from("<I", data, 0)[0]
        if magic != SUPERBLOCK_MAGIC:
            raise ValueError(f"Not an NE2 superblock: magic=0x{magic:08X}")
        version = struct.unpack_from("<I", data, 4)[0]
        root_btree_lba = struct.unpack_from("<Q", data, 8)[0]
        root_version = struct.unpack_from("<Q", data, 16)[0]
        root_timestamp = struct.unpack_from("<Q", data, 24)[0]
        num_blocks = struct.unpack_from("<Q", data, 32)[0]
        num_used = struct.unpack_from("<Q", data, 40)[0]
        num_free = struct.unpack_from("<Q", data, 48)[0]
        label_len = data[56]
        label = data[57:57 + label_len].decode("utf-8", errors="replace") if label_len > 0 else ""
        flags = struct.unpack_from("<I", data, 89)[0]
        freelist_lba = struct.unpack_from("<Q", data, 93)[0]
        snapshot_table_lba = struct.unpack_from("<Q", data, 101)[0]
        return cls(magic=magic, version=version, root_btree_lba=root_btree_lba,
                   root_version=root_version, root_timestamp=root_timestamp,
                   num_blocks=num_blocks, num_used=num_used, num_free=num_free,
                   label=label, flags=flags, freelist_lba=freelist_lba,
                   snapshot_table_lba=snapshot_table_lba)


class NeoDosFsV2Image:
    """Offline reader for NeoFS v2 (NE2) filesystem images."""

    def __init__(self, image_path: str):
        self._path = image_path
        with open(image_path, "rb") as f:
            self._data = f.read()
        self._sb = SuperblockNE2.deserialize(self._read_sectors(0, 1))

    def _read_sectors(self, lba: int, count: int) -> bytes:
        start = lba * SECTOR_SIZE
        end = start + count * SECTOR_SIZE
        if end > len(self._data):
            raise ValueError(f"Read beyond image: LBA {lba} * {count} sectors")
        return self._data[start:end]

    def _read_block(self, block_lba: int) -> bytes:
        return self._read_sectors(block_lba * 8, 8)

    def _read_node(self, block_lba: int) -> Optional[BTreeNode]:
        try:
            data = self._read_block(block_lba)
            return BTreeNode.deserialize(data)
        except (ValueError, IndexError):
            return None

    def _lookup_in_dir(self, dir_root_lba: int, name: str) -> Optional[DirEntryV2]:
        name_bytes = name.encode("utf-8")
        return self._btree_lookup(dir_root_lba, name_bytes)

    def _btree_lookup(self, root_lba: int, key: bytes) -> Optional[DirEntryV2]:
        node = self._read_node(root_lba)
        if node is None:
            return None
        for entry in node.entries:
            if entry.key == key:
                if node.is_leaf:
                    return DirEntryV2.deserialize(entry.value)
                else:
                    child_lba = struct.unpack_from("<Q", entry.value, 0)[0]
                    return self._btree_lookup(child_lba, key)
            if not node.is_leaf and entry.key > key:
                break
        if not node.is_leaf and node.entries:
            last = node.entries[0]
            child_lba = struct.unpack_from("<Q", last.value, 0)[0]
            for i, e in enumerate(node.entries):
                if e.key <= key and (i + 1 >= len(node.entries) or node.entries[i + 1].key > key):
                    child_lba = struct.unpack_from("<Q", e.value, 0)[0]
                    break
            return self._btree_lookup(child_lba, key)
        return None

    def _read_dir_entries(self, dir_root_lba: int) -> list[DirEntryV2]:
        entries = []
        self._btree_walk(dir_root_lba, entries)
        return entries

    def _btree_walk(self, root_lba: int, out: list[DirEntryV2]):
        node = self._read_node(root_lba)
        if node is None:
            return
        if node.is_leaf:
            for entry in node.entries:
                try:
                    de = DirEntryV2.deserialize(entry.value)
                    if de.name:
                        out.append(de)
                except ValueError:
                    pass
        else:
            for entry in node.entries:
                child_lba = struct.unpack_from("<Q", entry.value, 0)[0]
                self._btree_walk(child_lba, out)

    def _resolve_path(self, path: str) -> Optional[DirEntryV2]:
        parts = [p for p in path.replace("/", "\\").split("\\") if p and p not in (".", "")]
        if not parts:
            return None
        root_lba = self._sb.root_btree_lba
        current = None
        for i, part in enumerate(parts):
            if i == 0:
                root_node = self._read_node(root_lba)
                if root_node is None:
                    return None
                for entry in root_node.entries:
                    if entry.key.decode("utf-8", errors="replace") == part:
                        current = DirEntryV2.deserialize(entry.value)
                        break
                if current is None:
                    return None
            else:
                if current and current.is_dir and current.extent_lba > 0:
                    found = self._lookup_in_dir(current.extent_lba, part)
                    if found is None:
                        return None
                    current = found
                else:
                    return None
        return current

    def _read_file_data(self, entry: DirEntryV2) -> bytes:
        if entry.inline_len > 0:
            return entry.inline_data[:entry.inline_len]
        if entry.extent_lba == 0 or entry.extent_count == 0:
            return b""
        data = bytearray()
        for i in range(entry.extent_count):
            block_lba = entry.extent_lba + i
            try:
                data.extend(self._read_block(block_lba))
            except (ValueError, IndexError):
                break
        return bytes(data[:entry.size])

    # ── Public API ──

    @property
    def superblock(self) -> SuperblockNE2:
        return self._sb

    def dump_superblock(self) -> str:
        sb = self._sb
        return "\n".join([
            "NeoFS v2 (NE2) Superblock",
            f"  Magic:           NE2 (0x{SUPERBLOCK_MAGIC:08X})",
            f"  Version:         {sb.version}",
            f"  Label:           '{sb.label}'",
            f"  Total blocks:    {sb.num_blocks} ({sb.num_blocks * 8} sectors)",
            f"  Used blocks:     {sb.num_used}",
            f"  Free blocks:     {sb.num_free}",
            f"  Root B-tree:     LBA {sb.root_btree_lba}",
            f"  Root version:    {sb.root_version}",
            f"  Freelist:        LBA {sb.freelist_lba}" if sb.freelist_lba else "  Freelist:        (inline)",
            f"  Snapshots:       LBA {sb.snapshot_table_lba}" if sb.snapshot_table_lba else "  Snapshots:       none",
        ])

    def list_dir(self, path: str) -> Optional[list[dict]]:
        if path in ("\\", "/", ""):
            root_lba = self._sb.root_btree_lba
            entries = self._read_dir_entries(root_lba)
        else:
            parent = self._resolve_path(path)
            if parent is None or not parent.is_dir:
                return None
            root_lba = parent.extent_lba if parent.extent_lba > 0 else self._sb.root_btree_lba
            entries = self._read_dir_entries(root_lba)
        result = []
        for e in entries:
            result.append({
                "name": e.name,
                "type": "dir" if e.is_dir else "file",
                "size": e.size,
                "mode": e.mode,
                "mode_str": mode_str(e.mode),
                "inline": e.inline_len > 0,
                "extent_lba": e.extent_lba,
                "extent_count": e.extent_count,
            })
        result.sort(key=lambda x: (x["type"] != "dir", x["name"].lower()))
        return result

    def read_file(self, path: str) -> Optional[bytes]:
        entry = self._resolve_path(path)
        if entry is None or not entry.is_file:
            return None
        return self._read_file_data(entry)

    def stat(self, path: str) -> Optional[dict]:
        entry = self._resolve_path(path)
        if entry is None:
            return None
        return {
            "path": path,
            "name": entry.name,
            "type": "dir" if entry.is_dir else "file",
            "size": entry.size,
            "inode": 0,
            "mode": entry.mode,
            "mode_str": mode_str(entry.mode),
        }

    def resolve_path(self, path: str) -> Optional["ResolvedInfo"]:
        entry = self._resolve_path(path)
        if entry is None:
            return None
        return ResolvedInfo(path=path, entry=entry)


@dataclass
class ResolvedInfo:
    path: str
    entry: DirEntryV2
    inode_num: int = 0

    def mode_str(self) -> str:
        return mode_str(self.entry.mode)

    @property
    def size(self) -> int:
        return self.entry.size


def open_image(path: str) -> NeoDosFsV2Image:
    """Open an NE2 filesystem image for reading."""
    return NeoDosFsV2Image(path)
