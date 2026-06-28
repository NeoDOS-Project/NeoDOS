use std::env;
use std::fs;
use std::path::PathBuf;

fn decode_klc_to_utf8(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let mut u16s = Vec::with_capacity((bytes.len() - 2) / 2);
        for chunk in bytes[2..].chunks_exact(2) {
            u16s.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

fn klc_token_codepoint(token: Option<&str>) -> Option<u32> {
    let mut token = token?;
    if let Some(stripped) = token.strip_suffix('@') {
        token = stripped;
    }
    if token == "-1" || token == "@" {
        return None;
    }
    if token.len() == 1 {
        let b = token.as_bytes()[0];
        if b.is_ascii() {
            return Some(b as u32);
        }
    }
    if token.len() == 4 && token.bytes().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(v) = u32::from_str_radix(token, 16) {
            return Some(v);
        }
    }
    if token.len() == 5 && token.bytes().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(v) = u32::from_str_radix(token, 16) {
            if v <= 0xFFFF {
                return Some(v);
            }
        }
    }
    None
}

fn klc_token_is_dead(token: Option<&str>) -> bool {
    token.is_some_and(|t| t.ends_with('@'))
}

struct LayoutTables {
    normal: [u16; 256],
    shift: [u16; 256],
    altgr: [u16; 256],
    normal_dead: [u8; 256],
    shift_dead: [u8; 256],
    altgr_dead: [u8; 256],
}

fn fill_numpad_defaults(tbl: &mut LayoutTables) {
    let numpad: [(usize, u16); 14] = [
        (0x37, b'*' as u16),
        (0x47, b'7' as u16),
        (0x48, b'8' as u16),
        (0x49, b'9' as u16),
        (0x4A, b'-' as u16),
        (0x4B, b'4' as u16),
        (0x4C, b'5' as u16),
        (0x4D, b'6' as u16),
        (0x4E, b'+' as u16),
        (0x4F, b'1' as u16),
        (0x50, b'2' as u16),
        (0x51, b'3' as u16),
        (0x52, b'0' as u16),
        (0x53, b'.' as u16),
    ];
    for &(sc, ch) in &numpad {
        if tbl.normal[sc] == 0 {
            tbl.normal[sc] = ch;
        }
        if tbl.shift[sc] == 0 {
            tbl.shift[sc] = ch;
        }
    }
}

fn parse_klc_layout(text: &str) -> LayoutTables {
    let mut normal = [0u16; 256];
    let mut shift = [0u16; 256];
    let mut altgr = [0u16; 256];
    let mut normal_dead = [0u8; 256];
    let mut shift_dead = [0u8; 256];
    let mut altgr_dead = [0u8; 256];

    let mut in_layout = false;
    let mut header_cols: Option<Vec<u8>> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("LAYOUT") {
            in_layout = true;
            continue;
        }
        if line.starts_with("KEYNAME") {
            break;
        }
        if line.starts_with("DEADKEY") {
            break;
        }
        if !in_layout {
            continue;
        }

        if line.starts_with("//") {
            if header_cols.is_none() && line.contains("Cap") && line.contains("SC") {
                let cols: Vec<u8> = line
                    .split_whitespace()
                    .skip_while(|t| *t != "Cap")
                    .skip(1)
                    .filter_map(|t| t.parse::<u8>().ok())
                    .collect();
                if !cols.is_empty() {
                    header_cols = Some(cols);
                }
            }
            continue;
        }

        let mut tokens = line.split_whitespace();
        let sc_hex = match tokens.next() {
            Some(t) => t,
            None => continue,
        };
        let sc = match u8::from_str_radix(sc_hex, 16) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let _vk = tokens.next();
        let _cap = tokens.next();

        let cols = header_cols.as_deref().unwrap_or(&[0u8, 1u8]);
        for &col_id in cols {
            let token = tokens.next();
            let is_dead = klc_token_is_dead(token);
            let Some(cp) = klc_token_codepoint(token) else {
                continue;
            };
            let b = if cp <= 0xFFFF { cp as u16 } else { 0 };
            if b == 0 {
                continue;
            }
            match col_id {
                0 => { normal[sc as usize] = b; normal_dead[sc as usize] = is_dead as u8; }
                1 => { shift[sc as usize] = b; shift_dead[sc as usize] = is_dead as u8; }
                6 => { altgr[sc as usize] = b; altgr_dead[sc as usize] = is_dead as u8; }
                _ => {}
            }
        }
    }

    normal[0x0E] = 0x08;
    shift[0x0E] = 0x08;
    normal[0x0F] = b'\t' as u16;
    shift[0x0F] = b'\t' as u16;
    normal[0x1C] = b'\n' as u16;
    shift[0x1C] = b'\n' as u16;

    LayoutTables { normal, shift, altgr, normal_dead, shift_dead, altgr_dead }
}

struct ComposeEntry {
    dead: u8,
    base: u8,
    result: u8,
}

