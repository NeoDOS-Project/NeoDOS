//! xkbcompile — convert an XKB layout (`/usr/share/X11/xkb/symbols/*`) into
//! the NeoDOS `.kbd` format.
//!
//! Usage:
//!   xkbcompile <layout> [variant] [--name TAG] [--lang TAG] [--out FILE]
//!              [--symbols-dir DIR] [--keycodes FILE] [--compose FILE]
//!
//! Scope: single group, levels 1-3 (normal / shift / AltGr). It resolves
//! `include "file(section)"` chains, maps EVDEV keycodes to PC set-1
//! scancodes, translates X11 keysyms (Latin-1 / `U<hex>` / the common names),
//! marks dead keys and, when a Compose file is given, builds the composition
//! table for the dead keysyms used.
//!
//! Known limits: only the first keyboard group; levels 4+ (Shift+AltGr) are
//! dropped; exotic keysyms (Greek, Cyrillic, ligatures) are not translated.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const KBD_MAGIC: &[u8; 4] = b"KBD\0";

#[repr(C)]
#[derive(Clone, Copy)]
struct KeyEntry {
    normal: u16,
    shift: u16,
    altgr: u16,
    ctrl: u16,
    flags: u8,
    _pad: [u8; 7],
}

impl KeyEntry {
    fn empty() -> Self {
        KeyEntry { normal: 0xFFFF, shift: 0xFFFF, altgr: 0xFFFF, ctrl: 0xFFFF, flags: 0, _pad: [0u8; 7] }
    }
}

struct Compose {
    dead: u16,
    base: u16,
    result: u16,
}

struct Args {
    layout: String,
    variant: Option<String>,
    name: String,
    lang: String,
    out: PathBuf,
    symbols_dir: PathBuf,
    keycodes: PathBuf,
    compose: PathBuf,
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("xkbcompile: {msg}");
            eprintln!(
                "usage: xkbcompile <layout> [variant] [--name TAG] [--lang TAG] [--out FILE] \
                 [--symbols-dir DIR] [--keycodes FILE] [--compose FILE]"
            );
            std::process::exit(2);
        }
    };

    let mut keymap: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut visited: HashSet<(String, String)> = HashSet::new();
    if let Err(e) = resolve(&args.symbols_dir, &args.layout, args.variant.as_deref(), &mut keymap, &mut visited) {
        eprintln!("xkbcompile: {e}");
        std::process::exit(1);
    }

    let keycodes = match parse_keycodes(&args.keycodes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("xkbcompile: {}", e);
            std::process::exit(1);
        }
    };

    let mut entries = vec![KeyEntry::empty(); 256];
    let mut dead_labels: HashSet<u16> = HashSet::new();

    for (kname, syms) in &keymap {
        let ev = match keycodes.get(kname) {
            Some(v) => *v,
            None => continue,
        };
        // PC set-1 scancode = EVDEV keycode - 8 for the alphanumeric block.
        if ev < 9 {
            continue;
        }
        let set1 = ev - 8;
        if set1 == 0 || set1 > 0x58 {
            continue;
        }
        let mut e = KeyEntry::empty();
        for (level, sym) in syms.iter().enumerate() {
            if sym.is_empty() || sym == "NoSymbol" || sym == "VoidSymbol" {
                continue;
            }
            let (cp, dead) = match symbol_to_char(sym) {
                Some(v) => v,
                None => continue,
            };
            if dead {
                dead_labels.insert(cp);
            }
            match level {
                0 => { e.normal = cp; if dead { e.flags |= 0x01; } }
                1 => { e.shift = cp; if dead { e.flags |= 0x02; } }
                2 => { e.altgr = cp; if dead { e.flags |= 0x04; } }
                _ => {} // level 4+ (Shift+AltGr): no kernel column
            }
        }
        entries[set1 as usize] = e;
    }

    let compose = if args.compose.exists() {
        parse_compose(&args.compose, &dead_labels)
    } else {
        Vec::new()
    };

    let bytes = write_kbd(&args.name, &args.lang, &entries, &compose);
    if let Err(e) = fs::write(&args.out, &bytes) {
        eprintln!("xkbcompile: cannot write {}: {e}", args.out.display());
        std::process::exit(1);
    }
    let mapped = entries.iter().filter(|e| e.normal != 0xFFFF).count();
    eprintln!(
        "xkbcompile: wrote {} ({} keys, {} compose) [{}]",
        args.out.display(), mapped, compose.len(), args.name
    );
}

