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

#[repr(C)]
struct ComposeEntry {
    dead: u16,
    base: u16,
    result: u16,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: kbdcompile <input.klc> <output.kbd> [name] [langtag]");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];
    let name = if args.len() > 3 { &args[3] } else { "Unknown" };
    let lang_tag = if args.len() > 4 { &args[4] } else { "xx-XX" };

    let content = fs::read_to_string(input_path).unwrap_or_default();

    let mut entries: Vec<KeyEntry> = Vec::with_capacity(256);
    for _ in 0..256 {
        entries.push(KeyEntry::empty());
    }
    let compose: Vec<ComposeEntry> = Vec::new();

    // Parse KLC format — very simple parser
    let mut in_layout = false;
    let mut in_shift = false;
    let mut in_altgr = false;
    let mut shift_map: Vec<(u8, u16)> = Vec::new();
    let mut altgr_map: Vec<(u8, u16)> = Vec::new();
    let mut normal_map: Vec<(u8, u16)> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }

        match trimmed {
            "LAYOUT" => { in_layout = true; continue; }
            "SHIFTSTATE 0" => { in_shift = false; in_altgr = false; continue; }
            "SHIFTSTATE 1" => { in_shift = true; in_altgr = false; continue; }
            "SHIFTSTATE 2" => { in_shift = false; in_altgr = true; continue; }
            "COMPOSE" => {
                in_layout = false;
                continue;
            }
            "ENDKBD" => break,
            "KEYNAME" | "KEYNAME_EXT" | "DEADKEY" | "KEYNAME_DEAD" | "LIGATURE" | "DESCRIPTION" | "CODEPAGE" => {
                continue;
            }
            "ENDLAYOUT" => { in_layout = false; continue; }
            _ => {}
        }

        if in_layout && trimmed.starts_with("SC") {
            // Parse SCxxxx = hex_value
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() != 2 { continue; }
            let sc_str = parts[0].trim().trim_start_matches("SC");
            let val_str = parts[1].trim();
            if let (Ok(sc), Ok(val)) = (u8::from_str_radix(sc_str, 16), u16::from_str_radix(val_str, 16)) {
                normal_map.push((sc, val));
            }
        }

        if trimmed.starts_with("SC") && !in_layout && !in_shift && !in_altgr {
            // Another format: SCxxx=value outside LAYOUT block
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() == 2 {
                let sc_str = parts[0].trim().trim_start_matches("SC");
                let val_str = parts[1].trim();
                if let (Ok(sc), Ok(val)) = (u8::from_str_radix(sc_str, 16), u16::from_str_radix(val_str, 16)) {
                    normal_map.push((sc, val));
                }
            }
        }

        if in_shift && trimmed.starts_with("SC") {
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() == 2 {
                let sc_str = parts[0].trim().trim_start_matches("SC");
                let val_str = parts[1].trim();
                if let (Ok(sc), Ok(val)) = (u8::from_str_radix(sc_str, 16), u16::from_str_radix(val_str, 16)) {
                    shift_map.push((sc, val));
                }
            }
        }

        if in_altgr && trimmed.starts_with("SC") {
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() == 2 {
                let sc_str = parts[0].trim().trim_start_matches("SC");
                let val_str = parts[1].trim();
                if let (Ok(sc), Ok(val)) = (u8::from_str_radix(sc_str, 16), u16::from_str_radix(val_str, 16)) {
                    altgr_map.push((sc, val));
                }
            }
        }
    }

    // Fill entries
    for (sc, val) in &normal_map {
        if (*sc as usize) < 256 {
            entries[*sc as usize].normal = *val;
        }
    }
    for (sc, val) in &shift_map {
        if (*sc as usize) < 256 {
            entries[*sc as usize].shift = *val;
        }
    }
    for (sc, val) in &altgr_map {
        if (*sc as usize) < 256 {
            entries[*sc as usize].altgr = *val;
        }
    }

    // Write output
    let mut out = Vec::new();
    out.extend_from_slice(KBD_MAGIC); // Magic: 4 bytes
    out.extend_from_slice(&1u32.to_le_bytes()); // Version: 4 bytes

    // Name: 32 bytes
    let mut name_bytes = [0u8; 32];
    let nlen = name.len().min(31);
    name_bytes[..nlen].copy_from_slice(&name.as_bytes()[..nlen]);
    out.extend_from_slice(&name_bytes);

    // Lang tag: 16 bytes
    let mut tag_bytes = [0u8; 16];
    let tlen = lang_tag.len().min(15);
    tag_bytes[..tlen].copy_from_slice(&lang_tag.as_bytes()[..tlen]);
    out.extend_from_slice(&tag_bytes);

    // Scancode count: 4 bytes
    out.extend_from_slice(&256u32.to_le_bytes());

    // Key entries: 256 * 16 = 4096 bytes
    for entry in &entries {
        out.extend_from_slice(&entry.normal.to_le_bytes());
        out.extend_from_slice(&entry.shift.to_le_bytes());
        out.extend_from_slice(&entry.altgr.to_le_bytes());
        out.extend_from_slice(&entry.ctrl.to_le_bytes());
        out.push(entry.flags);
        out.extend_from_slice(&entry._pad);
    }

    // Compose count: 4 bytes
    out.extend_from_slice(&(compose.len() as u32).to_le_bytes());

    // Compose entries: each 6 bytes
    for entry in &compose {
        out.extend_from_slice(&entry.dead.to_le_bytes());
        out.extend_from_slice(&entry.base.to_le_bytes());
        out.extend_from_slice(&entry.result.to_le_bytes());
    }

    fs::write(output_path, &out).expect("Failed to write output file");
    eprintln!("Wrote {} bytes to {}", out.len(), output_path);
}
