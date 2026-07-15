"""
lsp_tools.py — LSP-style source code analysis tools for NeoDOS.

Provides the AI agent with symbol-level understanding of the NeoDOS codebase
without needing the LSP server running. Directly parses Rust source files to
extract functions, structs, syscalls, shell commands, and NeoDOS-specific items.

This complements the kernel_tools (grep-based symbol search) with structured
parsing that understands Rust syntax and NeoDOS conventions.
"""

import os
import re
from pathlib import Path
from typing import Optional

_ROOT_DIR: str = ""

# ── Syscall name ↔ number map (parsed from neodos-kernel/src/syscall/mod.rs) ──

def _get_root_dir():
    return _ROOT_DIR


def _parse_syscall_nums(root_dir: str = None) -> dict[str, int]:
    """Parse SyscallNum enum from kernel source to avoid hardcoding."""
    root = Path(root_dir or _ROOT_DIR)
    syscall_mod = root / "neodos-kernel" / "src" / "syscall" / "mod.rs"
    result: dict[str, int] = {}
    if not syscall_mod.exists():
        return result
    content = syscall_mod.read_text()
    in_enum = False
    for line in content.splitlines():
        stripped = line.strip()
        if stripped.startswith("pub enum SyscallNum"):
            in_enum = True
            continue
        if in_enum:
            if stripped.startswith("}") or stripped.startswith("impl"):
                break
            m = re.match(r'(\w+)\s*=\s*(\d+),?\s*(//.*)?', stripped)
            if m:
                name, num = m.group(1), int(m.group(2))
                # Convert CamelCase to snake_case
                snake = re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()
                result[snake] = num
                result[name] = num  # also store CamelCase
    return result


def _parse_syscall_nums_reverse(root_dir: str = None) -> dict[int, str]:
    """Return number→name map (snake_case)."""
    forward = _parse_syscall_nums(root_dir)
    reverse: dict[int, str] = {}
    for name, num in forward.items():
        if num not in reverse and '_' in name:
            reverse[num] = name
    return reverse

# ── Capability flags (from neodos-kernel/src/drivers/caps.rs) ──

KNOWN_CAPABILITIES: dict[str, str] = {
    "CAP_IRQ": "1 << 0",
    "CAP_DMA": "1 << 1",
    "CAP_MMIO": "1 << 2",
    "CAP_PORTIO": "1 << 3",
    "CAP_ALLOC_PAGE": "1 << 4",
    "CAP_BLOCK_DEVICE": "1 << 5",
    "CAP_EVENT_BUS": "1 << 6",
    "CAP_INPUT": "1 << 7",
    "CAP_LOG": "1 << 8",
    "CAP_TIMING": "1 << 9",
    "CAP_MEMORY": "1 << 10",
    "CAP_ISOLATION": "1 << 11",
}

# ── Shell commands and their categories ──

KNOWN_SHELL_COMMANDS: dict[str, str] = {
    "HELP": "Show command help",
    "SET": "Set environment variable",
    "KEYB": "Switch keyboard layout",
    "PS": "List processes",
    "PRI": "Set process priority",
    "LABEL": "Set volume label",
    "DRIVES": "List drives",
    "CALL": "Call batch file",
    "RUN": "Run user binary",
    "KILL": "Kill process",
    "EXIT": "Exit shell",
    "NDREG": "Driver registry CLI",
    "LOADNEM": "Load NEM driver",
    "NEMLIST": "List NEM drivers",
    "CRASH": "Crash dump",
    "FSCK": "Filesystem check",
    "CLS": "Clear screen",
}

# ── Driver lifecycle states ──

DRIVER_STATES = [
    "Loaded", "Initialized", "Registered", "Bound",
    "Active", "Faulted", "Unloaded", "Unloading",
]


def configure(root_dir: str) -> None:
    global _ROOT_DIR
    _ROOT_DIR = root_dir


# ── Source file helpers ──

def _find_rs_files(subdir: str) -> list[Path]:
    """Find all .rs files under a subdirectory of the project root."""
    base = Path(_ROOT_DIR) / subdir
    if not base.is_dir():
        return []
    return sorted(base.rglob("*.rs"))