fn parse_args() -> Result<Args, String> {
    let mut pos: Vec<String> = Vec::new();
    let mut name = None;
    let mut lang = None;
    let mut out = None;
    let mut symbols_dir = PathBuf::from("/usr/share/X11/xkb/symbols");
    let mut keycodes = PathBuf::from("/usr/share/X11/xkb/keycodes/evdev");
    let mut compose = PathBuf::from("/usr/share/X11/locale/en_US.UTF-8/Compose");

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--name" => name = it.next(),
            "--lang" => lang = it.next(),
            "--out" => out = it.next().map(PathBuf::from),
            "--symbols-dir" => symbols_dir = it.next().map(PathBuf::from).ok_or("--symbols-dir needs a value")?,
            "--keycodes" => keycodes = it.next().map(PathBuf::from).ok_or("--keycodes needs a value")?,
            "--compose" => compose = it.next().map(PathBuf::from).ok_or("--compose needs a value")?,
            "-h" | "--help" => return Err("help".into()),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => pos.push(other.to_string()),
        }
    }

    if pos.is_empty() {
        return Err("missing <layout>".into());
    }
    let layout = pos[0].clone();
    let variant = pos.get(1).cloned();
    let tag = variant.as_ref().map(|v| format!("{layout}-{v}")).unwrap_or_else(|| layout.clone());
    let name = name.unwrap_or_else(|| tag.clone());
    let lang = lang.unwrap_or(tag);
    let out = out.unwrap_or_else(|| PathBuf::from(format!("data/keyboard/{name}.kbd")));

    Ok(Args { layout, variant, name, lang, out, symbols_dir, keycodes, compose })
}

// ── XKB symbols parsing ─────────────────────────────────────────────

/// Resolve a `xkb_symbols` section, expanding `include` chains in order so
/// that later `key` definitions override earlier (included) ones.
fn resolve(
    dir: &Path,
    file: &str,
    section: Option<&str>,
    keymap: &mut BTreeMap<String, Vec<String>>,
    visited: &mut HashSet<(String, String)>,
) -> Result<(), String> {
    let key = (file.to_string(), section.unwrap_or_default().to_string());
    if !visited.insert(key) {
        return Ok(()); // cycle guard
    }

    let path = dir.join(file);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let body = find_section(&text, section).ok_or_else(|| {
        format!("{}: section '{}' not found", path.display(), section.unwrap_or("<default>"))
    })?;

    for stmt in split_statements(&body) {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        if let Some(rest) = s.strip_prefix("include") {
            if let Some(spec) = quoted(rest) {
                let (f, sec) = parse_include(&spec);
                resolve(dir, &f, sec.as_deref(), keymap, visited)?;
            }
        } else if s.starts_with("key") {
            if let Some((kname, syms)) = parse_key(s) {
                keymap.insert(kname, syms);
            }
        }
    }
    Ok(())
}

/// Return the body of `xkb_symbols "name" { ... }`. With `None`, the default
/// (or first) section is used.
fn find_section(text: &str, name: Option<&str>) -> Option<String> {
    let mut idx = 0usize;
    let mut first: Option<String> = None;
    let mut default: Option<String> = None;

    while let Some(rel) = text[idx..].find("xkb_symbols") {
        let start = idx + rel;
        let after = &text[start + "xkb_symbols".len()..];
        let sname = quoted(after).unwrap_or_default();
        // `default` attribute lives just before `xkb_symbols`.
        let prefix_start = text[..start].rfind('}').map(|p| p + 1).unwrap_or(0);
        let is_default = text[prefix_start..start].contains("default");

        let brace = match after.find('{') {
            Some(b) => b,
            None => break,
        };
        let body_start = start + "xkb_symbols".len() + brace + 1;
        let body = match balanced_braces(&text[body_start..]) {
            Some(b) => b,
            None => break,
        };

        if let Some(n) = name {
            if sname == n {
                return Some(body);
            }
        } else {
            if first.is_none() {
                first = Some(body.clone());
            }
            if is_default && default.is_none() {
                default = Some(body.clone());
            }
        }
        idx = body_start + body.len() + 1;
    }

    if name.is_none() {
        default.or(first)
    } else {
        None
    }
}

