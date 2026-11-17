//! kbdcompile — convert a Microsoft KLC layout into the kernel `.kbd` format.
//!
//! Usage: `kbdcompile <input.klc> <output.kbd> [name] [langtag]`
//!
//! KLC sections handled:
//! - `SHIFTSTATE`: ordered list of shift states (e.g. `0 1 2 6 3`).
//! - `LAYOUT`: `SC VK Cap <state0> <state1> ...` rows. Values are either a
//!   single literal character (`q`, `1`) or a 4-hex-digit Unicode codepoint
//!   (`00f1`); `-1` means "no mapping"; a trailing `@` marks a dead key.
//! - `DEADKEY <hex>`: `base hex -> composed hex` rows.
//! - `COMPOSE`: `dead base result` hex rows.
//!
//! Shift-state columns map onto the kernel `KeyEntry` fields:
//! `0 -> normal`, `1 -> shift`, `2 -> ctrl`, `6 -> altgr`. Dead-key flags use
//! bit0 (normal), bit1 (shift) and bit2 (altgr), matching `layout::is_dead_key`.

use std::fs;

const KBD_MAGIC: &[u8; 4] = b"KBD\0";

#[repr(C)]
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

struct ComposeEntry {
    dead: u16,
    base: u16,
    result: u16,
}

/// Parse a KLC layout cell. Returns `(codepoint, is_dead)` or `None` for `-1`.
fn parse_value(tok: &str) -> Option<(u16, bool)> {
    let mut s = tok.trim();
    let mut dead = false;
    if let Some(stripped) = s.strip_suffix('@') {
        dead = true;
        s = stripped;
    }
    if s.is_empty() || s == "-1" {
        return None;
    }
    // Single literal character (`q`, `Q`, `1`, `%`).
    if s.chars().count() == 1 {
        let c = s.chars().next().unwrap();
        return Some((c as u16, dead));
    }
    // 4-hex-digit Unicode codepoint.
    if let Ok(v) = u16::from_str_radix(s, 16) {
        return Some((v, dead));
    }
    // Fallback: first ASCII character.
    s.chars().next().filter(char::is_ascii).map(|c| (c as u16, dead))
}

