use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ── NLTv2 Binary Format ───────────────────────────────────────────────

const NLT2_MAGIC: &[u8; 4] = b"NLT2";
const NLT2_VERSION: u16 = 2;
const NLT2_HEADER_SIZE: u16 = 32;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

pub fn lang_to_id(lang: &str) -> u32 {
    match lang.to_lowercase().as_str() {
        "en-us" => 1,  "es-es" => 2,  "fr-fr" => 3,  "de-de" => 4,
        "it-it" => 5,  "pt-pt" => 6,  "pt-br" => 7,  "ca-es" => 8,
        "eu-es" => 9,  "gl-es" => 10, "en-gb" => 11, "ja-jp" => 12,
        "zh-cn" => 13, "ru-ru" => 14, "ar-sa" => 15, "nl-nl" => 16,
        "pl-pl" => 17, "sv-se" => 18, "da-dk" => 19, "fi-fi" => 20,
        "nb-no" => 21, "ko-kr" => 22, "tr-tr" => 23, "cs-cz" => 24,
        "hu-hu" => 25,
        _ => crc32(lang.to_lowercase().as_bytes()) & 0x7FFF | 0x8000,
    }
}

pub fn id_to_lang(id: u32) -> &'static str {
    match id {
        1 => "en-US",   2 => "es-ES",   3 => "fr-FR",   4 => "de-DE",
        5 => "it-IT",   6 => "pt-PT",   7 => "pt-BR",   8 => "ca-ES",
        9 => "eu-ES",   10 => "gl-ES",  11 => "en-GB",  12 => "ja-JP",
        13 => "zh-CN",  14 => "ru-RU",  15 => "ar-SA",  16 => "nl-NL",
        17 => "pl-PL",  18 => "sv-SE",  19 => "da-DK",  20 => "fi-FI",
        21 => "nb-NO",  22 => "ko-KR",  23 => "tr-TR",  24 => "cs-CZ",
        25 => "hu-HU",
        _ => "unknown",
    }
}

pub fn app_to_id(app: &str) -> u32 {
    match app.to_lowercase().as_str() {
        "neoshell" => 1,  "neoinit" => 2,  "corehelp" => 3,  "coredir" => 4,
        "corecopy" => 5,  "coretype" => 6, "neolocale" => 7, "neokey" => 8,
        "neomem" => 9,    "neotop" => 10,  "kill" => 11,     "ps" => 12,
        "label" => 13,    "fsck" => 14,    "ndreg" => 15,    "poweroff" => 16,
        "reboot" => 17,   "loadnem" => 18, "datetime" => 19, "ver" => 20,
        "echo" => 21,     "drives" => 22,  "pri" => 23,      "cd" => 24,
        "colors" => 25,   "progress" => 26,"vol" => 27,      "corerd" => 28,
        "coremd" => 29,   "coreren" => 30, "coredel" => 31,  "corecls" => 32,
        "tree" => 33,     "dhcpd" => 34,   "netcfg" => 35,   "ipconfig" => 36,
        "cpuinfo" => 37,  "stresscmd" => 38,"cmdtest" => 39,  "shtest" => 40,
        _ => crc32(app.to_lowercase().as_bytes()) & 0x7FFF | 0x8000,
    }
}

// ── TOML Source Format ─────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct NltSource {
    meta: NltMeta,
    ids: Option<BTreeMap<String, u32>>,
    strings: BTreeMap<String, String>,
}

#[derive(Debug, serde::Deserialize)]
struct NltMeta {
    app: String,
    language: String,
}

// ── Compilation ────────────────────────────────────────────────────────

struct CompiledEntry {
    id: u32,
    string: String,
}

fn compile(source: NltSource) -> Result<Vec<CompiledEntry>, String> {
    let mut entries = Vec::new();

    if let Some(id_map) = &source.ids {
        for (name, string) in &source.strings {
            let id = id_map.get(name).ok_or_else(|| {
                format!("string '{}' has no matching ID in [ids]", name)
            })?;
            entries.push(CompiledEntry {
                id: *id,
                string: string.clone(),
            });
        }
        for (name, _id) in id_map {
            if !source.strings.contains_key(name) {
                return Err(format!("ID '{}' defined but no matching string in [strings]", name));
            }
        }
    } else {
        let mut next_id: u32 = 1001;
        for (_name, string) in &source.strings {
            entries.push(CompiledEntry {
                id: next_id,
                string: string.clone(),
            });
            next_id += 1;
        }
    }

    entries.sort_by_key(|e| e.id);

    for i in 1..entries.len() {
        if entries[i].id == entries[i - 1].id {
            return Err(format!("duplicate string ID: {}", entries[i].id));
        }
    }

    Ok(entries)
}

