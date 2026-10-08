//! Key decoding: raw console bytes → [`Key`].
//!
//! NeoDOS consoles deliver raw bytes. Printable ASCII arrives as itself; the
//! arrow/editing keys arrive as ANSI escape sequences (`ESC [ A`, …). A lone
//! `ESC` is decoded as [`Key::Esc`] using a non-blocking look-ahead so that
//! `Esc` never hangs waiting for a sequence that will not come.

use crate::screen::Console;

/// A decoded key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Unknown,
}

/// Read and decode one key. Returns [`Key::Unknown`] when no byte is available.
pub fn read_key(con: &mut dyn Console) -> Key {
    match con.read_byte() {
        -1 => Key::Unknown,
        0x0D | 0x0A => Key::Enter,
        0x1B => decode_escape(con),
        0x08 | 0x7F => Key::Backspace,
        0x09 => Key::Tab,
        c if (0x20..=0x7E).contains(&c) => Key::Char(c as u8 as char),
        _ => Key::Unknown,
    }
}

fn decode_escape(con: &mut dyn Console) -> Key {
    match con.try_read_byte() {
        Some(c) if c == b'[' as i32 || c == b'O' as i32 => match con.try_read_byte() {
            Some(c) if c == b'A' as i32 => Key::Up,
            Some(c) if c == b'B' as i32 => Key::Down,
            Some(c) if c == b'C' as i32 => Key::Right,
            Some(c) if c == b'D' as i32 => Key::Left,
            Some(c) if c == b'H' as i32 => Key::Home,
            Some(c) if c == b'F' as i32 => Key::End,
            Some(c) if c == b'Z' as i32 => Key::BackTab,
            // ESC [ 5 ~ / ESC [ 6 ~ (consume the trailing '~').
            Some(c) if c == b'5' as i32 => {
                let _ = con.try_read_byte();
                Key::PageUp
            }
            Some(c) if c == b'6' as i32 => {
                let _ = con.try_read_byte();
                Key::PageDown
            }
            Some(_) | None => Key::Esc,
        },
        Some(_) | None => Key::Esc,
    }
}