fn hex_u16(tok: &str) -> Option<u16> {
    u16::from_str_radix(tok.trim().trim_start_matches("0x"), 16).ok()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: kbdcompile <input.klc> <output.kbd> [name] [langtag]");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];
    let name = if args.len() > 3 { args[3].clone() } else { "Unknown".to_string() };
    let lang_tag = if args.len() > 4 { args[4].clone() } else { "xx-XX".to_string() };

    let content = fs::read_to_string(input_path).unwrap_or_default();
    // Strip a UTF-8 BOM if present.
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);

    let mut entries: Vec<KeyEntry> = (0..256).map(|_| KeyEntry::empty()).collect();
    let mut compose: Vec<ComposeEntry> = Vec::new();

    // Ordered shift states from the SHIFTSTATE block (0 1 2 6 3 ...).
    let mut shift_states: Vec<u8> = Vec::new();
    let mut current_dead: Option<u16> = None;

    #[derive(PartialEq)]
    enum Mode { Scan, ShiftState, Layout, DeadKey, Compose, Skip }
    let mut mode = Mode::Scan;

    for raw in content.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Blank lines are separators inside blocks (e.g. between a
            // `DEADKEY` header and its rows); blocks end at the next header.
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }

        // Section headers are the first whitespace-delimited token.
        let first = trimmed.split_whitespace().next().unwrap_or("");
        match first {
            "SHIFTSTATE" => { mode = Mode::ShiftState; continue; }
            "LAYOUT" => { mode = Mode::Layout; continue; }
            "DEADKEY" => {
                let dead = trimmed.split_whitespace().nth(1).and_then(hex_u16);
                current_dead = dead;
                mode = if dead.is_some() { Mode::DeadKey } else { Mode::Skip };
                continue;
            }
            "COMPOSE" => { mode = Mode::Compose; continue; }
            "KEYNAME" | "KEYNAME_EXT" | "KEYNAME_DEAD" | "LIGATURE" | "DESCRIPTIONS"
            | "ENDKBD" | "ENDLAYOUT" | "LOCALENAME" | "LOCALEID" | "VERSION"
            | "ATTRIBUTES" | "ALTGR" => { mode = Mode::Skip; continue; }
            _ => {}
        }

        match mode {
            Mode::ShiftState => {
                if let Ok(state) = first.parse::<u8>() {
                    shift_states.push(state);
                }
            }
            Mode::Layout => {
                let cols: Vec<&str> = trimmed.split_whitespace().collect();
                if cols.len() < 3 { continue; }
                let sc = match u8::from_str_radix(cols[0], 16) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for (i, state) in shift_states.iter().enumerate() {
                    let idx = 3 + i;
                    if idx >= cols.len() { break; }
                    let (cp, dead) = match parse_value(cols[idx]) {
                        Some(v) => v,
                        None => continue,
                    };
                    let e = &mut entries[sc as usize];
                    match state {
                        0 => { e.normal = cp; if dead { e.flags |= 0x01; } }
                        1 => { e.shift = cp; if dead { e.flags |= 0x02; } }
                        2 => { e.ctrl = cp; }
                        6 => { e.altgr = cp; if dead { e.flags |= 0x04; } }
                        _ => {} // 3 (shift+ctrl): no kernel column
                    }
                }
            }
            Mode::DeadKey => {
                if let Some(dead) = current_dead {
                    let cols: Vec<&str> = trimmed.split_whitespace().collect();
                    if cols.len() >= 2 {
                        if let (Some(base), Some(result)) = (hex_u16(cols[0]), hex_u16(cols[1])) {
                            compose.push(ComposeEntry { dead, base, result });
                        }
                    }
                }
            }
            Mode::Compose => {
                let cols: Vec<&str> = trimmed.split_whitespace().collect();
                if cols.len() >= 3 {
                    if let (Some(dead), Some(base), Some(result)) =
                        (hex_u16(cols[0]), hex_u16(cols[1]), hex_u16(cols[2]))
                    {
                        compose.push(ComposeEntry { dead, base, result });
                    }
                }
            }
            Mode::Scan | Mode::Skip => {}
        }
    }

    // Write output.
    let mut out = Vec::with_capacity(60 + 256 * 16 + 4 + compose.len() * 6);
    out.extend_from_slice(KBD_MAGIC);
    out.extend_from_slice(&1u32.to_le_bytes());

    let mut name_bytes = [0u8; 32];
    let nlen = name.len().min(31);
    name_bytes[..nlen].copy_from_slice(&name.as_bytes()[..nlen]);
    out.extend_from_slice(&name_bytes);

    let mut tag_bytes = [0u8; 16];
    let tlen = lang_tag.len().min(15);
    tag_bytes[..tlen].copy_from_slice(&lang_tag.as_bytes()[..tlen]);
    out.extend_from_slice(&tag_bytes);

    out.extend_from_slice(&256u32.to_le_bytes());

    for entry in &entries {
        out.extend_from_slice(&entry.normal.to_le_bytes());
        out.extend_from_slice(&entry.shift.to_le_bytes());
        out.extend_from_slice(&entry.altgr.to_le_bytes());
        out.extend_from_slice(&entry.ctrl.to_le_bytes());
        out.push(entry.flags);
        out.extend_from_slice(&entry._pad);
    }

    out.extend_from_slice(&(compose.len() as u32).to_le_bytes());
    for entry in &compose {
        out.extend_from_slice(&entry.dead.to_le_bytes());
        out.extend_from_slice(&entry.base.to_le_bytes());
        out.extend_from_slice(&entry.result.to_le_bytes());
    }

    fs::write(output_path, &out).expect("Failed to write output file");
    let mapped = entries.iter().filter(|e| e.normal != 0xFFFF).count();
    eprintln!(
        "Wrote {} bytes to {} ({} scancodes, {} compose entries)",
        out.len(), output_path, mapped, compose.len()
    );
}
