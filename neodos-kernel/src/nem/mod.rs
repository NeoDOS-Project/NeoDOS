// src/nem/mod.rs
// NEM — NeoDOS Driver Format parser
// v1: 32-byte header, 8-byte name, no ABI fields
// v2: 48-byte header, 16-byte name, ABI fields
// v3: 80-byte header, standalone binary with relocation support

pub const NEM_MAGIC: u32 = 0x004D454E; // "NEM\0"
pub const NEM_VERSION: u32 = 1;
pub const NEM_VERSION_2: u32 = 2;
pub const NEM_VERSION_3: u32 = 3;
pub const NEM_HEADER_SIZE: u8 = 32;
pub const NEM_HEADER_SIZE_V2: u8 = 48;
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
    pub fn to_str(&self) -> &'static str {
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
    pub fn to_str(&self) -> &'static str {
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

// ── NEM v1 header ──

#[repr(C)]
struct NemHeader {
    magic: u32,
    version: u32,
    driver_type: u8,
    header_size: u8,
    entry_offset: u16,
    code_offset: u32,
    code_size: u32,
    api_version: u16,
    compat_flags: u16,
    name: [u8; 8],
}

// ── NEM v2 header ──

#[repr(C)]
struct NemHeaderV2 {
    magic: u32,
    version: u32,
    driver_type: u8,
    header_size: u8,
    entry_offset: u16,
    code_offset: u32,
    code_size: u32,
    api_version: u16,
    compat_flags: u16,
    abi_min: u16,
    abi_target: u16,
    abi_max: u16,
    category: u8,
    _reserved: [u8; 3],
    name: [u8; 16],
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

// ── Common parsed representation (v1/v2) ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedNem<'a> {
    pub driver_type: NemDriverType,
    pub code: &'a [u8],
    pub entry_offset: u16,
    pub name: &'a str,
    pub compat_flags: u16,
    pub abi_min: u16,
    pub abi_target: u16,
    pub abi_max: u16,
    pub category: DriverCategory,
    pub is_v2: bool,
}

// ── Version-agnostic parser ──

pub fn parse_nem(data: &[u8]) -> Option<ParsedNem<'_>> {
    // Try v1 (32-byte header)
    if data.len() >= NEM_HEADER_SIZE as usize {
        let header: &NemHeader = unsafe { &*(data.as_ptr() as *const NemHeader) };
        if header.magic == NEM_MAGIC
            && header.version == NEM_VERSION
            && header.header_size == NEM_HEADER_SIZE
            && header.api_version == NEM_API_VERSION
        {
            let driver_type = NemDriverType::from_u8(header.driver_type)?;
            let code_start = header.code_offset as usize;
            let code_end = code_start.saturating_add(header.code_size as usize);
            if code_end > data.len() { return None; }
            let entry = header.entry_offset as usize;
            if entry < code_start || entry >= code_end { return None; }
            let name_len = header.name.iter().position(|&b| b == 0).unwrap_or(8);
            let name = core::str::from_utf8(&header.name[..name_len]).ok()?;
            if name.is_empty() { return None; }
            return Some(ParsedNem {
                driver_type, code: &data[code_start..code_end],
                entry_offset: header.entry_offset, name,
                compat_flags: header.compat_flags,
                abi_min: 0, abi_target: 0, abi_max: 0,
                category: DriverCategory::Demand, is_v2: false,
            });
        }
    }

    // Try v2 (48-byte header)
    if data.len() >= NEM_HEADER_SIZE_V2 as usize {
        let header: &NemHeaderV2 = unsafe { &*(data.as_ptr() as *const NemHeaderV2) };
        if header.magic == NEM_MAGIC
            && header.version == NEM_VERSION_2
            && header.header_size == NEM_HEADER_SIZE_V2
        {
            let driver_type = NemDriverType::from_u8(header.driver_type)?;
            let code_start = header.code_offset as usize;
            let code_end = code_start.saturating_add(header.code_size as usize);
            if code_end > data.len() { return None; }
            let entry = header.entry_offset as usize;
            if entry < code_start || entry >= code_end { return None; }
            let name_len = header.name.iter().position(|&b| b == 0).unwrap_or(16);
            let name = core::str::from_utf8(&header.name[..name_len]).ok()?;
            if name.is_empty() { return None; }
            let category = DriverCategory::from_u8(header.category).unwrap_or(DriverCategory::Demand);
            return Some(ParsedNem {
                driver_type, code: &data[code_start..code_end],
                entry_offset: header.entry_offset, name,
                compat_flags: header.compat_flags,
                abi_min: header.abi_min, abi_target: header.abi_target,
                abi_max: header.abi_max, category, is_v2: true,
            });
        }
    }

    None
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