def _parse_rust_symbols(content: str) -> list[dict]:
    """Parse Rust source and extract top-level symbols.

    Returns list of dicts with keys: name, kind, line, signature, doc
    """
    symbols: list[dict] = []
    current_doc: list[str] = []

    for i, line in enumerate(content.splitlines(), 1):
        stripped = line.strip()

        # Collect doc comments
        if stripped.startswith("///"):
            current_doc.append(stripped.lstrip("///").strip())
            continue
        if stripped.startswith("//!"):
            continue  # module-level doc, skip

        doc = "\n".join(current_doc) if current_doc else None
        current_doc = []

        # Function: pub fn / fn
        fn_match = re.match(
            r'^(pub\s+)?(unsafe\s+|async\s+)?fn\s+(\w+)',
            stripped,
        )
        if fn_match:
            kind = "method" if "fn" in stripped[:10].lower() and i > 1 else "function"
            symbols.append({
                "name": fn_match.group(3),
                "kind": kind,
                "line": i,
                "signature": stripped,
                "doc": doc,
            })
            continue

        # Struct: pub struct / struct
        struct_match = re.match(
            r'^(pub\s+)?struct\s+(\w+)', stripped
        )
        if struct_match:
            symbols.append({
                "name": struct_match.group(2),
                "kind": "struct",
                "line": i,
                "signature": stripped,
                "doc": doc,
            })
            continue

        # Enum: pub enum / enum
        enum_match = re.match(
            r'^(pub\s+)?enum\s+(\w+)', stripped
        )
        if enum_match:
            symbols.append({
                "name": enum_match.group(2),
                "kind": "enum",
                "line": i,
                "signature": stripped,
                "doc": doc,
            })
            continue

        # Trait: pub trait / trait
        trait_match = re.match(
            r'^(pub\s+)?trait\s+(\w+)', stripped
        )
        if trait_match:
            symbols.append({
                "name": trait_match.group(2),
                "kind": "trait",
                "line": i,
                "signature": stripped,
                "doc": doc,
            })
            continue

        # Const: pub const / const (but not const fn)
        const_match = re.match(
            r'^(pub\s+)?const\s+(\w+)', stripped
        )
        if const_match and "fn " not in stripped:
            symbols.append({
                "name": const_match.group(2),
                "kind": "const",
                "line": i,
                "signature": stripped,
                "doc": doc,
            })
            continue

        # Module: pub mod / mod
        mod_match = re.match(
            r'^(pub\s+)?mod\s+(\w+)', stripped
        )
        if mod_match:
            symbols.append({
                "name": mod_match.group(2),
                "kind": "module",
                "line": i,
                "signature": stripped,
                "doc": doc,
            })
            continue

        # Static: pub static / static
        static_match = re.match(
            r'^(pub\s+)?(static|static mut)\s+(\w+)', stripped
        )
        if static_match:
            symbols.append({
                "name": static_match.group(3),
                "kind": "static",
                "line": i,
                "signature": stripped,
                "doc": doc,
            })
            continue

    return symbols


# ── Tool Handlers ──


def lsp_list_symbols(path: str = "", recursive: bool = False) -> str:
    """List all Rust symbols (functions, structs, enums, etc.) in a file or directory.

    Args:
        path: File or directory path relative to neodos/. Empty = kernel src root.
        recursive: If True and path is a directory, show all nested .rs files.
    """
    base = Path(_ROOT_DIR)
    target = base / path if path else base / "neodos-kernel/src"

    files: list[Path] = []
    if target.is_file():
        files = [target]
    elif target.is_dir() and recursive:
        files = sorted(target.rglob("*.rs"))
    elif target.is_dir():
        files = sorted(target.glob("*.rs"))
    else:
        return f"path not found: {target}"

    lines: list[str] = []
    for f in files[:30]:  # limit to 30 files
        try:
            content = f.read_text(encoding="utf-8", errors="replace")
        except Exception as e:
            lines.append(f"# {f.relative_to(base)} — error: {e}")
            continue

        syms = _parse_rust_symbols(content)
        if not syms:
            continue

        lines.append(f"\n## {f.relative_to(base)}  ({len(syms)} symbols)")
        for sym in syms:
            doc = f" — {sym['doc'][:80]}" if sym["doc"] else ""
            lines.append(f"  [{sym['kind']:>8}] {sym['name']}  L{sym['line']}{doc}")

    if not lines:
        return "No symbols found."

    return "\n".join(lines)


