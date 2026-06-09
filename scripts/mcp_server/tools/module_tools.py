"""
tools/module_tools.py — NeoDOS Runtime Module Analysis Tools.

Analyzes NEM v3 driver binaries and ELF DLLs offline.
All access goes through VFS (NeoDOS image) or direct file read
for pre-built driver/DLL binaries.
"""

from pathlib import Path
from ..parsers.nem_parser import parse_nem_v3, parse_nem_file, analyze_nem_driver, HST_EXPORTS
from ..parsers.elf_parser import parse_elf_file, analyze_elf


# ── Configuration ──

NEODOS_ROOT: Path = None
_CACHED_DRIVERS: dict[str, dict] = {}
_CACHED_DLLS: dict[str, dict] = {}


def configure(root_dir: str):
    global NEODOS_ROOT
    NEODOS_ROOT = Path(root_dir)
    _CACHED_DRIVERS.clear()
    _CACHED_DLLS.clear()


# ── Driver paths ──

def _driver_paths() -> list[Path]:
    """Return all known NEM driver file paths."""
    paths = []
    # Build output directory
    drv_out = NEODOS_ROOT / "drivers" / "target" / "x86_64-unknown-none" / "release"
    if drv_out.exists():
        paths.extend(drv_out.glob("*.nem"))

    # Also check project root for any .nem files
    paths.extend(NEODOS_ROOT.glob("*.nem"))

    # Check in drivers/ subdirectories
    for d in (NEODOS_ROOT / "drivers").iterdir():
        if d.is_dir():
            paths.extend(d.glob("*.nem"))
            paths.extend(d.glob("target/**/*.nem"))

    return sorted(set(p for p in paths if p.exists()))


def _dll_paths() -> list[Path]:
    """Return all known NXL file paths."""
    paths = []
    # Project root
    paths.extend(NEODOS_ROOT.glob("*.nxl"))
    # Userbin outputs
    ub = NEODOS_ROOT / "userbin"
    for d in ub.iterdir():
        if d.is_dir():
            paths.extend(d.glob("target/**/*.nxl"))
    return sorted(set(p for p in paths if p.exists()))


# ── Tool Implementations ──

def list_loaded_modules(category: str = "all") -> str:
    """List NEM drivers and DLLs found in build artifacts."""
    drivers = _driver_paths()
    dlls = _dll_paths()

    lines = [f"Runtime Modules:", ""]

    if category in ("all", "nem", "driver"):
        lines.append(f"NEM v3 Drivers ({len(drivers)} found):")
        if drivers:
            for p in drivers:
                try:
                    info = analyze_nem_driver(str(p))
                    if info:
                        compat = "✓" if info["abi_compatible"] else "✗"
                        lines.append(
                            f"  [{compat}] {info['name']:15s} "
                            f"{info['category']:7s} "
                            f"text={info['memory']['text']} "
                            f"data={info['memory']['data']} "
                            f"bss={info['memory']['bss']}  "
                            f"syms={info['num_symbols']} relocs={info['num_relocs']}"
                        )
                    else:
                        lines.append(f"  [?] {p.name} (parse failed)")
                except Exception as e:
                    lines.append(f"  [E] {p.name}: {e}")
        else:
            lines.append("  (none found — run build.sh --neodos-image first)")

    if category in ("all", "dll"):
        lines.append(f"\nNXLs ({len(dlls)} found):")
        if dlls:
            for p in dlls:
                try:
                    info = analyze_elf(str(p))
                    if info:
                        sections = info.get('sections', [])
                        custom_sections = ', '.join(s['name'] for s in sections if s['name'] not in ('.text', '.comment', '.symtab', '.shstrtab', '.strtab'))
                        lines.append(
                            f"  {p.name:25s} type={info['type']} "
                            f"segs={info['num_segments']} "
                            f"exports={len(info['exports'])}"
                            f"{'  [' + custom_sections + ']' if custom_sections else ''}"
                        )
                    else:
                        lines.append(f"  [?] {p.name} (parse failed)")
                except Exception as e:
                    lines.append(f"  [E] {p.name}: {e}")
        else:
            lines.append("  (none found)")

    return "\n".join(lines)


def get_module_symbols(name: str, format: str = "detailed") -> str:
    """Get symbols from a NEM driver or DLL by name (partial match)."""
    # Search NEM drivers
    drivers = _driver_paths()
    for p in drivers:
        try:
            data = open(p, "rb").read()
            drv = parse_nem_v3(data)
            if drv and (name.upper() in drv.name.upper()):
                if format == "detailed":
                    return drv.dump()
                else:
                    # Compact
                    lines = [f"NEM Driver: {drv.name}"]
                    for sym in drv.symbols:
                        sname = drv._get_str(sym.name_off)
                        if sname:
                            lines.append(f"  {sname}")
                    unresolved = drv.unresolved_symbols()
                    if unresolved:
                        lines.append(f"\nUnresolved: {', '.join(unresolved)}")
                    return "\n".join(lines)
        except:
            continue

    # Search DLLs
    dlls = _dll_paths()
    for p in dlls:
        try:
            elf = parse_elf_file(str(p))
            if elf:
                exports = [e["name"] for e in elf.exports()]
                if name.upper() in p.stem.upper():
                    if format == "detailed":
                        return elf.dump()
                    else:
                        return "\n".join([
                            f"DLL: {p.name}",
                            f"Entry: 0x{elf.entry_point:016X}",
                            "",
                            f"Exports ({len(exports)}):",
                        ] + [f"  {e}" for e in exports])
        except:
            continue

    return f"No module found matching '{name}'"


