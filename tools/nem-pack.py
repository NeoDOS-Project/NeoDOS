#!/usr/bin/env python3
"""
nem-pack — NeoDOS NEM v3 binary packer

Converts a Rust-compiled ELF relocatable object (.o) into a standalone
.nem runtime-loadable driver binary for NeoDOS.

Usage:
  python3 nem-pack.py <input.o> <output.nem> [options]
"""

import struct
import sys
import os

# ── NEM v3 constants ──

NEM3_MAGIC = b"NEM3"
NEM3_VERSION = 3
NEM3_HEADER_SIZE = 80  # bytes

# Relocation types (x86-64 ELF — actual values from psABI)
R_X86_64_NONE = 0
R_X86_64_64 = 1
R_X86_64_PC32 = 2
R_X86_64_GOT32 = 3
R_X86_64_PLT32 = 4
R_X86_64_COPY = 5
R_X86_64_GLOB_DAT = 6
R_X86_64_JUMP_SLOT = 7
R_X86_64_RELATIVE = 8
R_X86_64_GOTPCREL = 9
R_X86_64_32 = 10
R_X86_64_32S = 11

# NEM relocation types (stored in .nem file)
NEM_R_64 = 1
NEM_R_PC32 = 2
NEM_R_32 = 3
NEM_R_32S = 4
NEM_R_PLT32 = 5

# Convert x86-64 ELF relocation type → NEM relocation type
X86_64_TO_NEM = {
    R_X86_64_64: NEM_R_64,
    R_X86_64_PC32: NEM_R_PC32,
    R_X86_64_PLT32: NEM_R_PLT32,
    R_X86_64_32: NEM_R_32,
    R_X86_64_32S: NEM_R_32S,
}

# Section types
SECT_TEXT = 0
SECT_RODATA = 1
SECT_DATA = 2
SECT_BSS = 3

# ── ELF parsing ──

ELFCLASS64 = 2; ELFDATA2LSB = 1; ET_REL = 1
SHT_PROGBITS = 1; SHT_NOBITS = 8; SHT_RELA = 4
SHT_SYMTAB = 2; SHT_STRTAB = 3
SHF_ALLOC = 0x02; SHF_EXECINSTR = 0x04; SHF_WRITE = 0x01

def read_u32(d, o): return struct.unpack_from('<I', d, o)[0]
def read_u64(d, o): return struct.unpack_from('<Q', d, o)[0]
def read_u16(d, o): return struct.unpack_from('<H', d, o)[0]