/// Split a section body into `;`-terminated statements (respecting quotes and
/// brackets/braces).
fn split_statements(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut depth = 0i32;
    for c in body.chars() {
        match c {
            '"' => { in_quote = !in_quote; cur.push(c); }
            '[' | '{' => { depth += 1; cur.push(c); }
            ']' | '}' => { depth -= 1; cur.push(c); }
            ';' if !in_quote && depth <= 0 => { out.push(std::mem::take(&mut cur)); }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

fn parse_key(stmt: &str) -> Option<(String, Vec<String>)> {
    let lt = stmt.find('<')?;
    let gt = stmt[lt..].find('>')? + lt;
    let name = stmt[lt + 1..gt].trim().to_string();

    // Find the keysym list: the first `[` group not preceded by `type`/`symbols`.
    let bytes = stmt.as_bytes();
    let mut i = 0;
    while let Some(rel) = stmt[i..].find('[') {
        let p = i + rel;
        // preceding word
        let mut w = p;
        while w > 0 && (bytes[w - 1].is_ascii_alphanumeric() || bytes[w - 1] == b'_') {
            w -= 1;
        }
        let word = &stmt[w..p];
        if word == "type" || word == "symbols" {
            i = p + 1;
            continue;
        }
        let close = stmt[p..].find(']')? + p;
        let inner = &stmt[p + 1..close];
        let syms: Vec<String> = inner
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        return Some((name, syms));
    }
    Some((name, Vec::new()))
}

/// Parse the first quoted string in `s`.
fn quoted(s: &str) -> Option<String> {
    let start = s.find('"')? + 1;
    let end = s[start..].find('"')? + start;
    Some(s[start..end].to_string())
}

/// Parse `file` or `file(section)` into its parts.
fn parse_include(spec: &str) -> (String, Option<String>) {
    match spec.find('(') {
        Some(open) => {
            let file = spec[..open].trim().to_string();
            let section = spec[open + 1..].trim_end_matches(')').trim().to_string();
            (file, Some(section))
        }
        None => (spec.trim().to_string(), None),
    }
}

/// Body of a `{ ... }` block (the input must start right after the `{`).
fn balanced_braces(s: &str) -> Option<String> {
    let mut depth = 1i32;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[..i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

// ── keycodes ────────────────────────────────────────────────────────

fn parse_keycodes(path: &Path) -> Result<BTreeMap<String, u16>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("//") || line.starts_with("alias") {
            continue;
        }
        // <NAME> = 24;
        if let Some(lt) = line.find('<') {
            if let Some(gt) = line[lt..].find('>') {
                let name = line[lt + 1..lt + gt].trim().to_string();
                if let Some(eq) = line.find('=') {
                    let num: String = line[eq + 1..]
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if let Ok(v) = num.parse::<u16>() {
                        map.insert(name, v);
                    }
                }
            }
        }
    }
    Ok(map)
}

// ── keysyms ─────────────────────────────────────────────────────────

/// Map an XKB keysym name to `(codepoint, is_dead)`.
fn symbol_to_char(name: &str) -> Option<(u16, bool)> {
    let name = name.trim();
    if name.is_empty() || name == "NoSymbol" || name == "VoidSymbol" {
        return None;
    }
    if let Some(rest) = name.strip_prefix("dead_") {
        return dead_label(rest).map(|cp| (cp, true));
    }
    // Unicode keysym `U<hex>` (4-6 hex digits).
    if let Some(hex) = name.strip_prefix('U') {
        if hex.len() >= 4 && hex.len() <= 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                if let Ok(cp) = u16::try_from(v) {
                    return Some((cp, false));
                }
            }
        }
    }
    if name.chars().count() == 1 {
        let c = name.chars().next().unwrap();
        if (c as u32) < 0x80 {
            return Some((c as u16, false));
        }
    }
    named_keysym(name).map(|cp| (cp, false))
}

/// Spacing codepoint used as the compose key for each dead keysym (matches the
/// convention of the `.klc`-derived layouts).
fn dead_label(rest: &str) -> Option<u16> {
    Some(match rest {
        "grave" => 0x0060,
        "acute" => 0x00B4,
        "circumflex" => 0x005E,
        "tilde" => 0x007E,
        "diaeresis" => 0x00A8,
        "macron" => 0x00AF,
        "breve" => 0x02D8,
        "abovedot" => 0x02D9,
        "abovering" => 0x02DA,
        "doubleacute" => 0x02DD,
        "caron" => 0x02C7,
        "cedilla" => 0x00B8,
        "ogonek" => 0x02DB,
        "belowdot" => 0x0323,
        "hook" => 0x0309,
        "horn" => 0x031B,
        "stroke" => 0x0338,
        "belowcomma" => 0x0326,
        "iota" => 0x0345,
        "voiced_sound" => 0x3099,
        "semivoiced_sound" => 0x309A,
        _ => return None,
    })
}

fn named_keysym(name: &str) -> Option<u16> {
    Some(match name {
        "space" => 0x20,
        "exclam" => 0x21,
        "quotedbl" => 0x22,
        "numbersign" => 0x23,
        "dollar" => 0x24,
        "percent" => 0x25,
        "ampersand" => 0x26,
        "apostrophe" | "quoteright" => 0x27,
        "parenleft" => 0x28,
        "parenright" => 0x29,
        "asterisk" => 0x2A,
        "plus" => 0x2B,
        "comma" => 0x2C,
        "minus" => 0x2D,
        "period" => 0x2E,
        "slash" => 0x2F,
        "0" => 0x30,
        "1" => 0x31,
        "2" => 0x32,
        "3" => 0x33,
        "4" => 0x34,
        "5" => 0x35,
        "6" => 0x36,
        "7" => 0x37,
        "8" => 0x38,
        "9" => 0x39,
        "colon" => 0x3A,
        "semicolon" => 0x3B,
        "less" => 0x3C,
        "equal" => 0x3D,
        "greater" => 0x3E,
        "question" => 0x3F,
        "at" => 0x40,
        "bracketleft" => 0x5B,
        "backslash" => 0x5C,
        "bracketright" => 0x5D,
        "asciicircum" => 0x5E,
        "underscore" => 0x5F,
        "grave" | "quoteleft" => 0x60,
        "braceleft" => 0x7B,
        "bar" => 0x7C,
        "braceright" => 0x7D,
        "asciitilde" => 0x7E,
        // Latin-1 supplement
        "nobreakspace" => 0xA0,
        "exclamdown" => 0xA1,
        "cent" => 0xA2,
        "sterling" => 0xA3,
        "currency" => 0xA4,
        "yen" => 0xA5,
        "brokenbar" => 0xA6,
        "section" => 0xA7,
        "diaeresis" => 0xA8,
        "copyright" => 0xA9,
        "ordfeminine" => 0xAA,
        "guillemotleft" => 0xAB,
        "notsign" => 0xAC,
        "hyphen" => 0xAD,
        "registered" => 0xAE,
        "macron" => 0xAF,
        "degree" => 0xB0,
        "plusminus" => 0xB1,
        "twosuperior" => 0xB2,
        "threesuperior" => 0xB3,
        "acute" => 0xB4,
        "mu" => 0xB5,
        "paragraph" => 0xB6,
        "periodcentered" => 0xB7,
        "cedilla" => 0xB8,
        "onesuperior" => 0xB9,
        "masculine" => 0xBA,
        "guillemotright" => 0xBB,
        "onequarter" => 0xBC,
        "onehalf" => 0xBD,
        "threequarters" => 0xBE,
        "questiondown" => 0xBF,
        "Agrave" => 0xC0,
        "Aacute" => 0xC1,
        "Acircumflex" => 0xC2,
        "Atilde" => 0xC3,
        "Adiaeresis" => 0xC4,
        "Aring" => 0xC5,
        "AE" => 0xC6,
        "Ccedilla" => 0xC7,
        "Egrave" => 0xC8,
        "Eacute" => 0xC9,
        "Ecircumflex" => 0xCA,
        "Ediaeresis" => 0xCB,
        "Igrave" => 0xCC,
        "Iacute" => 0xCD,
        "Icircumflex" => 0xCE,
        "Idiaeresis" => 0xCF,
        "ETH" => 0xD0,
        "Ntilde" => 0xD1,
        "Ograve" => 0xD2,
        "Oacute" => 0xD3,
        "Ocircumflex" => 0xD4,
        "Otilde" => 0xD5,
        "Odiaeresis" => 0xD6,
        "multiply" => 0xD7,
        "Oslash" => 0xD8,
        "Ugrave" => 0xD9,
        "Uacute" => 0xDA,
        "Ucircumflex" => 0xDB,
        "Udiaeresis" => 0xDC,
        "Yacute" => 0xDD,
        "THORN" => 0xDE,
        "ssharp" => 0xDF,
        "agrave" => 0xE0,
        "aacute" => 0xE1,
        "acircumflex" => 0xE2,
        "atilde" => 0xE3,
        "adiaeresis" => 0xE4,
        "aring" => 0xE5,
        "ae" => 0xE6,
        "ccedilla" => 0xE7,
        "egrave" => 0xE8,
        "eacute" => 0xE9,
        "ecircumflex" => 0xEA,
        "ediaeresis" => 0xEB,
        "igrave" => 0xEC,
        "iacute" => 0xED,
        "icircumflex" => 0xEE,
        "idiaeresis" => 0xEF,
        "eth" => 0xF0,
        "ntilde" => 0xF1,
        "ograve" => 0xF2,
        "oacute" => 0xF3,
        "ocircumflex" => 0xF4,
        "otilde" => 0xF5,
        "odiaeresis" => 0xF6,
        "division" => 0xF7,
        "oslash" => 0xF8,
        "ugrave" => 0xF9,
        "uacute" => 0xFA,
        "ucircumflex" => 0xFB,
        "udiaeresis" => 0xFC,
        "yacute" => 0xFD,
        "thorn" => 0xFE,
        "ydiaeresis" => 0xFF,
        // Common extras
        "EuroSign" => 0x20AC,
        "oe" => 0x0153,
        "OE" => 0x0152,
        "leftsinglequotemark" => 0x2018,
        "rightsinglequotemark" => 0x2019,
        "leftdoublequotemark" => 0x201C,
        "rightdoublequotemark" => 0x201D,
        "endash" => 0x2013,
        "emdash" => 0x2014,
        "ellipsis" => 0x2026,
        "trademark" => 0x2122,
        "bullet" => 0x2022,
        _ => return None,
    })
}

// ── Compose ─────────────────────────────────────────────────────────

fn parse_compose(path: &Path, dead_labels: &HashSet<u16>) -> Vec<Compose> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("include") {
            continue;
        }
        let colon = match line.find(':') {
            Some(c) => c,
            None => continue,
        };
        let (keys_part, rest) = line.split_at(colon);
        let keys: Vec<&str> = keys_part
            .split('<')
            .skip(1)
            .filter_map(|s| s.split('>').next())
            .collect();
        if keys.len() != 2 {
            continue; // only two-key sequences
        }
        let (d_cp, d_dead) = match symbol_to_char(keys[0]) {
            Some(v) => v,
            None => continue,
        };
        if !d_dead || !dead_labels.contains(&d_cp) {
            continue;
        }
        let base = match symbol_to_char(keys[1]) {
            Some((cp, false)) => cp,
            _ => continue,
        };
        let result = match compose_result(rest) {
            Some(cp) => cp,
            None => continue,
        };
        out.push(Compose { dead: d_cp, base, result });
    }
    out
}

