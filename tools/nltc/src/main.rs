//! nltc — Neo Language Table Compiler.
//!
//! Compiles TOML sources to NLTv3 binaries. The binary format itself lives in
//! the shared `libnlt` crate so the compiler and the runtime can never drift.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use libnlt::lang::{app_to_id, id_to_lang, lang_name, lang_to_id};
use libnlt::{
    crc32, lzss, region, ENTRY_FLAG_PLURAL, FLAG_COMPRESSED, FLAG_REGION, FLAG_RTL, FLAG_SIGNED,
    FLAG_UTF16, HEADER_V3_SIZE, MAGIC_V3, PLURAL_CATEGORIES, VERSION_V3,
};

// ── TOML source format ─────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct NltSource {
    meta: NltMeta,
    #[serde(default)]
    ids: BTreeMap<String, u32>,
    #[serde(default)]
    strings: BTreeMap<String, String>,
    #[serde(default)]
    plural: BTreeMap<String, BTreeMap<String, String>>,
    region: Option<RegionSource>,
}

#[derive(Debug, serde::Deserialize)]
struct NltMeta {
    app: String,
    language: String,
}

#[derive(Debug, serde::Deserialize, Default)]
struct RegionSource {
    #[serde(default)]
    decimal_separator: String,
    #[serde(default)]
    thousands_separator: String,
    #[serde(default)]
    currency_symbol: String,
    #[serde(default)]
    date_short: String,
    #[serde(default)]
    date_long: String,
    #[serde(default)]
    time_pattern: String,
    #[serde(default)]
    time_24h: bool,
    #[serde(default)]
    currency_before: bool,
    #[serde(default = "default_true")]
    group_thousands: bool,
}

fn default_true() -> bool {
    true
}

// ── Compilation ────────────────────────────────────────────────────────

enum CompiledEntry {
    Single { id: u32, value: String },
    Plural { id: u32, forms: [String; PLURAL_CATEGORIES] },
}

impl CompiledEntry {
    fn id(&self) -> u32 {
        match self {
            CompiledEntry::Single { id, .. } | CompiledEntry::Plural { id, .. } => *id,
        }
    }
}

fn plural_category_index(name: &str) -> Option<usize> {
    match name {
        "zero" => Some(libnlt::plural::ZERO),
        "one" => Some(libnlt::plural::ONE),
        "two" => Some(libnlt::plural::TWO),
        "few" => Some(libnlt::plural::FEW),
        "many" => Some(libnlt::plural::MANY),
        "other" => Some(libnlt::plural::OTHER),
        _ => None,
    }
}

fn compile(source: &NltSource) -> Result<Vec<CompiledEntry>, String> {
    // Build the full name -> id map, requiring explicit IDs when provided.
    let has_ids = !source.ids.is_empty();
    let mut entries: Vec<CompiledEntry> = Vec::new();
    let mut auto_id: u32 = 1001;

    for (name, value) in &source.strings {
        let id = if has_ids {
            *source
                .ids
                .get(name)
                .ok_or_else(|| format!("string '{name}' has no matching ID in [ids]"))?
        } else {
            let id = auto_id;
            auto_id += 1;
            id
        };
        entries.push(CompiledEntry::Single {
            id,
            value: value.clone(),
        });
    }

    for (name, forms) in &source.plural {
        let id = if has_ids {
            *source
                .ids
                .get(name)
                .ok_or_else(|| format!("plural '{name}' has no matching ID in [ids]"))?
        } else {
            let id = auto_id;
            auto_id += 1;
            id
        };
        let mut slots: [String; PLURAL_CATEGORIES] = Default::default();
        for (cat, text) in forms {
            let idx = plural_category_index(cat)
                .ok_or_else(|| format!("plural '{name}' has unknown category '{cat}'"))?;
            slots[idx] = text.clone();
        }
        if slots[libnlt::plural::OTHER].is_empty() {
            return Err(format!("plural '{name}' must define an 'other' form"));
        }
        entries.push(CompiledEntry::Plural { id, forms: slots });
    }

    if has_ids {
        for (name, _) in &source.ids {
            if !source.strings.contains_key(name) && !source.plural.contains_key(name) {
                return Err(format!(
                    "ID '{name}' defined but no matching string or plural in the source"
                ));
            }
        }
    }

    entries.sort_by_key(|e| e.id());
    for w in entries.windows(2) {
        if w[0].id() == w[1].id() {
            return Err(format!("duplicate string ID: {}", w[1].id()));
        }
    }
    if entries.is_empty() {
        return Err("no strings or plurals defined".into());
    }
    Ok(entries)
}