def parse_elf(data):
    if len(data) < 64 or data[:4] != b'\x7fELF':
        raise ValueError("Not a valid ELF file")
    cls = data[4]
    if cls != ELFCLASS64:
        raise ValueError(f"Only 64-bit ELF supported, got class {cls}")
    ed = data[5]
    if ed != ELFDATA2LSB:
        raise ValueError("Only little-endian ELF supported")

    e_type = read_u16(data, 16)
    if e_type != ET_REL:
        raise ValueError(f"Expected relocatable .o file, got type {e_type}")

    e_shoff = read_u64(data, 40)
    e_shentsize = read_u16(data, 58)
    e_shnum = read_u16(data, 60)
    e_shstrndx = read_u16(data, 62)

    if e_shoff == 0 or e_shnum == 0:
        raise ValueError("No section headers")

    sections = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        sections.append({
            'name_idx': read_u32(data, off),
            'type': read_u32(data, off+4),
            'flags': read_u64(data, off+8),
            'offset': read_u64(data, off+24),
            'size': read_u64(data, off+32),
            'link': read_u32(data, off+40),
            'info': read_u32(data, off+44),
            'data': data[read_u64(data, off+24):read_u64(data, off+24)+read_u64(data, off+32)]
        })

    # Read section name string table
    shstrtab = data[sections[e_shstrndx]['offset']:]
    def get_name(idx):
        end = shstrtab.index(b'\x00', idx)
        return shstrtab[idx:end].decode('ascii', errors='replace')
    for s in sections:
        s['name'] = get_name(s['name_idx'])

    result = {'text': None, 'rodata': None, 'data': None, 'bss': None,
              'rela_text': [], 'rela_rodata': [], 'rela_data': [],
              'symtab': None, 'strtab': None, 'sections': sections}

    # Collect sections by category: concatenate multiple .text.*, .rodata.*, .data.*
    text_sections = []
    rodata_sections = []
    data_sections = []
    bss_size = 0

    for s in sections:
        if s['type'] == SHT_PROGBITS and s['flags'] & SHF_EXECINSTR:
            text_sections.append(s)
        elif s['type'] == SHT_PROGBITS and s['flags'] & SHF_ALLOC and not (s['flags'] & (SHF_EXECINSTR | SHF_WRITE)):
            rodata_sections.append(s)
        elif s['type'] == SHT_PROGBITS and s['flags'] & SHF_ALLOC and s['flags'] & SHF_WRITE:
            data_sections.append(s)
        elif s['type'] == SHT_NOBITS and s['flags'] & SHF_ALLOC and s['flags'] & SHF_WRITE:
            bss_size += s['size']
        elif s['type'] == SHT_SYMTAB:
            result['symtab'] = s
        elif s['type'] == SHT_STRTAB and s['name'] == '.strtab':
            result['strtab'] = s

    # Build concatenated section data + offset map
    def concat_sections(sec_list):
        data = b''
        offsets = {}
        for s in sec_list:
            offsets[s['name']] = len(data)
            data += s['data']
        return data, offsets

    text_data, text_offsets = concat_sections(text_sections)
    rodata_data, rodata_offsets = concat_sections(rodata_sections)
    data_data, data_offsets = concat_sections(data_sections)

    result['text'] = {'data': text_data, 'name': '.text'}
    result['rodata'] = {'data': rodata_data, 'name': '.rodata'}
    result['data'] = {'data': data_data, 'name': '.data'}
    result['bss'] = {'size': bss_size}

    # Section index → section name lookup for symbol section mapping
    sec_idx_to_name = {i: s['name'] for i, s in enumerate(sections)}

    # Helper: get offset within concatenated section + NEM section type for a symbol
    def get_sym_section_offset(shndx, value):
        if shndx >= len(sections):
            return None, None
        name = sec_idx_to_name.get(shndx, '')
        # Check text sections
        for oname, off in text_offsets.items():
            if name == oname:
                return off + value, SECT_TEXT
        # Check .rodata (including .rodata._ZN... etc)
        if name.startswith('.rodata'):
            for oname, off in rodata_offsets.items():
                if name == oname:
                    return off + value, SECT_RODATA
        # Check data sections
        for oname, off in data_offsets.items():
            if name == oname:
                return off + value, SECT_DATA
        # Also check .text (base match)
        if name in text_offsets:
            return text_offsets[name] + value, SECT_TEXT
        if name in rodata_offsets:
            return rodata_offsets[name] + value, SECT_RODATA
        if name in data_offsets:
            return data_offsets[name] + value, SECT_DATA
        return None, None

    # Parse symbols
    syms = []
    if result['symtab']:
        sd = result['symtab']['data']
        ss = sections[result['symtab']['link']]['data']
        ns = len(sd) // 24
        for i in range(ns):
            off = i * 24
            st_name = read_u32(sd, off)
            st_info = sd[off+4]; st_shndx = read_u16(sd, off+6)
            st_value = read_u64(sd, off+8)
            if st_name == 0: sym_name = ''
            else:
                end = ss.index(b'\x00', st_name)
                sym_name = ss[st_name:end].decode('ascii', errors='replace')
            # Adjust value to concatenated section offset
            adj_val, _ = get_sym_section_offset(st_shndx, st_value)
            if adj_val is not None:
                adj_value = adj_val
            else:
                adj_value = st_value
            _, nem_sec = get_sym_section_offset(st_shndx, st_value)
            syms.append({'name': sym_name, 'bind': st_info >> 4, 'type': st_info & 0xF,
                         'shndx': st_shndx, 'value': adj_value, 'nem_section': nem_sec if nem_sec is not None else 0xFFFF})
    result['symbols'] = syms

    # Build relocation section lists: gather all RELA sections per category
    rela_text = []
    rela_rodata = []
    rela_data = []
    for s in sections:
        if s['type'] != SHT_RELA:
            continue
        tgt = sections[s['info']]['name'] if s['info'] < len(sections) else ''
        if tgt == '.text' or tgt.startswith('.text.'):
            rela_text.append(s)
        elif tgt.startswith('.rodata') or tgt.startswith('.rodata.'):
            rela_rodata.append(s)
        elif tgt == '.data' or tgt.startswith('.data.'):
            rela_data.append(s)

    # Parse relocations, adjusting offsets to concatenated section base
    def parse_relocs(rela_sections, section_offsets):
        result_list = []
        for rs in rela_sections:
            base = section_offsets.get(rs['name'], 0)  # will be set below
            rd = rs['data']
            nr = len(rd) // 24
            for i in range(nr):
                off = i * 24
                r_off = read_u64(rd, off)
                r_info = read_u64(rd, off+8)
                r_add = struct.unpack_from('<q', rd, off+16)[0]
                result_list.append({'offset': r_off, 'type': r_info & 0xFFFFFFFF,
                                    'sym': r_info >> 32, 'addend': r_add})
        return result_list

    # For relocation offsets, we need the base offset of each reloc's target section
    # Build section name → base offset for all sections
    all_offsets = {}
    for name, off in text_offsets.items(): all_offsets[name] = off
    for name, off in rodata_offsets.items(): all_offsets[name] = off
    for name, off in data_offsets.items(): all_offsets[name] = off
    all_offsets['.text'] = 0  # base of concatenated text

    relocs = []
    for rs in sections:
        if rs['type'] != SHT_RELA or rs['size'] == 0:
            continue
        tgt = sections[rs['info']]['name'] if rs['info'] < len(sections) else ''
        base = all_offsets.get(tgt, 0)
        # Determine section type for NEM reloc
        if tgt == '.text' or tgt.startswith('.text.'):
            nem_sec = SECT_TEXT
        elif tgt.startswith('.rodata'):
            nem_sec = SECT_RODATA
        elif tgt == '.data' or tgt.startswith('.data.'):
            nem_sec = SECT_DATA
        else:
            continue
        rd = rs['data']
        nr = len(rd) // 24
        for i in range(nr):
            off = i * 24
            r_off = read_u64(rd, off)
            r_info = read_u64(rd, off+8)
            r_add = struct.unpack_from('<q', rd, off+16)[0]
            relocs.append({'offset': base + r_off, 'type': r_info & 0xFFFFFFFF,
                           'sym': r_info >> 32, 'addend': r_add, 'section': nem_sec})
    result['relocations'] = relocs

    return result


