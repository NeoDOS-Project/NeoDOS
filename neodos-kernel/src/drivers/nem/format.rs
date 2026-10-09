// src/nem/mod.rs
// NEM — NeoDOS Driver Format parser
// v3: 80-byte header, standalone binary with relocation support

pub const NEM_MAGIC: u32 = 0x004D454E; // "NEM\0"
pub const NEM_VERSION_3: u32 = 3;
pub const NEM_HEADER_SIZE_V3: u32 = 80;
pub const NEM_API_VERSION: u16 = 1;

// HAL ABI v0.3 encoding
pub const ABI_MIN_VALID: u16 = 1;
pub const ABI_TARGET: u16 = 1;
pub const ABI_MAX_VALID: u16 = 2;

// NEM v3 relocation types
pub const R_NEM_NONE: u8 = 0;
pub const R_NEM_64: u8 = 1;    // S + A (64-bit absolute)
pub const R_NEM_PC32: u8 = 2;  // S + A - P (32-bit PC-relative)
pub const R_NEM_32: u8 = 3;    // S + A (32-bit zero-extended)
pub const R_NEM_32S: u8 = 4;   // S + A (32-bit sign-extended)
pub const R_NEM_PLT32: u8 = 5; // S + A - P (same as PC32)

// NEM v3 section types
pub const NEM_SECT_TEXT: u8 = 0;
pub const NEM_SECT_RODATA: u8 = 1;
pub const NEM_SECT_DATA: u8 = 2;
pub const NEM_SECT_BSS: u8 = 3;
pub const NEM_SECT_UNDEF: u16 = 0xFFFF;
pub const NEM_SYM_TEXT: u16 = 0xFE;
pub const NEM_SYM_RODATA: u16 = 0xFD;
pub const NEM_SYM_DATA: u16 = 0xFC;

// Relocation entry size in bytes
pub const NEM_RELOC_SIZE: usize = 12;
pub const NEM_SYMBOL_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverCategory {
    Boot = 0,
    System = 1,
    Demand = 2,
}

impl DriverCategory {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(DriverCategory::Boot),
            1 => Some(DriverCategory::System),
            2 => Some(DriverCategory::Demand),
            _ => None,
        }
    }
    pub fn to_str(self) -> &'static str {
        match self {
            DriverCategory::Boot => "BOOT",
            DriverCategory::System => "SYSTEM",
            DriverCategory::Demand => "DEMAND",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NemDriverType {
    Null = 0,
    Echo = 1,
    Lifecycle = 2,
    Mutation = 3,
    Fault = 4,
    Burst = 5,
}

impl NemDriverType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(NemDriverType::Null),
            1 => Some(NemDriverType::Echo),
            2 => Some(NemDriverType::Lifecycle),
            3 => Some(NemDriverType::Mutation),
            4 => Some(NemDriverType::Fault),
            5 => Some(NemDriverType::Burst),
            _ => None,
        }
    }
    pub fn to_str(self) -> &'static str {
        match self {
            NemDriverType::Null => "null",
            NemDriverType::Echo => "echo",
            NemDriverType::Lifecycle => "lifecycle",
            NemDriverType::Mutation => "mutation",
            NemDriverType::Fault => "fault",
            NemDriverType::Burst => "burst",
        }
    }
    pub fn from_u16(v: u16) -> Option<Self> {
        Self::from_u8(v as u8)
    }
}

// ── NEM v3 header (80 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NemHeaderV3 {
    pub magic: [u8; 4],       // "NEM3"
    pub version: u32,          // 3
    pub header_size: u32,      // 80
    pub flags: u32,
    pub abi_min: u16,
    pub abi_target: u16,
    pub abi_max: u16,
    pub driver_type: u16,
    pub category: u16,
    pub text_size: u32,
    pub rodata_size: u32,
    pub data_size: u32,
    pub bss_size: u32,
    pub total_mem_size: u32,
    pub entry_init: u32,       // offset from text base
    pub entry_event: u32,
    pub entry_fini: u32,
    pub num_relocs: u32,
    pub relocs_offset: u32,
    pub syms_offset: u32,
    pub strtab_offset: u32,
    pub name_offset: u32,
}

// ── NEM v3 relocation entry (12 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NemReloc {
    pub offset: u32,       // byte offset from section base
    pub section: u16,      // section type (0=text, 1=rodata, 2=data)
    pub r_type: u8,        // relocation type (R_NEM_*)
    pub sym_idx: u8,       // symbol index (0xFF = none)
    pub addend: i32,
}

// ── NEM v3 symbol entry (16 bytes) ──

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NemSymbol {
    pub name_off: u32,     // offset into string table
    pub value: u32,        // section offset (or 0 for UNDEF)
    pub section: u16,      // section index (0xFFFF = UNDEF)
    pub info: u8,          // symbol info (type+binding)
    pub _pad1: u8,
    pub _pad2: u32,
}