fn build_nlt(source: NltSource, verbose: bool) -> Result<Vec<u8>, String> {
    let lang = source.meta.language.clone();
    let app = source.meta.app.clone();
    let entries = compile(source)?;
    let count = entries.len() as u32;
    if count == 0 {
        return Err("no strings defined in [strings]".into());
    }

    let index_size = count as usize * 8;
    let header_size = NLT2_HEADER_SIZE as usize;
    let data_start = header_size + index_size;

    let mut strings_buf = Vec::new();
    let mut offsets = Vec::new();
    for entry in &entries {
        offsets.push((data_start + strings_buf.len()) as u32);
        strings_buf.extend_from_slice(entry.string.as_bytes());
        strings_buf.push(0);
    }

    let total_size = data_start + strings_buf.len();
    let mut nlt = vec![0u8; total_size];

    // Header
    nlt[0..4].copy_from_slice(NLT2_MAGIC);
    nlt[4..6].copy_from_slice(&NLT2_VERSION.to_le_bytes());
    nlt[6..8].copy_from_slice(&NLT2_HEADER_SIZE.to_le_bytes());
    nlt[8..12].copy_from_slice(&lang_to_id(&lang).to_le_bytes());
    nlt[12..16].copy_from_slice(&app_to_id(&app).to_le_bytes());
    nlt[16..20].copy_from_slice(&count.to_le_bytes());

    // Index table
    for (i, entry) in entries.iter().enumerate() {
        let off = header_size + i * 8;
        nlt[off..off + 4].copy_from_slice(&entry.id.to_le_bytes());
        nlt[off + 4..off + 8].copy_from_slice(&offsets[i].to_le_bytes());
    }

    // String data
    nlt[data_start..].copy_from_slice(&strings_buf);

    // CRC32 (with checksum field = 0 for calculation)
    let checksum = crc32(&nlt);
    nlt[24..28].copy_from_slice(&checksum.to_le_bytes());

    if verbose {
        eprintln!("  entries: {}", count);
        eprintln!("  header: {} bytes", header_size);
        eprintln!("  index: {} bytes", index_size);
        eprintln!("  strings: {} bytes", strings_buf.len());
        eprintln!("  total: {} bytes", total_size);
        eprintln!("  crc32: 0x{:08X}", checksum);
    }

    Ok(nlt)
}

// ── Rust constants file generation ────────────────────────────────────

fn generate_rust_constants(source: &NltSource) -> Result<String, String> {
    let mut out = String::new();
    out.push_str("// Auto-generated by nltc. Do not edit.\n");
    out.push_str(&format!("// Source: {} ({})\n\n", source.meta.app, source.meta.language));
    out.push_str("#![allow(dead_code)]\n\n");

    if let Some(ids) = &source.ids {
        for (name, id) in ids {
            out.push_str(&format!("pub const {}: u32 = {};\n", name, id));
        }
    } else {
        let mut next_id: u32 = 1001;
        for (name, _) in &source.strings {
            out.push_str(&format!("pub const {}: u32 = {};\n", name, next_id));
            next_id += 1;
        }
    }

    out.push('\n');
    Ok(out)
}

// ── TOML source generation (scaffold) ─────────────────────────────────

fn generate_toml(app: &str, language: &str) -> String {
    format!(
        r#"[meta]
app = "{}"
language = "{}"

[ids]
# IDS_EXAMPLE = 1001

[strings]
# IDS_EXAMPLE = "Example string"
"#,
        app, language
    )
}

// ── CLI ────────────────────────────────────────────────────────────────

fn print_usage() {
    eprintln!("Neo Language Table Compiler (nltc) v0.2");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  nltc <input.toml> [output.nlt]");
    eprintln!("  nltc --check <input.toml>");
    eprintln!("  nltc --generate-ids <input.toml>");
    eprintln!("  nltc --generate-rust <input.toml> [output.rs]");
    eprintln!("  nltc --scaffold <app> <language>");
    eprintln!("  nltc --list-langs");
    eprintln!("  nltc --lang-id <language-tag>");
    eprintln!("  nltc --app-id <app-name>");
    eprintln!("  nltc --info <file.nlt>");
    eprintln!("  nltc --generate-all <locale-dir>");
    eprintln!();
}