def build_nem_v3(driver_name, elf, flags=0, abi_min=1, abi_target=1, abi_max=2,
                 driver_type=2, category=0):
    text = elf['text']; rodata = elf['rodata']; data = elf['data']; bss = elf['bss']
    relocs = elf['relocations']; symbols = elf['symbols']

    td = text['data'] if text else b''
    rd = rodata['data'] if rodata else b''
    dd = data['data'] if data else b''
    bs = bss['size'] if bss else 0

    ts = len(td); rs = len(rd); ds = len(dd)

    # Find entry points
    entry_init = entry_event = entry_fini = 0xFFFFFFFF
    for sym in symbols:
        if sym['name'] == 'driver_init' and sym['shndx'] != 0:
            entry_init = sym['value']
        elif sym['name'] == 'driver_on_event' and sym['shndx'] != 0:
            entry_event = sym['value']
        elif sym['name'] == 'driver_fini' and sym['shndx'] != 0:
            entry_fini = sym['value']

    if entry_init == 0xFFFFFFFF: entry_init = 0
    if entry_event == 0xFFFFFFFF: entry_event = 0
    if entry_fini == 0xFFFFFFFF: entry_fini = 0

    # Build string table + symbol table
    strtab = b''
    sym_entries = []
    undef_entries = []

    for sym in symbols:
        if not sym['name']: continue
        if sym['shndx'] == 0:
            # Undefined symbol
            name_off = len(strtab)
            strtab += sym['name'].encode('ascii') + b'\x00'
            undef_entries.append({
                'name_off': name_off, 'value': 0,
                'section': 0xFFFF, 'info': 0x10, 'size': 0
            })
        elif sym.get('nem_section', 0xFFFF) != 0xFFFF:
            name_off = len(strtab)
            strtab += sym['name'].encode('ascii') + b'\x00'
            ns = sym['nem_section']
            info = 0x12 if sym['type'] == 2 else 0x10  # FUNC or OBJECT
            sym_entries.append({
                'name_off': name_off, 'value': sym['value'],
                'section': ns, 'info': info, 'size': 0
            })

    # Build index: name -> nem_symbol_index
    nem_syms = sym_entries + undef_entries
    name_to_idx = {}
    for i, ns in enumerate(nem_syms):
        end = strtab.index(b'\x00', ns['name_off'])
        name = strtab[ns['name_off']:end].decode('ascii')
        name_to_idx[name] = i

    # Filter supported relocations only
    supported_relocs = []
    for rel in relocs:
        nem_type = X86_64_TO_NEM.get(rel['type'] & 0xFF, 0xFF)
        if nem_type == 0xFF:
            print(f"[!] Unknown x86-64 reloc type {rel['type'] & 0xFF}, skipping")
            continue
        supported_relocs.append(rel)

    reloc_data = b''
    for rel in supported_relocs:
        sym_idx = 0xFF  # default: no symbol
        sym = symbols[rel['sym']] if rel['sym'] < len(symbols) else None
        if sym and sym['name'] in name_to_idx:
            sym_idx = name_to_idx[sym['name']]
        elif sym and not sym['name']:
            sym_idx = 0xFF  # local/unnamed symbol

        addend_32 = rel['addend'] & 0xFFFFFFFF
        if addend_32 >= 0x80000000:
            addend_32 = addend_32 - 0x100000000  # convert to signed
        nem_type = X86_64_TO_NEM.get(rel['type'] & 0xFF, 0xFF)
        reloc_data += struct.pack('<IHBBi',
            rel['offset'] & 0xFFFFFFFF,
            rel['section'],
            nem_type,
            sym_idx & 0xFF,
            addend_32,
        )

    num_relocs = len(supported_relocs)
    num_syms = len(nem_syms)

    name_bytes = driver_name.encode('ascii') + b'\x00'
    while len(name_bytes) % 4 != 0: name_bytes += b'\x00'

    total_mem = ts + rs + ds + bs

    # Calculate offsets
    name_off = NEM3_HEADER_SIZE
    relocs_off = name_off + len(name_bytes)
    syms_off = relocs_off + num_relocs * 12
    strtab_off = syms_off + num_syms * 16
    text_off = strtab_off + len(strtab)
    rodata_off = text_off + ts
    data_off = rodata_off + rs

    # Build header (80 bytes)
    hdr = bytearray(NEM3_HEADER_SIZE)
    struct.pack_into('<4sI', hdr, 0, NEM3_MAGIC, NEM3_VERSION)
    struct.pack_into('<I', hdr, 8, NEM3_HEADER_SIZE)
    struct.pack_into('<I', hdr, 12, flags)
    struct.pack_into('<HHH', hdr, 16, abi_min, abi_target, abi_max)
    struct.pack_into('<HH', hdr, 22, driver_type, category)
    struct.pack_into('<IIIII', hdr, 28, ts, rs, ds, bs, total_mem)
    struct.pack_into('<III', hdr, 48, entry_init, entry_event, entry_fini)
    struct.pack_into('<IIIII', hdr, 60, num_relocs, relocs_off, syms_off, strtab_off, name_off)

    out = bytearray(hdr)
    out.extend(name_bytes)
    out.extend(reloc_data)
    for ns in nem_syms:
        out.extend(struct.pack('<IIHHI', ns['name_off'], ns['value'],
                               ns['section'] & 0xFFFF, ns['info'] & 0xFF, 0))
    out.extend(strtab)
    out.extend(td)
    out.extend(rd)
    out.extend(dd)

    return bytes(out)


