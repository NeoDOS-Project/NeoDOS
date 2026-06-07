"""
parsers/elf_parser.py — ELF64 Parser for NeoDOS DLL & User Binary Analysis.

Parses ELF64 binaries (little-endian, x86-64) to analyze DLL exports,
DLL imports, segment layout, and entry points.
"""

import struct
from dataclasses import dataclass, field
from typing import Optional


# ── ELF Constants ──

ELF_MAGIC = b"\x7fELF"
EM_X86_64 = 62
ET_EXEC = 2
ET_DYN = 3
ET_REL = 1

PT_NULL = 0
PT_LOAD = 1
PT_DYNAMIC = 2
PT_GNU_EH_FRAME = 0x6474e550
PT_GNU_STACK = 0x6474e551
PT_GNU_RELRO = 0x6474e552

SHT_SYMTAB = 2
SHT_STRTAB = 3
SHT_DYNSYM = 11

DT_NEEDED = 1
DT_STRTAB = 5
DT_SYMTAB = 6
DT_STRSZ = 10
DT_SONAME = 14
DT_PLTGOT = 3
DT_HASH = 4
DT_RELA = 7
DT_RELASZ = 8
DT_RELAENT = 9
DT_INIT = 12
DT_FINI = 13


# ── Data Structures ──

@dataclass
class ElfIdent:
    magic: bytes
    cls: int
    data: int
    version: int
    osabi: int
    abiversion: int

    @property
    def is_64bit(self) -> bool:
        return self.cls == 2

    @property
    def is_le(self) -> bool:
        return self.data == 1


@dataclass
class ElfHeader:
    ident: ElfIdent
    e_type: int
    e_machine: int
    e_version: int
    e_entry: int
    e_phoff: int
    e_shoff: int
    e_flags: int
    e_ehsize: int
    e_phentsize: int
    e_phnum: int
    e_shentsize: int
    e_shnum: int
    e_shstrndx: int


@dataclass
class ProgramHeader:
    p_type: int
    p_flags: int
    p_offset: int
    p_vaddr: int
    p_paddr: int
    p_filesz: int
    p_memsz: int
    p_align: int


@dataclass
class SectionHeader:
    sh_name: int
    sh_type: int
    sh_flags: int
    sh_addr: int
    sh_offset: int
    sh_size: int
    sh_link: int
    sh_info: int
    sh_addralign: int
    sh_entsize: int


@dataclass
class ElfSymbol:
    st_name: int
    st_info: int
    st_other: int
    st_shndx: int
    st_value: int
    st_size: int