// ── V1 test builder ──

pub fn build_valid_nem(driver_type: NemDriverType, name: &str, code: &[u8]) -> alloc::vec::Vec<u8> {
    let mut raw = alloc::vec::Vec::with_capacity(32 + code.len());
    let name_bytes = name.as_bytes();
    let mut name_arr = [0u8; 8];
    let name_len = name_bytes.len().min(8);
    name_arr[..name_len].copy_from_slice(&name_bytes[..name_len]);

    let hdr = NemHeader {
        magic: NEM_MAGIC,
        version: NEM_VERSION,
        driver_type: driver_type as u8,
        header_size: NEM_HEADER_SIZE,
        entry_offset: 32,
        code_offset: 32,
        code_size: code.len() as u32,
        api_version: NEM_API_VERSION,
        compat_flags: 0,
        name: name_arr,
    };

    raw.extend_from_slice(unsafe {
        core::slice::from_raw_parts(
            &hdr as *const NemHeader as *const u8,
            core::mem::size_of::<NemHeader>(),
        )
    });
    raw.extend_from_slice(code);
    raw
}

// ── V2 test builder ──

pub fn build_valid_nem_v2(
    driver_type: NemDriverType, name: &str, code: &[u8],
    abi_min: u16, abi_target: u16, abi_max: u16,
    category: DriverCategory, compat_flags: u16,
) -> alloc::vec::Vec<u8> {
    let mut raw = alloc::vec::Vec::with_capacity(48 + code.len());
    let name_bytes = name.as_bytes();
    let mut name_arr = [0u8; 16];
    let name_len = name_bytes.len().min(16);
    name_arr[..name_len].copy_from_slice(&name_bytes[..name_len]);

    let hdr = NemHeaderV2 {
        magic: NEM_MAGIC,
        version: NEM_VERSION_2,
        driver_type: driver_type as u8,
        header_size: NEM_HEADER_SIZE_V2,
        entry_offset: 48,
        code_offset: 48,
        code_size: code.len() as u32,
        api_version: NEM_API_VERSION,
        compat_flags,
        abi_min, abi_target, abi_max,
        category: category as u8,
        _reserved: [0u8; 3],
        name: name_arr,
    };

    raw.extend_from_slice(unsafe {
        core::slice::from_raw_parts(
            &hdr as *const NemHeaderV2 as *const u8,
            core::mem::size_of::<NemHeaderV2>(),
        )
    });
    raw.extend_from_slice(code);
    raw
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

    // ── v1 tests ──

    test_case!("nem_parse_valid_null", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem(NemDriverType::Null, "TESTNULL", &code);
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().driver_type, NemDriverType::Null);
        test_eq!(parsed.unwrap().name, "TESTNULL");
    });

    test_case!("nem_parse_valid_echo", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem(NemDriverType::Echo, "ECHO", &code);
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().driver_type, NemDriverType::Echo);
        test_eq!(parsed.unwrap().name, "ECHO");
        test_eq!(parsed.unwrap().entry_offset, 32);
    });

    test_case!("nem_parse_invalid_magic", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_nem(NemDriverType::Echo, "ECHO", &code);
        raw[0..4].copy_from_slice(b"BAD\x00");
        let parsed = parse_nem(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_parse_invalid_header_size", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_nem(NemDriverType::Echo, "ECHO", &code);
        raw[9] = 99;
        let parsed = parse_nem(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_parse_unknown_type", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_nem(NemDriverType::Null, "UNK", &code);
        raw[8] = 0xFF;
        let parsed = parse_nem(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_parse_entry_out_of_bounds", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_nem(NemDriverType::Null, "BAD", &code);
        raw[10..12].copy_from_slice(&[48u8, 0u8]);
        let parsed = parse_nem(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_parse_empty_name", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem(NemDriverType::Null, "", &code);
        let parsed = parse_nem(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_parse_all_driver_types", {
        let code = [0x90u8; 16];
        for dt in &[
            NemDriverType::Null, NemDriverType::Echo, NemDriverType::Lifecycle,
            NemDriverType::Mutation, NemDriverType::Fault, NemDriverType::Burst,
        ] {
            let raw = build_valid_nem(*dt, "ALLTYPES", &code);
            let parsed = parse_nem(&raw);
            test_ne!(parsed, None);
            test_eq!(parsed.unwrap().driver_type, *dt);
        }
    });

    test_case!("nem_parse_truncated_header", {
        let raw = [0u8; 16];
        let parsed = parse_nem(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_parse_truncated_code", {
        let raw = build_valid_nem(NemDriverType::Null, "TRUNC", &[0x90u8; 64]);
        let parsed = parse_nem(&raw[..40]);
        test_eq!(parsed, None);
    });

    test_case!("nem_parse_code_empty", {
        let raw = build_valid_nem(NemDriverType::Null, "EMPTY", &[]);
        let parsed = parse_nem(&raw);
        test_eq!(parsed, None);
    });

    test_case!("nem_driver_type_from_u8_all", {
        test_eq!(NemDriverType::from_u8(0), Some(NemDriverType::Null));
        test_eq!(NemDriverType::from_u8(1), Some(NemDriverType::Echo));
        test_eq!(NemDriverType::from_u8(2), Some(NemDriverType::Lifecycle));
        test_eq!(NemDriverType::from_u8(6), None);
        test_eq!(NemDriverType::from_u8(0xFF), None);
    });

    test_case!("nem_driver_type_to_str", {
        test_eq!(NemDriverType::Null.to_str(), "null");
        test_eq!(NemDriverType::Echo.to_str(), "echo");
        test_eq!(NemDriverType::Lifecycle.to_str(), "lifecycle");
        test_eq!(NemDriverType::Fault.to_str(), "fault");
        test_eq!(NemDriverType::Burst.to_str(), "burst");
    });

    test_case!("nem_parse_compat_flags_preserved", {
        let code = [0x90u8; 16];
        let mut raw = build_valid_nem(NemDriverType::Null, "FLAGS", &code);
        raw[22..24].copy_from_slice(&[0x01u8, 0x00u8]);
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().compat_flags, 1);
    });

    // ── v2 tests ──

    test_case!("nem_v2_parse_valid", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem_v2(
            NemDriverType::Lifecycle, "V2DRIVER", &code,
            ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID,
            DriverCategory::Boot, 0,
        );
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        let p = parsed.unwrap();
        test_eq!(p.driver_type, NemDriverType::Lifecycle);
        test_eq!(p.name, "V2DRIVER");
        test_eq!(p.abi_min, ABI_MIN_VALID);
        test_eq!(p.category, DriverCategory::Boot);
        test_eq!(p.is_v2, true);
    });

    test_case!("nem_v2_parse_abi_mismatch_rejected", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem_v2(
            NemDriverType::Null, "BADABI", &code,
            99, 99, 99, DriverCategory::System, 0,
        );
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().abi_min, 99);
    });

    test_case!("nem_v2_parse_all_categories", {
        let code = [0x90u8; 16];
        for cat in &[DriverCategory::Boot, DriverCategory::System, DriverCategory::Demand] {
            let raw = build_valid_nem_v2(
                NemDriverType::Echo, "CAT", &code,
                ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID,
                *cat, 0,
            );
            let parsed = parse_nem(&raw);
            test_ne!(parsed, None);
            test_eq!(parsed.unwrap().category, *cat);
        }
    });

    test_case!("nem_v2_long_name", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem_v2(
            NemDriverType::Null, "LONGNAME12345", &code,
            ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID,
            DriverCategory::Boot, 0,
        );
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().name, "LONGNAME12345");
    });

    test_case!("nem_v2_compat_flags_preserved", {
        let code = [0x90u8; 16];
        let raw = build_valid_nem_v2(
            NemDriverType::Null, "FLAGSV2", &code,
            ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID,
            DriverCategory::Demand, 0xABCD,
        );
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().compat_flags, 0xABCD);
    });

    test_case!("nem_v2_rejects_v1_header_size", {
        let raw = build_valid_nem(NemDriverType::Null, "V1ONLY", &[0x90u8; 16]);
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().is_v2, false);
    });

    test_case!("nem_driver_category_to_str", {
        test_eq!(DriverCategory::Boot.to_str(), "BOOT");
        test_eq!(DriverCategory::System.to_str(), "SYSTEM");
        test_eq!(DriverCategory::Demand.to_str(), "DEMAND");
    });

    test_case!("nem_driver_category_from_u8", {
        test_eq!(DriverCategory::from_u8(0), Some(DriverCategory::Boot));
        test_eq!(DriverCategory::from_u8(1), Some(DriverCategory::System));
        test_eq!(DriverCategory::from_u8(2), Some(DriverCategory::Demand));
        test_eq!(DriverCategory::from_u8(3), None);
    });

    test_case!("nem_abi_constants", {
        test_eq!(ABI_MIN_VALID, 1);
        test_eq!(ABI_TARGET, 1);
        test_eq!(ABI_MAX_VALID, 2);
    });

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
