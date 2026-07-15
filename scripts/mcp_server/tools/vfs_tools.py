"""
tools/vfs_tools.py — NeoDOS VFS Analysis Tools.

Provides read-only access to NeoDOS filesystem images via VFS abstraction.
NeoFS v1 (NEOD) has been removed. NeoFS v2 (NE2) is the only native format.
"""

import os
from pathlib import Path
from typing import Optional
from ..parsers.neodos_v2_fs import NeoDosFsV2Image, open_image, SuperblockNE2


NEODOS_ROOT: Optional[Path] = None
_IMAGES: dict[str, NeoDosFsV2Image] = {}
_IMAGE_MTIMES: dict[str, float] = {}  # mtime-based cache invalidation


def configure(root_dir: str):
    global NEODOS_ROOT
    NEODOS_ROOT = Path(root_dir)
    _IMAGES.clear()
    _IMAGE_MTIMES.clear()


def _get_image(drive_letter: str = "C") -> NeoDosFsV2Image:
    """Get or cache the image parser for a drive letter.

    Invalidates cache if the image file's mtime has changed.
    """
    drive = drive_letter.upper()

    # Check cache with mtime invalidation
    if drive in _IMAGES:
        cached_mtime = _IMAGE_MTIMES.get(drive)
        if cached_mtime is not None:
            current_mtime = _get_image_mtime(drive)
            if current_mtime is not None and current_mtime == cached_mtime:
                return _IMAGES[drive]
            else:
                # Image changed, invalidate
                del _IMAGES[drive]
                _IMAGE_MTIMES.pop(drive, None)

    if NEODOS_ROOT is None:
        raise FileNotFoundError("NEODOS_ROOT not configured")

    candidates = []
    if drive == "C":
        candidates = [
            NEODOS_ROOT / "scripts" / "neodos_image.img",
            NEODOS_ROOT / "disk_image.img",
        ]
        disk_img = NEODOS_ROOT / "disk_image.img"
        if disk_img.exists() and not candidates[0].exists():
            extracted = _extract_neodos_partition(str(disk_img))
            if extracted:
                _IMAGES[drive] = extracted
                _IMAGE_MTIMES[drive] = disk_img.stat().st_mtime
                return extracted
    elif drive == "D":
        candidates = [
            NEODOS_ROOT / "scripts" / "neodos_image2.img",
        ]

    for img_path in candidates:
        if img_path.exists():
            try:
                img = open_image(str(img_path))
                _IMAGES[drive] = img
                _IMAGE_MTIMES[drive] = img_path.stat().st_mtime
                return img
            except ValueError:
                continue

    raise FileNotFoundError(
        f"No NeoDOS image found for drive {drive}.\n"
        f"Searched: {[str(p) for p in candidates]}"
    )


def _get_image_mtime(drive: str) -> Optional[float]:
    """Get current mtime of the image file for a drive."""
    if NEODOS_ROOT is None:
        return None
    candidates = []
    if drive == "C":
        candidates = [
            NEODOS_ROOT / "scripts" / "neodos_image.img",
            NEODOS_ROOT / "disk_image.img",
        ]
    elif drive == "D":
        candidates = [NEODOS_ROOT / "scripts" / "neodos_image2.img"]
    for p in candidates:
        if p.exists():
            return p.stat().st_mtime
    return None


def _extract_neodos_partition(disk_img_path: str) -> Optional[NeoDosFsV2Image]:
    """Extract the NeoDOS partition from a GPT disk image."""
    import struct
    import tempfile

    try:
        with open(disk_img_path, "rb") as f:
            mbr = f.read(512)
            if len(mbr) < 512:
                return None
            f.seek(512)
            gpt_hdr = f.read(92)
            if len(gpt_hdr) < 92:
                return None
            part_ent_lba = struct.unpack_from("<Q", gpt_hdr, 72)[0]
            num_part_ents = struct.unpack_from("<I", gpt_hdr, 80)[0]
            part_ent_size = struct.unpack_from("<I", gpt_hdr, 84)[0]
            neodos_guid = bytes.fromhex("A2A0D0EBE5B9334487C068B6B72699C7")
            f.seek(part_ent_lba * 512)
            for i in range(min(num_part_ents, 128)):
                entry = f.read(part_ent_size)
                if len(entry) < part_ent_size:
                    break
                if entry[0:16] == neodos_guid:
                    start_lba = struct.unpack_from("<Q", entry, 32)[0]
                    end_lba = struct.unpack_from("<Q", entry, 40)[0]
                    part_size = (end_lba - start_lba + 1) * 512
                    f.seek(start_lba * 512)
                    part_data = f.read(part_size)
                    tmp = tempfile.NamedTemporaryFile(delete=False, suffix=".neodos")
                    tmp.write(part_data)
                    tmp.close()
                    try:
                        img = open_image(tmp.name)
                        os.unlink(tmp.name)
                        return img
                    except ValueError:
                        os.unlink(tmp.name)
                        return None
        return None
    except Exception:
        return None


# ── Tool Implementations ──


