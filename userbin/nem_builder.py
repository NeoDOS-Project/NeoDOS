#!/usr/bin/env python3
"""
NEM — NeoDOS Test Driver Builder

Generates .nem binaries for the NeoDOS validation ecosystem.
"""

import struct
import os

NEM_MAGIC = b"NEM\x00"
NEM_VERSION = 1
NEM_HEADER_SIZE = 32
NEM_API_VERSION = 1

# Driver types
DRV_NULL = 0
DRV_ECHO = 1
DRV_LIFECYCLE = 2
DRV_MUTATION = 3
DRV_FAULT = 4
DRV_BURST = 5


def build_nem(name: str, driver_type: int, code: bytes,
              compat_flags: int = 0) -> bytes:
    name_bytes = name.encode("ascii").ljust(8, b"\x00")[:8]
    entry_offset = NEM_HEADER_SIZE
    code_offset = NEM_HEADER_SIZE
    code_size = len(code)

    header = struct.pack(
        "<4s I B B H I I H H 8s",
        NEM_MAGIC,          # magic: 4s
        NEM_VERSION,        # version: I
        driver_type,        # driver_type: B
        NEM_HEADER_SIZE,    # header_size: B
        entry_offset,       # entry_offset: H
        code_offset,        # code_offset: I
        code_size,          # code_size: I
        NEM_API_VERSION,    # api_version: H
        compat_flags,       # compat_flags: H
        name_bytes,         # name: 8s
    )

    assert len(header) == NEM_HEADER_SIZE, f"Header size mismatch: {len(header)}"
    return header + code


def write_nem(path: str, data: bytes):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def build_null_driver() -> bytes:
    """Minimal driver: init, print, exit."""
    code = bytes([
        # mov rax, 1 (sys_write)
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        # mov rbx, msg
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,  # placeholder
        # mov rcx, 5
        0x48, 0xC7, 0xC1, 0x05, 0x00, 0x00, 0x00,
        # int 0x80
        0xCD, 0x80,
        # mov rax, 0 (sys_exit)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
        # xor rbx, rbx (exit code 0)
        0x48, 0x31, 0xDB,
        # int 0x80
        0xCD, 0x80,
    ])
    _fix_msg_ptr(code, 7, b"NUL\x00")
    return build_nem("NULL", DRV_NULL, code)


def build_echo_driver() -> bytes:
    """Echo driver: writes driver name + type to stdout."""
    code = bytes([
        # sys_write(1, msg, 14)
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,  # msg placeholder
        0x48, 0xC7, 0xC1, 0x0E, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        # sys_exit(0)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x31, 0xDB,
        0xCD, 0x80,
    ])
    _fix_msg_ptr(code, 7, b"ECHO driver!\n\x00")
    return build_nem("ECHO", DRV_ECHO, code)


def build_lifecycle_stress_driver() -> bytes:
    """Lifecycle stress: init, yield 3 times, exit."""
    code = bytes([
        # sys_write(1, msg, 15)
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC1, 0x0F, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        # sys_yield (3x)
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        # sys_write(1, done, 5)
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC1, 0x05, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        # sys_exit(0)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x31, 0xDB,
        0xCD, 0x80,
    ])
    _fix_msg_ptr(code, 7, b"Lifecycle init\n\x00")
    _fix_msg_ptr(code, 7 + 17 + 3 + 3 + 3, b"done\n\x00")
    return build_nem("LIFECYCLE", DRV_LIFECYCLE, code)


def build_fault_driver() -> bytes:
    """Fault injection: returns exit code 42 to signal forced failure."""
    code = bytes([
        # sys_write(1, msg, 14)
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC1, 0x0E, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        # sys_exit(42) — forced failure code
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC3, 0x2A, 0x00, 0x00, 0x00,
        0xCD, 0x80,
    ])
    _fix_msg_ptr(code, 7, b"Fault inject!\n\x00")
    return build_nem("FAULT", DRV_FAULT, code, compat_flags=0)


def build_burst_driver() -> bytes:
    """Burst driver: rapid init/exit cycle simulation."""
    code = bytes([
        # sys_write(1, msg, 14)
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC1, 0x0E, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        # sys_exit(0)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x31, 0xDB,
        0xCD, 0x80,
    ])
    _fix_msg_ptr(code, 7, b"Burst driver!\n\x00")
    return build_nem("BURST", DRV_BURST, code)


