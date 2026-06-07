"""
parsers/nem_parser.py — NEM v3 Driver Format Parser.

Parses NeoDOS NEM v3 standalone driver binaries (.nem files).
80-byte header with sections, relocations, symbols, and string table.
Used for offline analysis of NEM driver images.
"""

import struct
from dataclasses import dataclass
from typing import Optional


# ── Constants ──

NEM_MAGIC_V3 = b"NEM3"
NEM_HEADER_SIZE_V3 = 80
NEM_RELOC_SIZE = 12
NEM_SYMBOL_SIZE = 16

R_NEM_NONE = 0
R_NEM_64 = 1
R_NEM_PC32 = 2
R_NEM_32 = 3
R_NEM_32S = 4
R_NEM_PLT32 = 5

RELOC_NAMES = {
    R_NEM_NONE: "R_NEM_NONE",
    R_NEM_64: "R_NEM_64",
    R_NEM_PC32: "R_NEM_PC32",
    R_NEM_32: "R_NEM_32",
    R_NEM_32S: "R_NEM_32S",
    R_NEM_PLT32: "R_NEM_PLT32",
}

DRIVER_TYPE_NAMES = {
    0: "Null",
    1: "Echo",
    2: "Lifecycle",
    3: "Mutation",
    4: "Fault",
    5: "Burst",
}

CATEGORY_NAMES = {
    0: "BOOT",
    1: "SYSTEM",
    2: "DEMAND",
}

HST_EXPORTS = [
    "hst_push_input_byte",
    "hst_log",
    "hst_get_ticks",
    "hst_ack_irq",
    "hst_push_event",
    "hst_inb", "hst_outb", "hst_inw", "hst_outw", "hst_inl", "hst_outl",
    "hst_register_block_device",
    "hst_unregister_block_device",
]

ABI_MIN_VALID = 1
ABI_TARGET = 1
ABI_MAX_VALID = 2


# ── Data Structures ──

@dataclass
class NemHeaderV3:
    magic: bytes
    version: int
    header_size: int
    flags: int
    abi_min: int
    abi_target: int
    abi_max: int
    driver_type: int
    category: int
    text_size: int
    rodata_size: int
    data_size: int
    bss_size: int
    total_mem_size: int
    entry_init: int
    entry_event: int
    entry_fini: int
    num_relocs: int
    relocs_offset: int
    syms_offset: int
    strtab_offset: int
    name_offset: int


@dataclass
class NemReloc:
    offset: int
    section: int
    r_type: int
    sym_idx: int
    addend: int


@dataclass
class NemSymbol:
    name_off: int
    value: int
    section: int
    info: int


