"""
tools/kernel_tools.py — NeoDOS Kernel Introspection Tools.

Provides tools for analyzing kernel source code structure,
including file index, symbol search, architecture overview,
and build validation.
"""

import os
import re
from pathlib import Path


# ── Configuration ──

NEODOS_ROOT: Path = None
KERNEL_SRC: Path = None

SOURCE_EXTENSIONS = {".rs"}
SKIP_DIRS = {"target", "__pycache__", ".git"}


def configure(root_dir: str):
    global NEODOS_ROOT, KERNEL_SRC
    NEODOS_ROOT = Path(root_dir)
    KERNEL_SRC = NEODOS_ROOT / "neodos-kernel" / "src"


# ── Helpers ──

def _find_all_rs_files() -> list[Path]:
    if KERNEL_SRC is None or not KERNEL_SRC.exists():
        return []
    return sorted(KERNEL_SRC.rglob("*.rs"))


def _format_file_list(files: list[Path], base: Path = None) -> str:
    if base is None:
        base = KERNEL_SRC
    lines = []
    for f in files:
        try:
            rel = f.relative_to(base)
        except ValueError:
            rel = f
        lines.append(f"  {rel}")
    return "\n".join(lines)


def _count_lines(filepath: Path) -> int:
    try:
        with open(filepath) as f:
            return sum(1 for _ in f)
    except:
        return 0


# ── Tool Implementations ──

def kernel_index(format: str = "tree") -> str:
    """Show kernel source index with file counts and metadata."""
    files = _find_all_rs_files()
    if not files:
        return "ERROR: Kernel source not found. Set NEODOS_ROOT."

    total_lines = sum(_count_lines(f) for f in files)

    # Read version info
    agents_md = NEODOS_ROOT / "AGENTS.md"
    kernel_version = "?"
    if agents_md.exists():
        for line in open(agents_md):
            m = re.match(r"^v(\d+\.\d+\.\d+)", line)
            if m:
                kernel_version = m.group(1)
                break

    # Group by directory
    from collections import defaultdict
    groups = defaultdict(list)
    for f in files:
        rel = f.relative_to(KERNEL_SRC)
        parent = str(rel.parent) if rel.parent != "." else "."
        groups[parent].append(f)

    lines = [
        f"NeoDOS Kernel Index",
        f"====================",
        f"Version:  v{kernel_version}",
        f"Source:   {KERNEL_SRC}",
        f"Files:    {len(files)}",
        f"Lines:    {total_lines}",
        f"",
        f"Subsystem hierarchy:",
    ]

    for dirname in sorted(groups):
        dir_files = groups[dirname]
        dir_lines = sum(_count_lines(f) for f in dir_files)
        label = dirname if dirname != "." else "(root)"
        lines.append(f"  {label}/  ({len(dir_files)} files, {dir_lines} lines)")
        if format == "tree":
            for f in dir_files:
                name = f.name
                flines = _count_lines(f)
                lines.append(f"    ├── {name}  ({flines} lines)")

    lines.append(f"\nTotal: {len(files)} source files, {total_lines} lines of Rust")
    return "\n".join(lines)


def search_symbol(query: str, max_results: int = 30) -> str:
    """Search for symbols (fn, struct, const, trait) in kernel source."""
    files = _find_all_rs_files()
    if not files:
        return "ERROR: Kernel source not found."

    pattern = re.compile(re.escape(query), re.IGNORECASE)
    results = []

    # Patterns to match definitions
    defn_patterns = [
        re.compile(r"^\s*(pub\s+)?(unsafe\s+)?(extern\s+\"[^\"]+\"\s+)?fn\s+(\w+)"),
        re.compile(r"^\s*(pub\s+)?(struct|enum|trait|union|type|const|static)\s+(\w+)"),
        re.compile(r"^\s*#\[.*\]"),
    ]

    for f in files:
        try:
            with open(f) as fh:
                for i, line in enumerate(fh, 1):
                    if pattern.search(line):
                        rel = str(f.relative_to(KERNEL_SRC))
                        is_defn = any(dp.search(line) for dp in defn_patterns)
                        marker = "def" if is_defn else "ref"
                        results.append((rel, i, line.rstrip(), marker))
        except:
            continue

    if not results:
        return f"No matches for '{query}'."

    # Show definitions first, then references
    defs = [r for r in results if r[3] == "def"]
    refs = [r for r in results if r[3] == "ref"]
    shown = defs + refs
    total = len(results)

    lines = [f"Symbol search: '{query}' — {len(results)} total, showing {min(total, max_results)}"]
    for rel, lineno, text, marker in shown[:max_results]:
        prefix = "  ╞" if marker == "def" else "  │"
        lines.append(f"{prefix} {rel}:{lineno}: {text}")

    if total > max_results:
        lines.append(f"  ... and {total - max_results} more matches")

    return "\n".join(lines)