struct BuildOptions {
    compress: bool,
    utf16: bool,
    sign: bool,
    sign_seed: [u8; 32],
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            compress: true,
            utf16: false,
            sign: true,
            sign_seed: libnlt::signature::DEV_SEED,
        }
    }
}

fn encode_str(out: &mut Vec<u8>, s: &str, utf16: bool) {
    if utf16 {
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    } else {
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }
}

/// Build the `[index][strings]` payload.
fn build_payload(entries: &[CompiledEntry], utf16: bool) -> Result<(Vec<u8>, u32), String> {
    let count = entries.len() as u32;
    let index_size = entries.len() * libnlt::ENTRY_V3_SIZE;
    let mut out = vec![0u8; index_size];

    for (i, entry) in entries.iter().enumerate() {
        match entry {
            CompiledEntry::Single { id, value } => {
                let off = out.len() as u32;
                libnlt::write_index_v3(&mut out, i, *id, off, 0);
                encode_str(&mut out, value, utf16);
            }
            CompiledEntry::Plural { id, forms } => {
                let base = out.len();
                let mut rel = [0u8; PLURAL_CATEGORIES];
                // Reserve group header: count + rel offsets.
                out.push(PLURAL_CATEGORIES as u8);
                out.extend_from_slice(&[0u8; PLURAL_CATEGORIES]);
                for (c, form) in forms.iter().enumerate() {
                    let r = out.len() - base;
                    if r > 255 {
                        return Err("plural group too large (offset > 255)".into());
                    }
                    rel[c] = r as u8;
                    encode_str(&mut out, form, utf16);
                }
                out[base + 1..base + 1 + PLURAL_CATEGORIES].copy_from_slice(&rel);
                libnlt::write_index_v3(&mut out, i, *id, base as u32, ENTRY_FLAG_PLURAL);
            }
        }
    }
    Ok((out, count))
}

fn build_region(src: &RegionSource) -> Vec<u8> {
    let mut buf = [0u8; 512];
    let n = region::encode(
        &src.decimal_separator,
        &src.thousands_separator,
        &src.currency_symbol,
        &src.date_short,
        &src.date_long,
        &src.time_pattern,
        src.time_24h,
        src.currency_before,
        src.group_thousands,
        &mut buf,
    )
    .unwrap_or(0);
    buf[..n].to_vec()
}

fn build_nlt(source: &NltSource, opts: &BuildOptions, verbose: bool) -> Result<Vec<u8>, String> {
    let entries = compile(source)?;
    let count = entries.len() as u32;
    let (payload, _) = build_payload(&entries, opts.utf16)?;
    let payload_size = payload.len();
    let payload_crc = crc32(&payload);

    let mut flags = 0u32;
    if opts.utf16 {
        flags |= FLAG_UTF16;
    }
    if libnlt::plural::is_rtl(lang_to_id(&source.meta.language)) {
        flags |= FLAG_RTL;
    }

    // Optional compression (only kept if it actually shrinks the payload).
    let (stored, compressed) = if opts.compress {
        let mut bound = vec![0u8; lzss::compress_bound(payload.len())];
        match lzss::compress(&payload, &mut bound) {
            Some(n) if n < payload_size => {
                bound.truncate(n);
                (bound, true)
            }
            _ => (payload.clone(), false),
        }
    } else {
        (payload.clone(), false)
    };
    if compressed {
        flags |= FLAG_COMPRESSED;
    }

    let region_block = source.region.as_ref().map(build_region).unwrap_or_default();
    if !region_block.is_empty() {
        flags |= FLAG_REGION;
    }
    if opts.sign {
        flags |= FLAG_SIGNED;
    }

    let payload_offset = HEADER_V3_SIZE as u32;
    let region_offset = if region_block.is_empty() {
        0
    } else {
        payload_offset + stored.len() as u32
    };
    let sig_offset = if opts.sign {
        payload_offset + stored.len() as u32 + region_block.len() as u32
    } else {
        0
    };

    let mut nlt = vec![0u8; HEADER_V3_SIZE];
    nlt[0..4].copy_from_slice(&MAGIC_V3);
    nlt[4..6].copy_from_slice(&VERSION_V3.to_le_bytes());
    nlt[6..8].copy_from_slice(&(HEADER_V3_SIZE as u16).to_le_bytes());
    nlt[8..12].copy_from_slice(&lang_to_id(&source.meta.language).to_le_bytes());
    nlt[12..16].copy_from_slice(&app_to_id(&source.meta.app).to_le_bytes());
    nlt[16..20].copy_from_slice(&count.to_le_bytes());
    nlt[20..24].copy_from_slice(&flags.to_le_bytes());
    nlt[24..28].copy_from_slice(&payload_offset.to_le_bytes());
    nlt[28..32].copy_from_slice(&(payload_size as u32).to_le_bytes());
    nlt[32..36].copy_from_slice(&(stored.len() as u32).to_le_bytes());
    nlt[36..40].copy_from_slice(&payload_crc.to_le_bytes());
    nlt[40..44].copy_from_slice(&region_offset.to_le_bytes());
    nlt[44..48].copy_from_slice(&(region_block.len() as u32).to_le_bytes());
    nlt[48..52].copy_from_slice(&sig_offset.to_le_bytes());
    nlt[52..56].copy_from_slice(&(if opts.sign { 64u32 } else { 0 }).to_le_bytes());

    nlt.extend_from_slice(&stored);
    nlt.extend_from_slice(&region_block);

    if opts.sign {
        let mut sig = [0u8; 64];
        if !libnlt::signature::sign(&opts.sign_seed, &nlt, &mut sig) {
            return Err("signing failed (invalid seed)".into());
        }
        nlt.extend_from_slice(&sig);
    }

    if verbose {
        eprintln!("  entries:    {count}");
        eprintln!("  payload:    {payload_size} bytes (stored {})", stored.len());
        eprintln!("  compressed: {compressed}");
        eprintln!("  utf16:      {}", opts.utf16);
        eprintln!("  region:     {} bytes", region_block.len());
        eprintln!("  signed:     {}", opts.sign);
        eprintln!("  total:      {} bytes", nlt.len());
        eprintln!("  crc32:      0x{payload_crc:08X}");
    }

    Ok(nlt)
}