def vfs_list(path: str = "\\", drive: str = "C") -> str:
    """List directory contents from NeoDOS filesystem through VFS."""
    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"

    if ":" in path:
        path = path.split(":", 1)[1]
    path = path.replace("/", "\\").rstrip("\\") or "\\"

    entries = img.list_dir(path)
    if entries is None:
        return f"VFS Error: '{path}' not found on {drive}:"

    lines = [f" Directory of {drive}:{path}"]
    lines.append("")
    total_files = 0
    total_dirs = 0
    for e in entries:
        if e["type"] == "dir":
            total_dirs += 1
            lines.append(f"  {e['name']:20s}  <DIR>          {e['mode_str']}")
        else:
            total_files += 1
            lines.append(f"  {e['name']:20s}  {e['size']:>8d}  {e['mode_str']}")
    lines.append("")
    lines.append(f"  {total_files:>3d} File(s)")
    lines.append(f"  {total_dirs:>3d} Dir(s)")
    return "\n".join(lines)


def vfs_read(path: str, drive: str = "C") -> str:
    """Read a file from NeoDOS filesystem through VFS."""
    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"

    if ":" in path:
        path = path.split(":", 1)[1]
    path = path.replace("/", "\\").rstrip("\\")

    data = img.read_file(path)
    if data is None:
        return f"VFS Error: '{path}' not found on {drive}:"

    try:
        text = data.decode("ascii")
        if text and text[-1] == "\n":
            text = text[:-1]
        return text
    except UnicodeDecodeError:
        lines = [f"Binary file: {len(data)} bytes", ""]
        for i in range(0, min(len(data), 1024), 16):
            chunk = data[i:i + 16]
            hex_part = " ".join(f"{b:02x}" for b in chunk)
            ascii_part = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
            lines.append(f"  {i:06x}  {hex_part:48s}  {ascii_part}")
        if len(data) > 1024:
            lines.append(f"  ... ({len(data) - 1024} more bytes)")
        return "\n".join(lines)


def vfs_stat(path: str, drive: str = "C") -> str:
    """Get file/directory metadata from NeoDOS FS through VFS."""
    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"

    if ":" in path:
        path = path.split(":", 1)[1]
    path = path.replace("/", "\\").rstrip("\\")

    info = img.stat(path)
    if info is None:
        return f"VFS Error: '{path}' not found on {drive}:"

    return "\n".join([
        f"Path:   {drive}:{info['path']}",
        f"Name:   {info['name']}",
        f"Type:   {info['type']}",
        f"Size:   {info['size']} bytes",
        f"Mode:   {info['mode_str']}",
    ])


def vfs_resolve(path: str, drive: str = "C") -> str:
    """Resolve a path through VFS, handling fallback search."""
    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"

    if ":" in path:
        drive_letter, _, rest = path.partition(":")
        path = rest
        return vfs_resolve(path, drive_letter)

    path = path.replace("/", "\\").rstrip("\\")
    info = img.resolve_path(path)
    if info:
        return "\n".join([
            f"Resolved: {drive}:{info.path}",
            f"  Type:   {info.mode_str()}",
            f"  Size:   {info.size} bytes",
        ])

    for d in ["C", "D", "A", "B"]:
        if d == drive:
            continue
        try:
            alt_img = _get_image(d)
            fname = path.split("\\")[-1] if "\\" in path else path
            alt_info = alt_img.resolve_path(fname)
            if alt_info:
                return "\n".join([
                    f"Resolved via fallback: {d}:{alt_info.path}",
                    f"  (original path '{path}' not found on {drive}:)",
                    f"  Type:   {alt_info.mode_str()}",
                    f"  Size:   {alt_info.size} bytes",
                ])
        except FileNotFoundError:
            continue

    return f"VFS Error: '{path}' not found on any drive"


def vfs_dump_superblock(drive: str = "C") -> str:
    """Dump NeoDOS filesystem superblock."""
    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"
    return img.dump_superblock()


def vfs_dump_inodes(drive: str = "C") -> str:
    return "NeoFS v2: uses B-tree directories (no fixed inode table)"


def vfs_tree(path: str = "\\", drive: str = "C", max_depth: int = 8, _depth: int = 0, _indent: str = "") -> str:
    """Recursive directory tree listing with configurable depth limit.

    Args:
        path: Root directory path.
        drive: Drive letter (C, D, A).
        max_depth: Maximum recursion depth (default 8, 0 = no limit).
    """
    if _depth >= max_depth > 0:
        return f"{_indent}└── ... (max depth {max_depth} reached)"

    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"

    if ":" in path:
        path = path.split(":", 1)[1]
    path = path.replace("/", "\\").rstrip("\\") or "\\"

    entries = img.list_dir(path)
    if entries is None:
        return f"VFS Error: '{path}' not found"

    lines = []
    dirs = [e for e in entries if e["type"] == "dir"]
    files = [e for e in entries if e["type"] == "file"]

    for e in files:
        lines.append(f"{_indent}├── {e['name']}  ({e['size']} bytes)")

    for i, d in enumerate(dirs):
        prefix = "└── " if i == len(dirs) - 1 and not files else "├── "
        lines.append(f"{_indent}{prefix}{d['name']}/")
        child_prefix = "    " if i == len(dirs) - 1 else "│   "
        sub = vfs_tree(f"{path}\\{d['name']}", drive, max_depth,
                       _depth + 1, _indent + child_prefix)
        lines.append(sub if sub else "")

    return "\n".join(lines)
