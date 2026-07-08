"""
parsers/registry_hive.py — NeoDOS Registry Hive (NEOH) Parser.

Read-only parser for NEOH binary hive format matching the kernel's
cell-based hive implementation in neodos-kernel/src/cm/hive.rs and
src/cm/mod.rs.

NEOH Header (16 bytes):
  [0..4)   magic: u32 LE = "NEOH" (0x484F454E)
  [4..8)   version: u32 LE
  [8..12)  entry_count: u32 LE (cells serialized)
  [12..16) checksum: u32 LE (wrapping_add of all cell data)

Cell encoding:
  [0..4)   cell_idx: u32 LE
  [4]      cell_type: u8 (0=Free, 1=Key, 2=Value, 3=Security)

KeyCell (type 1):
  [5..7)   name_len: u16 LE
  [7..7+name_len) name: UTF-8
  [+0]     parent_cell: u32 LE
  [+4]     subkeys_head: u32 LE
  [+8]     subkeys_sibling: u32 LE
  [+12]    values_head: u32 LE
  [+16]    sec_desc_cell: u32 LE (0xFFFFFFFF = NULL)
  [+20]    last_write: u64 LE

ValueCell (type 2):
  [5..7)   name_len: u16 LE
  [7..7+name_len) name: UTF-8
  [+0]     value_type: u32 LE (0=None, 1=REG_SZ, 2=REG_DWORD, 3=REG_BINARY)
  [+4]     data_len: u32 LE
  [+8..8+data_len) data
  [+0]     next: u32 LE

SecurityCell (type 3):
  [5..9)   sd_len: u32 LE
  [9..9+sd_len) sd_data
  [+0]     next: u32 LE
"""

import struct
from dataclasses import dataclass, field
from typing import Optional


# ── Constants ──

NEOH_MAGIC = 0x484F454E  # "NEOH" little-endian

REG_NONE = 0
REG_SZ = 1
REG_DWORD = 2
REG_BINARY = 3

REG_TYPE_NAMES = {
    0: "REG_NONE",
    1: "REG_SZ",
    2: "REG_DWORD",
    3: "REG_BINARY",
}

CELL_FREE = 0
CELL_KEY = 1
CELL_VALUE = 2
CELL_SECURITY = 3

CELL_TYPE_NAMES = {
    0: "Free",
    1: "Key",
    2: "Value",
    3: "Security",
}

NULL_CELL = 0xFFFFFFFF


# ── Data Structures ──

@dataclass
class KeyCell:
    idx: int
    name: str
    parent_cell: int
    subkeys_head: int
    subkeys_sibling: int
    values_head: int
    sec_desc_cell: int
    last_write: int

    def dump(self) -> str:
        return (
            f"KeyCell[#{self.idx}] '{self.name}' "
            f"parent={self.parent_cell} "
            f"subkeys_head={self.subkeys_head} "
            f"sibling={self.subkeys_sibling} "
            f"values_head={self.values_head} "
            f"sec={self.sec_desc_cell} "
            f"write={self.last_write}"
        )


@dataclass
class ValueCell:
    idx: int
    name: str
    value_type: int
    data: bytes
    next: int

    @property
    def type_name(self) -> str:
        return REG_TYPE_NAMES.get(self.value_type, f"type={self.value_type}")

    @property
    def display_value(self) -> str:
        if self.value_type == REG_DWORD and len(self.data) >= 4:
            return str(struct.unpack_from("<I", self.data, 0)[0])
        elif self.value_type == REG_SZ:
            return self.data.decode("ascii", errors="replace").rstrip("\x00")
        elif self.value_type == REG_BINARY:
            if len(self.data) <= 64:
                return self.data.hex()
            return self.data[:64].hex() + f"... ({len(self.data)} bytes)"
        return self.data.hex() if self.data else "(empty)"

    def dump(self) -> str:
        return (
            f"ValueCell[#{self.idx}] '{self.name}' "
            f"type={self.type_name} "
            f"data={self.display_value} "
            f"next={self.next}"
        )


@dataclass
class SecurityCell:
    idx: int
    sd_data: bytes
    next: int

    def dump(self) -> str:
        return (
            f"SecurityCell[#{self.idx}] "
            f"sd_len={len(self.sd_data)} "
            f"next={self.next}"
        )


