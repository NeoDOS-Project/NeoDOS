"""
tools/vfs_tools.py — NeoDOS VFS Analysis Tools.

Provides read-only access to NeoDOS filesystem images via VFS abstraction.
All filesystem operations go through the VFS layer (NeoDosFsImage parser),
never accessing raw disk sectors directly.
"""

import os
from pathlib import Path
from ..parsers.neodos_fs import NeoDosFsImage


# ── Configuration ──

NEODOS_ROOT: Path = None
_IMAGES: dict[str, NeoDosFsImage] = {}  # drive letter -> image parser


def configure(root_dir: str):
    global NEODOS_ROOT
    NEODOS_ROOT = Path(root_dir)
    _IMAGES.clear()


def _get_image(drive_letter: str = "C") -> NeoDosFsImage:
    """Get or cache the image parser for a drive letter."""
    drive = drive_letter.upper()
    if drive in _IMAGES:
        return _IMAGES[drive]

    # Determine image path based on drive letter
    if drive == "C":
        candidates = [
            NEODOS_ROOT / "scripts" / "neodos_image.img",
            NEODOS_ROOT / "disk_image.img",
        ]
        # Also try extracting from disk_image.img
        disk_img = NEODOS_ROOT / "disk_image.img"
        if disk_img.exists() and not candidates[0].exists():
            # Try to extract NeoDOS partition from GPT disk image
            extracted = _extract_neodos_partition(str(disk_img))
            if extracted:
                _IMAGES[drive] = extracted
                return extracted
    elif drive == "D":
        candidates = [
            NEODOS_ROOT / "scripts" / "neodos_image2.img",
        ]
    else:
        candidates = []

    for img_path in candidates:
        if img_path.exists():
            try:
                img = NeoDosFsImage(str(img_path))
                _IMAGES[drive] = img
                return img
            except ValueError as e:
                continue

    raise FileNotFoundError(
        f"No NeoDOS image found for drive {drive}.\n"
        f"Searched: {[str(p) for p in candidates]}"
    )


def _extract_neodos_partition(disk_img_path: str) -> Optional[NeoDosFsImage]:
    """Extract the NeoDOS partition from a GPT disk image.
    GPT: Partition 2 (NeoDOS FS) starts at LBA ~206848.
    """
    import struct
    SECTOR_SIZE = 512
    GPT_PART_ENTRY_SIZE = 128
    GPT_FIRST_ENTRY_LBA = 2  # LBA 2 for GPT partition entries

    try:
        with open(disk_img_path, "rb") as f:
            # Read protective MBR
            mbr = f.read(SECTOR_SIZE)
            if len(mbr) < SECTOR_SIZE:
                return None

            # Read GPT header at LBA 1
            f.seek(SECTOR_SIZE)
            gpt_hdr = f.read(92)
            if len(gpt_hdr) < 92:
                return None

            # Parse GPT header for partition entry start
            part_ent_lba = struct.unpack_from("<Q", gpt_hdr, 72)[0]
            num_part_ents = struct.unpack_from("<I", gpt_hdr, 80)[0]
            part_ent_size = struct.unpack_from("<I", gpt_hdr, 84)[0]

            if part_ent_size != GPT_PART_ENTRY_SIZE:
                return None

            # NeoDOS partition GUID: EBD0A0A2-B9E5-4433-87C0-68B6B72699C7
            neodos_guid = bytes.fromhex("A2A0D0EBE5B9334487C068B6B72699C7")

            # Scan partitions from LBA 2
            f.seek(part_ent_lba * SECTOR_SIZE)
            for i in range(min(num_part_ents, 128)):
                entry = f.read(GPT_PART_ENTRY_SIZE)
                if len(entry) < GPT_PART_ENTRY_SIZE:
                    break
                part_type_guid = entry[0:16]
                if part_type_guid == neodos_guid:
                    start_lba = struct.unpack_from("<Q", entry, 32)[0]
                    end_lba = struct.unpack_from("<Q", entry, 40)[0]
                    # Extract partition data
                    part_size = (end_lba - start_lba + 1) * SECTOR_SIZE
                    f.seek(start_lba * SECTOR_SIZE)
                    part_data = f.read(part_size)
                    # Create a temporary file-like object
                    # Since NeoDosFsImage expects a file path, write to temp
                    import tempfile
                    tmp = tempfile.NamedTemporaryFile(delete=False, suffix=".neodos")
                    tmp.write(part_data)
                    tmp.close()
                    img = NeoDosFsImage(tmp.name)
                    # Clean up temp file after parsing (data is in memory)
                    try:
                        os.unlink(tmp.name)
                    except:
                        pass
                    return img

            return None
    except Exception as e:
        return None


# ── Tool Implementations ──

def vfs_list(path: str = "\\", drive: str = "C") -> str:
    """List directory contents from NeoDOS filesystem through VFS."""
    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"

    # Normalize path
    if ":" in path:
        path = path.split(":", 1)[1]
    path = path.replace("/", "\\").rstrip("\\") or "\\"

    entries = img.list_dir(path)
    if entries is None:
        return f"VFS Error: '{path}' not found on {drive}:"

    # Format as DOS DIR output
    lines = [f" Directory of {drive}:{path}"]
    lines.append("")
    total_files = 0
    total_dirs = 0
    for e in entries:
        if e["type"] == "deleted":
            continue
        if e["type"] == "dir":
            total_dirs += 1
            lines.append(f"  {e['name']:20s}  <DIR>          {e['perms']}")
        else:
            total_files += 1
            lines.append(f"  {e['name']:20s}  {e['size']:>8d}  {e['perms']}")
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

    # Try text, else show hex
    try:
        text = data.decode("ascii")
        if text and text[-1] == "\n":
            text = text[:-1]
        return text
    except UnicodeDecodeError:
        # Show hex dump
        lines = [f"Binary file: {len(data)} bytes", ""]
        for i in range(0, min(len(data), 1024), 16):
            chunk = data[i:i+16]
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
        f"Inode:  {info['inode']}",
        f"Mode:   {info['mode']}",
        f"Perms:  {info['perms']}",
        f"UID:    {info['uid']}",
        f"GID:    {info['gid']}",
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
            f"  Inode:  {info.inode.inode_num}",
            f"  Type:   {info.inode.mode_str()}",
            f"  Size:   {info.inode.size} bytes",
        ])

    # Fallback search: try all filesystem drives
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
                    f"  Inode:  {alt_info.inode.inode_num}",
                    f"  Type:   {alt_info.inode.mode_str()}",
                    f"  Size:   {alt_info.inode.size} bytes",
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
    """Dump NeoDOS inode table."""
    try:
        img = _get_image(drive)
    except FileNotFoundError as e:
        return f"VFS Error: {e}"
    return img.dump_inode_table()


def vfs_tree(path: str = "\\", drive: str = "C", indent: str = "") -> str:
    """Recursive directory tree listing."""
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
    dirs = [e for e in entries if e["type"] == "dir" and e["name"] not in (".", "..")]
    files = [e for e in entries if e["type"] == "file"]

    for e in files:
        lines.append(f"{indent}├── {e['name']}  ({e['size']} bytes)")

    for i, d in enumerate(dirs):
        prefix = "└── " if i == len(dirs) - 1 and not files else "├── "
        lines.append(f"{indent}{prefix}{d['name']}/")
        child_prefix = "    " if i == len(dirs) - 1 else "│   "
        sub = vfs_tree(f"{path}\\{d['name']}", drive, indent + child_prefix)
        lines.append(sub)

    return "\n".join(lines)