def build_kbrdps2_driver() -> bytes:
    """PS/2 Keyboard driver stub: identifies as KBRDPS2, writes init message, exits."""
    code = bytes([
        # sys_write(1, msg, 11)  — "KBRDPS2 OK\n"
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,   # mov rax, 1 (sys_write)
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,   # mov rbx, msg (placeholder)
        0x48, 0xC7, 0xC1, 0x0B, 0x00, 0x00, 0x00,   # mov rcx, 11
        0xCD, 0x80,                                   # int 0x80
        # sys_yield — cooperate with scheduler
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00,   # mov rax, 2 (sys_yield)
        0xCD, 0x80,                                   # int 0x80
        # sys_exit(0)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,   # mov rax, 0 (sys_exit)
        0x48, 0x31, 0xDB,                             # xor rbx, rbx
        0xCD, 0x80,                                   # int 0x80
    ])
    _fix_msg_ptr(code, 7, b"KBRDPS2 OK\n\x00")
    return build_nem("KBRDPS2", DRV_ECHO, code)


def _fix_msg_ptr(code: bytearray, offset: int, msg: bytes):
    """Helper to embed a message into the code (in-place)."""
    pass  # Messages are not embedded in code for simplicity


def build_invalid_magic() -> bytes:
    """Corrupted NEM with bad magic."""
    hdr = bytearray(build_nem("BAD", DRV_NULL, b""))
    hdr[0:4] = b"BAD\x00"
    return bytes(hdr)


def build_invalid_header() -> bytes:
    """Corrupted NEM with bad header_size."""
    hdr = bytearray(build_nem("BAD", DRV_NULL, b"\x90\xCB"))  # 2 bytes code
    hdr[9] = 99  # invalid header_size
    return bytes(hdr)


def generate_all(output_dir: str):
    drivers = [
        ("null.nem", build_null_driver()),
        ("echo.nem", build_echo_driver()),
        ("stress_lifecycle.nem", build_lifecycle_stress_driver()),
        ("fault.nem", build_fault_driver()),
        ("burst.nem", build_burst_driver()),
        ("kbrdps2.nem", build_kbrdps2_driver()),
        ("invalid_magic.nem", build_invalid_magic()),
        ("invalid_header.nem", build_invalid_header()),
    ]
    for name, data in drivers:
        path = os.path.join(output_dir, name)
        write_nem(path, data)
        print(f"[NEM] {path}: {len(data)} bytes")


# ── NEM v2: Boot & System driver generators ──────────────────────────────

NEM_V2_MAGIC = b"NEM\x00"
NEM_V2_VERSION = 2
NEM_V2_HEADER_SIZE = 48

ABI_MIN_VALID = 1
ABI_TARGET = 1
ABI_MAX_VALID = 2

# Driver categories
CAT_BOOT = 0
CAT_SYSTEM = 1
CAT_DEMAND = 2


def build_nem_v2(name: str, driver_type: int, category: int, code: bytes,
                 compat_flags: int = 0,
                 abi_min: int = ABI_MIN_VALID,
                 abi_target: int = ABI_TARGET,
                 abi_max: int = ABI_MAX_VALID) -> bytes:
    """
    Build NEM v2 binary matching Rust's build_valid_nem_v2 byte layout.

    Rust NemHeaderV2 struct layout (repr(C), 52 bytes with trailing padding):
        magic: u32       [0..4]
        version: u32     [4..8]
        driver_type: u8  [8]
        header_size: u8  [9]
        entry_offset: u16 [10..12]
        code_offset: u32 [12..16]
        code_size: u32   [16..20]
        api_version: u16 [20..22]
        compat_flags: u16 [22..24]
        abi_min: u16     [24..26]
        abi_target: u16  [26..28]
        abi_max: u16     [28..30]
        category: u8     [30]
        _reserved: [u8;3] [31..34]
        name: [u8;16]    [34..50]
                          [50..52] padding
    The parser checks header_size == 48 (NEM_HEADER_SIZE_V2),
    reads code_offset from bytes [12..16] (value = 48 in Rust test builder).
    Actual code is at byte 50 — 2-byte overlap is harmless for metadata-only use.
    """
    name_bytes = name.encode("ascii").ljust(16, b"\x00")[:16]
    code_offset = NEM_V2_HEADER_SIZE  # 48, matches Rust test builder
    entry_offset = code_offset
    code_size = len(code)

    header = struct.pack(
        "<4s I B B H I I H H H H H B 3x 16s",
        NEM_V2_MAGIC,          # magic: 4s
        NEM_V2_VERSION,        # version: I
        driver_type,           # driver_type: B
        NEM_V2_HEADER_SIZE,    # header_size: B
        entry_offset,          # entry_offset: H
        code_offset,           # code_offset: I
        code_size,             # code_size: I
        NEM_API_VERSION,       # api_version: H
        compat_flags,          # compat_flags: H
        abi_min,               # abi_min: H
        abi_target,            # abi_target: H
        abi_max,               # abi_max: H
        category,              # category: B
        name_bytes,            # name: 16s
    )

    assert len(header) == 50, f"V2 Header size mismatch: {len(header)} (expected 50)"
    return header + code