// ── Rust constants generation ─────────────────────────────────────────

fn generate_rust_constants(source: &NltSource) -> Result<String, String> {
    let mut out = String::new();
    out.push_str("// Auto-generated by nltc. Do not edit.\n");
    out.push_str(&format!(
        "// Source: {} ({})\n\n",
        source.meta.app, source.meta.language
    ));
    out.push_str("#![allow(dead_code)]\n\n");

    if source.ids.is_empty() {
        let mut names: Vec<&String> = source.strings.keys().collect();
        names.extend(source.plural.keys());
        names.sort();
        let mut id = 1001u32;
        for name in names {
            out.push_str(&format!("pub const {name}: u32 = {id};\n"));
            id += 1;
        }
    } else {
        for (name, id) in &source.ids {
            out.push_str(&format!("pub const {name}: u32 = {id};\n"));
        }
    }
    out.push('\n');
    Ok(out)
}

// ── Bindings verification + frozen IDs + keymap (#572/#573/#578) ──────

/// Deterministic catalog id map: explicit `[ids]`, else nltc's default
/// assignment (sorted string/plural names from 1001).
fn catalog_id_map(source: &NltSource) -> BTreeMap<String, u32> {
    if !source.ids.is_empty() {
        return source.ids.clone();
    }
    let mut names: Vec<&String> = source.strings.keys().collect();
    names.extend(source.plural.keys());
    names.sort();
    let mut out = BTreeMap::new();
    let mut id = 1001u32;
    for n in names {
        out.insert(n.clone(), id);
        id += 1;
    }
    out
}

/// Parse `[pub] const NAME: u32 = N;` declarations from Rust source.
///
/// Intentionally a lightweight line parser (no full Rust front-end): it only
/// understands decimal `u32` constants, which is what translation id files use.
fn parse_rust_u32_consts(text: &str) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        let line = line.strip_prefix("pub ").unwrap_or(line);
        let Some(rest) = line.strip_prefix("const ") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(':') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("u32") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = rest.trim().trim_end_matches(';').trim().replace('_', "");
        if let Ok(n) = value.parse::<u32>() {
            out.insert(name.trim().to_string(), n);
        }
    }
    out
}

