"""
parsers/neodos_fs.py — NeoDOS Filesystem Image Parser.

Read-only parser matching the kernel's Inode and DirectoryEntry structs
in neodos-kernel/src/fs/neodos_fs.rs.

Inode (256 bytes, #[repr(packed)]):
  [0..4)   inode_num: u32
  [4..6)   mode: u16          (0x40=dir, 0x80=file; bits 0-4 = RWXSD)
  [6..10)  size: u32
  [10..18) atime: u64
  [18..26) mtime: u64
  [26..34) ctime: u64
  [34..36) link_count: u16
  [36..40) owner_uid: u32
  [40..44) owner_gid: u32
  [44..92) direct_blocks: [u32; 12]
  [92..96) indirect_block: u32
  [96..256) padding

DirEntry (256 bytes):
  [0..4)   inode_num: u32
  [4]      name_len: u8
  [5]      entry_type: u8     (1=file, 2=dir)
  [6]      attributes: u8
  [7..256) name: [u8; 249]
"""

import struct
from dataclasses import dataclass
from typing import Optional


# ── Constants ──

SECTOR_SIZE = 512
INODE_SIZE = 256
INODE_SECTORS = 63           # LBAs 1-63
MAX_INODES = 126
BLOCK_SIZE = 4096
BLOCK_SECTORS = BLOCK_SIZE // SECTOR_SIZE  # 8
DATA_START_SECTOR = 200      # LBA where data blocks begin
DIR_ENTRY_SIZE = 256

NEODOS_MAGIC = 0x4F444F4E    # "NEOD" little-endian

# Inode mode bits (matching kernel)
MODE_DIR = 0x40
MODE_FILE = 0x80
PERM_R = 0x0001
PERM_W = 0x0002
PERM_X = 0x0004
PERM_S = 0x0008
PERM_D = 0x0010

# Directory entry types
ENTRY_FILE = 1
ENTRY_DIR = 2

ENTRY_TYPE_NAMES = {
    0: "unused",
    1: "file",
    2: "dir",
}


# ── Data Structures ──

@dataclass
class Superblock:
    magic: int
    version: int
    block_size: int
    num_blocks: int
    max_inodes: int
    num_inodes: int
    root_inode: int = 0
    label: str = ""
    checksum: int = 0

    @classmethod
    def parse(cls, data: bytes) -> Optional["Superblock"]:
        if len(data) < 48:
            return None
        magic, block_size, num_blocks, num_inodes, created = \
            struct.unpack_from("<IIIiQ", data, 0)
        if magic != NEODOS_MAGIC:
            return None
        label_len = data[24] if len(data) > 24 else 0
        if label_len > 11:
            label_len = 0
        label = data[25:25 + label_len].decode("ascii", errors="replace") if label_len > 0 else ""
        # No root_inode in superblock — root is always 0
        return cls(magic=magic, version=0, block_size=block_size,
                   num_blocks=num_blocks, max_inodes=num_inodes,
                   num_inodes=0, root_inode=0,
                   label=label, checksum=0)

    def dump(self) -> str:
        return (
            f"Superblock:\n"
            f"  Magic:         0x{self.magic:08X} ('NEOD')\n"
            f"  Version:       {self.version}\n"
            f"  Block size:    {self.block_size}\n"
            f"  Num blocks:    {self.num_blocks}\n"
            f"  Max inodes:    {self.max_inodes}\n"
            f"  Label:         '{self.label}'\n"
        )