// ── Parsed NEM v3 representation ──

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedNemV3<'a> {
    pub header: &'a NemHeaderV3,
    pub name: &'a str,
    pub text: &'a [u8],
    pub rodata: &'a [u8],
    pub data: &'a [u8],
    pub bss_size: u32,
    pub relocs: &'a [NemReloc],
    pub symbols: &'a [NemSymbol],
    pub strtab: &'a [u8],
    pub driver_type: NemDriverType,
    pub category: DriverCategory,
}

// ── NEM v3 parser ──

pub fn parse_nem_v3(data: &[u8]) -> Option<ParsedNemV3<'_>> {
    if data.len() < NEM_HEADER_SIZE_V3 as usize {
        return None;
    }

    let header: &NemHeaderV3 = unsafe { &*(data.as_ptr() as *const NemHeaderV3) };

    // Validate magic and version
    if &header.magic != b"NEM3" {
        return None;
    }
    if header.version != NEM_VERSION_3 {
        return None;
    }
    if header.header_size < NEM_HEADER_SIZE_V3 {
        return None;
    }
    if header.total_mem_size == 0 || header.total_mem_size > 1024 * 1024 {
        return None; // max 1MB per driver
    }

    // Validate offsets
    let hdr_end = header.header_size as usize;
    if hdr_end > data.len() { return None; }

    let name_off = header.name_offset as usize;
    if name_off >= data.len() { return None; }
    let name_end = data[name_off..].iter().position(|&b| b == 0).unwrap_or(16);
    let name = core::str::from_utf8(&data[name_off..name_off + name_end]).ok()?;
    if name.is_empty() { return None; }

    let relocs_off = header.relocs_offset as usize;
    let num_relocs = header.num_relocs as usize;
    let relocs_end = relocs_off + num_relocs * NEM_RELOC_SIZE;
    if relocs_end > data.len() { return None; }

    let syms_off = header.syms_offset as usize;
    // Calculate number of symbols from available space
    let strtab_off = header.strtab_offset as usize;
    if syms_off > strtab_off || strtab_off > data.len() { return None; }
    let syms_raw_size = strtab_off - syms_off;
    let num_syms = syms_raw_size / NEM_SYMBOL_SIZE;
    let syms_end = syms_off + num_syms * NEM_SYMBOL_SIZE;
    if syms_end > strtab_off { return None; }

    // Infer string table size from symbol name offsets and null terminators.
    // NEM v3 header stores strtab_offset but not strtab_size.
    let mut strtab_size = 0usize;
    for i in 0..num_syms {
        let sym_off = syms_off + i * NEM_SYMBOL_SIZE;
        let name_off = u32::from_le_bytes([
            data[sym_off],
            data[sym_off + 1],
            data[sym_off + 2],
            data[sym_off + 3],
        ]) as usize;
        let name_abs = strtab_off.saturating_add(name_off);
        if name_abs >= data.len() {
            return None;
        }
        let rel_end = data[name_abs..].iter().position(|&b| b == 0)?;
        let end = name_off + rel_end + 1; // include NUL
        if end > strtab_size {
            strtab_size = end;
        }
    }
    let sec_data_start = strtab_off.saturating_add(strtab_size);
    if sec_data_start > data.len() {
        return None;
    }
    let strtab = &data[strtab_off..sec_data_start];

    // Section data starts after strtab (size inferred from symbols)
    let text_end = sec_data_start + header.text_size as usize;
    let rodata_end = text_end + header.rodata_size as usize;
    let data_end = rodata_end + header.data_size as usize;
    if text_end > data.len() || rodata_end > data.len() || data_end > data.len() {
        return None;
    }

    let driver_type = NemDriverType::from_u16(header.driver_type)?;
    let category = DriverCategory::from_u8(header.category as u8).unwrap_or(DriverCategory::Demand);

    let relocs: &[NemReloc] = if num_relocs > 0 {
        unsafe { core::slice::from_raw_parts(
            data.as_ptr().add(relocs_off) as *const NemReloc, num_relocs) }
    } else {
        &[]
    };

    let symbols: &[NemSymbol] = if num_syms > 0 {
        unsafe { core::slice::from_raw_parts(
            data.as_ptr().add(syms_off) as *const NemSymbol, num_syms) }
    } else {
        &[]
    };

    Some(ParsedNemV3 {
        header,
        name,
        text: &data[sec_data_start..text_end],
        rodata: &data[text_end..rodata_end],
        data: &data[rodata_end..data_end],
        bss_size: header.bss_size,
        relocs,
        symbols,
        strtab,
        driver_type,
        category,
    })
}

// ── V3 test builder ──