fn cmd_compile(input: &Path, output: &Path, verbose: bool) {
    let source_str = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: cannot read '{}': {}", input.display(), e); std::process::exit(1); }
    };
    let source: NltSource = match toml::from_str(&source_str) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: TOML parse error in '{}': {}", input.display(), e); std::process::exit(1); }
    };
    if source.meta.app.is_empty() { eprintln!("ERROR: [meta] app field is required"); std::process::exit(1); }
    if source.meta.language.is_empty() { eprintln!("ERROR: [meta] language field is required"); std::process::exit(1); }
    if source.strings.is_empty() { eprintln!("ERROR: no strings defined in [strings]"); std::process::exit(1); }

    if verbose { eprintln!("Compiling {} -> {}:", input.display(), output.display()); }
    match build_nlt(source, verbose) {
        Ok(nlt) => {
            match std::fs::write(output, &nlt) {
                Ok(_) => eprintln!("OK  {} -> {} ({} bytes)", input.display(), output.display(), nlt.len()),
                Err(e) => { eprintln!("ERROR: cannot write '{}': {}", output.display(), e); std::process::exit(1); }
            }
        }
        Err(e) => { eprintln!("ERROR: compilation failed: {}", e); std::process::exit(1); }
    }
}

fn cmd_check(input: &Path) {
    let source_str = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: cannot read '{}': {}", input.display(), e); std::process::exit(1); }
    };
    let source: NltSource = match toml::from_str(&source_str) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: TOML parse error: {}", e); std::process::exit(1); }
    };
    let mut errors = 0;
    if source.meta.app.is_empty() { eprintln!("ERROR: [meta] app field is required"); errors += 1; }
    if source.meta.language.is_empty() { eprintln!("ERROR: [meta] language field is required"); errors += 1; }
    if source.strings.is_empty() { eprintln!("ERROR: no strings defined"); errors += 1; }

    if let Some(ids) = &source.ids {
        for (name, _string) in &source.strings {
            if !ids.contains_key(name) { eprintln!("ERROR: string '{}' has no matching ID in [ids]", name); errors += 1; }
        }
        for (name, _id) in ids {
            if !source.strings.contains_key(name) { eprintln!("ERROR: ID '{}' has no matching string in [strings]", name); errors += 1; }
        }
        let mut seen: Vec<u32> = Vec::new();
        for (_name, id) in ids {
            if seen.contains(id) { eprintln!("ERROR: duplicate ID {} in [ids]", id); errors += 1; }
            seen.push(*id);
        }
    }

    if errors == 0 {
        eprintln!("OK  {} — valid ({} strings)", input.display(), source.strings.len());
    } else {
        eprintln!("FAILED {} with {} error(s)", input.display(), errors);
        std::process::exit(1);
    }
}

fn cmd_generate_ids(input: &Path) {
    let source_str = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: cannot read '{}': {}", input.display(), e); std::process::exit(1); }
    };
    let mut source_val: toml::Value = match toml::from_str(&source_str) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: TOML parse error: {}", e); std::process::exit(1); }
    };
    if source_val.get("ids").is_some() {
        eprintln!("OK  [ids] section already exists — no changes needed");
        return;
    }
    let strings = match source_val.get("strings").and_then(|s| s.as_table()) {
        Some(t) => t,
        None => { eprintln!("ERROR: no [strings] section found"); std::process::exit(1); }
    };
    let mut ids = toml::value::Table::new();
    let mut next_id: i64 = 1001;
    for (name, _) in strings {
        ids.insert(name.clone(), toml::Value::Integer(next_id));
        next_id += 1;
    }
    source_val.as_table_mut().unwrap().insert("ids".to_string(), toml::Value::Table(ids));
    match std::fs::write(input, source_val.to_string()) {
        Ok(_) => eprintln!("OK  IDs written to '{}'", input.display()),
        Err(e) => { eprintln!("ERROR: cannot write '{}': {}", input.display(), e); std::process::exit(1); }
    }
}

fn cmd_generate_rust(input: &Path, output: Option<&Path>) {
    let source_str = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: cannot read '{}': {}", input.display(), e); std::process::exit(1); }
    };
    let source: NltSource = match toml::from_str(&source_str) {
        Ok(s) => s,
        Err(e) => { eprintln!("ERROR: TOML parse error: {}", e); std::process::exit(1); }
    };
    match generate_rust_constants(&source) {
        Ok(rust_code) => {
            if let Some(out_path) = output {
                match std::fs::write(out_path, &rust_code) {
                    Ok(_) => eprintln!("OK  Rust constants written to '{}'", out_path.display()),
                    Err(e) => { eprintln!("ERROR: cannot write '{}': {}", out_path.display(), e); std::process::exit(1); }
                }
            } else {
                print!("{}", rust_code);
            }
        }
        Err(e) => { eprintln!("ERROR: {}", e); std::process::exit(1); }
    }
}

