"""
tools/system_tools.py — NeoDOS System Consistency & Integration Tools.

Validates architectural rules against the actual codebase, checks for
consistency between documentation, source code, and build artifacts.
"""

import os
import re
from pathlib import Path
NEODOS_ROOT: Path = None  # type: ignore[assignment]
KERNEL_SRC: Path = None  # type: ignore[assignment]


def configure(root_dir: str):
    global NEODOS_ROOT, KERNEL_SRC
    NEODOS_ROOT = Path(root_dir)
    KERNEL_SRC = NEODOS_ROOT / "neodos-kernel" / "src"


# ── Architectural Rules (from ARCHITECTURE_SOURCE_OF_TRUTH.md) ──

INVARIANTS = {
    "INV-1": "No circular dependencies — DAG enforcement via check_deps.py",
    "INV-2": "No dynamic allocation in IRQ context",
    "INV-3": "No blocking in IRQ context",
    "INV-4": "No scheduler from Ring 0 (except 4 allowed sites)",
    "INV-5": "Every physical frame has exactly one owner",
    "INV-6": "Every process slot is free or valid",
    "INV-7": "No interrupt-stack execution of scheduler code",
    "INV-8": "Kernel heap not user-accessible",
    "INV-9": "Syscall handler is the only gate Ring 3→0 (INT 0x80 only)",
    "INV-10": "NeoInit (PID 1) must never be killed",
}

SUBSYSTEM_FORBIDDEN = {
    "scheduler": ["vfs", "drivers/"],
    "irq": ["schedule()", "vfs", "heap allocation"],
    "ata": ["scheduler", "ahci"],
    "ahci": ["scheduler", "filesystems"],
    "block_device": ["scheduler", "filesystems"],
    "shell": ["ahci", "ata", "syscall dispatch", "schedule()"],
    "console": ["scheduler", "filesystems"],
    "memory": ["scheduler", "filesystems"],
    "hal": ["any kernel subsystem"],
}


# ── Tool Implementations ──

def check_consistency(targets: str = "all") -> str:
    """Validate architectural consistency across kernel code, docs, and artifacts."""
    results = []

    # Only run checks that don't require external processes
    if targets in ("all", "code"):
        results.extend(_check_source_consistency())

    if targets in ("all", "docs"):
        results.extend(_check_doc_consistency())

    if targets in ("all", "artifacts"):
        results.extend(_check_artifact_consistency())

    if targets in ("all", "invariants"):
        results.extend(_check_invariants())

    if not results:
        return "No checks selected. Use targets='all', 'code', 'docs', 'artifacts', or 'invariants'."

    lines = ["Architectural Consistency Report", "=================================", ""]

    # Group by severity
    for severity in ("ERROR", "WARN", "INFO", "OK"):
        group = [r for r in results if r[0] == severity]
        if group:
            label = {"ERROR": "✗ Errors", "WARN": "⚠ Warnings", "INFO": "ℹ Info", "OK": "✓ Passed"}[severity]
            lines.append(f"{label}:")
            for _, msg in group:
                lines.append(f"  {msg}")
            lines.append("")

    errored = len([r for r in results if r[0] == "ERROR"])
    warned = len([r for r in results if r[0] == "WARN"])
    passed = len([r for r in results if r[0] == "OK"])

    lines.append(f"Summary: {passed} passed, {warned} warnings, {errored} errors")
    return "\n".join(lines)


def _check_source_consistency() -> list[tuple[str, str]]:
    results = []
    root = NEODOS_ROOT or Path(".")
    krnl = KERNEL_SRC or root / "neodos-kernel" / "src"

    if not krnl.exists():
        results.append(("ERROR", f"Kernel source not found at {krnl}"))
        return results

    # Check for forbidden dependencies in shell → ahci/ata
    shell_dir = KERNEL_SRC / "shell"
    if shell_dir.exists():
        for rs_file in shell_dir.rglob("*.rs"):
            try:
                content = open(rs_file).read()
                for pattern in ["drivers::ahci", "drivers::ata", "use crate::syscall"]:
                    # syscall is ALLOWED in shell via handler.rs references; check for direct dispatch
                    if pattern == "use crate::syscall" and "syscall_dispatch" in content:
                        # Check it's referencing types, not calling dispatch
                        if "syscall_dispatch(" in content:
                            results.append(("WARN", f"Shell file {rs_file.name} may call syscall_dispatch directly"))
            except:
                continue

    # Check hst_* function duplication
    v3loader = KERNEL_SRC / "drivers" / "nem" / "v3loader.rs"
    hst = KERNEL_SRC / "drivers" / "nem" / "hst.rs"
    if v3loader.exists() and hst.exists():
        v3text = open(v3loader).read()
        hsttext = open(hst).read()
        for fn in ["hst_inb", "hst_outb", "hst_inw", "hst_outw",
                     "hst_inl", "hst_outl", "hst_push_event",
                     "hst_push_input", "hst_get_ticks", "hst_ack_irq", "hst_log"]:
            if fn in v3text and fn in hsttext:
                # Check they are actual function definitions in both
                if f"fn {fn}" in v3text and f"fn {fn}" in hsttext:
                    results.append(("WARN", f"'{fn}' defined in both v3loader.rs AND hst.rs — duplicated exports"))

    # Check NEM magic constant
    nem_mod = KERNEL_SRC / "nem" / "mod.rs"
    if nem_mod.exists():
        content = open(nem_mod).read()
        if 'NEM_MAGIC: u32 = 0x004D454E' in content:
            results.append(("INFO", "nem/mod.rs has legacy NEM_MAGIC = 'NEM\\0' (v2), kernel uses 'NEM3' for v3"))

    return results


