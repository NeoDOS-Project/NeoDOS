use alloc::vec::Vec;
use crate::kbd::{KbdLayoutInfo, KBD_SHIFT, KBD_CTRL, KBD_ALTGR};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KeyEntry {
    pub normal: u16,
    pub shift: u16,
    pub altgr: u16,
    pub ctrl: u16,
    pub flags: u8,
    _pad: [u8; 7],
}

impl KeyEntry {
    pub const fn empty() -> Self {
        KeyEntry {
            normal: 0xFFFF,
            shift: 0xFFFF,
            altgr: 0xFFFF,
            ctrl: 0xFFFF,
            flags: 0,
            _pad: [0u8; 7],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComposeEntry {
    pub dead: u16,
    pub base: u16,
    pub result: u16,
}

#[derive(Clone, Debug)]
pub struct KbdLayout {
    pub name: [u8; 32],
    pub lang_tag: [u8; 16],
    pub entries: [KeyEntry; 256],
    pub compose: Vec<ComposeEntry>,
}

impl KbdLayout {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.name[..end]).unwrap_or("?")
    }

    pub fn lang_tag_str(&self) -> &str {
        let end = self.lang_tag.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.lang_tag[..end]).unwrap_or("?")
    }

    pub fn to_info(&self, index: u32) -> KbdLayoutInfo {
        let mut info = KbdLayoutInfo {
            index,
            name: [0u8; 32],
            lang_tag: [0u8; 16],
            scancode_count: 256,
            compose_count: self.compose.len() as u32,
        };
        let name_bytes = self.name_str().as_bytes();
        let len = name_bytes.len().min(31);
        info.name[..len].copy_from_slice(&name_bytes[..len]);
        let tag_bytes = self.lang_tag_str().as_bytes();
        let tlen = tag_bytes.len().min(15);
        info.lang_tag[..tlen].copy_from_slice(&tag_bytes[..tlen]);
        info
    }
}

const KBD_MAGIC: [u8; 4] = [b'K', b'B', b'D', 0];

pub fn load_kbd(path: &str) -> Result<KbdLayout, ()> {
    let mut buf = [0u8; 8192];
    let size = crate::globals::with_vfs(|vfs| -> Result<usize, ()> {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| ())?;
        if node.mode & crate::fs::vfs::MODE_FILE == 0 {
            return Err(());
        }
        let mut tmp = [0u8; 8192];
        let n = vfs.read(drive_idx, node.inode, 0, &mut tmp).map_err(|_| ())?;
        buf[..n].copy_from_slice(&tmp[..n]);
        Ok(n)
    }).map_err(|_| ())?;

    if size < 16 {
        return Err(());
    }
    if buf[..4] != KBD_MAGIC {
        return Err(());
    }

    let version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    if version != 1 {
        return Err(());
    }

    let mut layout = KbdLayout {
        name: [0u8; 32],
        lang_tag: [0u8; 16],
        entries: [KeyEntry::empty(); 256],
        compose: Vec::new(),
    };

    // Name (bytes 8-40)
    let name_end = buf[8..40].iter().position(|&b| b == 0).unwrap_or(32);
    layout.name[..name_end].copy_from_slice(&buf[8..8+name_end]);

    // Lang tag (bytes 40-56)
    let tag_end = buf[40..56].iter().position(|&b| b == 0).unwrap_or(16);
    layout.lang_tag[..tag_end].copy_from_slice(&buf[40..40+tag_end]);

    // Scancode count (bytes 56-60)
    let sc_count = u32::from_le_bytes(buf[56..60].try_into().unwrap()) as usize;
    let sc_count = sc_count.min(256);

    // Key table starts at byte 60
    let mut offset = 60usize;
    for i in 0..sc_count {
        if offset + 16 > size { break; }
        let normal = u16::from_le_bytes(buf[offset..offset+2].try_into().unwrap());
        let shift = u16::from_le_bytes(buf[offset+2..offset+4].try_into().unwrap());
        let altgr = u16::from_le_bytes(buf[offset+4..offset+6].try_into().unwrap());
        let ctrl = u16::from_le_bytes(buf[offset+6..offset+8].try_into().unwrap());
        let flags = buf[offset+8];
        layout.entries[i] = KeyEntry {
            normal, shift, altgr, ctrl,
            flags,
            _pad: [0u8; 7],
        };
        offset += 16;
    }

    // Compose count
    if offset + 4 > size { return Ok(layout); }
    let compose_count = u32::from_le_bytes(buf[offset..offset+4].try_into().unwrap()) as usize;
    offset += 4;

    for _ in 0..compose_count {
        if offset + 6 > size { break; }
        let dead = u16::from_le_bytes(buf[offset..offset+2].try_into().unwrap());
        let base = u16::from_le_bytes(buf[offset+2..offset+4].try_into().unwrap());
        let result = u16::from_le_bytes(buf[offset+4..offset+6].try_into().unwrap());
        layout.compose.push(ComposeEntry { dead, base, result });
        offset += 6;
    }

    Ok(layout)
}

