// src/vga.rs (now acts as a generic console)

use core::fmt::{Write, Result, Arguments};
use crate::graphics::RENDERER;
use crate::font;

const VGA_WIDTH: usize = 160;
const VGA_HEIGHT: usize = 50;

static mut ROW: usize = 0;
static mut COL: usize = 0;

pub struct VgaWriter;

impl Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> Result {
        for byte in s.bytes() {
            write_char(byte);
        }
        Ok(())
    }
}

pub fn _print(args: Arguments) {
    let mut writer = VgaWriter;
    let _ = Write::write_fmt(&mut writer, args);
    crate::serial_print!("{}", args);
}

pub fn init() {
    // Legacy VGA init could go here, but we are using GOP
}

pub fn write_char(c: u8) {
    unsafe {
        match c {
            b'\n' => {
                ROW += 1;
                COL = 0;
            }
            b'\r' => {
                COL = 0;
            }
            b'\x08' => {
                if COL > 0 { COL -= 1; }
                draw_char_at(b' ', ROW, COL, 0x000000);
            }
            c => {
                draw_char_at(c, ROW, COL, 0xFFFFFF); // White text
                COL += 1;
            }
        }

        if COL >= VGA_WIDTH {
            COL = 0;
            ROW += 1;
        }

        if ROW >= VGA_HEIGHT {
            // No scrolling for now, just reset
            ROW = 0;
            if let Some(ref r) = RENDERER {
                r.clear(0x000000);
            }
        }
    }
}

fn draw_char_at(c: u8, row: usize, col: usize, color: u32) {
    let x = col * font::FONT_WIDTH;
    let y = row * font::FONT_HEIGHT;
    font::draw_char(c as char, x, y, color);
}

pub fn print_decimal(value: u64) {
    let mut writer = VgaWriter;
    let _ = write!(writer, "{}", value);
}

pub fn print_hex(value: u64) {
    let mut writer = VgaWriter;
    let _ = write!(writer, "0x{:x}", value);
}

pub fn print_str(s: &str) {
    for byte in s.bytes() {
        write_char(byte);
    }
}

pub fn draw_cursor(visible: bool) {
    unsafe {
        if visible {
            draw_char_at(b'_', ROW, COL, 0xFFFFFF);
        } else {
            draw_char_at(b' ', ROW, COL, 0x000000);
        }
    }
}

pub fn clear_screen() {
    unsafe {
        ROW = 0;
        COL = 0;
        if let Some(ref r) = RENDERER {
            r.clear(0x000000);
        }
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::vga::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\r\n"));
    ($($arg:tt)*) => ($crate::print!("{}\r\n", format_args!($($arg)*)));
}
