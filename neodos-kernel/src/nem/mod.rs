// src/nem/mod.rs
// NEM — NeoDOS Test Driver Format (v1) parser

pub const NEM_MAGIC: u32 = 0x004D454E; // "NEM\0"
pub const NEM_VERSION: u32 = 1;
pub const NEM_HEADER_SIZE: u8 = 32;
pub const NEM_API_VERSION: u16 = 1;

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
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedNem<'a> {
    pub driver_type: NemDriverType,
    pub code: &'a [u8],
    pub entry_offset: u16,
    pub name: &'a str,
    pub compat_flags: u16,
}

pub fn parse_nem(data: &[u8]) -> Option<ParsedNem<'_>> {
    if data.len() < NEM_HEADER_SIZE as usize {
        return None;
    }
    // SAFETY: NemHeader is #[repr(C)] with no padding; verified by size check above
    let header: &NemHeader = unsafe { &*(data.as_ptr() as *const NemHeader) };

    if header.magic != NEM_MAGIC
        || header.version != NEM_VERSION
        || header.header_size != NEM_HEADER_SIZE
        || header.api_version != NEM_API_VERSION
    {
        return None;
    }

    let driver_type = NemDriverType::from_u8(header.driver_type)?;

    let code_start = header.code_offset as usize;
    let code_end = code_start.saturating_add(header.code_size as usize);
    if code_end > data.len() {
        return None;
    }

    let entry = header.entry_offset as usize;
    if entry < code_start || entry >= code_end {
        return None;
    }

    let name_len = header.name.iter().position(|&b| b == 0).unwrap_or(8);
    let name = core::str::from_utf8(&header.name[..name_len]).ok()?;
    if name.is_empty() {
        return None;
    }

    Some(ParsedNem {
        driver_type,
        code: &data[code_start..code_end],
        entry_offset: header.entry_offset,
        name,
        compat_flags: header.compat_flags,
    })
}

fn build_valid_nem(driver_type: NemDriverType, name: &str, code: &[u8]) -> alloc::vec::Vec<u8> {
    let mut raw = alloc::vec::Vec::with_capacity(32 + code.len());
    let mut name_bytes = [0u8; 8];
    let nb = name.as_bytes();
    let len = nb.len().min(8);
    name_bytes[..len].copy_from_slice(&nb[..len]);

    raw.extend_from_slice(&NEM_MAGIC.to_le_bytes());
    raw.extend_from_slice(&NEM_VERSION.to_le_bytes());
    raw.push(driver_type as u8);
    raw.push(NEM_HEADER_SIZE);
    raw.extend_from_slice(&(32u16).to_le_bytes()); // entry_offset
    raw.extend_from_slice(&(32u32).to_le_bytes()); // code_offset
    raw.extend_from_slice(&(code.len() as u32).to_le_bytes());
    raw.extend_from_slice(&(NEM_API_VERSION as u16).to_le_bytes());
    raw.extend_from_slice(&0u16.to_le_bytes()); // compat_flags
    raw.extend_from_slice(&name_bytes);
    raw.extend_from_slice(code);
    raw
}

pub fn register_nem_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;

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
        raw[10..12].copy_from_slice(&[48u8, 0u8]); // entry past code
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
            NemDriverType::Null,
            NemDriverType::Echo,
            NemDriverType::Lifecycle,
            NemDriverType::Mutation,
            NemDriverType::Fault,
            NemDriverType::Burst,
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
        test_eq!(NemDriverType::from_u8(3), Some(NemDriverType::Mutation));
        test_eq!(NemDriverType::from_u8(4), Some(NemDriverType::Fault));
        test_eq!(NemDriverType::from_u8(5), Some(NemDriverType::Burst));
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
        raw[22..24].copy_from_slice(&[0x01u8, 0x00u8]); // compat_flags = 1
        let parsed = parse_nem(&raw);
        test_ne!(parsed, None);
        test_eq!(parsed.unwrap().compat_flags, 1);
    });
}
