#!/usr/bin/env python3
"""
Dependency validation tool for NeoDOS kernel.

Scans all Rust source files and checks for forbidden
cross-subsystem dependencies.

Usage:
    python3 scripts/check_deps.py
    python3 scripts/check_deps.py --fix   # auto-fix some violations
"""

import os
import re
import sys
from pathlib import Path

SRC_DIR = Path(__file__).resolve().parent.parent / "neodos-kernel" / "src"

# ── Subsystem definitions ────────────────────────────────────────────
# Each entry: (subsystem_name, file_path_pattern, [allowed_dependencies])

SUBSYSTEMS = {
    "arch": {
        "paths": ["arch/"],
        "allowed": ["scheduler", "syscall"],
        "forbidden_deps": [
            "shell",
            "fs",
            "input",
            "console",
            "graphics",
            "font",
        ],
    },
    "scheduler": {
        "paths": ["scheduler.rs"],
        "allowed": [],
        "forbidden_deps": [
            "drivers/ahci",
            "drivers/ata",
            "drivers/block",
            "drivers/fat32",
            "drivers/gpt",
            "drivers/iso9660",
            "drivers/keyboard",
            "drivers/pci",
            "drivers/rtc",
            "drivers/acpi",
            "drivers/usb_hid",
            "fs",  # scheduler must not depend on VFS
        ],
    },
    "syscall": {
        "paths": ["syscall.rs"],
        "allowed": [
            "scheduler",
            "input",
            "console",
            "serial",
            "arch/x64/paging",
            "arch/x64/gdt",
            "fs/vfs",
            "globals",
            "memory",
        ],
        # syscalls are allowed to call many things, but not:
        "forbidden_deps": [
            "drivers/ahci",
            "drivers/ata",
            "drivers/fat32",
            "drivers/keyboard",
            "drivers/pci",
            "drivers/rtc",
            "drivers/acpi",
            "drivers/usb_hid",
            "shell",
        ],
    },
    "vfs": {
        "paths": ["fs/vfs.rs"],
        "allowed": [],
        "forbidden_deps": [
            "drivers/",
            "arch/x64/paging",
            "arch/x64/gdt",
            "arch/x64/idt",
            "scheduler",
            "input",
            "memory",
            "shell",
            "syscall",
        ],
    },
    "neodos_fs": {
        "paths": ["fs/neodos_fs.rs"],
        "allowed": ["buffer/block_cache", "drivers/block", "drivers/ata", "globals"],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "arch/",
            "input",
        ],
    },
    "fat32": {
        "paths": ["drivers/fat32.rs"],
        "allowed": ["drivers/block", "drivers/ata", "globals"],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "arch/",
            "fs/",
            "input",
        ],
    },
    "block_device": {
        "paths": ["drivers/block.rs"],
        "allowed": ["drivers/ahci", "drivers/ata"],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "fs/",
            "input",
            "console",
            "graphics",
            "font",
            "arch/x64/idt",
            "arch/x64/gdt",
        ],
    },
    "ata": {
        "paths": ["drivers/ata.rs"],
        "allowed": [
            "drivers/block",
        ],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "fs/",
            "input",
            "console",
            "globals",
        ],
    },
    "ahci": {
        "paths": ["drivers/ahci.rs"],
        "allowed": [],
        "forbidden_deps": [
            "scheduler",
            "shell",
            "fs/",
            "input",
        ],
    },
    "shell": {
        "paths": ["shell/"],
        "allowed": [
            "fs/vfs",
            "scheduler",
            "arch/x64/paging",
            "input",
            "console",
            "globals",
            "drivers/block",
        ],
        "forbidden_deps": [
            "drivers/ahci",
            "drivers/ata",
            "drivers/fat32",
            "drivers/pci",
            "drivers/rtc",
            "drivers/acpi",
            "syscall",
        ],
    },
    "input": {
        "paths": ["input.rs"],
        "allowed": [],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "fs/",
            "drivers/",
            "arch/",
        ],
    },
    "console": {
        "paths": ["console.rs", "graphics.rs", "font.rs"],
        "allowed": ["serial", "graphics", "font"],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "fs/",
            "drivers/",
        ],
    },
    "memory": {
        "paths": ["memory.rs"],
        "allowed": [],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "fs/",
            "drivers/",
            "input",
        ],
    },
    "paging": {
        "paths": ["arch/x64/paging.rs"],
        "allowed": ["memory"],
        "forbidden_deps": [
            "scheduler",
            "syscall",
            "shell",
            "fs/",
            "drivers/",
            "input",
        ],
    },
}

# ── Dependency extraction ───────────────────────────────────────────

IMPORT_RE = re.compile(r'^\s*use\s+(?:crate::)?([^;]+);')
CRATE_IMPORT_RE = re.compile(r'^\s*use\s+(\S+);')


def get_owning_subsystem(file_path: str) -> str | None:
    rel = os.path.relpath(file_path, str(SRC_DIR)).replace("\\", "/")
    for name, info in SUBSYSTEMS.items():
        for pat in info["paths"]:
            if rel.startswith(pat) or rel == pat:
                return name
    return None


def extract_crate_imports(file_path: str) -> list[str]:
    imports = []
    with open(file_path, "r") as f:
        for line in f:
            m = IMPORT_RE.match(line)
            if m:
                imports.append(m.group(1))
    return imports


def check_forbidden(from_subsystem: str, imported_path: str) -> list[str]:
    violations = []
    info = SUBSYSTEMS.get(from_subsystem)
    if not info:
        return violations

    for forbidden in info.get("forbidden_deps", []):
        if forbidden in imported_path:
            # Check if it's in allowed list
            allowed = False
            for allow in info.get("allowed", []):
                if allow in imported_path:
                    allowed = True
                    break
            if not allowed:
                violations.append(forbidden)
    return violations


def main():
    src = str(SRC_DIR)
    violations = []

    for root, dirs, files in os.walk(src):
        for fname in files:
            if not fname.endswith(".rs"):
                continue
            fpath = os.path.join(root, fname)
            subsystem = get_owning_subsystem(fpath)
            if not subsystem:
                continue

            imports = extract_crate_imports(fpath)
            for imp in imports:
                forbidden = check_forbidden(subsystem, imp)
                if forbidden:
                    rel = os.path.relpath(fpath, src)
                    violations.append(
                        f"  {rel}: imports '{imp}' which contains "
                        f"forbidden dep(s): {', '.join(forbidden)}"
                    )

    print("=" * 60)
    print("NeoDOS Dependency Check")
    print("=" * 60)

    if violations:
        print(f"\n❌ {len(violations)} violation(s) found:\n")
        for v in violations:
            print(v)
        sys.exit(1)
    else:
        print("\n✅ No dependency violations found.")
        sys.exit(0)


if __name__ == "__main__":
    main()