/// `--verify-bindings <rust.rs> <catalog.toml>` (#572).
///
/// Fails when the Rust id constants and the catalog disagree. Extra Rust
/// constants (not in the catalog) are reported as warnings, since a source file
/// may legitimately hold unrelated `u32` constants.
fn cmd_verify_bindings(rust: &Path, catalog: &Path) {
    let text = match std::fs::read_to_string(rust) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", rust.display());
            std::process::exit(2);
        }
    };
    let source = match read_source(catalog) {
        Ok(s) => s,
        Err(_) => std::process::exit(2),
    };
    let catalog_ids = catalog_id_map(&source);
    let rust_ids = parse_rust_u32_consts(&text);

    let mut problems = 0usize;
    // Direction 1 (errors): every id constant referenced in code must exist in
    // the catalog with the same value. Candidate constants are those named
    // `IDS_*` (the project convention) or already present in the catalog, so
    // unrelated `u32` tuning constants are ignored.
    for (name, id) in &rust_ids {
        if !(name.starts_with("IDS_") || catalog_ids.contains_key(name)) {
            continue;
        }
        match catalog_ids.get(name) {
            Some(c) if c == id => {}
            Some(c) => {
                eprintln!("MISMATCH {name}: rust={id} catalog={c}");
                problems += 1;
            }
            None => {
                eprintln!("MISSING  {name} (rust id {id}) not found in catalog {}", catalog.display());
                problems += 1;
            }
        }
    }
    // Direction 2 (warnings): catalog keys not referenced from this file are
    // allowed (usage text may be unused or resolved elsewhere).
    for (name, id) in &catalog_ids {
        if !rust_ids.contains_key(name) {
            eprintln!(
                "UNUSED   {name} (catalog id {id}) has no matching constant in {} (warning)",
                rust.display()
            );
        }
    }

    if problems == 0 {
        println!(
            "OK  {} bindings cover {} ({} rust keys, {} catalog keys)",
            rust.display(),
            catalog.display(),
            rust_ids.len(),
            catalog_ids.len()
        );
    } else {
        eprintln!("FAILED: {problems} problem(s)");
        std::process::exit(1);
    }
}

/// `--require-ids <toml>` (#573): fail when a shipping catalog omits `[ids]`.
fn cmd_require_ids(input: &Path) {
    let source = match read_source(input) {
        Ok(s) => s,
        Err(_) => std::process::exit(2),
    };
    if let Err(e) = validate_source(&source) {
        eprintln!("FAILED {}: {e}", input.display());
        std::process::exit(1);
    }
    if source.ids.is_empty() {
        eprintln!(
            "FAILED {}: [ids] is required — entry ids are a stable ABI and must not be auto-assigned",
            input.display()
        );
        std::process::exit(1);
    }
    println!(
        "OK  {} has frozen [ids] ({} keys)",
        input.display(),
        source.ids.len()
    );
}

/// `--generate-keymap <locale-dir> [out.rs]` (#578): emit a sorted
/// `("<app>.<NAME>", id)` table for compile-time `tr!` resolution.
fn cmd_generate_keymap(locale_dir: &Path, output: Option<&Path>) {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(locale_dir) {
        Ok(it) => it
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("toml"))
            .collect(),
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", locale_dir.display());
            std::process::exit(2);
        }
    };
    files.sort();

    let mut entries: Vec<(String, u32)> = Vec::new();
    for path in files {
        let source = match read_source(&path) {
            Ok(s) => s,
            Err(_) => std::process::exit(2),
        };
        let app = source.meta.app.clone();
        for (name, id) in catalog_id_map(&source) {
            entries.push((format!("{app}.{name}"), id));
        }
    }
    entries.sort();
    entries.dedup();

    for w in entries.windows(2) {
        if w[0].0 == w[1].0 && w[0].1 != w[1].1 {
            eprintln!(
                "ERROR: key '{}' maps to conflicting ids {} and {}",
                w[0].0, w[0].1, w[1].1
            );
            std::process::exit(1);
        }
    }

    let mut code = String::new();
    code.push_str("// Auto-generated by nltc --generate-keymap. Do not edit.\n");
    code.push_str("// Symbolic string keys (\"<app>.<NAME>\") -> numeric i18n ids.\n\n");
    code.push_str("#![allow(dead_code)]\n\n");
    code.push_str("/// Sorted key -> id table (binary-searchable).\n");
    code.push_str("pub static NLT_KEYMAP: &[(&str, u32)] = &[\n");
    for (k, id) in &entries {
        code.push_str(&format!("    (\"{k}\", {id}),\n"));
    }
    code.push_str("];\n");

    match output {
        Some(p) => match std::fs::write(p, &code) {
            Ok(_) => eprintln!(
                "OK  keymap ({} keys) written to '{}'",
                entries.len(),
                p.display()
            ),
            Err(e) => {
                eprintln!("ERROR: cannot write '{}': {e}", p.display());
                std::process::exit(1);
            }
        },
        None => print!("{code}"),
    }
}