def sys_loadlib_analyze(path: str) -> str:
    """Analyze what sys_loadlib would do with a given path.
    Does NOT actually load — validates format, ABI, and dependencies."""
    # Try as direct file path first
    file_path = Path(path)
    if not file_path.exists():
        file_path = NEODOS_ROOT / path
    if not file_path.exists():
        file_path = NEODOS_ROOT / "userbin" / path
    if not file_path.exists():
        return f"Cannot find '{path}'"

    data = open(file_path, "rb").read()

    # Detect format
    if data[:4] == b"\x7fELF":
        return _analyze_elf_load(data, file_path)
    elif data[:4] == b"NEM3":
        return _analyze_nem_load(data, file_path)
    else:
        return f"Unknown format for '{file_path.name}' (not ELF or NEM3)"


def _analyze_elf_load(data: bytes, file_path: Path) -> str:
    from ..parsers.elf_parser import parse_elf
    elf = parse_elf(data)
    if elf is None:
        return f"Invalid ELF: {file_path.name}"

    lines = [
        f"sys_loadlib analysis for '{file_path.name}':",
        f"  Format:       ELF64 {'NXL' if elf.is_dll else 'EXEC'}",
        f"  Entry point:  0x{elf.entry_point:016X}",
        f"  Segments:     {len(elf.segments)}",
        f"  Exports:      {len(elf.exports())}",
        f"  Vaddr range:  {elf.segments[0].p_vaddr if elf.segments else 0:#x} – "
        f"{elf.segments[-1].p_vaddr + elf.segments[-1].p_memsz if elf.segments else 0:#x}",
        "",
        "Verdict: Can be loaded via sys_loadlib (RAX=21)",
        f"  → Would load into DLL region (0x1E000000, 8×256KB slots)",
        f"  → ELF segments would be identity-mapped at {elf.segments[0].p_vaddr:#x} if base=0",
        f"  → Pages marked USER_ACCESSIBLE + READ_ONLY",
    ]

    if elf.is_dll:
        exports = elf.exports()
        if exports:
            lines.append(f"\n  Export table ({len(exports)} symbols at base):")
            for e in exports[:20]:
                lines.append(f"    {e['name']}")
            if len(exports) > 20:
                lines.append(f"    ... and {len(exports) - 20} more")

    return "\n".join(lines)


def _analyze_nem_load(data: bytes, file_path: Path) -> str:
    drv = parse_nem_v3(data)
    if drv is None:
        return f"Invalid NEM: {file_path.name}"

    lines = [
        f"sys_loadlib analysis for '{file_path.name}':",
        f"  Format:       NEM v3 Driver",
        f"  Name:         {drv.name}",
        f"  Type:         {drv.driver_type_name}",
        f"  Category:     {drv.category_name}",
        f"  ABI:          {'COMPATIBLE' if drv.abi_compatible else 'INCOMPATIBLE'} "
        f"(min={drv.header.abi_min} target={drv.header.abi_target} max={drv.header.abi_max})",
        f"  Memory:       text={drv.header.text_size} rodata={drv.header.rodata_size} "
        f"data={drv.header.data_size} bss={drv.bss_size}",
        f"  Entry points: init=0x{drv.header.entry_init:08X} "
        f"event=0x{drv.header.entry_event:08X}",
        f"  Symbols:      {len(drv.symbols)}",
        f"  Relocations:  {len(drv.relocs)}",
        "",
    ]

    if drv.abi_warnings:
        for w in drv.abi_warnings:
            lines.append(f"  ⚠ {w}")

    unresolved = drv.unresolved_symbols()
    hst_needed = [s for s in unresolved if s.startswith("hst_")]
    other_needed = [s for s in unresolved if not s.startswith("hst_")]
    if hst_needed:
        lines.append(f"\n  HST exports required ({len(hst_needed)}):")
        lines.append(f"    {', '.join(hst_needed)}")
        missing = [s for s in hst_needed if s not in HST_EXPORTS]
        if missing:
            lines.append(f"  ⚠ UNKNOWN HST exports: {', '.join(missing)}")
    if other_needed:
        lines.append(f"\n  Other dependencies required ({len(other_needed)}):")
        lines.append(f"    {', '.join(other_needed)}")

    lines.append("")
    if drv.category_name == "BOOT":
        lines.append("Verdict: BOOT driver — loaded at Phase 3.85 with full capabilities")
    elif drv.category_name == "SYSTEM":
        lines.append("Verdict: SYSTEM driver — loaded at Phase 3.85 with port I/O + IRQ + MMIO")
    elif drv.category_name == "DEMAND":
        lines.append("Verdict: DEMAND driver — on-demand, sandboxed (EventBus + Log only)")

    return "\n".join(lines)


def get_module_info(name: str) -> str:
    """Get structured info about a loaded module (driver or DLL)."""
    return get_module_symbols(name, format="detailed")
