// src/vga.rs (now acts as a generic console)

use core::fmt::{Write, Result, Arguments};
use crate::graphics::RENDERER;
use crate::font;

const VGA_WIDTH: usize = 160;
const VGA_HEIGHT: usize = 50;

use core::sync::atomic::{AtomicUsize, Ordering};

static ROW: AtomicUsize = AtomicUsize::new(0);
static COL: AtomicUsize = AtomicUsize::new(0);

pub struct VgaWriter;

impl Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> Result {
        for c in s.chars() {
            write_codepoint(c as u32);
        }
        Ok(())
    }
}

pub fn write_codepoint(cp: u32) {
    let index: u8 = if cp <= 0xFF {
        cp as u8
    } else if cp == 0x20AC {
        0x80
    } else {
        0x81
    };
    write_char(index);
}

pub fn _print(args: Arguments) {
    let mut writer = VgaWriter;
    let _ = Write::write_fmt(&mut writer, args);
    crate::serial_print!("{}", args);
}

pub fn init() {
    // Legacy VGA init could go here, but we are using GOP
}

fn console_max_row() -> usize {
    if let Some(ref r) = *RENDERER.lock() {
        let max_rows = r.fb.height / font::FONT_HEIGHT;
        max_rows.min(VGA_HEIGHT).max(1)
    } else {
        VGA_HEIGHT
    }
}

pub fn write_char(c: u8) {
    let mut r = ROW.load(Ordering::SeqCst);
    let mut col = COL.load(Ordering::SeqCst);

    match c {
        b'\n' => {
            r += 1;
            col = 0;
        }
        b'\r' => {
            col = 0;
        }
        b'\x08' => {
            if col > 0 { col -= 1; }
            draw_char_at(b' ', r, col, 0x000000);
        }
        c => {
            draw_char_at(c, r, col, 0xFFFFFF);
            col += 1;
        }
    }

    let max_row = console_max_row();
    let max_col = console_width();

    if col >= max_col {
        col = 0;
        r += 1;
    }

    if r >= max_row {
        scroll();
        r = max_row - 1;
        col = 0;
    }

    ROW.store(r, Ordering::SeqCst);
    COL.store(col, Ordering::SeqCst);
}

fn draw_char_at(c: u8, row: usize, col: usize, color: u32) {
    let x = col * font::FONT_WIDTH;
    let y = row * font::FONT_HEIGHT;
    font::draw_char(c, x, y, color);
}

fn scroll() {
    if let Some(ref r) = *RENDERER.lock() {
        let fb = r.fb.base_address as *mut u32;
        let stride = r.fb.stride;
        let row_h = font::FONT_HEIGHT;
        let fb_height_pixels = r.fb.height;
        let visible_rows = fb_height_pixels / row_h;
        if visible_rows < 2 { return; }
        let rows_total = visible_rows * row_h;

        // Shift all pixel rows up by one character row
        unsafe {
            core::ptr::copy(
                fb.add(row_h * stride),
                fb,
                (rows_total - row_h) * stride,
            );

            // Clear last character row
            let last = fb.add((rows_total - row_h) * stride);
            core::ptr::write_bytes(last, 0, row_h * stride * 4);
        }
    }
}

#[allow(dead_code)]
pub fn print_decimal(value: u64) {
    let mut writer = VgaWriter;
    let _ = write!(writer, "{}", value);
}

#[allow(dead_code)]
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
    let max_row = console_max_row();
    let max_col = console_width();
    let r = ROW.load(Ordering::SeqCst).min(max_row - 1);
    let c = COL.load(Ordering::SeqCst).min(max_col - 1);
    if visible {
        draw_char_at(b'_', r, c, 0xFFFFFF);
    } else {
        draw_char_at(b' ', r, c, 0x000000);
    }
}

pub fn clear_screen() {
    ROW.store(0, Ordering::SeqCst);
    COL.store(0, Ordering::SeqCst);
    if let Some(ref r) = *RENDERER.lock() {
        r.clear(0x000000);
    }
}

fn console_width() -> usize {
    if let Some(ref r) = *RENDERER.lock() {
        (r.fb.width / font::FONT_WIDTH).min(VGA_WIDTH).max(1)
    } else {
        VGA_WIDTH
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::console::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\r\n"));
    ($($arg:tt)*) => ($crate::print!("{}\r\n", format_args!($($arg)*)));
}
