//! Abstract console backend and screen helpers.
//!
//! [`Console`] is the only I/O the toolkit performs. The NeoDOS glue implements
//! it over `libneodos::console`; host tests implement it with a scripted buffer.

use alloc::string::String;

/// A minimal text console.
pub trait Console {
    /// Write a string (no trailing newline).
    fn write_str(&mut self, s: &str);

    /// Read one byte, blocking until one is available. Returns a negative value
    /// on error/EOF.
    fn read_byte(&mut self) -> i32;

    /// Non-blocking read: `Some(byte)` if one is available right now.
    ///
    /// The default implementation falls back to [`Console::read_byte`], which is
    /// only correct when that implementation does not block on empty input.
    fn try_read_byte(&mut self) -> Option<i32> {
        match self.read_byte() {
            b if b < 0 => None,
            b => Some(b),
        }
    }

    /// Clear the screen and home the cursor.
    fn clear(&mut self);

    /// Set colors (terminal index 0–15). Default: no-op.
    fn set_color(&mut self, _fg: u8, _bg: u8) {}

    /// Reset colors. Default: no-op.
    fn reset_color(&mut self) {}
}

/// A rendering surface over a [`Console`].
pub struct Screen<'a> {
    con: &'a mut dyn Console,
    /// Usable width in columns (default [`Screen::DEFAULT_WIDTH`]).
    pub width: usize,
}

impl<'a> Screen<'a> {
    /// Default width used when the backend cannot report one.
    pub const DEFAULT_WIDTH: usize = 80;

    pub fn new(con: &'a mut dyn Console) -> Self {
        Screen {
            con,
            width: Self::DEFAULT_WIDTH,
        }
    }

    /// Mutable access to the underlying console.
    pub fn console(&mut self) -> &mut dyn Console {
        self.con
    }

    pub fn clear(&mut self) {
        self.con.clear();
    }

    pub fn write(&mut self, s: &str) {
        self.con.write_str(s);
    }

    /// Write `s` followed by a newline.
    pub fn line(&mut self, s: &str) {
        self.con.write_str(s);
        self.con.write_str("\r\n");
    }

    pub fn blank(&mut self) {
        self.con.write_str("\r\n");
    }

    /// A `=====` rule spanning the width (capped at 60 columns).
    pub fn separator(&mut self) {
        let n = self.width.min(60);
        let mut s = String::new();
        for _ in 0..n {
            s.push('=');
        }
        self.line(&s);
    }

    /// A centered title.
    pub fn title(&mut self, s: &str) {
        self.centered(s);
    }

    /// Write `s` centered within the screen width.
    pub fn centered(&mut self, s: &str) {
        let len = s.chars().count();
        let pad = self.width.saturating_sub(len) / 2;
        let mut line = String::new();
        for _ in 0..pad {
            line.push(' ');
        }
        line.push_str(s);
        self.line(&line);
    }

    /// Prompt without a newline; the cursor stays at the end.
    pub fn prompt(&mut self, s: &str) {
        self.con.write_str(s);
    }

    /// Read one decoded key, blocking until a real key is available.
    pub fn read_key(&mut self) -> crate::keys::Key {
        loop {
            match crate::keys::read_key(&mut *self.con) {
                crate::keys::Key::Unknown => continue,
                key => return key,
            }
        }
    }

    /// Wait for any key (discards it).
    pub fn wait_key(&mut self) {
        let _ = self.read_key();
    }

    /// Suspend until the user presses a key, showing a hint line.
    pub fn pause(&mut self, hint: &str) {
        self.blank();
        self.prompt(hint);
        self.wait_key();
    }
}