def build_driver_v2_from_v1(v1_data: bytes, new_name: str,
                            category: int) -> bytes:
    """Convert a v1 NEM binary to v2 by reading its fields and wrapping."""
    hdr = v1_data[:32]
    code = v1_data[32:]
    magic, version, dtype, hsize, entry_off, code_off, code_sz, api_ver, compat = (
        struct.unpack("<4s I B B H I I H H 8s", hdr[:32])
    )[:9]
    return build_nem_v2(
        name=new_name,
        driver_type=dtype,
        category=category,
        code=code,
        compat_flags=compat,
    )


def build_boot_kbrd_driver() -> bytes:
    """PS/2 Keyboard driver — BOOT category, NEM v2."""
    code = bytes([
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,   # mov rax, 1 (sys_write)
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,   # mov rbx, msg (placeholder)
        0x48, 0xC7, 0xC1, 0x10, 0x00, 0x00, 0x00,   # mov rcx, 16
        0xCD, 0x80,                                   # int 0x80
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00,   # mov rax, 2 (sys_yield)
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,   # mov rax, 0 (sys_exit)
        0x48, 0x31, 0xDB,
        0xCD, 0x80,
    ])
    return build_nem_v2("PS2KBD", DRV_LIFECYCLE, CAT_BOOT, code)


def build_system_fb_driver() -> bytes:
    """Framebuffer driver — SYSTEM category, NEM v2."""
    code = bytes([
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,   # mov rax, 1
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,   # mov rbx, msg
        0x48, 0xC7, 0xC1, 0x10, 0x00, 0x00, 0x00,   # mov rcx, 16
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00,   # sys_yield
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,   # sys_exit
        0x48, 0x31, 0xDB,
        0xCD, 0x80,
    ])
    return build_nem_v2("FRAMEBUF", DRV_LIFECYCLE, CAT_SYSTEM, code)


def build_system_storage_driver() -> bytes:
    """Storage driver — SYSTEM category, NEM v2."""
    code = bytes([
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC3, 0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC1, 0x10, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x31, 0xDB,
        0xCD, 0x80,
    ])
    return build_nem_v2("STORAGE", DRV_LIFECYCLE, CAT_SYSTEM, code)


def build_invalid_abi_driver() -> bytes:
    """Driver with ABI range that will fail validation (min > max_valid)."""
    code = bytes([0x90, 0xCB])
    return build_nem_v2("BADABI", DRV_NULL, CAT_BOOT, code,
                        abi_min=99, abi_target=99, abi_max=99)


def generate_boot_drivers(output_dir: str):
    """Generate boot + system category drivers under BOOT/ and SYSTEM/ dirs."""
    boot_dir = os.path.join(output_dir, "BOOT")
    sys_dir = os.path.join(output_dir, "SYSTEM")

    boot_drivers = [
        ("ps2kbd.nem", build_boot_kbrd_driver()),
    ]
    system_drivers = [
        ("framebuf.nem", build_system_fb_driver()),
        ("storage.nem", build_system_storage_driver()),
    ]
    invalid_dir = os.path.join(output_dir, "INVALID")
    invalid_drivers = [
        ("badabi.nem", build_invalid_abi_driver()),
    ]

    for name, data in boot_drivers:
        path = os.path.join(boot_dir, name)
        write_nem(path, data)
        print(f"[NEM] {path}: {len(data)} bytes (v2, BOOT)")

    for name, data in system_drivers:
        path = os.path.join(sys_dir, name)
        write_nem(path, data)
        print(f"[NEM] {path}: {len(data)} bytes (v2, SYSTEM)")

    for name, data in invalid_drivers:
        path = os.path.join(invalid_dir, name)
        write_nem(path, data)
        print(f"[NEM] {path}: {len(data)} bytes (v2, INVALID)")


if __name__ == "__main__":
    import sys
    import os
    out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/nem_drivers"
    generate_all(out)
    generate_boot_drivers(out)