fn cmd_scaffold(app: &str, language: &str) {
    let toml = generate_toml(app, language);
    println!("{}", toml);
}

fn cmd_list_langs() {
    eprintln!("Known language IDs:");
    for id in 1..=25 {
        let tag = id_to_lang(id);
        let (name, native) = match id {
            1 => ("English", "English"),
            2 => ("Spanish", "Español"),
            3 => ("French", "Français"),
            4 => ("German", "Deutsch"),
            5 => ("Italian", "Italiano"),
            6 => ("Portuguese", "Português"),
            7 => ("Brazilian Portuguese", "Português (Brasil)"),
            8 => ("Catalan", "Català"),
            9 => ("Basque", "Euskara"),
            10 => ("Galician", "Galego"),
            11 => ("British English", "English (UK)"),
            12 => ("Japanese", "日本語"),
            13 => ("Chinese (Simplified)", "简体中文"),
            14 => ("Russian", "Русский"),
            15 => ("Arabic", "العربية"),
            16 => ("Dutch", "Nederlands"),
            17 => ("Polish", "Polski"),
            18 => ("Swedish", "Svenska"),
            19 => ("Danish", "Dansk"),
            20 => ("Finnish", "Suomi"),
            21 => ("Norwegian Bokmål", "Norsk Bokmål"),
            22 => ("Korean", "한국어"),
            23 => ("Turkish", "Türkçe"),
            24 => ("Czech", "Čeština"),
            25 => ("Hungarian", "Magyar"),
            _ => ("Unknown", "Unknown"),
        };
        eprintln!("  {:4}  {:<7}  {} ({})", id, tag, name, native);
    }
}

fn cmd_lang_id(tag: &str) {
    let id = lang_to_id(tag);
    println!("{}", id);
    if id >= 0x8000 {
        eprintln!("WARNING: '{}' is not a standard tag (hash ID: {:#x})", tag, id);
    } else {
        let lang_name = id_to_lang(id);
        eprintln!("INFO: '{}' = {} ({})", tag, id, lang_name);
    }
}

fn cmd_app_id(name: &str) {
    let id = app_to_id(name);
    println!("{}", id);
    if id >= 0x8000 {
        eprintln!("WARNING: '{}' is not a known app (hash ID: {:#x})", name, id);
    }
}

fn cmd_info(path: &Path) {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => { eprintln!("ERROR: cannot read '{}': {}", path.display(), e); std::process::exit(1); }
    };
    if data.len() < 4 { eprintln!("ERROR: file too small"); std::process::exit(1); }

    if &data[..4] == b"NLT2" {
        cmd_info_v2(&data, path);
    } else {
        eprintln!("ERROR: not a valid NLTv2 file (magic: {:02X?})", &data[..4]);
        std::process::exit(1);
    }
}

fn cmd_info_v2(data: &[u8], path: &Path) {
    if data.len() < 32 { eprintln!("ERROR: truncated NLTv2 file"); return; }
    let version = u16::from_le_bytes([data[4], data[5]]);
    let header_size = u16::from_le_bytes([data[6], data[7]]);
    let lang_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let app_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let count = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let flags = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
    let stored_crc = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let mut check_data = data.to_vec();
    check_data[24..28].copy_from_slice(&[0, 0, 0, 0]);
    let actual_crc = crc32(&check_data);

    println!("=== NLTv2 File Info ===");
    println!("  File:       {}", path.display());
    println!("  Size:       {} bytes", data.len());
    println!("  Format:     NLTv2");
    println!("  Version:    {}", version);
    println!("  HdrSize:    {}", header_size);
    println!("  Language:   {} (ID={})", id_to_lang(lang_id), lang_id);
    println!("  AppID:      {}", app_id);
    println!("  Strings:    {}", count);
    println!("  Flags:      0x{:08X}", flags);
    println!("  CRC32:      0x{:08X} {}", stored_crc,
        if stored_crc == actual_crc { "(OK)" } else { "(MISMATCH!)" });

    let entry_size = 8usize;
    let header_sz = header_size as usize;
    println!();
    let show = (count as usize).min(16);
    println!("  String entries (first {}):", show);
    for i in 0..show {
        let off = header_sz + i * entry_size;
        if off + 8 > data.len() { break; }
        let sid = u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
        let str_off = u32::from_le_bytes([data[off+4], data[off+5], data[off+6], data[off+7]]) as usize;
        let s = if str_off < data.len() {
            let end = data[str_off..].iter().position(|&b| b == 0).unwrap_or(data.len() - str_off);
            String::from_utf8_lossy(&data[str_off..str_off + end])
        } else {
            "INVALID_OFFSET".into()
        };
        println!("    {:6}: {}", sid, s);
    }
    if count > 16 {
        println!("    ... and {} more entries", count - 16);
    }
}