/// `--check-coverage <locale-dir> [base-lang]` (#572): every locale must define
/// the same keys (and ids) as the base locale for each app.
fn cmd_check_coverage(locale_dir: &Path, base_lang: &str) -> i32 {
    let mut locale_dirs: Vec<PathBuf> = match std::fs::read_dir(locale_dir) {
        Ok(it) => it
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect(),
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", locale_dir.display());
            return 2;
        }
    };
    locale_dirs.sort();

    // locale -> app -> (name -> id)
    let mut all: BTreeMap<String, BTreeMap<String, BTreeMap<String, u32>>> = BTreeMap::new();
    for dir in &locale_dirs {
        let lang = match dir.file_name().and_then(|n| n.to_str()) {
            Some(l) => l.to_string(),
            None => continue,
        };
        let mut apps = BTreeMap::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if path.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            match read_source(&path) {
                Ok(s) => {
                    apps.insert(s.meta.app.clone(), catalog_id_map(&s));
                }
                Err(_) => return 2,
            }
        }
        all.insert(lang, apps);
    }

    let base_name = if all.contains_key(base_lang) {
        base_lang.to_string()
    } else {
        match all.keys().next() {
            Some(k) => k.clone(),
            None => {
                eprintln!("ERROR: no locale directories under '{}'", locale_dir.display());
                return 2;
            }
        }
    };

    let mut issues = 0usize;
    let base = all.get(&base_name).cloned().unwrap_or_default();
    for (lang, apps) in &all {
        if *lang == base_name {
            continue;
        }
        for (app, base_ids) in &base {
            match apps.get(app) {
                None => {
                    eprintln!("MISSING APP  {lang}: '{app}.toml' not found (base '{base_name}')");
                    issues += 1;
                }
                Some(ids) => {
                    for (name, id) in base_ids {
                        match ids.get(name) {
                            Some(l) if l == id => {}
                            Some(l) => {
                                eprintln!("ID DRIFT     {lang}/{app}: {name} base={id} {lang}={l}");
                                issues += 1;
                            }
                            None => {
                                eprintln!("MISSING KEY  {lang}/{app}: {name} (id {id})");
                                issues += 1;
                            }
                        }
                    }
                    for name in ids.keys() {
                        if !base_ids.contains_key(name) {
                            eprintln!("EXTRA KEY    {lang}/{app}: {name} not in base");
                            issues += 1;
                        }
                    }
                }
            }
        }
    }

    if issues == 0 {
        println!(
            "OK  coverage: {} locales, base '{base_name}', {} apps",
            all.len(),
            base.len()
        );
        0
    } else {
        eprintln!("FAILED: {issues} coverage issue(s)");
        1
    }
}

// ── TOML scaffold ─────────────────────────────────────────────────────

fn generate_toml(app: &str, language: &str) -> String {
    format!(
        r#"[meta]
app = "{app}"
language = "{language}"

[ids]
# IDS_EXAMPLE = 1001

[strings]
# IDS_EXAMPLE = "Example string"

# [plural.IDS_FILES]
# one = "{{0}} file"
# other = "{{0}} files"

# [region]
# decimal_separator = "."
# thousands_separator = ","
# currency_symbol = "$"
# date_short = "MM/dd/yyyy"
# date_long = "MMMM d, yyyy"
# time_pattern = "hh:mm:ss A"
# time_24h = false
# currency_before = true
# group_thousands = true
"#
    )
}

// ── CLI ───────────────────────────────────────────────────────────────

fn print_usage() {
    eprintln!("Neo Language Table Compiler (nltc) v0.3 — NLTv3");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  nltc <input.toml> [output.nlt] [options]");
    eprintln!("  nltc --check <input.toml>");
    eprintln!("  nltc --generate-ids <input.toml>");
    eprintln!("  nltc --generate-rust <input.toml> [output.rs]");
    eprintln!("  nltc --scaffold <app> <language>");
    eprintln!("  nltc --list-langs");
    eprintln!("  nltc --lang-id <language-tag>");
    eprintln!("  nltc --app-id <app-name>");
    eprintln!("  nltc --info <file.nlt>");
    eprintln!("  nltc --verify <file.nlt> [hex-pubkey]");
    eprintln!("  nltc --verify-bindings <rust.rs> <catalog.toml>");
    eprintln!("  nltc --require-ids <input.toml>");
    eprintln!("  nltc --check-coverage <locale-dir> [base-lang]");
    eprintln!("  nltc --generate-keymap <locale-dir> [out.rs]");
    eprintln!("  nltc --generate-all <locale-dir>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --no-compress         Disable LZSS compression");
    eprintln!("  --utf16               Store the string blob as UTF-16LE");
    eprintln!("  --no-sign             Do not sign the output");
    eprintln!("  --sign-key <hex32>    Sign with a 32-byte hex seed");
    eprintln!("  --verbose             Print build statistics");
    eprintln!();
}

