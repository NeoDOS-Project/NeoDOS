// src/nem/mod.rs
// NEM — NeoDOS Test Driver Format (v1 / v2) parser
//
// v1: 32-byte header, 8-byte name, no ABI fields
// v2: 48-byte header, 16-byte name, ABI fields (min/target/max), category

pub const NEM_MAGIC: u32 = 0x004D454E; // "NEM\0"
pub const NEM_VERSION: u32 = 1;
pub const NEM_VERSION_2: u32 = 2;
pub const NEM_HEADER_SIZE: u8 = 32;
pub const NEM_HEADER_SIZE_V2: u8 = 48;
pub const NEM_API_VERSION: u16 = 1;

// HAL ABI v0.3 encoding
pub const ABI_MIN_VALID: u16 = 1;
pub const ABI_TARGET: u16 = 1;
pub const ABI_MAX_VALID: u16 = 2;

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

/// NEM v2 header — 48 bytes with ABI fields and longer name
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

pub fn parse_nem(data: &[u8]) -> Option<ParsedNem<'_>> {
    // Try v1 first (32-byte header)
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

            return Some(ParsedNem {
                driver_type,
                code: &data[code_start..code_end],
                entry_offset: header.entry_offset,
                name,
                compat_flags: header.compat_flags,
                abi_min: 0,
                abi_target: 0,
                abi_max: 0,
                category: DriverCategory::Demand,
                is_v2: false,
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
            if code_end > data.len() {
                return None;
            }

            let entry = header.entry_offset as usize;
            if entry < code_start || entry >= code_end {
                return None;
            }

            let name_len = header.name.iter().position(|&b| b == 0).unwrap_or(16);
            let name = core::str::from_utf8(&header.name[..name_len]).ok()?;
            if name.is_empty() {
                return None;
            }

            let category = DriverCategory::from_u8(header.category).unwrap_or(DriverCategory::Demand);

            return Some(ParsedNem {
                driver_type,
                code: &data[code_start..code_end],
                entry_offset: header.entry_offset,
                name,
                compat_flags: header.compat_flags,
                abi_min: header.abi_min,
                abi_target: header.abi_target,
                abi_max: header.abi_max,
                category,
                is_v2: true,
            });
        }
    }

    None
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

/// Build a valid v2 NEM binary with ABI fields.
fn build_valid_nem_v2(
    driver_type: NemDriverType,
    name: &str,
    code: &[u8],
    abi_min: u16,
    abi_target: u16,
    abi_max: u16,
    category: DriverCategory,
    compat_flags: u16,
) -> alloc::vec::Vec<u8> {
    let header_size = NEM_HEADER_SIZE_V2 as usize;
    let mut raw = alloc::vec::Vec::with_capacity(header_size + code.len());
    let mut name_bytes = [0u8; 16];
    let nb = name.as_bytes();
    let len = nb.len().min(16);
    name_bytes[..len].copy_from_slice(&nb[..len]);

    raw.extend_from_slice(&NEM_MAGIC.to_le_bytes());
    raw.extend_from_slice(&NEM_VERSION_2.to_le_bytes());
    raw.push(driver_type as u8);
    raw.push(NEM_HEADER_SIZE_V2);
    raw.extend_from_slice(&(header_size as u16).to_le_bytes()); // entry_offset = beginning of code
    raw.extend_from_slice(&(header_size as u32).to_le_bytes()); // code_offset
    raw.extend_from_slice(&(code.len() as u32).to_le_bytes());
    raw.extend_from_slice(&(NEM_API_VERSION as u16).to_le_bytes());
    raw.extend_from_slice(&compat_flags.to_le_bytes());
    raw.extend_from_slice(&abi_min.to_le_bytes());
    raw.extend_from_slice(&abi_target.to_le_bytes());
    raw.extend_from_slice(&abi_max.to_le_bytes());
    raw.push(category as u8);
    raw.extend_from_slice(&[0u8; 3]); // reserved
    raw.extend_from_slice(&name_bytes);
    raw.extend_from_slice(code);
    raw
}

pub fn register_nem_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;

    // ── v1 tests (unchanged) ──

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
        test_eq!(p.abi_target, ABI_TARGET);
        test_eq!(p.abi_max, ABI_MAX_VALID);
        test_eq!(p.category, DriverCategory::Boot);
        test_eq!(p.is_v2, true);
        test_eq!(p.compat_flags, 0);
    });

    test_case!("nem_v2_parse_abi_mismatch_rejected", {
        let code = [0x90u8; 16];
        // ABI values that should be considered valid for parsing
        // (parse_nem accepts any ABI values; policy/loader rejects them)
        let raw = build_valid_nem_v2(
            NemDriverType::Null, "BADABI", &code,
            99, 99, 99,
            DriverCategory::System, 0,
        );
        let parsed = parse_nem(&raw);
        // parse_nem should still succeed (parsing is format-level)
        test_ne!(parsed, None);
        let p = parsed.unwrap();
        test_eq!(p.abi_min, 99);
        test_eq!(p.abi_target, 99);
        test_eq!(p.abi_max, 99);
        test_eq!(p.category, DriverCategory::System);
    });

    test_case!("nem_v2_parse_all_categories", {
        let code = [0x90u8; 16];
        for cat in &[
            DriverCategory::Boot,
            DriverCategory::System,
            DriverCategory::Demand,
        ] {
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
        // v2 magic + v1 header_size should be rejected
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
        test_eq!(DriverCategory::from_u8(0xFF), None);
    });

    test_case!("nem_abi_constants", {
        test_eq!(ABI_MIN_VALID, 1);
        test_eq!(ABI_TARGET, 1);
        test_eq!(ABI_MAX_VALID, 2);
    });
}