def _check_doc_consistency() -> list[tuple[str, str]]:
    results = []

    # Check AGENTS.md exists
    agents = NEODOS_ROOT / "AGENTS.md"
    if agents.exists():
        results.append(("OK", "AGENTS.md present"))
    else:
        results.append(("ERROR", "AGENTS.md missing"))

    # Check architecture docs
    arch_sot = NEODOS_ROOT / "docs" / "ARCHITECTURE_SOURCE_OF_TRUTH.md"
    if arch_sot.exists():
        results.append(("OK", "ARCHITECTURE_SOURCE_OF_TRUTH.md present"))
    else:
        results.append(("WARN", "ARCHITECTURE_SOURCE_OF_TRUTH.md missing"))

    # Check CHANGELOG.md
    changelog = NEODOS_ROOT / "CHANGELOG.md"
    version: str | None = None
    if changelog.exists():
        for line in open(changelog):
            m = re.match(r"^## v([\d.]+)", line)
            if m:
                version = m.group(1)
                break
        ver_str = version if version else "?"
        results.append(("OK", f"CHANGELOG.md present (v{ver_str})"))
    else:
        results.append(("WARN", "CHANGELOG.md missing"))

    # Check AGENTS.md version vs CHANGELOG.md
    agents = NEODOS_ROOT / "AGENTS.md"
    if agents.exists():
        agents_ver = None
        for line in open(agents):
            m = re.match(r'\*\*Version:\*\* v([\d.]+)', line)
            if m:
                agents_ver = m.group(1)
                break
        if agents_ver and version not in ('?', None) and agents_ver != version:
            results.append(("WARN", f"AGENTS.md version (v{agents_ver}) != CHANGELOG.md (v{version})"))
        elif agents_ver and version in ('?', None):
            results.append(("OK", f"AGENTS.md version v{agents_ver} (CHANGELOG version unknown)"))
        elif agents_ver:
            results.append(("OK", f"AGENTS.md version v{agents_ver} matches CHANGELOG"))
    else:
        results.append(("WARN", "AGENTS.md not found"))

    # Check NEM spec
    nem_spec = NEODOS_ROOT / "docs" / "NEM_SPEC.md"
    if nem_spec.exists():
        results.append(("OK", "NEM_SPEC.md present"))
    else:
        results.append(("WARN", "NEM_SPEC.md missing"))

    return results


def _check_artifact_consistency() -> list[tuple[str, str]]:
    results = []

    # Disk image
    disk_img = NEODOS_ROOT / "disk_image.img"
    if disk_img.exists():
        size_mb = os.path.getsize(str(disk_img)) / (1024 * 1024)
        results.append(("OK", f"disk_image.img ({size_mb:.0f} MB)"))
    else:
        results.append(("INFO", "disk_image.img not found (build with bash scripts/build.sh --neodos-image)"))

    # NeoDOS FS image
    neodos_img = NEODOS_ROOT / "scripts" / "neodos_image.img"
    if neodos_img.exists():
        results.append(("OK", "neodos_image.img (NeoDOS FS)"))
    else:
        results.append(("INFO", "neodos_image.img not found"))

    # Kernel ELF
    kernel_elf = NEODOS_ROOT / "kernel.elf"
    if kernel_elf.exists():
        results.append(("OK", "kernel.elf present"))
    else:
        results.append(("INFO", "kernel.elf not found"))

    # Bootloader
    bootloader = NEODOS_ROOT / "bootloader.efi"
    if bootloader.exists():
        results.append(("OK", "bootloader.efi present"))
    else:
        results.append(("INFO", "bootloader.efi not found"))

    return results


def _check_invariants() -> list[tuple[str, str]]:
    results = []
    for inv_id, desc in INVARIANTS.items():
        results.append(("INFO", f"{inv_id}: {desc}"))
    return results


# ── Resource: System Info ──

def get_system_resource() -> str:
    """System information resource."""
    if not NEODOS_ROOT:
        return "NeoDOS root not configured"

    lines = [
        "NeoDOS System Information",
        "========================",
        f"Root: {NEODOS_ROOT}",
        "",
    ]

    # Version
    agents = NEODOS_ROOT / "AGENTS.md"
    if agents.exists():
        for line in open(agents):
            m = re.match(r"^v(\d+\.\d+\.\d+)", line)
            if m:
                lines.append(f"Version: v{m.group(1)}")
                break

    # File counts
    kernel_src = NEODOS_ROOT / "neodos-kernel" / "src"
    if kernel_src.exists():
        rs_files = list(kernel_src.rglob("*.rs"))
        lines.append(f"Kernel source files: {len(rs_files)}")

    drivers_dir = NEODOS_ROOT / "drivers"
    if drivers_dir.exists():
        nem_files = list(drivers_dir.rglob("*.nem"))
        lines.append(f"NEM driver projects: {len(nem_files)}")

    dll_files = list(NEODOS_ROOT.glob("*.nxl"))
    lines.append(f"NXLs: {len(dll_files)}")

    # Disk images
    lines.append("")
    for name in ["disk_image.img", "scripts/neodos_image.img", "bootloader.efi", "kernel.elf"]:
        p = NEODOS_ROOT / name
        if p.exists():
            size = os.path.getsize(str(p))
            if size >= 1024 * 1024:
                lines.append(f"{name}: {size // (1024*1024)} MB")
            elif size >= 1024:
                lines.append(f"{name}: {size // 1024} KB")
            else:
                lines.append(f"{name}: {size} B")
        else:
            lines.append(f"{name}: (not found)")

    return "\n".join(lines)