@dataclass
class ElfBinary:
    header: ElfHeader
    segments: list[ProgramHeader]
    sections: list[SectionHeader]
    data: bytes

    # Parsed content
    shstrtab: bytes = b""
    strtab: bytes = b""
    dynstr: bytes = b""
    symtab: list[ElfSymbol] = field(default_factory=list)
    dynsym: list[ElfSymbol] = field(default_factory=list)

    def _get_str(self, strtab: bytes, name_off: int) -> str:
        if name_off >= len(strtab):
            return ""
        end = strtab.find(b"\x00", name_off)
        if end == -1:
            return strtab[name_off:].decode("ascii", errors="replace")
        return strtab[name_off:end].decode("ascii", errors="replace")

    def section_name(self, sh: SectionHeader) -> str:
        return self._get_str(self.shstrtab, sh.sh_name)

    def symbol_name(self, sym: ElfSymbol, strtab: bytes) -> str:
        return self._get_str(strtab, sym.st_name)

    @property
    def is_executable(self) -> bool:
        return self.header.e_type == ET_EXEC

    @property
    def is_dll(self) -> bool:
        return self.header.e_type == ET_DYN

    @property
    def entry_point(self) -> int:
        return self.header.e_entry

    def text_segment(self) -> Optional[ProgramHeader]:
        for seg in self.segments:
            if seg.p_type == PT_LOAD and (seg.p_flags & 1):  # PF_X
                return seg
        return None

    def segments_at_vaddr(self, vaddr: int) -> list[ProgramHeader]:
        return [s for s in self.segments if s.p_type == PT_LOAD
                and s.p_vaddr <= vaddr < s.p_vaddr + s.p_memsz]

    def exports(self) -> list[dict]:
        """Return exported symbols (DLL interface).
        Checks .dynsym first, falls back to .symtab for non-DLL ELF executables."""
        result = []

        # Try .dynsym first
        strtab_dyn = self.dynstr if self.dynstr else b""
        for sym in self.dynsym:
            if (sym.st_info >> 4) in (1, 2):  # STB_GLOBAL or STB_WEAK
                name = self.symbol_name(sym, strtab_dyn)
                if name:
                    result.append({
                        "name": name,
                        "value": sym.st_value,
                        "size": sym.st_size,
                        "info": sym.st_info,
                    })

        # Fall back to .symtab if .dynsym was empty
        if not result and self.symtab:
            strtab = self.strtab if self.strtab else b""
            for sym in self.symtab:
                bind = sym.st_info >> 4
                stype = sym.st_info & 0xF
                # GLOBAL or WEAK functions/objects
                if bind in (1, 2) and stype in (1, 2, 0):
                    name = self.symbol_name(sym, strtab)
                    if name and name not in ("_start",):
                        result.append({
                            "name": name,
                            "value": sym.st_value,
                            "size": sym.st_size,
                            "info": sym.st_info,
                        })

        return result

    def imports(self) -> list[str]:
        """Return DT_NEEDED library names."""
        # This would need DT_NEEDED parsing from DYNAMIC segment
        return []

    def dump_header(self) -> str:
        h = self.header
        type_names = {ET_EXEC: "EXEC", ET_DYN: "DYN", ET_REL: "REL"}
        etype = type_names.get(h.e_type, f"0x{h.e_type:04X}")
        lines = [
            f"ELF64 Header:",
            f"  Type:     {etype}",
            f"  Machine:  x86_64 ({h.e_machine})",
            f"  Entry:    0x{h.e_entry:016X}",
            f"  Sections: {h.e_shnum} @ 0x{h.e_shoff:08X}",
            f"  Segments: {h.e_phnum} @ 0x{h.e_phoff:08X}",
            f"  Flags:    0x{h.e_flags:08X}",
        ]
        return "\n".join(lines)

    def dump_segments(self) -> str:
        lines = [f"Segments ({len(self.segments)}):"]
        for i, seg in enumerate(self.segments):
            flags = ""
            flags += "R" if seg.p_flags & 4 else " "
            flags += "W" if seg.p_flags & 2 else " "
            flags += "X" if seg.p_flags & 1 else " "
            lines.append(
                f"  [{i}] {seg.p_type:3d} {flags} "
                f"vaddr=0x{seg.p_vaddr:016X} "
                f"filesz=0x{seg.p_filesz:06X} memsz=0x{seg.p_memsz:06X} "
                f"off=0x{seg.p_offset:08X}"
            )
        return "\n".join(lines)

    def dump_symbols(self, syms: list[ElfSymbol], strtab: bytes, title: str) -> str:
        if not syms:
            return f"{title}: (none)"
        bind_names = {0: "LOCAL", 1: "GLOBAL", 2: "WEAK"}
        type_names = {0: "NOTYPE", 1: "OBJECT", 2: "FUNC", 3: "SECTION", 4: "FILE"}
        lines = [f"{title} ({len(syms)}):"]
        for sym in syms:
            name = self.symbol_name(sym, strtab)
            bind = bind_names.get(sym.st_info >> 4, f"0x{sym.st_info>>4:X}")
            stype = type_names.get(sym.st_info & 0xF, f"0x{sym.st_info&0xF:X}")
            if not name and stype == "SECTION":
                continue
            prefix = "  [E]" if name in ("_start", "driver_init", "driver_on_event",
                                         "driver_activate", "driver_fini") else "     "
            lines.append(
                f"{prefix} {name or '(anon)':25s} {bind:6s} {stype:6s} "
                f"value=0x{sym.st_value:016X} size={sym.st_size}"
            )
        return "\n".join(lines)

    def dump(self) -> str:
        parts = [
            self.dump_header(),
            "",
            self.dump_segments(),
            "",
            self.dump_symbols(self.symtab, self.strtab, "Symbol table (.symtab)"),
            "",
            self.dump_symbols(self.dynsym, self.dynstr, "Dynamic symbols (.dynsym)"),
            "",
        ]
        if self.exports():
            parts.append(f"Exports ({len(self.exports())}):")
            for e in self.exports():
                parts.append(f"  {e['name']:30s} value=0x{e['value']:016X} size={e['size']}")
            parts.append("")
        return "\n".join(parts)


# ── Parser ──

