"""
tools/libneodos_tools.py — libneodos API Analysis Tools.

Analyzes the libneodos userspace library API, its export table (AbiTable),
syscall wrappers, and compatibility with runtime modules.
"""

from pathlib import Path


# ── Configuration ──

NEODOS_ROOT: Path = None


def configure(root_dir: str):
    global NEODOS_ROOT
    NEODOS_ROOT = Path(root_dir)


# ── AbiTable specification (mirrors libneodos/src/export.rs) ──

ABI_TABLE_FIELDS = [
    # (field_name, rust_type, c_type, size_bytes)
    ("sys_exit", "extern \"C\" fn(u32) -> !", "void (*)(uint32_t)", 8),
    ("sys_write", "extern \"C\" fn(u8, *const u8, usize) -> i64", "int64_t (*)(uint8_t, const uint8_t*, size_t)", 8),
    ("sys_read", "extern \"C\" fn(u8, *mut u8, usize) -> i64", "int64_t (*)(uint8_t, uint8_t*, size_t)", 8),
    ("sys_getpid", "extern \"C\" fn() -> u32", "uint32_t (*)()", 8),
    ("sys_yield", "extern \"C\" fn()", "void (*)()", 8),
    ("sys_open", "extern \"C\" fn(*const u8) -> i64", "int64_t (*)(const uint8_t*)", 8),
    ("sys_readfile", "extern \"C\" fn(u8, *mut u8, usize) -> i64", "int64_t (*)(uint8_t, uint8_t*, size_t)", 8),
    ("sys_writefile", "extern \"C\" fn(u8, *const u8, usize) -> i64", "int64_t (*)(uint8_t, const uint8_t*, size_t)", 8),
    ("sys_close", "extern \"C\" fn(u8) -> i64", "int64_t (*)(uint8_t)", 8),
    ("sys_brk", "extern \"C\" fn(u64) -> i64", "int64_t (*)(uint64_t)", 8),
    ("sys_mmap", "extern \"C\" fn(u64, u64, u16, u16, u64) -> i64", "int64_t (*)(uint64_t, uint64_t, uint16_t, uint16_t, uint64_t)", 8),
    ("sys_munmap", "extern \"C\" fn(u64, u64) -> i64", "int64_t (*)(uint64_t, uint64_t)", 8),
    ("sys_pipe", "extern \"C\" fn(*mut u64) -> i64", "int64_t (*)(uint64_t*)", 8),
    ("sys_dup2", "extern \"C\" fn(u8, u8) -> i64", "int64_t (*)(uint8_t, uint8_t)", 8),
    ("sys_waitpid", "extern \"C\" fn(u32) -> i64", "int64_t (*)(uint32_t)", 8),
    ("stdout_write", "extern \"C\" fn(*const u8, usize) -> i64", "int64_t (*)(const uint8_t*, size_t)", 8),
    ("stderr_write", "extern \"C\" fn(*const u8, usize) -> i64", "int64_t (*)(const uint8_t*, size_t)", 8),
    ("stdin_read", "extern \"C\" fn(*mut u8, usize) -> i64", "int64_t (*)(uint8_t*, size_t)", 8),
    ("dll_print", "extern \"C\" fn(*const u8, usize)", "void (*)(const uint8_t*, size_t)", 8),
    ("dll_eprint", "extern \"C\" fn(*const u8, usize)", "void (*)(const uint8_t*, size_t)", 8),
    ("file_open", "extern \"C\" fn(*const u8) -> i64", "int64_t (*)(const uint8_t*)", 8),
    ("file_read", "extern \"C\" fn(u8, *mut u8, usize) -> i64", "int64_t (*)(uint8_t, uint8_t*, size_t)", 8),
    ("file_write", "extern \"C\" fn(u8, *const u8, usize) -> i64", "int64_t (*)(uint8_t, const uint8_t*, size_t)", 8),
    ("brk", "extern \"C\" fn(u64) -> i64", "int64_t (*)(uint64_t)", 8),
    ("sbrk", "extern \"C\" fn(i64) -> i64", "int64_t (*)(int64_t)", 8),
    ("mmap", "extern \"C\" fn(u64, u16, u16) -> i64", "int64_t (*)(uint64_t, uint16_t, uint16_t)", 8),
    ("munmap", "extern \"C\" fn(u64, u64) -> i64", "int64_t (*)(uint64_t, uint64_t)", 8),
    ("err_einval", "i64", "int64_t", 8),
    ("err_enonet", "i64", "int64_t", 8),
    ("err_enomem", "i64", "int64_t", 8),
    ("err_eacces", "i64", "int64_t", 8),
    ("err_ebadf", "i64", "int64_t", 8),
    ("err_efault", "i64", "int64_t", 8),
    ("err_enosys", "i64", "int64_t", 8),
    ("err_eagain", "i64", "int64_t", 8),
    ("err_epipe", "i64", "int64_t", 8),
    ("err_eenoent", "i64", "int64_t", 8),
    ("err_enotdir", "i64", "int64_t", 8),
    ("err_eisdir", "i64", "int64_t", 8),
    ("err_eio", "i64", "int64_t", 8),
    ("err_enodev", "i64", "int64_t", 8),
    ("err_ebusy", "i64", "int64_t", 8),
    ("sys_chdir", "extern \"C\" fn(*const u8) -> i64", "int64_t (*)(const uint8_t*)", 8),
    ("sys_getcwd", "extern \"C\" fn(*mut u8, usize) -> i64", "int64_t (*)(uint8_t*, size_t)", 8),
    ("sys_loadlib", "extern \"C\" fn(*const u8) -> i64", "int64_t (*)(const uint8_t*)", 8),
    ("version", "u32", "uint32_t", 4),
    ("_reserved", "[u64; 2]", "uint64_t[2]", 16),
]