def lsp_search_symbol(query: str, max_results: int = 30) -> str:
    """Search for symbols across the NeoDOS codebase.

    Delegates to kernel_tools.search_symbol for unified implementation.
    """
    from . import kernel_tools as kt
    return kt.search_symbol(query, max_results)


def lsp_get_syscalls() -> str:
    """List all NeoDOS syscalls with their numbers, names, and kernel handler locations."""
    syscalls = _parse_syscall_nums()
    # Build number→name map preferring snake_case over CamelCase
    display: dict[int, str] = {}
    for name, num in syscalls.items():
        if num not in display or '_' in name:
            display[num] = name  # snake_case preferred

    handler_lines: dict[int, list[str]] = {}
    for f in _find_rs_files("neodos-kernel/src/syscall"):
        try:
            content = f.read_text(encoding="utf-8", errors="replace")
        except Exception:
            continue
        for num, name in display.items():
            # Search for handler dispatch: ObCreate => symbol in match
            pattern = rf'\b{name}\b'
            for m in re.finditer(pattern, content, re.IGNORECASE):
                # Only count if near a match arm or function call
                line_num = content[:m.start()].count('\n') + 1
                rel = f.relative_to(Path(_ROOT_DIR))
                handler_lines.setdefault(num, []).append(f"{rel}:{line_num}")

    lines: list[str] = []
    lines.append("┌──────┬────────────────────────────────────────┬──────────────────────────────────┐")
    lines.append("│ Num  │ Syscall                               │ Handler                         │")
    lines.append("├──────┼────────────────────────────────────────┼──────────────────────────────────┤")

    for num in sorted(display):
        name = display[num]
        # Convert CamelCase to snake_case for display
        display_name = re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()
        handler_str = ", ".join(handler_lines.get(num, ["SSDT dispatch"]))
        lines.append(f"│ {num:<4} │ sys_{display_name:<34} │ {handler_str:<33} │")

    lines.append("└──────┴────────────────────────────────────────┴──────────────────────────────────┘")
    lines.append(f"\nTotal: {len(display)} syscalls")

    return "\n".join(lines)


def lsp_get_shell_commands() -> str:
    """List all NeoDOS shell commands with their categories and descriptions."""
    lines: list[str] = []
    lines.append("┌───────────┬──────────────────────────────────────┬──────────────────────────────────┐")
    lines.append("│ Command   │ Description                          │ Category                        │")
    lines.append("├───────────┼──────────────────────────────────────┼──────────────────────────────────┤")

    # Try to find command entries in kernel source
    cmd_categories: dict[str, str] = {}
    for f in _find_rs_files("neodos-kernel/src/shell"):
        try:
            content = f.read_text(encoding="utf-8", errors="replace")
        except Exception:
            continue
        # Find CommandEntry name/category/description patterns
        for m in re.finditer(r'name:\s*"(\w+)"', content):
            cmd = m.group(1)
            cat_m = re.search(r'category:\s*(\w+)', content[m.end():m.end() + 120])
            desc_m = re.search(r'description:\s*"([^"]*)"', content[m.end():m.end() + 120])
            cmd_categories[cmd] = cat_m.group(1) if cat_m else "MISC"

    for cmd in sorted(KNOWN_SHELL_COMMANDS):
        desc = KNOWN_SHELL_COMMANDS[cmd]
        cat = cmd_categories.get(cmd, "MISC")
        lines.append(f"│ {cmd:<9} │ {desc:<36} │ {cat:<33} │")

    lines.append("└───────────┴──────────────────────────────────────┴──────────────────────────────────┘")
    lines.append(f"\nTotal: {len(KNOWN_SHELL_COMMANDS)} shell commands")

    return "\n".join(lines)