@dataclass
class RegistryHive:
    version: int
    cells: dict[int, object] = field(default_factory=dict)

    def root_key(self) -> Optional[KeyCell]:
        """Cell 0 is always the root key."""
        cell = self.cells.get(0)
        if isinstance(cell, KeyCell):
            return cell
        return None

    def get_key(self, idx: int) -> Optional[KeyCell]:
        cell = self.cells.get(idx)
        return cell if isinstance(cell, KeyCell) else None

    def get_value(self, idx: int) -> Optional[ValueCell]:
        cell = self.cells.get(idx)
        return cell if isinstance(cell, ValueCell) else None

    def _walk_siblings(self, head: int) -> list[int]:
        """Follow sibling chain, return ordered list of cell indices."""
        result = []
        idx = head
        while idx != NULL_CELL and idx != 0:
            cell = self.cells.get(idx)
            if not isinstance(cell, KeyCell):
                break
            result.append(idx)
            idx = cell.subkeys_sibling
        return result

    def key_children(self, key_idx: int) -> list[KeyCell]:
        """Return ordered list of subkey cells."""
        key = self.get_key(key_idx)
        if key is None:
            return []
        result: list[KeyCell] = []
        for i in self._walk_siblings(key.subkeys_head):
            child = self.get_key(i)
            if child is not None:
                result.append(child)
        return result

    def key_values(self, key_idx: int) -> list[ValueCell]:
        """Return ordered list of value cells."""
        key = self.get_key(key_idx)
        if key is None:
            return []
        result = []
        idx = key.values_head
        while idx != NULL_CELL:
            val = self.get_value(idx)
            if val is None:
                break
            result.append(val)
            idx = val.next
        return result

    def find_key(self, start_idx: int, name: str) -> Optional[KeyCell]:
        """Find subkey by name (case-insensitive) under start_idx."""
        key = self.get_key(start_idx)
        if key is None:
            return None
        idx = key.subkeys_head
        while idx != NULL_CELL:
            child = self.get_key(idx)
            if child and child.name.upper() == name.upper():
                return child
            idx = child.subkeys_sibling if child else NULL_CELL
        return None

    def open_key_by_path(self, path: str) -> Optional[KeyCell]:
        """Walk a registry path like '\\Registry\\Machine\\System'."""
        path = path.replace("/", "\\").strip("\\")
        if not path:
            return self.root_key()

        parts = [p for p in path.split("\\") if p]
        current = 0  # root is always cell 0

        for part in parts:
            key = self.find_key(current, part)
            if key is None:
                return None
            current = key.idx
        return self.get_key(current)

    def dump_key_tree(self, key_idx: int = 0, indent: str = "") -> str:
        """Recursive tree dump starting from key_idx."""
        key = self.get_key(key_idx)
        if key is None:
            return "(not found)"

        lines = []
        values = self.key_values(key_idx)
        for v in values:
            lines.append(f"{indent}  {v.name} = {v.display_value}  [{v.type_name}]")

        children = self.key_children(key_idx)
        for i, child in enumerate(children):
            prefix = "└── " if i == len(children) - 1 else "├── "
            lines.append(f"{indent}{prefix}{child.name}/")
            sub_prefix = "    " if i == len(children) - 1 else "│   "
            sub = self.dump_key_tree(child.idx, indent + sub_prefix)
            if sub:
                lines.append(sub)

        return "\n".join(lines)


# ── Parser ──

def parse_hive(data: bytes) -> Optional[RegistryHive]:
    """Parse a NEOH binary hive buffer."""
    if len(data) < 16:
        return None

    magic, version, entry_count, checksum = struct.unpack_from("<IIII", data, 0)
    if magic != NEOH_MAGIC:
        return None

    hive = RegistryHive(version=version)
    offset = 16
    verified_checksum = 0

    for _ in range(entry_count):
        if offset + 5 > len(data):
            break
        cell_idx = struct.unpack_from("<I", data, offset)[0]
        cell_type = data[offset + 4]
        offset += 5
        verified_checksum += cell_idx + cell_type

        if cell_type == CELL_KEY:
            if offset + 2 > len(data):
                break
            name_len = struct.unpack_from("<H", data, offset)[0]
            offset += 2
            name_bytes = data[offset:offset + name_len]
            offset += name_len
            name = name_bytes.decode("utf-8", errors="replace").rstrip("\x00")

            if offset + 24 > len(data):
                break
            (parent_cell, subkeys_head, subkeys_sibling,
             values_head, sec_desc_cell) = struct.unpack_from("<IIIII", data, offset)
            offset += 20
            last_write = struct.unpack_from("<Q", data, offset)[0]
            offset += 8
            verified_checksum += name_len + parent_cell + subkeys_head + subkeys_sibling + values_head + sec_desc_cell + (last_write & 0xFFFFFFFF) + ((last_write >> 32) & 0xFFFFFFFF)

            hive.cells[cell_idx] = KeyCell(
                idx=cell_idx, name=name,
                parent_cell=parent_cell,
                subkeys_head=subkeys_head,
                subkeys_sibling=subkeys_sibling,
                values_head=values_head,
                sec_desc_cell=sec_desc_cell,
                last_write=last_write,
            )

        elif cell_type == CELL_VALUE:
            if offset + 2 > len(data):
                break
            name_len = struct.unpack_from("<H", data, offset)[0]
            offset += 2
            name_bytes = data[offset:offset + name_len]
            offset += name_len
            name = name_bytes.decode("utf-8", errors="replace").rstrip("\x00")

            if offset + 8 > len(data):
                break
            value_type, data_len = struct.unpack_from("<II", data, offset)
            offset += 8
            value_data = data[offset:offset + data_len]
            offset += data_len
            if offset + 4 > len(data):
                break
            next_cell = struct.unpack_from("<I", data, offset)[0]
            offset += 4
            verified_checksum += name_len + value_type + data_len + next_cell

            hive.cells[cell_idx] = ValueCell(
                idx=cell_idx, name=name,
                value_type=value_type,
                data=value_data,
                next=next_cell,
            )

        elif cell_type == CELL_SECURITY:
            if offset + 4 > len(data):
                break
            sd_len = struct.unpack_from("<I", data, offset)[0]
            offset += 4
            sd_data = data[offset:offset + sd_len]
            offset += sd_len
            if offset + 4 > len(data):
                break
            next_cell = struct.unpack_from("<I", data, offset)[0]
            offset += 4
            verified_checksum += sd_len + next_cell

            hive.cells[cell_idx] = SecurityCell(
                idx=cell_idx,
                sd_data=sd_data,
                next=next_cell,
            )

        # Skip Free cells — they have no payload after type byte
        # (Free cells are not serialized in practice)

    return hive


def parse_hive_from_vfs(vfs_img, hive_name: str) -> Optional[RegistryHive]:
    """Load and parse a hive file from a NeoDOS filesystem image."""
    path = f"\\System\\Registry\\{hive_name.upper()}.HIV"
    data = vfs_img.read_file(path)
    if data is None:
        return None
    return parse_hive(data)
