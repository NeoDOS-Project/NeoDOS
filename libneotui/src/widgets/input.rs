//! Append-only line input.

use alloc::string::String;

use crate::keys::Key;
use crate::screen::Screen;

/// Maximum accepted input length.
pub const MAX_INPUT: usize = 128;

/// Read a line into `out` (append-only, backspace supported).
///
/// Returns `true` when the user pressed Enter, `false` on `Esc`.
pub fn read_line(screen: &mut Screen, prompt: &str, out: &mut String) -> bool {
    out.clear();
    screen.prompt(prompt);
    loop {
        match screen.read_key() {
            Key::Enter => {
                screen.write("\r\n");
                return true;
            }
            Key::Esc => return false,
            Key::Backspace => {
                if out.pop().is_some() {
                    // Erase the previous cell: backspace, space, backspace.
                    screen.write("\x08 \x08");
                }
            }
            Key::Char(c) if !c.is_control() && out.len() + c.len_utf8() <= MAX_INPUT => {
                out.push(c);
                let mut buf = [0u8; 4];
                screen.write(c.encode_utf8(&mut buf));
            }
            _ => {}
        }
    }
}