def get_kernel_architecture() -> str:
    """Return kernel memory layout, boot phases, and subsystem boundaries."""
    from . import subsystem_tools
    phases = subsystem_tools.boot_phases()
    # Extract just the phase table from boot_phases output
    phase_lines = []
    capture = False
    for line in phases.splitlines():
        if line.startswith("Phase") and "Description" in line:
            capture = True
            continue
        if capture and line.strip().startswith("Source"):
            break
        if capture and line.strip():
            phase_lines.append("  " + line)
    phases_str = "\n".join(phase_lines) if phase_lines else "  (see boot_phases tool)"

    return (
        "Memory Layout:\n"
        "  Kernel image            0x00400000     ~1 MB (ELF loaded at 0x4000000)\n"
        "  Kernel heap             0x01000000     16 MB (slab + linked_list_allocator)\n"
        "  User window             0x00400000     4 MB (32 x 128 KB process slots)\n"
        "  User heap               0x10000000     32 MB (16 x 2 MB, demand-paged)\n"
        "  DLL region              0x1E000000     2 MB (8 x 256 KB NXL slots)\n"
        "  TEB                     0x00007000     4 KB (USER_ACCESSIBLE, SEH)\n"
        "  mmap region             0x20000000     32 MB (lazy allocation)\n"
        "  Driver isolation        0x30000000     16 MB (16 x 1 MB NEM slots)\n"
        "\n"
        "Boot Phases:\n"
        f"{phases_str}\n"
        "\n"
        "Syscall Table:\n"
        "  RAX 0-4:   Process (Exit, Yield, WaitAlertable, SleepEx, SetExceptionHandler)\n"
        "  RAX 10-12: Memory (Brk, Mmap, Munmap)\n"
        "  RAX 20-25: I/O (Write, Read, Dup2, Close, Poll, LoadLib)\n"
        "  RAX 30:    Console (CursorBlink)\n"
        "  RAX 35:    Driver (DriverUnload)\n"
        "  RAX 40-48: Object Manager (ObOpen..ObSnapshot)\n"
        "  RAX 50-59: Registry Cm (CmOpenKey..CmUnloadHive)\n"
        "  Total: 32 syscalls, SSDT dispatch via INT 0x80\n"
        "\n"
        "Forbidden Dependencies:\n"
        "  Scheduler   -> NO VFS, BlockDevice, AHCI/ATA\n"
        "  IRQ handler -> NO schedule(), VFS, heap allocation\n"
        "  Block driver-> NO scheduler, filesystems\n"
        "  Console     -> NO scheduler, filesystems, drivers\n"
        "  Frame alloc -> NO scheduler, filesystems, drivers\n"
        "  Shell       -> NO AHCI, ATA, syscall dispatch"
    )


def get_build_errors() -> str:
    """Check for potential build issues: circular deps, ABI mismatches, sources."""
    issues = []

    # Check check_deps.py
    check_deps = NEODOS_ROOT / "scripts" / "check_deps.py"
    if check_deps.exists():
        issues.append(("INFO", "check_deps.py exists — run `python3 scripts/check_deps.py` to validate"))

    # Check build script
    build_sh = NEODOS_ROOT / "scripts" / "build.sh"
    if build_sh.exists():
        issues.append(("OK", "build.sh available"))

    # Check for kernel source
    if KERNEL_SRC and KERNEL_SRC.exists():
        issues.append(("OK", f"Kernel source at {KERNEL_SRC}"))
    else:
        issues.append(("ERROR", "Kernel source not found!"))

    # Check for NEM / nem format mismatch
    nem_root = KERNEL_SRC / "nem" / "mod.rs" if KERNEL_SRC else None
    nem_drv = KERNEL_SRC / "drivers" / "nem" / "mod.rs" if KERNEL_SRC else None
    if nem_root and nem_root.exists():
        content = open(nem_root).read()
        if "NEM_MAGIC = 0x004D454E" in content:
            issues.append(("WARN", "nem/mod.rs defines NEM_MAGIC as 'NEM\\0' (v2 legacy), but v3 uses 'NEM3'"))
        if b"NEM3" in content.encode():
            issues.append(("OK", "NEM v3 format defined in nem/mod.rs"))
    if nem_drv and nem_drv.exists():
        issues.append(("OK", "NEM v3 loader in drivers/nem/"))

    # Check export duplication between v3loader.rs and hst.rs
    v3loader = KERNEL_SRC / "drivers" / "nem" / "v3loader.rs" if KERNEL_SRC else None
    hst = KERNEL_SRC / "drivers" / "nem" / "hst.rs" if KERNEL_SRC else None
    if v3loader and v3loader.exists() and hst and hst.exists():
        v3text = open(v3loader).read()
        hsttext = open(hst).read()
        overlap = 0
        for fn in ["hst_inb", "hst_outb", "hst_inw", "hst_outw", "hst_inl", "hst_outl",
                     "hst_push_event", "hst_push_input", "hst_get_ticks", "hst_ack_irq", "hst_log"]:
            if fn in v3text and fn in hsttext:
                overlap += 1
        if overlap >= 3:
            issues.append(("WARN", f"{overlap} hst_* functions duplicated in v3loader.rs AND hst.rs"))

    # ELF loader
    elf_path = KERNEL_SRC / "elf.rs" if KERNEL_SRC else None
    if elf_path and elf_path.exists():
        issues.append(("OK", "ELF64 loader in elf.rs"))

    # nxl.rs
    nxl_path = KERNEL_SRC / "nxl.rs" if KERNEL_SRC else None
    if nxl_path and nxl_path.exists():
        issues.append(("OK", "NXL system in nxl.rs"))

    # Check for the AGENTS.md version consistency
    changelog = NEODOS_ROOT / "CHANGELOG.md"
    if changelog.exists():
        for line in open(changelog):
            m = re.match(r"^## \[v(\d+\.\d+\.\d+)\]", line)
            if m:
                issues.append(("OK", f"CHANGELOG version v{m.group(1)}"))
                break

    lines = ["Build & Architecture Validation:"]
    for severity, msg in issues:
        label = {"OK": "✓", "WARN": "⚠", "ERROR": "✗", "INFO": "ℹ"}
        lines.append(f"  [{label.get(severity, '?')}] {msg}")

    return "\n".join(lines)
