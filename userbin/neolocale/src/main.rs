//! neolocale — NLT file validator and inspector (host tool).
//!
//! Operates directly on `.nlt` files on the host filesystem. It shares the
//! `libnlt` format library with the runtime and the compiler, so what it
//! validates is exactly what the kernel/userland will accept.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use libnlt::lang::id_to_lang;
use libnlt::{self, ENTRY_FLAG_PLURAL, MAGIC_V2, MAGIC_V3};

fn usage() {
    eprintln!("neolocale — NLT file validator/inspector");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  neolocale validate <file.nlt>      Validate a single NLT file");
    eprintln!("  neolocale stats <file.nlt>         Show entry statistics");
    eprintln!("  neolocale diff <a.nlt> <b.nlt>     Compare two NLT files");
    eprintln!("  neolocale check <dir> [base]       Check translation coverage across locales");
    eprintln!("  neolocale create <app> <language>  Print a starter TOML source");
    eprintln!("  neolocale info <file.nlt>          Alias of stats");
}

fn read(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", path.display());
            None
        }
    }
}

type EntryMap = Vec<(u32, String)>;

fn decoded_entries(data: &[u8]) -> Option<(libnlt::Header, EntryMap)> {
    let header = libnlt::parse_header(data)?;
    let mut scratch = vec![0u8; header.payload_size as usize + 16];
    let mut out = vec![0u8; header.payload_size as usize + header.region_size as usize + 16];
    let decoded = libnlt::decode(data, &mut scratch, &mut out)?;

    let mut map = EntryMap::new();
    for i in 0..header.entry_count as usize {
        let (id, off, flags) = libnlt::read_index(decoded.payload, header.version, i)?;
        let text = if flags & ENTRY_FLAG_PLURAL != 0 {
            let base = off as usize;
            let count = *decoded.payload.get(base)? as usize;
            let mut forms = Vec::new();
            for c in 0..count {
                let rel = *decoded.payload.get(base + 1 + c)? as usize;
                let s = libnlt::str_at(decoded.payload, (base + rel) as u32).unwrap_or("");
                if !s.is_empty() {
                    forms.push(s.to_string());
                }
            }
            format!("<plural: {}>", forms.join(" | "))
        } else {
            libnlt::str_at(decoded.payload, off).unwrap_or("").to_string()
        };
        map.push((id, text));
    }
    Some((header, map))
}

fn cmd_validate(path: &Path) -> i32 {
    let data = match read(path) {
        Some(d) => d,
        None => return 1,
    };

    let mut errors = Vec::new();
    let magic = &data[..data.len().min(4)];
    if magic != MAGIC_V2 && magic != MAGIC_V3 {
        errors.push("bad magic (expected NLT2 or NLT3)".to_string());
    }

    match libnlt::parse_header(&data) {
        Some(header) => {
            if header.entry_count == 0 {
                errors.push("table has no entries".to_string());
            }
            if header.is_signed() && header.signature_size != 64 {
                errors.push("signature flag set but size != 64".to_string());
            }
            if header.is_v3() {
                let mut scratch = vec![0u8; header.payload_size as usize + 16];
                let mut out =
                    vec![0u8; header.payload_size as usize + header.region_size as usize + 16];
                match libnlt::decode(&data, &mut scratch, &mut out) {
                    Some(decoded) => {
                        // Monotonic ID order (required for binary search).
                        let mut prev: Option<u32> = None;
                        for i in 0..header.entry_count as usize {
                            if let Some((id, _off, _fl)) =
                                libnlt::read_index(decoded.payload, header.version, i)
                            {
                                if let Some(p) = prev {
                                    if id <= p {
                                        errors.push(format!("IDs not strictly ascending at #{i}"));
                                    }
                                }
                                prev = Some(id);
                            }
                        }
                    }
                    None => errors.push("payload failed to decode (crc/compression/utf16)".to_string()),
                }
            }
        }
        None => errors.push("header validation failed".to_string()),
    }

    if errors.is_empty() {
        println!("OK  {} is valid", path.display());
        0
    } else {
        println!("FAILED {}:", path.display());
        for e in &errors {
            println!("  - {e}");
        }
        1
    }
}

fn cmd_stats(path: &Path) -> i32 {
    let data = match read(path) {
        Some(d) => d,
        None => return 1,
    };
    let (header, entries) = match decoded_entries(&data) {
        Some(v) => v,
        None => {
            eprintln!("ERROR: cannot decode '{}'", path.display());
            return 1;
        }
    };

    println!("=== {} ===", path.display());
    println!("  Format:      NLTv{}", header.version);
    println!("  Language:    {} (ID={})", id_to_lang(header.language_id), header.language_id);
    println!("  AppID:       {}", header.application_id);
    println!("  Entries:     {}", header.entry_count);
    println!("  Payload:     {} bytes (stored {})", header.payload_size, header.payload_stored);
    println!(
        "  Flags:       compressed={} utf16={} rtl={} signed={} region={}",
        header.is_compressed(),
        header.is_utf16(),
        header.is_rtl(),
        header.is_signed(),
        header.has_region()
    );
    println!();
    let show = entries.len().min(24);
    for (id, text) in entries.iter().take(show) {
        println!("    {id:6}: {text}");
    }
    if entries.len() > show {
        println!("    ... and {} more", entries.len() - show);
    }
    0
}