fn cmd_generate_all(locale_dir: &Path) {
    if !locale_dir.exists() {
        eprintln!("ERROR: directory not found: '{}'", locale_dir.display());
        std::process::exit(1);
    }
    let mut compiled = 0u32;
    let mut failed = 0u32;
    for entry in std::fs::read_dir(locale_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") { continue; }
        let input = &path;
        let mut output = path.to_path_buf();
        output.set_extension("nlt");
        eprintln!("Compiling: {} ...", input.display());
        let source_str = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => { eprintln!("  ERROR: read failed: {}", e); failed += 1; continue; }
        };
        let source: NltSource = match toml::from_str(&source_str) {
            Ok(s) => s,
            Err(e) => { eprintln!("  ERROR: TOML parse: {}", e); failed += 1; continue; }
        };
        match build_nlt(source, false) {
            Ok(nlt) => {
                match std::fs::write(&output, &nlt) {
                    Ok(_) => { eprintln!("  -> {} ({} bytes)", output.display(), nlt.len()); compiled += 1; }
                    Err(e) => { eprintln!("  ERROR: write failed: {}", e); failed += 1; }
                }
            }
            Err(e) => { eprintln!("  ERROR: compilation: {}", e); failed += 1; }
        }
    }
    eprintln!("\nResult: {} compiled, {} failed", compiled, failed);
    if failed > 0 { std::process::exit(1); }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { print_usage(); std::process::exit(1); }

    match args[1].as_str() {
        "--check" => {
            if args.len() < 3 { eprintln!("Usage: nltc --check <input.toml>"); std::process::exit(1); }
            cmd_check(Path::new(&args[2]));
        }
        "--generate-ids" => {
            if args.len() < 3 { eprintln!("Usage: nltc --generate-ids <input.toml>"); std::process::exit(1); }
            cmd_generate_ids(Path::new(&args[2]));
        }
        "--generate-rust" => {
            if args.len() < 3 { eprintln!("Usage: nltc --generate-rust <input.toml> [output.rs]"); std::process::exit(1); }
            let output = if args.len() >= 4 { Some(Path::new(&args[3])) } else { None };
            cmd_generate_rust(Path::new(&args[2]), output);
        }
        "--scaffold" => {
            if args.len() < 4 { eprintln!("Usage: nltc --scaffold <app> <language>"); std::process::exit(1); }
            cmd_scaffold(&args[2], &args[3]);
        }
        "--list-langs" => cmd_list_langs(),
        "--lang-id" => {
            if args.len() < 3 { eprintln!("Usage: nltc --lang-id <language-tag>"); std::process::exit(1); }
            cmd_lang_id(&args[2]);
        }
        "--app-id" => {
            if args.len() < 3 { eprintln!("Usage: nltc --app-id <app-name>"); std::process::exit(1); }
            cmd_app_id(&args[2]);
        }
        "--info" => {
            if args.len() < 3 { eprintln!("Usage: nltc --info <file.nlt>"); std::process::exit(1); }
            cmd_info(Path::new(&args[2]));
        }
        "--generate-all" => {
            if args.len() < 3 { eprintln!("Usage: nltc --generate-all <locale-dir>"); std::process::exit(1); }
            cmd_generate_all(Path::new(&args[2]));
        }
        "--verbose" => {
            if args.len() < 3 { eprintln!("Usage: nltc --verbose <input.toml> [output.nlt]"); std::process::exit(1); }
            let input = Path::new(&args[2]);
            let output = if args.len() >= 4 { PathBuf::from(&args[3]) } else { let mut o = input.to_path_buf(); o.set_extension("nlt"); o };
            cmd_compile(input, &output, true);
        }
        _ => {
            let input = Path::new(&args[1]);
            if !input.exists() { eprintln!("ERROR: file not found: '{}'", input.display()); std::process::exit(1); }
            let output = if args.len() >= 3 { PathBuf::from(&args[2]) } else { let mut o = input.to_path_buf(); o.set_extension("nlt"); o };
            cmd_compile(input, &output, false);
        }
    }
}