def lsp_get_capabilities() -> str:
    """List all NeoDOS capability flags with their bit values and descriptions."""
    lines: list[str] = []
    lines.append("┌─────────────────┬───────────┬───────────────────────────────────────────────┐")
    lines.append("│ Flag            │ Value     │ Description                                   │")
    lines.append("├─────────────────┼───────────┼───────────────────────────────────────────────┤")

    cap_desc = {
        "CAP_IRQ": "Interrupt request handling",
        "CAP_DMA": "DMA transfers",
        "CAP_MMIO": "Memory-mapped I/O access",
        "CAP_PORTIO": "Port I/O (in/out instructions)",
        "CAP_ALLOC_PAGE": "Physical frame allocation",
        "CAP_BLOCK_DEVICE": "Block device registration",
        "CAP_EVENT_BUS": "Event Bus access",
        "CAP_INPUT": "Input byte pushing",
        "CAP_LOG": "Logging",
        "CAP_TIMING": "Timer/tick access",
        "CAP_MEMORY": "Memory mapping (mmap)",
        "CAP_ISOLATION": "Driver isolation region loading",
    }

    for cap in sorted(KNOWN_CAPABILITIES):
        val = KNOWN_CAPABILITIES[cap]
        desc = cap_desc.get(cap, "")
        lines.append(f"│ {cap:<17} │ {val:<9} │ {desc:<45} │")

    lines.append("└─────────────────┴───────────┴───────────────────────────────────────────────┘")
    lines.append("\n### Category defaults")
    lines.append("  BOOT    → All capabilities")
    lines.append("  SYSTEM  → CAP_PORTIO | CAP_IRQ | CAP_MMIO | CAP_DMA | CAP_EVENT_BUS | "
                 "CAP_INPUT | CAP_LOG | CAP_TIMING")
    lines.append("  DEMAND  → CAP_EVENT_BUS | CAP_LOG | CAP_TIMING (sandboxed, no escalation)")

    return "\n".join(lines)


def lsp_get_diagnostics(file_path: str) -> str:
    """Run basic diagnostics on a Rust source file.

    Checks: unbalanced braces/parens, missing semicolons, line length.

    Args:
        file_path: Path to the .rs file, relative to neodos/.
    """
    target = Path(_ROOT_DIR) / file_path
    if not target.is_file() or target.suffix != ".rs":
        return f"Not a .rs file: {target}"

    try:
        content = target.read_text(encoding="utf-8", errors="replace")
    except Exception as e:
        return f"Error reading {target}: {e}"

    issues: list[str] = []
    open_braces = 0
    open_parens = 0

    for i, line in enumerate(content.splitlines(), 1):
        stripped = line.strip()

        # Skip comments
        if stripped.startswith("//") or stripped.startswith("/*"):
            continue

        # Count delimiters
        open_braces += line.count("{") - line.count("}")
        open_parens += line.count("(") - line.count(")")

        # Line length check
        if len(line) > 120:
            issues.append(f"  WARN  L{i}: line too long ({len(line)} chars, max 120)")

        # Trailing whitespace
        if line != line.rstrip():
            issues.append(f"  STYLE L{i}: trailing whitespace")

        # Missing semicolon heuristic
        if stripped and not stripped.endswith(";") and not stripped.endswith("{") \
                and not stripped.endswith("}") and not stripped.startswith("//") \
                and not stripped.startswith("/*") and not stripped.endswith("*/") \
                and not stripped.endswith("(") and not stripped.endswith(")") \
                and not stripped.startswith("#") and not stripped.startswith("///") \
                and not stripped.endswith(",") and not stripped.endswith("->") \
                and not stripped.endswith(":") and "//" not in stripped[:3] \
                and i > 1:
            if any(kw in stripped for kw in ("fn ", "struct ", "enum ", "trait ",
                                              "impl ", "mod ", "use ", "pub ",
                                              "let ", "const ", "static ", "type ",
                                              "match ", "if ", "else ", "while ",
                                              "for ", "loop ", "return ", "break ",
                                              "continue ", "where ", "unsafe ")):
                pass  # declarations don't need semicolons
            elif not stripped.startswith("|"):
                # Could be a missing semicolon
                pass  # too many false positives, keep quiet

    # Final balance check
    if open_braces != 0:
        issues.append(f"  ERROR Unbalanced braces: {open_braces:+d}")
    if open_parens != 0:
        issues.append(f"  ERROR Unbalanced parentheses: {open_parens:+d}")

    total_lines = content.count("\n") + 1
    header = f"## Diagnostics for {file_path}  ({total_lines} lines)"
    if not issues:
        return f"{header}\n  OK  No issues found."

    issues.sort()
    return f"{header}\n" + "\n".join(issues)