fn parse_seed(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let b = hex.as_bytes();
    for i in 0..32 {
        let hi = (b[i * 2] as char).to_digit(16)? as u8;
        let lo = (b[i * 2 + 1] as char).to_digit(16)? as u8;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn read_source(input: &Path) -> Result<NltSource, ()> {
    let bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", input.display());
            return Err(());
        }
    };
    // BOM detection (UTF-8 / UTF-16LE source files).
    let text = match libnlt::utf16::detect_bom(&bytes) {
        Some("utf-8") => String::from_utf8_lossy(&bytes[3..]).into_owned(),
        Some("utf-16le") => {
            let mut out = Vec::new();
            if libnlt::utf16::utf16le_bytes_to_utf8(&bytes[2..], &mut out).is_none() {
                eprintln!("ERROR: invalid UTF-16LE source '{}'", input.display());
                return Err(());
            }
            String::from_utf8_lossy(&out).into_owned()
        }
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    };
    match toml::from_str(&text) {
        Ok(s) => Ok(s),
        Err(e) => {
            eprintln!("ERROR: TOML parse error in '{}': {e}", input.display());
            Err(())
        }
    }
}

fn options_from_args(args: &[String], start: usize) -> BuildOptions {
    let mut opts = BuildOptions::default();
    let mut i = start;
    while i < args.len() {
        match args[i].as_str() {
            "--no-compress" => opts.compress = false,
            "--utf16" => opts.utf16 = true,
            "--no-sign" => opts.sign = false,
            "--sign-key" => {
                if let Some(hex) = args.get(i + 1) {
                    match parse_seed(hex) {
                        Some(seed) => opts.sign_seed = seed,
                        None => {
                            eprintln!("ERROR: --sign-key expects 64 hex characters");
                            std::process::exit(1);
                        }
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    opts
}

fn cmd_compile(input: &Path, output: &Path, opts: &BuildOptions) {
    let source = match read_source(input) {
        Ok(s) => s,
        Err(_) => std::process::exit(1),
    };
    if let Err(e) = validate_source(&source) {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
    match build_nlt(&source, opts, true) {
        Ok(nlt) => match std::fs::write(output, &nlt) {
            Ok(_) => eprintln!("OK  {} -> {} ({} bytes)", input.display(), output.display(), nlt.len()),
            Err(e) => {
                eprintln!("ERROR: cannot write '{}': {e}", output.display());
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("ERROR: compilation failed: {e}");
            std::process::exit(1);
        }
    }
}

fn validate_source(source: &NltSource) -> Result<(), String> {
    if source.meta.app.is_empty() {
        return Err("[meta] app field is required".into());
    }
    if source.meta.language.is_empty() {
        return Err("[meta] language field is required".into());
    }
    compile(source).map(|_| ())
}

fn cmd_check(input: &Path) {
    match read_source(input) {
        Ok(source) => match validate_source(&source) {
            Ok(_) => eprintln!(
                "OK  {} — valid ({} strings, {} plural groups)",
                input.display(),
                source.strings.len(),
                source.plural.len()
            ),
            Err(e) => {
                eprintln!("FAILED {}: {e}", input.display());
                std::process::exit(1);
            }
        },
        Err(_) => std::process::exit(1),
    }
}

fn cmd_generate_ids(input: &Path) {
    let text = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", input.display());
            std::process::exit(1);
        }
    };
    let mut value: toml::Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ERROR: TOML parse error: {e}");
            std::process::exit(1);
        }
    };
    if value.get("ids").is_some() {
        eprintln!("OK  [ids] section already exists — no changes needed");
        return;
    }
    let mut names: Vec<String> = Vec::new();
    if let Some(t) = value.get("strings").and_then(|s| s.as_table()) {
        names.extend(t.keys().cloned());
    }
    if let Some(t) = value.get("plural").and_then(|s| s.as_table()) {
        names.extend(t.keys().cloned());
    }
    names.sort();
    let mut ids = toml::value::Table::new();
    let mut next: i64 = 1001;
    for name in names {
        ids.insert(name, toml::Value::Integer(next));
        next += 1;
    }
    value
        .as_table_mut()
        .unwrap()
        .insert("ids".to_string(), toml::Value::Table(ids));
    match std::fs::write(input, value.to_string()) {
        Ok(_) => eprintln!("OK  IDs written to '{}'", input.display()),
        Err(e) => {
            eprintln!("ERROR: cannot write '{}': {e}", input.display());
            std::process::exit(1);
        }
    }
}

fn cmd_generate_rust(input: &Path, output: Option<&Path>) {
    let source = match read_source(input) {
        Ok(s) => s,
        Err(_) => std::process::exit(1),
    };
    match generate_rust_constants(&source) {
        Ok(code) => {
            if let Some(p) = output {
                match std::fs::write(p, &code) {
                    Ok(_) => eprintln!("OK  Rust constants written to '{}'", p.display()),
                    Err(e) => {
                        eprintln!("ERROR: cannot write '{}': {e}", p.display());
                        std::process::exit(1);
                    }
                }
            } else {
                print!("{code}");
            }
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_list_langs() {
    eprintln!("Known language IDs:");
    for id in 1..=25 {
        eprintln!("  {:4}  {:<7}  {}", id, id_to_lang(id), lang_name(id));
    }
}

fn cmd_lang_id(tag: &str) {
    let id = lang_to_id(tag);
    println!("{id}");
    if id >= 0x8000 {
        eprintln!("WARNING: '{tag}' is not a standard tag (hash ID: {id:#x})");
    } else {
        eprintln!("INFO: '{tag}' = {id} ({})", id_to_lang(id));
    }
}

fn cmd_app_id(name: &str) {
    let id = app_to_id(name);
    println!("{id}");
    if id >= 0x8000 {
        eprintln!("WARNING: '{name}' is not a known app (hash ID: {id:#x})");
    }
}

fn cmd_info(path: &Path) {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", path.display());
            std::process::exit(1);
        }
    };
    let header = match libnlt::parse_header(&data) {
        Some(h) => h,
        None => {
            eprintln!("ERROR: not a valid NLT file (v2/v3)");
            std::process::exit(1);
        }
    };

    println!("=== NLT File Info ===");
    println!("  File:       {}", path.display());
    println!("  Size:       {} bytes", data.len());
    println!("  Format:     NLTv{}", header.version);
    println!(
        "  Language:   {} (ID={})",
        id_to_lang(header.language_id),
        header.language_id
    );
    println!("  AppID:      {}", header.application_id);
    println!("  Entries:    {}", header.entry_count);
    println!("  Flags:      0x{:08X}", header.flags);
    println!("    compressed={} utf16={} rtl={} signed={} region={} plural={}",
        header.is_compressed(),
        header.is_utf16(),
        header.is_rtl(),
        header.is_signed(),
        header.has_region(),
        header.flags & libnlt::FLAG_PLURAL != 0,
    );
    println!("  Payload:    {} bytes (stored {})", header.payload_size, header.payload_stored);
    println!("  Region:     {} bytes", header.region_size);
    println!("  Signature:  {} bytes", header.signature_size);

    // Decode and show a few entries.
    let mut scratch = vec![0u8; header.payload_size as usize + 16];
    let mut out = vec![0u8; header.payload_size as usize + header.region_size as usize + 16];
    if let Some(decoded) = libnlt::decode(&data, &mut scratch, &mut out) {
        println!();
        let show = (header.entry_count as usize).min(16);
        println!("  Entries (first {show}):");
        for i in 0..show {
            if let Some((id, off, eflags)) =
                libnlt::read_index(decoded.payload, header.version, i)
            {
                if eflags & ENTRY_FLAG_PLURAL != 0 {
                    println!("    {id:6}: <plural group>");
                } else if let Some(s) = libnlt::str_at(decoded.payload, off) {
                    println!("    {id:6}: {s}");
                }
            }
        }
    }
}

fn cmd_verify(path: &Path, key_hex: Option<&str>) {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("ERROR: cannot read '{}': {e}", path.display());
            std::process::exit(1);
        }
    };
    let header = match libnlt::parse_header(&data) {
        Some(h) => h,
        None => {
            eprintln!("FAILED: not a valid NLT file");
            std::process::exit(1);
        }
    };
    if !header.is_signed() || header.signature_size != 64 {
        eprintln!("FAILED: file is not signed");
        std::process::exit(1);
    }
    let pubkey = match key_hex {
        Some(h) => match parse_seed(h) {
            Some(k) => k,
            None => {
                eprintln!("ERROR: key must be 64 hex characters");
                std::process::exit(1);
            }
        },
        None => libnlt::signature::public_from_seed(&libnlt::signature::DEV_SEED).unwrap(),
    };
    let sig_start = header.signature_offset as usize;
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&data[sig_start..sig_start + 64]);
    // The compiler signs everything before the signature block.
    if libnlt::signature::verify(&pubkey, &data[..sig_start], &sig) {
        println!("OK  signature valid");
    } else {
        eprintln!("FAILED: signature invalid");
        std::process::exit(1);
    }
}

fn cmd_generate_all(locale_dir: &Path, opts: &BuildOptions) {
    if !locale_dir.exists() {
        eprintln!("ERROR: directory not found: '{}'", locale_dir.display());
        std::process::exit(1);
    }
    let mut compiled = 0u32;
    let mut failed = 0u32;
    for entry in std::fs::read_dir(locale_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let mut output = path.clone();
        output.set_extension("nlt");
        eprintln!("Compiling: {} ...", path.display());
        let source = match read_source(&path) {
            Ok(s) => s,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        match build_nlt(&source, opts, false) {
            Ok(nlt) => match std::fs::write(&output, &nlt) {
                Ok(_) => {
                    eprintln!("  -> {} ({} bytes)", output.display(), nlt.len());
                    compiled += 1;
                }
                Err(e) => {
                    eprintln!("  ERROR: write failed: {e}");
                    failed += 1;
                }
            },
            Err(e) => {
                eprintln!("  ERROR: {e}");
                failed += 1;
            }
        }
    }
    eprintln!("\nResult: {compiled} compiled, {failed} failed");
    if failed > 0 {
        std::process::exit(1);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "--check" => cmd_check(Path::new(&args[2])),
        "--generate-ids" => cmd_generate_ids(Path::new(&args[2])),
        "--generate-rust" => {
            let output = args.get(3).map(Path::new);
            cmd_generate_rust(Path::new(&args[2]), output);
        }
        "--scaffold" => {
            if args.len() < 4 {
                eprintln!("Usage: nltc --scaffold <app> <language>");
                std::process::exit(1);
            }
            println!("{}", generate_toml(&args[2], &args[3]));
        }
        "--list-langs" => cmd_list_langs(),
        "--lang-id" => cmd_lang_id(&args[2]),
        "--app-id" => cmd_app_id(&args[2]),
        "--info" => cmd_info(Path::new(&args[2])),
        "--verify" => cmd_verify(Path::new(&args[2]), args.get(3).map(|s| s.as_str())),
        "--verify-bindings" => {
            if args.len() < 4 {
                eprintln!("Usage: nltc --verify-bindings <rust.rs> <catalog.toml>");
                std::process::exit(1);
            }
            cmd_verify_bindings(Path::new(&args[2]), Path::new(&args[3]));
        }
        "--require-ids" => cmd_require_ids(Path::new(&args[2])),
        "--check-coverage" => {
            let base = args.get(3).map(|s| s.as_str()).unwrap_or("en-US");
            std::process::exit(cmd_check_coverage(Path::new(&args[2]), base));
        }
        "--generate-keymap" => {
            let output = args.get(3).map(Path::new);
            cmd_generate_keymap(Path::new(&args[2]), output);
        }
        "--generate-all" => {
            let opts = options_from_args(&args, 3);
            cmd_generate_all(Path::new(&args[2]), &opts);
        }
        _ => {
            let input = Path::new(&args[1]);
            if !input.exists() {
                eprintln!("ERROR: file not found: '{}'", input.display());
                std::process::exit(1);
            }
            // Optional explicit output before flags.
            let (output, opts_start) = if args.len() >= 3 && !args[2].starts_with("--") {
                (PathBuf::from(&args[2]), 3)
            } else {
                let mut o = input.to_path_buf();
                o.set_extension("nlt");
                (o, 2)
            };
            let opts = options_from_args(&args, opts_start);
            cmd_compile(input, &output, &opts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pub_and_private_u32_consts() {
        let src = "const IDS_A: u32 = 1001;\n\
                   pub const IDS_B: u32 = 1_002;\n\
                   pub const NAME: &str = \"x\";\n\
                   const OTHER: u64 = 5;\n";
        let m = parse_rust_u32_consts(src);
        assert_eq!(m.get("IDS_A"), Some(&1001));
        assert_eq!(m.get("IDS_B"), Some(&1002));
        assert!(!m.contains_key("NAME"));
        assert!(!m.contains_key("OTHER"));
    }

    #[test]
    fn catalog_id_map_falls_back_to_sorted_assignment() {
        let src = NltSource {
            meta: NltMeta {
                app: "x".into(),
                language: "en-US".into(),
            },
            ids: BTreeMap::new(),
            strings: [
                ("B".to_string(), "b".to_string()),
                ("A".to_string(), "a".to_string()),
            ]
            .into_iter()
            .collect(),
            plural: BTreeMap::new(),
            region: None,
        };
        let m = catalog_id_map(&src);
        assert_eq!(m.get("A"), Some(&1001));
        assert_eq!(m.get("B"), Some(&1002));
    }

    #[test]
    fn explicit_ids_win_over_auto_assignment() {
        let src = NltSource {
            meta: NltMeta {
                app: "x".into(),
                language: "en-US".into(),
            },
            ids: [("A".to_string(), 7u32)].into_iter().collect(),
            strings: [("A".to_string(), "a".to_string())].into_iter().collect(),
            plural: BTreeMap::new(),
            region: None,
        };
        assert_eq!(catalog_id_map(&src).get("A"), Some(&7));
    }
}