fn parse_klc_compose(text: &str) -> Vec<ComposeEntry> {
    let mut entries = Vec::new();
    let mut current_dead: Option<u8> = None;
    let mut in_compose = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("KEYNAME") || line.starts_with("ENDKBD") {
            if line.starts_with("KEYNAME") || line.starts_with("ENDKBD") {
                break;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("DEADKEY") {
            let hex = rest.split_whitespace().next().unwrap_or("");
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                current_dead = Some(v as u8);
                in_compose = true;
            }
            continue;
        }

        if in_compose {
            if line.starts_with("//") {
                continue;
            }
            let mut parts = line.split_whitespace();
            let base_token = parts.next();
            let result_token = parts.next();
            if let (Some(base_t), Some(result_t)) = (base_token, result_token) {
                let base_val = u32::from_str_radix(base_t, 16);
                let result_val = u32::from_str_radix(result_t, 16);
                if let (Ok(base), Ok(result)) = (base_val, result_val) {
                    if let Some(&dead) = current_dead.as_ref() {
                        if base <= 0xFF && result <= 0xFF {
                            entries.push(ComposeEntry {
                                dead,
                                base: base as u8,
                                result: result as u8,
                            });
                        }
                    }
                }
            }
        }
    }

    entries
}

fn write_u16_array(out: &mut String, name: &str, data: &[u16]) {
    out.push_str(&format!("pub const {name}: [u16; {}] = [\n", data.len()));
    for (i, &b) in data.iter().enumerate() {
        out.push_str(&format!("  {b},"));
        if (i + 1) % 16 == 0 {
            out.push('\n');
        }
    }
    if !data.len().is_multiple_of(16) {
        out.push('\n');
    }
    out.push_str("];\n\n");
}

fn write_u8_array(out: &mut String, name: &str, data: &[u8]) {
    out.push_str(&format!("pub const {name}: [u8; {}] = [\n", data.len()));
    for (i, &b) in data.iter().enumerate() {
        out.push_str(&format!("  {b},"));
        if (i + 1) % 16 == 0 {
            out.push('\n');
        }
    }
    if !data.len().is_multiple_of(16) {
        out.push('\n');
    }
    out.push_str("];\n\n");
}

fn write_table(out: &mut String, prefix: &str, tbl: &LayoutTables) {
    write_u16_array(out, &format!("{prefix}_NORMAL"), &tbl.normal);
    write_u16_array(out, &format!("{prefix}_SHIFT"), &tbl.shift);
    write_u16_array(out, &format!("{prefix}_ALTGR"), &tbl.altgr);
    write_u8_array(out, &format!("{prefix}_NORMAL_DEAD"), &tbl.normal_dead);
    write_u8_array(out, &format!("{prefix}_SHIFT_DEAD"), &tbl.shift_dead);
    write_u8_array(out, &format!("{prefix}_ALTGR_DEAD"), &tbl.altgr_dead);
}

fn write_compose(out: &mut String, prefix: &str, entries: &[ComposeEntry]) {
    let count = entries.len();
    let mut dead = Vec::with_capacity(count);
    let mut base = Vec::with_capacity(count);
    let mut result = Vec::with_capacity(count);
    for e in entries {
        dead.push(e.dead);
        base.push(e.base);
        result.push(e.result);
    }
    out.push_str(&format!("pub const {prefix}_COMPOSE_COUNT: usize = {count};\n"));
    write_u8_array(out, &format!("{prefix}_COMPOSE_DEAD"), &dead);
    write_u8_array(out, &format!("{prefix}_COMPOSE_BASE"), &base);
    write_u8_array(out, &format!("{prefix}_COMPOSE_RESULT"), &result);
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let us_path = manifest_dir.join("layouts/KBDUS.klc");
    let sp_path = manifest_dir.join("layouts/KBDSP.klc");
    println!("cargo:rerun-if-changed={}", us_path.display());
    println!("cargo:rerun-if-changed={}", sp_path.display());

    let us_bytes = fs::read(&us_path).expect("read KBDUS.klc");
    let us_text = decode_klc_to_utf8(&us_bytes);
    let mut us_tbl = parse_klc_layout(&us_text);
    fill_numpad_defaults(&mut us_tbl);
    let us_compose = parse_klc_compose(&us_text);

    let sp_bytes = fs::read(&sp_path).expect("read KBDSP.klc");
    let sp_text = decode_klc_to_utf8(&sp_bytes);
    let mut sp_tbl = parse_klc_layout(&sp_text);
    fill_numpad_defaults(&mut sp_tbl);
    let sp_compose = parse_klc_compose(&sp_text);

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_path = out_dir.join("kbd_layout.rs");

    let mut out = String::new();
    out.push_str("// @generated by build.rs - do not edit\n\n");

    write_table(&mut out, "KBDUS", &us_tbl);
    write_compose(&mut out, "KBDUS", &us_compose);

    write_table(&mut out, "KBDSP", &sp_tbl);
    write_compose(&mut out, "KBDSP", &sp_compose);

    fs::write(&out_path, out).expect("write kbd_layout.rs");
    
    // Copiar a kernel para que lo use
    let kernel_out = manifest_dir.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("neodos-kernel/src/drivers/nem/drivers/kbd_layout.rs"))
        .expect("kernel path");
    fs::copy(&out_path, &kernel_out).expect("copy kbd_layout.rs to kernel");
}