def main():
    if len(sys.argv) < 3:
        print("Usage: python3 nem-pack.py <input.o> <output.nem> [options]")
        print("  --name NAME       Driver name")
        print("  --type N          Driver type (0-5, default: 2=Lifecycle)")
        print("  --category N      Category (0=Boot, 1=System, 2=Demand, default: 0)")
        print("  --abi-min N       Min ABI (default: 1)")
        print("  --abi-target N    Target ABI (default: 1)")
        print("  --abi-max N       Max ABI (default: 2)")
        print("  --flags N         Flags (default: 0)")
        sys.exit(1)

    inp = sys.argv[1]; out = sys.argv[2]
    name = os.path.splitext(os.path.basename(inp))[0]
    kw = {'abi_min': 1, 'abi_target': 1, 'abi_max': 2, 'driver_type': 2, 'category': 0, 'flags': 0}
    i = 3
    while i < len(sys.argv):
        v = sys.argv
        if v[i] == '--name' and i+1 < len(v): name = v[i+1]; i += 2
        elif v[i] == '--type' and i+1 < len(v): kw['driver_type'] = int(v[i+1]); i += 2
        elif v[i] == '--category' and i+1 < len(v): kw['category'] = int(v[i+1]); i += 2
        elif v[i] == '--abi-min' and i+1 < len(v): kw['abi_min'] = int(v[i+1]); i += 2
        elif v[i] == '--abi-target' and i+1 < len(v): kw['abi_target'] = int(v[i+1]); i += 2
        elif v[i] == '--abi-max' and i+1 < len(v): kw['abi_max'] = int(v[i+1]); i += 2
        elif v[i] == '--flags' and i+1 < len(v): kw['flags'] = int(v[i+1]); i += 2
        else: print(f"[!] Unknown: {v[i]}"); sys.exit(1)

    with open(inp, 'rb') as f: ed = f.read()
    elf = parse_elf(ed)
    if not elf['text']: print("[!] No .text section"); sys.exit(1)

    nem = build_nem_v3(name, elf, **kw)
    with open(out, 'wb') as f: f.write(nem)

    ts = len(elf['text']['data'])
    rs = len(elf['rodata']['data']) if elf['rodata'] else 0
    ds = len(elf['data']['data']) if elf['data'] else 0
    bs = elf['bss']['size'] if elf['bss'] else 0
    nr = len(elf['relocations'])
    ns = len([s for s in elf['symbols'] if s['name']])
    nu = len([s for s in elf['symbols'] if s['name'] and s['shndx'] == 0])

    print(f"[NEM] {out}: {len(nem)} bytes | text={ts} rodata={rs} data={ds} bss={bs} | relocs={nr} syms={ns}({nu} undef)")

if __name__ == '__main__':
    main()