ABI_TABLE_SIZE = sum(f[3] for f in ABI_TABLE_FIELDS)  # should be 448 bytes (56 * 8)
ABI_TABLE_MAX_FIELDS = 56


# ── Syscall mapping (syscall number → libneodos function) ──

SYSCALL_MAP = [
    ("exit", 0, "sys_exit"),
    ("write", 1, "sys_write"),
    ("yield", 2, "sys_yield"),
    ("getpid", 3, "sys_getpid"),
    ("read", 4, "sys_read"),
    ("pipe", 5, "sys_pipe"),
    ("dup2", 6, "sys_dup2"),
    ("waitpid", 9, "sys_waitpid"),
    ("open", 10, "sys_open"),
    ("readfile", 11, "sys_readfile"),
    ("writefile", 12, "sys_writefile"),
    ("close", 13, "sys_close"),
    ("ioctl", 14, "sys_ioctl"),
    ("register_device", 15, "sys_register_device"),
    ("chdir", 16, "sys_chdir"),
    ("getcwd", 17, "sys_getcwd"),
    ("brk", 18, "sys_brk"),
    ("mmap", 19, "sys_mmap"),
    ("munmap", 20, "sys_munmap"),
    ("loadlib", 21, "sys_loadlib"),
]


# ── Error constants ──

ERROR_CONSTANTS = {
    -1: "EINVAL  (Invalid argument)",
    -2: "ENOENT  (No such entry)",
    -3: "ENOMEM  (Out of memory)",
    -4: "EACCES  (Permission denied)",
    -5: "EBADF   (Bad file descriptor)",
    -6: "EFAULT  (Bad address)",
    -7: "ENOSYS  (Not implemented)",
    -8: "EAGAIN  (Try again / would block)",
    -9: "EPIPE   (Broken pipe)",
    -10: "EEXIST  (File exists)",
    -11: "ENOTDIR (Not a directory)",
    -12: "EISDIR  (Is a directory)",
    -13: "EIO     (I/O error)",
    -14: "ENODEV  (No such device)",
    -15: "EBUSY   (Device busy)",
}


# ── Tool Implementations ──

def analyze_libneodos_api(detail: str = "summary") -> str:
    """Analyze the libneodos userspace API and its ABI table."""
    lines = [
        "libneodos — NeoDOS Standard Userspace Library",
        "==============================================",
        f"ABI version: 1",
        f"Location:    NXL slot 0 at 0x1E000000 (auto-loaded at boot)",
        f"Format:      ELF64 NXL, loaded via sys_loadlib (RAX=21)",
        f"ABI Table:   56 fields, {ABI_TABLE_SIZE} bytes",
        "",
        "Modules:",
        "  export.rs   — AbiTable struct at DLL base",
        "  syscall.rs  — Raw syscall wrappers via ABI table",
        "  io.rs       — Stdout/Stdin/Stderr with fmt::Write",
        "  fs.rs       — File::open/read/write",
        "  mem.rs      — brk/sbrk/mmap/munmap",
        "  macros.rs   — print!/println!/eprint!/eprintln! (CRLF)",
        "",
        "Memory Model:",
        "  Code+stack:  0x400000 (4 MB user window, 32 × 128 KB slots)",
        "  Heap:        0x10000000 (32 MB, demand-paged 4 KB)",
        "  DLL region:  0x1E000000 (2 MB, 8 × 256 KB slots)",
        "  mmap region: 0x20000000 (32 MB, lazy allocation)",
        "",
        "Calling Convention (syscall):",
        "  RAX = syscall number",
        "  RBX = arg0, RCX = arg1, RDX = arg2, R8 = arg3, R9 = arg4",
        "  Return: RAX (≥ 0 success, < 0 error)",
        "  Trigger: INT 0x80",
    ]

    if detail == "full":
        lines.extend([
            "",
            "ABI Table Layout (at 0x1E000000):",
        ])
        offset = 0
        for i, (name, rtype, ctype, size) in enumerate(ABI_TABLE_FIELDS):
            lines.append(f"  [{i:2d}] +0x{offset:03X} {name:20s}  {rtype}")
            offset += size

        lines.extend([
            "",
            "Syscall → ABI Table Mapping:",
        ])
        for name, num, table_fn in SYSCALL_MAP:
            lines.append(f"  RAX={num:2d}  {name:12s}  →  AbiTable::{table_fn}")

        lines.extend([
            "",
            "Error Constants:",
        ])
        for code, desc in sorted(ERROR_CONSTANTS.items()):
            lines.append(f"  {code:4d}  {desc}")

    return "\n".join(lines)