pub fn load_layouts() -> usize {
    let mut layouts = Vec::new();

    // First try built-in fallback layout
    let fallback = KbdLayout {
        name: {
            let mut n = [0u8; 32];
            n[..2].copy_from_slice(b"US");
            n
        },
        lang_tag: {
            let mut t = [0u8; 16];
            t[..5].copy_from_slice(b"en-US");
            t
        },
        entries: builtin_us_layout(),
        compose: Vec::new(),
    };
    layouts.push(fallback);

    // Try loading from C:\System\Keyboard\*.kbd
    let dir_path = "C:\\System\\Keyboard";
    let has_dir = crate::globals::with_vfs(|vfs| -> bool {
        vfs.resolve_path(dir_path).is_ok()
    });

    if has_dir {
        // Try Spanish.kbd
        if let Ok(layout) = load_kbd("C:\\System\\Keyboard\\Spanish.kbd") {
            // Replace the fallback US if it's Spanish (or keep both)
            if layouts.iter().any(|l| l.name_str() == "Spanish") {
                // already loaded
            } else {
                layouts.push(layout);
            }
        }
    }

    let count = layouts.len();
    let mut kbd = crate::kbd::KBD.lock();
    kbd.layouts = layouts;
    count
}

pub fn lookup_codepoint(layout: &KbdLayout, scancode: u8, modifiers: u8) -> Option<u16> {
    let idx = scancode as usize;
    if idx >= 256 { return None; }
    let entry = &layout.entries[idx];
    let use_altgr = (modifiers & KBD_ALTGR) != 0;
    let use_shift = (modifiers & KBD_SHIFT) != 0;
    let use_ctrl = (modifiers & KBD_CTRL) != 0;

    let cp = if use_ctrl && entry.ctrl != 0xFFFF {
        entry.ctrl
    } else if use_altgr && entry.altgr != 0xFFFF {
        entry.altgr
    } else if use_shift && entry.shift != 0xFFFF {
        entry.shift
    } else if entry.normal != 0xFFFF {
        entry.normal
    } else {
        return None;
    };

    // Apply CapsLock for letters
    let cp = if (modifiers & crate::kbd::KBD_CAPS) != 0 {
        if cp >= b'a' as u16 && cp <= b'z' as u16 {
            cp - 32
        } else if cp >= b'A' as u16 && cp <= b'Z' as u16 {
            cp + 32
        } else {
            cp
        }
    } else {
        cp
    };

    Some(cp)
}

pub fn is_dead_key(layout: &KbdLayout, scancode: u8, modifiers: u8) -> bool {
    let idx = scancode as usize;
    if idx >= 256 { return false; }
    let entry = &layout.entries[idx];
    let use_altgr = (modifiers & KBD_ALTGR) != 0;
    let use_shift = (modifiers & KBD_SHIFT) != 0;

    let flags = if use_altgr { (entry.flags >> 2) & 1 }
               else if use_shift { (entry.flags >> 1) & 1 }
               else { entry.flags & 1 };
    flags == 1
}

pub fn compose(dead: u16, base: u16) -> u16 {
    // Try to find a compose entry matching this dead+base combination
    let kbd = crate::kbd::KBD.lock();
    if let Some(layout) = kbd.active_layout() {
        for entry in &layout.compose {
            if entry.dead == dead && entry.base == base {
                return entry.result;
            }
        }
    }
    // No compose match found; return '?' or the base character
    if dead == base { dead }
    else { b'?' as u16 }
}