@dataclass
class NemDriver:
    header: NemHeaderV3
    name: str
    text: bytes
    rodata: bytes
    data: bytes
    bss_size: int
    relocs: list[NemReloc]
    symbols: list[NemSymbol]
    strtab: bytes
    file_size: int

    @property
    def driver_type_name(self) -> str:
        return DRIVER_TYPE_NAMES.get(self.header.driver_type, f"UNKNOWN({self.header.driver_type})")

    @property
    def category_name(self) -> str:
        return CATEGORY_NAMES.get(self.header.category, f"UNKNOWN({self.header.category})")

    @property
    def abi_compatible(self) -> bool:
        h = self.header
        return (
            h.abi_min <= ABI_MAX_VALID
            and h.abi_max >= ABI_MIN_VALID
            and ABI_MIN_VALID <= h.abi_target <= ABI_MAX_VALID
        )

    @property
    def abi_warnings(self) -> list[str]:
        warnings = []
        h = self.header
        if h.abi_max < ABI_TARGET:
            warnings.append(f"Driver ABI max ({h.abi_max}) < kernel target ({ABI_TARGET})")
        if h.abi_target > ABI_TARGET:
            warnings.append(f"Driver targets newer ABI ({h.abi_target}) than kernel ({ABI_TARGET})")
        return warnings

    def unresolved_symbols(self) -> list[str]:
        """Return list of symbols that reference HST exports or are truly unresolved."""
        result = []
        for sym in self.symbols:
            if sym.section == 0xFFFF:  # UNDEF
                name = self._get_str(sym.name_off)
                if name:
                    result.append(name)
        return result

    def _get_str(self, name_off: int) -> str:
        if name_off >= len(self.strtab):
            return ""
        end = self.strtab.find(b"\x00", name_off)
        if end == -1:
            return self.strtab[name_off:].decode("ascii", errors="replace")
        return self.strtab[name_off:end].decode("ascii", errors="replace")

    def dump(self) -> str:
        h = self.header
        lines = [
            f"NEM v3 Driver: '{self.name}'",
            f"  Magic:           {self.magic_str()}",
            f"  Version:         {h.version}",
            f"  Header size:     {h.header_size}",
            f"  Flags:           0x{h.flags:08X}",
            f"  ABI:             min={h.abi_min} target={h.abi_target} max={h.abi_max}",
            f"  Compatible:      {'YES' if self.abi_compatible else 'NO'}",
        ]
        if self.abi_warnings:
            for w in self.abi_warnings:
                lines.append(f"  ABI Warning:      {w}")
        lines += [
            f"  Driver type:      {self.driver_type_name} (0x{h.driver_type:04X})",
            f"  Category:         {self.category_name} (0x{h.category:04X})",
            f"  Memory:           text={h.text_size} rodata={h.rodata_size} "
            f"data={h.data_size} bss={h.bss_size} total={h.total_mem_size}",
            f"  Entry points:     init=0x{h.entry_init:08X} event=0x{h.entry_event:08X} "
            f"fini=0x{h.entry_fini:08X}",
            f"  Relocs:           {h.num_relocs} @ offset 0x{h.relocs_offset:08X}",
            f"  Symbols:          {len(self.symbols)} @ offset 0x{h.syms_offset:08X}",
            f"  String table:     {len(self.strtab)} bytes @ offset 0x{h.strtab_offset:08X}",
            f"  File size:        {self.file_size} bytes",
        ]
        if self.symbols:
            lines.append(f"\n  Symbols ({len(self.symbols)}):")
            for i, sym in enumerate(self.symbols):
                name = self._get_str(sym.name_off)
                sect = f"0x{sym.section:04X}" if sym.section == 0xFFFF else f"{sym.section}"
                lines.append(f"    [{i}] '{name}' value=0x{sym.value:08X} section={sect} info=0x{sym.info:02X}")
        if self.relocs:
            lines.append(f"\n  Relocations ({len(self.relocs)}):")
            for i, rel in enumerate(self.relocs):
                rtype = RELOC_NAMES.get(rel.r_type, f"R_UNKNOWN({rel.r_type})")
                sect = f"section={rel.section}"
                sym = f"sym={rel.sym_idx}" if rel.sym_idx != 0xFF else "no-sym"
                lines.append(f"    [{i}] @0x{rel.offset:08X} {rtype:15s} {sect} {sym} addend={rel.addend:+d}")
        unk = self.unresolved_symbols()
        if unk:
            is_hst = [s for s in unk if s.startswith("hst_")]
            other = [s for s in unk if not s.startswith("hst_")]
            if is_hst:
                lines.append(f"\n  HST exports needed ({len(is_hst)}): {', '.join(is_hst)}")
            if other:
                lines.append(f"\n  Other unresolved ({len(other)}): {', '.join(other)}")
        return "\n".join(lines)

    def magic_str(self) -> str:
        try:
            return self.header.magic.decode("ascii")
        except:
            return self.header.magic.hex()


# ── Parser ──