/// First Unicode codepoint of the `"..."` string after the `:`.
fn compose_result(rest: &str) -> Option<u16> {
    let q = rest.find('"')? + 1;
    let end = rest[q..].find('"')? + q;
    rest[q..end].chars().next().map(|c| c as u16)
}

// ── .kbd writer ─────────────────────────────────────────────────────

fn write_kbd(name: &str, lang: &str, entries: &[KeyEntry], compose: &[Compose]) -> Vec<u8> {
    let mut out = Vec::with_capacity(60 + 256 * 16 + 4 + compose.len() * 6);
    out.extend_from_slice(KBD_MAGIC);
    out.extend_from_slice(&1u32.to_le_bytes());

    let mut name_bytes = [0u8; 32];
    let nlen = name.len().min(31);
    name_bytes[..nlen].copy_from_slice(&name.as_bytes()[..nlen]);
    out.extend_from_slice(&name_bytes);

    let mut tag_bytes = [0u8; 16];
    let tlen = lang.len().min(15);
    tag_bytes[..tlen].copy_from_slice(&lang.as_bytes()[..tlen]);
    out.extend_from_slice(&tag_bytes);

    out.extend_from_slice(&256u32.to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.normal.to_le_bytes());
        out.extend_from_slice(&e.shift.to_le_bytes());
        out.extend_from_slice(&e.altgr.to_le_bytes());
        out.extend_from_slice(&e.ctrl.to_le_bytes());
        out.push(e.flags);
        out.extend_from_slice(&e._pad);
    }
    out.extend_from_slice(&(compose.len() as u32).to_le_bytes());
    for c in compose {
        out.extend_from_slice(&c.dead.to_le_bytes());
        out.extend_from_slice(&c.base.to_le_bytes());
        out.extend_from_slice(&c.result.to_le_bytes());
    }
    out
}