def check_abi_compatibility(
    module_type: str = "elf",
    abi_min: int = None,
    abi_target: int = None,
    abi_max: int = None,
) -> str:
    """Check ABI compatibility between a module and the kernel/libneodos."""
    from ..parsers.nem_parser import ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID

    lines = [
        "ABI Compatibility Check:",
        f"  Kernel ABI range:   {ABI_MIN_VALID} – {ABI_MAX_VALID} (target: {ABI_TARGET})",
        f"  libneodos ABI:      {ABI_TARGET}",
        f"",
    ]

    if module_type == "nem" and abi_min is not None:
        compatible = (
            abi_min <= ABI_MAX_VALID
            and abi_max >= ABI_MIN_VALID
            and ABI_MIN_VALID <= abi_target <= ABI_MAX_VALID
        )
        lines.append(f"  Module ABI:          min={abi_min} target={abi_target} max={abi_max}")
        lines.append(f"  Compatible:          {'✓ YES' if compatible else '✗ NO'}")

        if not compatible:
            if abi_min > ABI_MAX_VALID:
                lines.append(f"    ✗ Driver requires ABI ≥{abi_min} but kernel max is {ABI_MAX_VALID}")
            if abi_max < ABI_MIN_VALID:
                lines.append(f"    ✗ Driver max ABI ({abi_max}) < kernel min ({ABI_MIN_VALID})")
            if abi_target < ABI_MIN_VALID or abi_target > ABI_MAX_VALID:
                lines.append(f"    ✗ Driver target ABI ({abi_target}) outside kernel range")
        else:
            if abi_max < ABI_TARGET:
                lines.append(f"    ⚠ Driver ABI max ({abi_max}) < kernel target ({ABI_TARGET})")
            if abi_target > ABI_TARGET:
                lines.append(f"    ⚠ Driver targets newer ABI ({abi_target}) than kernel ({ABI_TARGET})")

    elif module_type == "elf":
        lines.append(f"  Module type: ELF — ELF binaries don't carry ABI version info.")
        lines.append(f"  Compatibility is determined by the syscall ABI at runtime.")
        lines.append(f"  libneodos ABI v{ABI_TARGET} is the interface contract.")

    return "\n".join(lines)


def analyze_libneodos_coverage() -> str:
    """Analyze which kernel syscalls are wrapped in libneodos vs. which are missing."""
    syscall_names = {
        0: "exit", 1: "write", 2: "yield", 3: "getpid", 4: "read",
        5: "pipe", 6: "dup2", 9: "waitpid", 10: "open", 11: "readfile",
        12: "writefile", 13: "close", 14: "ioctl", 15: "register_device",
        16: "chdir", 17: "getcwd", 18: "brk", 19: "mmap", 20: "munmap", 21: "loadlib",
    }

    # libneodos syscall wrappers from export table
    libneodos_syscalls = {
        0: "sys_exit", 1: "sys_write", 2: "sys_yield", 3: "sys_getpid",
        4: "sys_read",
        5: "sys_pipe", 6: "sys_dup2",
        9: "sys_waitpid",
        10: "sys_open", 11: "sys_readfile", 12: "sys_writefile", 13: "sys_close",
        # 14, 15 NOT in AbiTable
        16: "sys_chdir", 17: "sys_getcwd",
        18: "sys_brk", 19: "sys_mmap", 20: "sys_munmap",
        21: "sys_loadlib",
    }

    table_fn_names = set(v for v in libneodos_syscalls.values())

    lines = ["libneodos Syscall Coverage Analysis:", ""]
    covered = 0
    total = len(syscall_names)

    for num in sorted(syscall_names):
        name = syscall_names[num]
        if num in libneodos_syscalls:
            wrapper = libneodos_syscalls[num]
            covered += 1
            lines.append(f"  ✓ RAX={num:2d}  {name:12s}  →  AbiTable::{wrapper}")
        else:
            lines.append(f"  ○ RAX={num:2d}  {name:12s}  (missing in AbiTable)")

    lines.append(f"\nCoverage: {covered}/{total} syscalls ({100 * covered // total}%)")
    if covered < total:
        missing = [n for num, n in sorted(syscall_names.items()) if num not in libneodos_syscalls]
        lines.append(f"Missing: {', '.join(missing)}")

    return "\n".join(lines)