def parse_nem_v3(data: bytes) -> Optional[NemDriver]:
    """Parse a NEM v3 binary blob. Returns NemDriver or None."""
    if len(data) < NEM_HEADER_SIZE_V3:
        return None

    magic = data[0:4]
    if magic != NEM_MAGIC_V3:
        return None

    (version, header_size, flags, abi_min, abi_target, abi_max,
     driver_type, category, text_size, rodata_size, data_size,
     bss_size, total_mem_size, entry_init, entry_event, entry_fini,
     num_relocs, relocs_offset, syms_offset, strtab_offset, name_offset) = \
        struct.unpack_from("<IIIHHHHHIIIIIIIIIIIII", data, 4)

    if version != 3:
        return None

    hdr = NemHeaderV3(
        magic=magic, version=version, header_size=header_size, flags=flags,
        abi_min=abi_min, abi_target=abi_target, abi_max=abi_max,
        driver_type=driver_type, category=category,
        text_size=text_size, rodata_size=rodata_size, data_size=data_size,
        bss_size=bss_size, total_mem_size=total_mem_size,
        entry_init=entry_init, entry_event=entry_event, entry_fini=entry_fini,
        num_relocs=num_relocs, relocs_offset=relocs_offset,
        syms_offset=syms_offset, strtab_offset=strtab_offset,
        name_offset=name_offset,
    )

    # Validate total size
    file_size = len(data)

    # Name
    name_off = name_offset
    if name_off >= file_size:
        return None
    name_end = data.find(b"\x00", name_off)
    if name_end == -1:
        name = data[name_off:].decode("ascii", errors="replace")
    else:
        name = data[name_off:name_end].decode("ascii", errors="replace")
    if not name:
        return None

    # Relocations
    relocs = []
    if num_relocs > 0:
        off = relocs_offset
        for _ in range(num_relocs):
            if off + NEM_RELOC_SIZE > file_size:
                break
            roff, rsect, rtype, sym_idx, addend = \
                struct.unpack_from("<IHbBi", data, off)
            relocs.append(NemReloc(roff, rsect, rtype & 0xFF, sym_idx & 0xFF, addend))
            off += NEM_RELOC_SIZE

    # Symbols
    syms = []
    if syms_offset > 0 and strtab_offset > syms_offset:
        syms_raw_size = strtab_offset - syms_offset
        num_syms = syms_raw_size // NEM_SYMBOL_SIZE
        off = syms_offset
        for _ in range(num_syms):
            if off + NEM_SYMBOL_SIZE > file_size:
                break
            name_off2, value, sect, info = \
                struct.unpack_from("<IIHBxI", data, off)
            syms.append(NemSymbol(name_off2, value, sect, info))
            off += NEM_SYMBOL_SIZE

    # String table
    # Infer size from symbol name offsets
    strtab_size = 0
    for sym in syms:
        abs_off = strtab_offset + sym.name_off
        if abs_off >= file_size:
            continue
        end = data.find(b"\x00", abs_off)
        sz = sym.name_off + (end - abs_off) + 1 if end != -1 else sym.name_off + 8
        if sz > strtab_size:
            strtab_size = sz
    strtab_end = strtab_offset + strtab_size
    if strtab_end > file_size:
        strtab_end = file_size
    strtab = data[strtab_offset:strtab_end]

    # Sections
    sec_start = strtab_end if strtab_end > 0 else header_size
    text_end = sec_start + text_size
    rodata_end = text_end + rodata_size
    data_end = rodata_end + data_size

    text = data[sec_start:text_end] if text_size > 0 and text_end <= file_size else b""
    rodata = data[text_end:rodata_end] if rodata_size > 0 and rodata_end <= file_size else b""
    raw_data = data[rodata_end:data_end] if data_size > 0 and data_end <= file_size else b""

    return NemDriver(
        header=hdr, name=name,
        text=text, rodata=rodata, data=raw_data,
        bss_size=bss_size,
        relocs=relocs, symbols=syms, strtab=strtab,
        file_size=file_size,
    )


def parse_nem_file(path: str) -> Optional[NemDriver]:
    """Parse a NEM v3 driver from file path."""
    with open(path, "rb") as f:
        data = f.read()
    return parse_nem_v3(data)


def analyze_nem_driver(path: str) -> Optional[dict]:
    """Convenience: parse a NEM driver and return structured analysis dict."""
    drv = parse_nem_file(path)
    if drv is None:
        return None
    return {
        "name": drv.name,
        "type": drv.driver_type_name,
        "category": drv.category_name,
        "abi_compatible": drv.abi_compatible,
        "abi": {
            "min": drv.header.abi_min,
            "target": drv.header.abi_target,
            "max": drv.header.abi_max,
        },
        "memory": {
            "text": drv.header.text_size,
            "rodata": drv.header.rodata_size,
            "data": drv.header.data_size,
            "bss": drv.bss_size,
            "total": drv.header.total_mem_size,
        },
        "entrypoints": {
            "init": drv.header.entry_init,
            "event": drv.header.entry_event,
            "fini": drv.header.entry_fini,
        },
        "num_relocs": len(drv.relocs),
        "num_symbols": len(drv.symbols),
        "unresolved": drv.unresolved_symbols(),
        "abi_warnings": drv.abi_warnings,
        "file_size": drv.file_size,
    }