def parse_elf(data: bytes) -> Optional[ElfBinary]:
    """Parse an ELF64 binary. Returns ElfBinary or None."""
    if len(data) < 64:
        return None
    if data[:4] != ELF_MAGIC:
        return None

    ident = ElfIdent(
        magic=data[:4],
        cls=data[4],
        data=data[5],
        version=data[6],
        osabi=data[7],
        abiversion=data[8],
    )

    if not ident.is_64bit or not ident.is_le:
        return None

    (e_type, e_machine, e_version, e_entry, e_phoff, e_shoff,
     e_flags, e_ehsize, e_phentsize, e_phnum, e_shentsize, e_shnum,
     e_shstrndx) = struct.unpack_from("<HHIQQQIHHHHHH", data, 16)

    if e_machine != EM_X86_64:
        return None

    header = ElfHeader(
        ident=ident,
        e_type=e_type, e_machine=e_machine, e_version=e_version,
        e_entry=e_entry, e_phoff=e_phoff, e_shoff=e_shoff,
        e_flags=e_flags, e_ehsize=e_ehsize, e_phentsize=e_phentsize,
        e_phnum=e_phnum, e_shentsize=e_shentsize, e_shnum=e_shnum,
        e_shstrndx=e_shstrndx,
    )

    # Parse program headers
    segments = []
    if e_phentsize == 56:  # Elf64_Phdr
        off = e_phoff
        for _ in range(e_phnum):
            if off + 56 > len(data):
                break
            (p_type, p_flags, p_offset, p_vaddr, p_paddr,
             p_filesz, p_memsz, p_align) = struct.unpack_from("<IIQQQQQQ", data, off)
            segments.append(ProgramHeader(p_type, p_flags, p_offset, p_vaddr, p_paddr, p_filesz, p_memsz, p_align))
            off += 56

    # Parse section headers
    sections = []
    if e_shentsize == 64:  # Elf64_Shdr
        off = e_shoff
        for _ in range(e_shnum):
            if off + 64 > len(data):
                break
            (sh_name, sh_type, sh_flags, sh_addr, sh_offset,
             sh_size, sh_link, sh_info, sh_addralign,
             sh_entsize) = struct.unpack_from("<IIQQQQIIQQ", data, off)
            sections.append(SectionHeader(sh_name, sh_type, sh_flags, sh_addr, sh_offset, sh_size, sh_link, sh_info, sh_addralign, sh_entsize))
            off += 64

    elf = ElfBinary(header=header, segments=segments, sections=sections, data=data)

    # Parse section string table
    if e_shstrndx < len(sections):
        sh = sections[e_shstrndx]
        elf.shstrtab = data[sh.sh_offset:sh.sh_offset + sh.sh_size]

    # Parse symbol tables and string tables
    for sh in sections:
        name = elf.section_name(sh)
        if sh.sh_type == SHT_SYMTAB:
            elf.strtab = _parse_strtab(data, sections, sh)
            elf.symtab = _parse_symtab(data, sh)
        elif sh.sh_type == SHT_DYNSYM:
            elf.dynstr = _parse_strtab(data, sections, sh)
            elf.dynsym = _parse_symtab(data, sh)

    return elf


def _parse_symtab(data: bytes, sh: SectionHeader) -> list[ElfSymbol]:
    """Parse Elf64_Sym entries from a symbol table section."""
    syms = []
    if sh.sh_entsize != 24:
        return syms
    off = sh.sh_offset
    end = off + sh.sh_size
    while off + 24 <= end:
        (st_name, st_info, st_other, st_shndx, st_value, st_size) = \
            struct.unpack_from("<IBBHQQ", data, off)
        syms.append(ElfSymbol(st_name, st_info, st_other, st_shndx, st_value, st_size))
        off += 24
    return syms


def _parse_strtab(data: bytes, sections: list[SectionHeader], sym_sh: SectionHeader) -> bytes:
    """Find the string table associated with a symbol table."""
    link = sym_sh.sh_link
    if link < len(sections):
        sh = sections[link]
        return data[sh.sh_offset:sh.sh_offset + sh.sh_size]
    return b""


def parse_elf_file(path: str) -> Optional[ElfBinary]:
    """Parse an ELF binary from file path."""
    with open(path, "rb") as f:
        data = f.read()
    return parse_elf(data)


def analyze_elf(path: str) -> Optional[dict]:
    """Convenience: parse ELF and return structured analysis."""
    elf = parse_elf_file(path)
    if elf is None:
        return None

    exports = elf.exports()
    sections_info = [
        {
            "name": elf.section_name(sh),
            "addr": sh.sh_addr,
            "size": sh.sh_size,
            "type": sh.sh_type,
        }
        for sh in elf.sections if elf.section_name(sh)
    ]
    return {
        "type": "DLL" if elf.is_dll else ("EXEC" if elf.is_executable else "REL"),
        "entry": elf.entry_point,
        "num_segments": len(elf.segments),
        "num_sections": len(elf.sections),
        "segments": [
            {
                "type": s.p_type,
                "flags": s.p_flags,
                "vaddr": s.p_vaddr,
                "size": s.p_memsz,
                "file_size": s.p_filesz,
            }
            for s in elf.segments
        ],
        "exports": exports,
        "sections": sections_info,
        "dynsym_count": len(elf.dynsym),
        "symtab_count": len(elf.symtab),
    }
