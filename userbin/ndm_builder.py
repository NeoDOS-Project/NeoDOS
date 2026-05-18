#!/usr/bin/env python3
"""
ndm_builder.py — Shared NDM v1 module builder.

Usage:
    from ndm_builder import build_ndm, NdmHeader, ModuleType

    hdr = NdmHeader(name="MYDRV", module_type=ModuleType.DRIVER)
    ndm = build_ndm(hdr, code_bytes, data_bytes)
    with open("mydrv.ndm", "wb") as f:
        f.write(ndm)
"""

import struct

NDM_MAGIC = 0x004D444E
NDM_ABI_VERSION = 1
NDM_HEADER_SIZE = 64


class ModuleType:
    DRIVER = 0
    FILESYSTEM = 1
    SHELL_EXTENSION = 2


class NdmHeader:
    """Build a 64-byte NDM v1 header."""

    def __init__(self, name: str, module_type: int = ModuleType.DRIVER,
                 compat_flags: int = 0):
        assert len(name) <= 15, f"Module name too long: {name}"
        self.name = name.ljust(16, '\x00')[:16].encode('ascii')
        self.module_type = module_type
        self.compat_flags = compat_flags

    def encode(self, code_size: int, data_size: int) -> bytes:
        code_offset = NDM_HEADER_SIZE
        data_offset = code_offset + code_size
        hdr = struct.pack('<I', NDM_MAGIC)          # magic
        hdr += struct.pack('<I', NDM_ABI_VERSION)    # version
        hdr += struct.pack('<B', self.module_type)    # module_type
        hdr += struct.pack('<B', 0)                   # reserved1
        hdr += struct.pack('<H', NDM_HEADER_SIZE)     # header_size
        hdr += struct.pack('<I', code_offset)         # entry_offset = code_offset
        hdr += struct.pack('<I', code_offset)         # code_offset
        hdr += struct.pack('<I', code_size)           # code_size
        hdr += struct.pack('<I', data_offset)         # data_offset
        hdr += struct.pack('<I', data_size)           # data_size
        hdr += struct.pack('<I', NDM_ABI_VERSION)     # api_version
        hdr += struct.pack('<I', 0)                   # _reserved2
        hdr += self.name                              # name (16 bytes)
        hdr += struct.pack('<B', self.compat_flags)   # compat_flags
        hdr += b'\x00' * 7                           # _padding
        assert len(hdr) == NDM_HEADER_SIZE
        return hdr


def build_ndm(header: NdmHeader, code: bytes, data: bytes = b'') -> bytes:
    """Build a complete NDM binary from header + code + data."""
    hdr_bytes = header.encode(len(code), len(data))
    return hdr_bytes + code + data