@dataclass
class Inode:
    inode_num: int
    mode: int
    size: int
    atime: int
    mtime: int
    ctime: int
    link_count: int
    owner_uid: int
    owner_gid: int
    direct_blocks: list[int]
    indirect_block: int

    @classmethod
    def parse(cls, data: bytes) -> Optional["Inode"]:
        if len(data) < INODE_SIZE:
            return None
        inode_num = struct.unpack_from("<I", data, 0)[0]
        mode = struct.unpack_from("<H", data, 4)[0]
        if mode == 0 or mode == 0xFFFF:
            return None  # free inode
        size = struct.unpack_from("<I", data, 6)[0]
        atime = struct.unpack_from("<Q", data, 10)[0]
        mtime = struct.unpack_from("<Q", data, 18)[0]
        ctime = struct.unpack_from("<Q", data, 26)[0]
        link_count = struct.unpack_from("<H", data, 34)[0]
        owner_uid = struct.unpack_from("<I", data, 36)[0]
        owner_gid = struct.unpack_from("<I", data, 40)[0]
        direct_blocks = list(struct.unpack_from("<12I", data, 44))
        indirect_block = struct.unpack_from("<I", data, 92)[0]
        return cls(
            inode_num=inode_num,
            mode=mode,
            size=size,
            atime=atime,
            mtime=mtime,
            ctime=ctime,
            link_count=link_count,
            owner_uid=owner_uid,
            owner_gid=owner_gid,
            direct_blocks=direct_blocks,
            indirect_block=indirect_block,
        )

    def is_dir(self) -> bool:
        return (self.mode & MODE_DIR) != 0

    def is_file(self) -> bool:
        return (self.mode & MODE_FILE) != 0

    def mode_str(self) -> str:
        if self.is_dir():
            return "DIR"
        elif self.is_file():
            return "FILE"
        parts = []
        if self.mode & MODE_DIR:
            parts.append("D")
        if self.mode & MODE_FILE:
            parts.append("F")
        return "|".join(parts) if parts else f"0x{self.mode:04x}"

    def perms_str(self) -> str:
        s = ""
        s += "R" if self.mode & PERM_R else "-"
        s += "W" if self.mode & PERM_W else "-"
        s += "X" if self.mode & PERM_X else "-"
        s += "S" if self.mode & PERM_S else "-"
        s += "D" if self.mode & PERM_D else "-"
        return s

    def dump(self) -> str:
        blocks = [b for b in self.direct_blocks if b != 0]
        return (
            f"Inode #{self.inode_num}:\n"
            f"  Mode:  0x{self.mode:04X} ({self.mode_str()}) perms={self.perms_str()}\n"
            f"  Size:  {self.size} bytes\n"
            f"  Links: {self.link_count}  UID: {self.owner_uid}  GID: {self.owner_gid}\n"
            f"  Times: c={self.ctime} m={self.mtime} a={self.atime}\n"
            f"  Direct blocks: {blocks}\n"
            f"  Indirect block: {self.indirect_block}"
        )


@dataclass
class DirEntry:
    inode_num: int
    name_len: int
    entry_type: int
    attributes: int
    name: str

    def type_str(self) -> str:
        return ENTRY_TYPE_NAMES.get(self.entry_type, f"type={self.entry_type}")


@dataclass
class FileInfo:
    inode: Inode
    name: str
    path: str


# ── Filesystem Image Reader ──

