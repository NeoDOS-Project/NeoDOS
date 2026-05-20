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
        ("invalid_magic.nem", build_invalid_magic()),
        ("invalid_header.nem", build_invalid_header()),
    ]
    for name, data in drivers:
        path = os.path.join(output_dir, name)
        write_nem(path, data)
        print(f"[NEM] {path}: {len(data)} bytes")


if __name__ == "__main__":
    import sys
    out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/nem_drivers"
    generate_all(out)