fn builtin_us_layout() -> [KeyEntry; 256] {
    let mut entries = [KeyEntry::empty(); 256];
    // Basic US keyboard layout (scancode set 1)
    let us_map: [(u8, u16, u16); 66] = [
        (0x01, 0x1B, 0x1B), // Escape
        (0x02, b'1' as u16, b'!' as u16),
        (0x03, b'2' as u16, b'@' as u16),
        (0x04, b'3' as u16, b'#' as u16),
        (0x05, b'4' as u16, b'$' as u16),
        (0x06, b'5' as u16, b'%' as u16),
        (0x07, b'6' as u16, b'^' as u16),
        (0x08, b'7' as u16, b'&' as u16),
        (0x09, b'8' as u16, b'*' as u16),
        (0x0A, b'9' as u16, b'(' as u16),
        (0x0B, b'0' as u16, b')' as u16),
        (0x0C, b'-' as u16, b'_' as u16),
        (0x0D, b'=' as u16, b'+' as u16),
        (0x0E, 0x08, 0x08), // Backspace
        (0x0F, b'\t' as u16, b'\t' as u16), // Tab
        (0x10, b'q' as u16, b'Q' as u16),
        (0x11, b'w' as u16, b'W' as u16),
        (0x12, b'e' as u16, b'E' as u16),
        (0x13, b'r' as u16, b'R' as u16),
        (0x14, b't' as u16, b'T' as u16),
        (0x15, b'y' as u16, b'Y' as u16),
        (0x16, b'u' as u16, b'U' as u16),
        (0x17, b'i' as u16, b'I' as u16),
        (0x18, b'o' as u16, b'O' as u16),
        (0x19, b'p' as u16, b'P' as u16),
        (0x1A, b'[' as u16, b'{' as u16),
        (0x1B, b']' as u16, b'}' as u16),
        (0x1C, b'\n' as u16, b'\n' as u16), // Enter
        (0x1E, b'a' as u16, b'A' as u16),
        (0x1F, b's' as u16, b'S' as u16),
        (0x20, b'd' as u16, b'D' as u16),
        (0x21, b'f' as u16, b'F' as u16),
        (0x22, b'g' as u16, b'G' as u16),
        (0x23, b'h' as u16, b'H' as u16),
        (0x24, b'j' as u16, b'J' as u16),
        (0x25, b'k' as u16, b'K' as u16),
        (0x26, b'l' as u16, b'L' as u16),
        (0x27, b';' as u16, b':' as u16),
        (0x28, b'\'' as u16, b'"' as u16),
        (0x29, b'`' as u16, b'~' as u16),
        (0x2B, b'\\' as u16, b'|' as u16),
        (0x2C, b'z' as u16, b'Z' as u16),
        (0x2D, b'x' as u16, b'X' as u16),
        (0x2E, b'c' as u16, b'C' as u16),
        (0x2F, b'v' as u16, b'V' as u16),
        (0x30, b'b' as u16, b'B' as u16),
        (0x31, b'n' as u16, b'N' as u16),
        (0x32, b'm' as u16, b'M' as u16),
        (0x33, b',' as u16, b'<' as u16),
        (0x34, b'.' as u16, b'>' as u16),
        (0x35, b'/' as u16, b'?' as u16),
        (0x39, b' ' as u16, b' ' as u16), // Space
        (0x37, b'*' as u16, b'*' as u16), // Numpad *
        (0x4A, b'-' as u16, b'-' as u16), // Numpad -
        (0x4E, b'+' as u16, b'+' as u16), // Numpad +
        (0x47, b'7' as u16, b'7' as u16), // Numpad 7
        (0x48, b'8' as u16, b'8' as u16),
        (0x49, b'9' as u16, b'9' as u16),
        (0x4B, b'4' as u16, b'4' as u16),
        (0x4C, b'5' as u16, b'5' as u16),
        (0x4D, b'6' as u16, b'6' as u16),
        (0x4F, b'1' as u16, b'1' as u16),
        (0x50, b'2' as u16, b'2' as u16),
        (0x51, b'3' as u16, b'3' as u16),
        (0x52, b'0' as u16, b'0' as u16),
        (0x53, b'.' as u16, b'.' as u16), // Numpad .
    ];
    for (sc, normal, shift) in &us_map {
        let idx = *sc as usize;
        entries[idx].normal = *normal;
        entries[idx].shift = *shift;
    }
    entries
}