class NeoDosFsImage:
    """Read-only parser for a NeoDOS filesystem image file."""

    def __init__(self, path: str):
        self.path = path
        with open(path, "rb") as f:
            self.data = f.read()
        self.superblock: Optional[Superblock] = None
        self.inodes: dict[int, Inode] = {}
        self._parse()

    def _parse(self):
        if len(self.data) < SECTOR_SIZE:
            raise ValueError(f"Image too small: {len(self.data)} bytes")

        sb = Superblock.parse(self.data[:SECTOR_SIZE])
        if sb is None:
            raise ValueError("Invalid superblock")
        self.superblock = sb

        # Inode table: sectors 1..63, each 512 bytes, each inode 256 bytes
        inode_table = self.data[SECTOR_SIZE:SECTOR_SIZE * (1 + INODE_SECTORS)]
        for i in range(MAX_INODES):
            offset = i * INODE_SIZE
            if offset + INODE_SIZE > len(inode_table):
                break
            chunk = inode_table[offset:offset + INODE_SIZE]
            ino = Inode.parse(chunk)
            if ino is not None:
                self.inodes[ino.inode_num] = ino

    def _block_to_offset(self, block_num: int) -> int:
        """Data blocks start at DATA_START_SECTOR (200), each block is BLOCK_SECTORS (8)."""
        sector = DATA_START_SECTOR + block_num * BLOCK_SECTORS
        return sector * SECTOR_SIZE

    def read_block(self, block_num: int) -> bytes:
        off = self._block_to_offset(block_num)
        blk = self.data[off:off + BLOCK_SIZE]
        return blk.ljust(BLOCK_SIZE, b"\x00")

    def read_inode_data(self, inode: Inode) -> bytes:
        if inode.size == 0:
            return b""
        chunks = []
        remaining = inode.size

        # Collect all non-zero direct blocks (block 0 is valid)
        all_blocks = [b for b in inode.direct_blocks]

        # Also check if block 0 is the only valid block (root dir)
        if all(b == 0 for b in all_blocks):
            pass  # no blocks

        for b in all_blocks:
            if remaining <= 0:
                break
            data = self.read_block(b)
            take = min(len(data), remaining)
            chunks.append(data[:take])
            remaining -= take

        if remaining > 0 and inode.indirect_block != 0:
            indirect = self.read_block(inode.indirect_block)
            for i in range(BLOCK_SIZE // 4):
                if remaining <= 0:
                    break
                b = struct.unpack_from("<I", indirect, i * 4)[0]
                if b == 0:
                    break
                data = self.read_block(b)
                take = min(len(data), remaining)
                chunks.append(data[:take])
                remaining -= take

        return b"".join(chunks)

    def readdir(self, dir_inode: Inode) -> list[DirEntry]:
        if not dir_inode.is_dir():
            raise ValueError(f"Inode {dir_inode.inode_num} not a directory")
        raw = self.read_inode_data(dir_inode)
        entries = []
        pos = 0
        while pos + DIR_ENTRY_SIZE <= len(raw):
            entry_data = raw[pos:pos + DIR_ENTRY_SIZE]
            inode_num = struct.unpack_from("<I", entry_data, 0)[0]
            if inode_num == 0:
                break  # end of directory
            name_len = entry_data[4]
            entry_type = entry_data[5]
            attributes = entry_data[6]
            name_bytes = entry_data[7:7 + name_len] if name_len <= 249 else b""
            name = name_bytes.decode("ascii", errors="replace")
            entries.append(DirEntry(
                inode_num=inode_num,
                name_len=name_len,
                entry_type=entry_type,
                attributes=attributes,
                name=name,
            ))
            pos += DIR_ENTRY_SIZE
        return entries

    def lookup(self, dir_inode_num: int, name: str) -> Optional[DirEntry]:
        ino = self.inodes.get(dir_inode_num)
        if ino is None or not ino.is_dir():
            return None
        for e in self.readdir(ino):
            if e.name.upper() == name.upper():
                return e
        return None

    def resolve_path(self, path: str) -> Optional[FileInfo]:
        path = path.replace("/", "\\").strip()
        if ":" in path:
            path = path.split(":", 1)[1]
        path = path.lstrip("\\")
        if not path:
            root = self.inodes.get(0)
            return FileInfo(root, "\\", "\\") if root else None

        parts = [p for p in path.split("\\") if p]
        current = 0  # root inode is always 0

        for i, part in enumerate(parts):
            entry = self.lookup(current, part)
            if entry is None:
                return None
            if i == len(parts) - 1:
                ino = self.inodes.get(entry.inode_num)
                if ino is None:
                    return None
                full = "\\" + "\\".join(parts)
                return FileInfo(ino, parts[-1], full)
            dir_ino = self.inodes.get(entry.inode_num)
            if dir_ino is None or not dir_ino.is_dir():
                return None
            current = entry.inode_num
        return None

    def read_file(self, path: str) -> Optional[bytes]:
        info = self.resolve_path(path)
        if info is None or info.inode.is_dir():
            return None
        return self.read_inode_data(info.inode)

    def list_dir(self, path: str) -> Optional[list[dict]]:
        info = self.resolve_path(path)
        if info is None or not info.inode.is_dir():
            return None
        entries = self.readdir(info.inode)
        result = []
        for e in entries:
            ino = self.inodes.get(e.inode_num)
            result.append({
                "name": e.name,
                "type": e.type_str(),
                "inode": e.inode_num,
                "size": ino.size if ino else 0,
                "mode_str": ino.mode_str() if ino else "?",
                "perms": ino.perms_str() if ino else "?",
            })
        return result

    def stat(self, path: str) -> Optional[dict]:
        info = self.resolve_path(path)
        if info is None:
            return None
        return {
            "name": info.name,
            "path": info.path,
            "inode": info.inode.inode_num,
            "type": info.inode.mode_str(),
            "size": info.inode.size,
            "mode": f"0x{info.inode.mode:04X}",
            "perms": info.inode.perms_str(),
            "uid": info.inode.owner_uid,
            "gid": info.inode.owner_gid,
        }

    def dump_superblock(self) -> str:
        return self.superblock.dump() if self.superblock else "(no superblock)"

    def dump_inode_table(self) -> str:
        lines = [f"Inode table ({len(self.inodes)} inodes):"]
        for num in sorted(self.inodes):
            ino = self.inodes[num]
            blocks = [b for b in ino.direct_blocks if b != 0]
            lines.append(
                f"  #{num:3d}  {ino.mode_str():8s}  {ino.perms_str():5s}  "
                f"{ino.size:8d}  blocks={blocks}"
            )
        return "\n".join(lines)
