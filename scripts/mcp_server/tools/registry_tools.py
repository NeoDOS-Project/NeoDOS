"""
tools/registry_tools.py — NeoDOS Registry Hive Analysis Tools.

Provides read-only access to NeoDOS registry hives stored in
C:\\System\\Registry\\*.HIV on the NeoDOS filesystem image.

Uses the VFS layer to read hive files, then parses the NEOH binary
format to navigate keys and values.
"""

from pathlib import Path
from typing import Optional
from ..parsers.registry_hive import (
    RegistryHive, parse_hive_from_vfs,
    KeyCell, ValueCell, REG_TYPE_NAMES,
)


# ── Configuration ──

NEODOS_ROOT: Optional[Path] = None

# Reuse VFS image cache (we import the _get_image function)
_vfs_tools = None


def configure(root_dir: str):
    global NEODOS_ROOT, _vfs_tools
    NEODOS_ROOT = Path(root_dir)
    from . import vfs_tools
    _vfs_tools = vfs_tools


def _get_vfs_image(drive: str = "C"):
    """Get the VFS image parser for a drive letter."""
    if _vfs_tools is None:
        return None
    try:
        return _vfs_tools._get_image(drive)
    except Exception:
        return None


def _format_value(v: ValueCell) -> str:
    """Format a value cell for display."""
    type_name = REG_TYPE_NAMES.get(v.value_type, f"type={v.value_type}")

    if v.value_type == 2 and len(v.data) >= 4:  # REG_DWORD
        import struct
        display = str(struct.unpack_from("<I", v.data, 0)[0])
    elif v.value_type == 1:  # REG_SZ
        display = v.data.decode("ascii", errors="replace").rstrip("\x00")
    elif v.value_type == 3:  # REG_BINARY
        if len(v.data) <= 64:
            display = v.data.hex()
        else:
            display = v.data[:64].hex() + f"... ({len(v.data)} bytes)"
    elif v.value_type == 0:
        display = "(empty)"
    else:
        display = v.data.hex() if v.data else "(empty)"

    return f"{v.name:<32s}  {type_name:<12s}  {display}"


# ── Tool Implementations ──

def registry_list(key_path: str = "\\", hive: str = "SYSTEM", drive: str = "C") -> str:
    """List registry key contents: subkeys and values."""
    img = _get_vfs_image(drive)
    if img is None:
        return "Error: Could not open NeoDOS filesystem image."

    h = parse_hive_from_vfs(img, hive)
    if h is None:
        return f"Error: Hive '{hive}' not found at \\System\\Registry\\{hive.upper()}.HIV"

    key = h.open_key_by_path(key_path)
    if key is None:
        return f"Error: Key '{key_path}' not found in {hive} hive."

    lines = [f"Registry: {hive} hive  [{h.version}]"]
    lines.append(f"Key: {key_path}")
    lines.append(f"  Subkeys: {len(h.key_children(key.idx))}  Values: {len(h.key_values(key.idx))}")
    lines.append("")

    children = h.key_children(key.idx)
    if children:
        lines.append("  Subkeys:")
        for child in children:
            c_count = len(h.key_children(child.idx))
            v_count = len(h.key_values(child.idx))
            lines.append(f"    {child.name:<32s}  [{c_count} subkeys, {v_count} values]")

    values = h.key_values(key.idx)
    if values:
        lines.append("")
        lines.append("  Values:")
        for v in values:
            lines.append("    " + _format_value(v))

    if not children and not values:
        lines.append("  (empty key)")

    return "\n".join(lines)


def registry_query(key_path: str = "\\", value_name: Optional[str] = None, hive: str = "SYSTEM", drive: str = "C") -> str:
    """Query a specific registry value by name."""
    img = _get_vfs_image(drive)
    if img is None:
        return "Error: Could not open NeoDOS filesystem image."

    h = parse_hive_from_vfs(img, hive)
    if h is None:
        return f"Error: Hive '{hive}' not found."

    key = h.open_key_by_path(key_path)
    if key is None:
        return f"Error: Key '{key_path}' not found in {hive} hive."

    if value_name is None:
        # Show all values
        values = h.key_values(key.idx)
        if not values:
            return f"No values under {key_path}"
        lines = [f"Values under {hive}:{key_path}"]
        for v in values:
            lines.append("  " + _format_value(v))
        return "\n".join(lines)

    # Find specific value by name
    for v in h.key_values(key.idx):
        if v.name.upper() == value_name.upper():
            return _format_value(v)

    return f"Error: Value '{value_name}' not found under {key_path}."


def registry_tree(key_path: str = "\\", hive: str = "SYSTEM", drive: str = "C") -> str:
    """Recursive registry key tree dump."""
    img = _get_vfs_image(drive)
    if img is None:
        return "Error: Could not open NeoDOS filesystem image."

    h = parse_hive_from_vfs(img, hive)
    if h is None:
        return f"Error: Hive '{hive}' not found."

    key = h.open_key_by_path(key_path)
    if key is None:
        return f"Error: Key '{key_path}' not found in {hive} hive."

    result = h.dump_key_tree(key.idx)
    return f"Registry Tree: {hive}:{key_path}\n\n{result}"


def registry_hive_info(hive: str = "SYSTEM", drive: str = "C") -> str:
    """Show information about a registry hive: version, cell count, root key."""
    img = _get_vfs_image(drive)
    if img is None:
        return "Error: Could not open NeoDOS filesystem image."

    h = parse_hive_from_vfs(img, hive)
    if h is None:
        return f"Error: Hive '{hive}' not found at \\System\\Registry\\{hive.upper()}.HIV"

    root = h.root_key()
    child_count = len(h.key_children(0)) if root else 0

    # Count cells by type
    from ..parsers.registry_hive import KeyCell as KC, ValueCell as VC, SecurityCell as SC
    key_count = sum(1 for c in h.cells.values() if isinstance(c, KC))
    val_count = sum(1 for c in h.cells.values() if isinstance(c, VC))
    sec_count = sum(1 for c in h.cells.values() if isinstance(c, SC))

    lines = [
        f"Hive:        {hive.upper()}",
        f"Format:      NEOH v{h.version}",
        f"Total cells: {len(h.cells)}",
        f"  Key cells:    {key_count}",
        f"  Value cells:  {val_count}",
        f"  Security:     {sec_count}",
    ]

    if root:
        lines.append(f"Root key:    '{root.name}' ({child_count} direct subkeys)")

    return "\n".join(lines)