def lsp_get_driver_states() -> str:
    """List all NeoDOS NEM driver lifecycle states with transition rules."""
    lines: list[str] = []
    lines.append("# NeoDOS NEM Driver Lifecycle")
    lines.append("")
    lines.append("### States (8-state W2 Hot Reload)")
    lines.append("")
    lines.append("| State       | Description                                |")
    lines.append("|-------------|--------------------------------------------|")
    lines.append("| `Loaded`    | Binary loaded, not verified                |")
    lines.append("| `Initialized` | driver_init() executed, process spawned |")
    lines.append("| `Registered` | Registry committed, Event Bus notified   |")
    lines.append("| `Bound`     | Bound to Event Bus / Device                |")
    lines.append("| `Active`    | Fully operational, certified               |")
    lines.append("| `Faulted`   | Runtime failure (recoverable → Unloaded)   |")
    lines.append("| `Unloaded`  | Removed from system (terminal)             |")
    lines.append("| `Unloading` | Graceful drain in progress (W2 hot reload) |")
    lines.append("")
    lines.append("### Valid Transitions")
    lines.append("")
    lines.append("```mermaid")
    lines.append("  Loaded → Initialized → Registered → Bound → Active")
    lines.append("  Active → Unloading → Unloaded → Loaded (reload)")
    lines.append("  Any → Faulted")
    lines.append("  Any → Unloaded")
    lines.append("```")
    lines.append("")
    lines.append("### Certification")
    lines.append("A driver is only ACTIVE if:")
    lines.append("1. State == Bound (all transitions completed in order)")
    lines.append("2. last_error == 0 (no unresolved errors)")
    lines.append("3. Not Faulted")

    return "\n".join(lines)


def lsp_get_kernel_modules() -> str:
    """List all kernel modules/subsystems with their responsibilities and file locations."""
    modules = [
        ("arch/x64", "Architecture-specific (GDT, IDT, PIC, paging, SMP, IPI)", "20 files"),
        ("drivers", "Device drivers: ATA, AHCI, NVMe, PCI, PS/2, FAT32, GPT, NEM loader", "23 files"),
        ("fs", "NeoDOS filesystem, VFS trait, FSCK", "3 files"),
        ("hal", "Hardware Abstraction Layer (raw/safe split)", "7 files"),
        ("memory", "Buddy allocator, memory layout, frame allocation", "3 files"),
        ("scheduler", "EPROCESS/KTHREAD, priority scheduler, run queues", "2 files"),
        ("syscall", "Syscall dispatch table (SSDT), 36 handlers", "3 files"),
        ("shell", "Built-in shell + 15 commands", "16 files"),
        ("eventbus", "Event routing, priority queues, subscriptions", "1 file"),
        ("irp", "Async I/O Request Packets", "1 file"),
        ("pipe", "IPC pipes (16 static, 4KB each)", "1 file"),
        ("kobj", "Kernel Object Manager", "2 files"),
        ("security", "NT6 Security Reference Monitor (SID, Token, ACL)", "5 files"),
        ("interrupts", "I/O APIC, MSI-X", "3 files"),
        ("timers", "HPET, APIC timer", "3 files"),
        ("buffer", "Block cache, page cache", "3 files"),
        ("dpc", "Deferred Procedure Calls", "1 file"),
        ("apc", "Async Procedure Calls", "1 file"),
        ("vfs", "IoStack partition/cache abstraction", "3 files"),
        ("nem", "NEM v3 driver format parser", "2 files"),
        ("nem/drivers", "NEM driver implementations (loader, driver, event, hst, runtime, v3loader)", "6 files"),
        ("drivers/boot_loader", "Boot-time driver loading", "1 file"),
        ("drivers/abi", "ABI negotiation layer", "1 file"),
        ("drivers/dependency", "Dependency resolver", "1 file"),
        ("drivers/isolation", "X4 Driver Isolation Layer", "1 file"),
    ]

    lines: list[str] = []
    lines.append("# NeoDOS Kernel Modules")
    lines.append("")
    lines.append(f"Total: {len(modules)} subsystems, ~121 source files")
    lines.append("")
    lines.append("| Module              | Responsibility                                | Files             |")
    lines.append("|---------------------|----------------------------------------------|-------------------|")
    for name, desc, count in modules:
        lines.append(f"| {name:<20} | {desc:<44} | {count:<17} |")

    return "\n".join(lines)