pub fn build_valid_nem_v3(
    name: &str,
    code: &[u8],
    rodata: &[u8],
    data: &[u8],
    bss_size: u32,
    relocs: &[NemReloc],
) -> alloc::vec::Vec<u8> {
    let name_bytes = name.as_bytes();
    let mut raw = alloc::vec::Vec::new();

    // Build string table + symbol entries (minimal: no symbols for testing)
    // For test purposes, we just build a valid header + sections

    let name_len_padded = ((name_bytes.len() + 1) + 3) & !3;
    let mut name_buf = alloc::vec::Vec::with_capacity(name_len_padded);
    name_buf.extend_from_slice(name_bytes);
    name_buf.push(0);
    while name_buf.len() < name_len_padded { name_buf.push(0); }

    let num_relocs = relocs.len() as u32;
    let relocs_off = NEM_HEADER_SIZE_V3 + name_len_padded as u32;
    let syms_off = relocs_off + num_relocs * NEM_RELOC_SIZE as u32;
    let strtab_off = syms_off; // empty strtab
    let name_off = NEM_HEADER_SIZE_V3;

    let total_mem = code.len() as u32 + rodata.len() as u32 + data.len() as u32 + bss_size;

    let hdr = NemHeaderV3 {
        magic: *b"NEM3",
        version: NEM_VERSION_3,
        header_size: NEM_HEADER_SIZE_V3,
        flags: 0,
        abi_min: ABI_MIN_VALID,
        abi_target: ABI_TARGET,
        abi_max: ABI_MAX_VALID,
        driver_type: NemDriverType::Lifecycle as u16,
        category: DriverCategory::Boot as u16,
        text_size: code.len() as u32,
        rodata_size: rodata.len() as u32,
        data_size: data.len() as u32,
        bss_size,
        total_mem_size: total_mem,
        entry_init: 0,
        entry_event: 0,
        entry_fini: 0,
        num_relocs,
        relocs_offset: relocs_off,
        syms_offset: syms_off,
        strtab_offset: strtab_off,
        name_offset: name_off,
    };

    raw.extend_from_slice(unsafe {
        core::slice::from_raw_parts(
            &hdr as *const NemHeaderV3 as *const u8,
            core::mem::size_of::<NemHeaderV3>(),
        )
    });
    raw.extend_from_slice(&name_buf);

    // Relocations
    for rel in relocs {
        raw.extend_from_slice(unsafe {
            core::slice::from_raw_parts(
                rel as *const NemReloc as *const u8,
                NEM_RELOC_SIZE,
            )
        });
    }

    // Sections
    raw.extend_from_slice(code);
    raw.extend_from_slice(rodata);
    raw.extend_from_slice(data);

    raw
}

pub fn register_nem_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;


    // ── v3 tests ──

    test_case!("nem_v3_parse_valid", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem_v3("V3TEST", &code, &[], &[], 0, &[]);
        let parsed = parse_nem_v3(&raw);
        test_ne!(parsed, None);
        let p = parsed.unwrap();
        test_eq!(p.name, "V3TEST");
        test_eq!(p.text.len(), 16);
        test_eq!(p.rodata.len(), 0);
        test_eq!(p.data.len(), 0);
        test_eq!(p.bss_size, 0);
        test_eq!(p.relocs.len(), 0);
        test_eq!(p.driver_type, NemDriverType::Lifecycle);
        test_eq!(p.category, DriverCategory::Boot);
    });

    test_case!("nem_v3_parse_invalid_magic", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_nem_v3("BAD", &code, &[], &[], 0, &[]);
        raw[0..4].copy_from_slice(b"BAD\x00");
        let parsed = parse_nem_v3(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_v3_parse_truncated_header", {
        let raw = [0u8; 40];
        let parsed = parse_nem_v3(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_v3_parse_empty_code", {
        let raw = build_valid_nem_v3("EMPTY", &[], &[], &[], 0, &[]);
        let parsed = parse_nem_v3(&raw);
        test_eq!(parsed, None); // total_mem_size == 0
    });

    test_case!("nem_v3_parse_with_sections", {
        let code = [0x90u8; 32];
        let rodata = b"hello world";
        let data = [0x42u8; 8];
        let raw = build_valid_nem_v3("MULTISEC", &code, rodata, &data, 16, &[]);
        let parsed = parse_nem_v3(&raw);
        test_ne!(parsed, None);
        let p = parsed.unwrap();
        test_eq!(p.text.len(), 32);
        test_eq!(p.rodata, b"hello world");
        test_eq!(p.data, &[0x42u8; 8]);
        test_eq!(p.bss_size, 16);
    });

    test_case!("nem_v3_parse_with_relocs", {
        let code = [0x90u8; 64];
        let relocs = [
            NemReloc { offset: 0x10, section: 0, r_type: R_NEM_PC32, sym_idx: 0, addend: -4 },
            NemReloc { offset: 0x20, section: 0, r_type: R_NEM_64, sym_idx: 1, addend: 0 },
        ];
        let raw = build_valid_nem_v3("RELOCTST", &code, &[], &[], 0, &relocs);
        let parsed = parse_nem_v3(&raw);
        test_ne!(parsed, None);
        let p = parsed.unwrap();
        test_eq!(p.relocs.len(), 2);
        test_eq!(p.relocs[0].offset, 0x10);
        test_eq!(p.relocs[0].r_type, R_NEM_PC32);
        test_eq!(p.relocs[1].offset, 0x20);
        test_eq!(p.relocs[1].r_type, R_NEM_64);
    });
}