fn cmd_diff(a: &Path, b: &Path) -> i32 {
    let da = match read(a) {
        Some(d) => d,
        None => return 1,
    };
    let db = match read(b) {
        Some(d) => d,
        None => return 1,
    };
    let (_, ea) = match decoded_entries(&da) {
        Some(v) => v,
        None => {
            eprintln!("ERROR: cannot decode '{}'", a.display());
            return 1;
        }
    };
    let (_, eb) = match decoded_entries(&db) {
        Some(v) => v,
        None => {
            eprintln!("ERROR: cannot decode '{}'", b.display());
            return 1;
        }
    };

    let map_a: BTreeMap<u32, &str> = ea.iter().map(|(id, s)| (*id, s.as_str())).collect();
    let map_b: BTreeMap<u32, &str> = eb.iter().map(|(id, s)| (*id, s.as_str())).collect();

    let mut only_a = 0;
    let mut only_b = 0;
    let mut different = 0;

    println!("=== diff {} <-> {} ===", a.display(), b.display());
    for (id, sa) in &map_a {
        match map_b.get(id) {
            Some(sb) if sb == sa => {}
            Some(sb) => {
                different += 1;
                println!("  ~ {id}: {sa:?} != {sb:?}");
            }
            None => {
                only_a += 1;
                println!("  - {id}: only in {}", a.display());
            }
        }
    }
    for (id, _) in &map_b {
        if !map_a.contains_key(id) {
            only_b += 1;
            println!("  + {id}: only in {}", b.display());
        }
    }
    println!();
    println!("  {only_a} only in A, {only_b} only in B, {different} different");
    0
}

fn cmd_check(dir: &Path, base_lang: &str) -> i32 {
    let mut locales: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                locales.push(e.path());
            }
        }
    }
    locales.sort();
    if locales.is_empty() {
        eprintln!("ERROR: no locale subdirectories in '{}'", dir.display());
        return 1;
    }

    let base_dir = dir.join(base_lang);
    let mut missing = 0usize;

    for loc in &locales {
        let tag = loc.file_name().unwrap().to_string_lossy().to_string();
        if tag == base_lang {
            continue;
        }
        let mut app_names: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&base_dir) {
            for e in rd.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) == Some("nlt") {
                    app_names.push(e.path().file_stem().unwrap().to_string_lossy().to_string());
                }
            }
        }
        app_names.sort();

        for app in &app_names {
            let target = loc.join(format!("{app}.nlt"));
            if !target.exists() {
                println!("  MISSING  {tag}/{app}.nlt");
                missing += 1;
                continue;
            }
            let base = base_dir.join(format!("{app}.nlt"));
            let data_base = std::fs::read(&base).ok();
            let data_tgt = std::fs::read(&target).ok();
            if let (Some(dbb), Some(dbt)) = (data_base, data_tgt) {
                if let (Some((_, eb)), Some((_, et))) = (decoded_entries(&dbb), decoded_entries(&dbt))
                {
                    let ids_b: BTreeSet<u32> = eb.iter().map(|(id, _)| *id).collect();
                    let ids_t: BTreeSet<u32> = et.iter().map(|(id, _)| *id).collect();
                    for id in ids_b.difference(&ids_t) {
                        println!("  UNTRANSLATED {tag}/{app}.nlt id={id}");
                        missing += 1;
                    }
                }
            }
        }
    }

    if missing == 0 {
        println!("OK  all locales complete relative to {base_lang}");
        0
    } else {
        println!();
        println!("{missing} issue(s) found");
        1
    }
}

fn cmd_create(app: &str, language: &str) -> i32 {
    println!(
        r#"[meta]
app = "{app}"
language = "{language}"

[ids]

[strings]
"#
    );
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(1);
    }
    let code = match args[1].as_str() {
        "validate" if args.len() >= 3 => cmd_validate(Path::new(&args[2])),
        "stats" | "info" if args.len() >= 3 => cmd_stats(Path::new(&args[2])),
        "diff" if args.len() >= 4 => cmd_diff(Path::new(&args[2]), Path::new(&args[3])),
        "check" if args.len() >= 3 => {
            let base = args.get(3).map(|s| s.as_str()).unwrap_or("en-US");
            cmd_check(Path::new(&args[2]), base)
        }
        "create" if args.len() >= 4 => cmd_create(&args[2], &args[3]),
        _ => {
            usage();
            1
        }
    };
    std::process::exit(code);
}
