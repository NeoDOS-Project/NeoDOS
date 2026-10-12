// src/console.rs
// ANSI-capable console driver: parses escape sequences, renders with 16-color palette

use core::fmt::{Write, Result, Arguments};
use crate::graphics::RENDERER;
use crate::font;

const VGA_WIDTH: usize = 160;
const VGA_HEIGHT: usize = 50;

use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};

static ROW: AtomicUsize = AtomicUsize::new(0);
static COL: AtomicUsize = AtomicUsize::new(0);

// ── Cursor blink ──
static CURSOR_VISIBLE: AtomicBool = AtomicBool::new(true);
static CURSOR_BLINK_ENABLED: AtomicBool = AtomicBool::new(false);
static CURSOR_BLINK_COUNTER: AtomicUsize = AtomicUsize::new(0);
const CURSOR_BLINK_INTERVAL: usize = 18; // ticks (~every 18ms at 1KHz = ~55 Hz)


mod ansi;
mod tests;

use ansi::{ANSI_STATE, ANSI_NORMAL, ANSI_ESC, handle_ansi_byte, current_fg_rgb, current_bg_rgb, ANSI_FG, ANSI_BG, ANSI_BOLD, ANSI_COLORS};
pub use tests::{register_ansi_tests, register_render_tests};

// ── VgaWriter (fmt::Write) ─────────────────────────────────────────────────
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
    } else {
        match cp {
            0x20AC => 0x80, // €
            0x2500 => 0x82, // ─
            0x2502 => 0x83, // │
            0x2514 => 0x84, // └
            0x251C => 0x85, // ├
            _      => 0x81, // full block fallback
        }
    };
    write_char(index);
}

pub fn _print(args: Arguments) {
    let mut writer = VgaWriter;
    let _ = Write::write_fmt(&mut writer, args);
    crate::serial_print!("{}", args);
}

pub fn init() {}

#[derive(Clone, Copy)]
pub struct ConsoleState {
    pub row: usize,
    pub col: usize,
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub cursor_visible: bool,
}

impl ConsoleState {
    pub const fn new() -> Self {
        ConsoleState { row: 0, col: 0, fg: 7, bg: 0, bold: false, cursor_visible: true }
    }
}

pub fn save_state() -> ConsoleState {
    ConsoleState {
        row: ROW.load(Ordering::SeqCst),
        col: COL.load(Ordering::SeqCst),
        fg: ANSI_FG.load(Ordering::Relaxed),
        bg: ANSI_BG.load(Ordering::Relaxed),
        bold: ANSI_BOLD.load(Ordering::Relaxed),
        cursor_visible: CURSOR_VISIBLE.load(Ordering::Relaxed),
    }
}

pub fn restore_state(state: &ConsoleState) {
    ROW.store(state.row, Ordering::SeqCst);
    COL.store(state.col, Ordering::SeqCst);
    ANSI_FG.store(state.fg, Ordering::Relaxed);
    ANSI_BG.store(state.bg, Ordering::Relaxed);
    ANSI_BOLD.store(state.bold, Ordering::Relaxed);
    CURSOR_VISIBLE.store(state.cursor_visible, Ordering::Relaxed);
    draw_cursor(state.cursor_visible);
}

pub fn redraw_from_shadow(shadow: &crate::input::vt::VtShadowBuffer) {
    // Runs from the keyboard IRQ (Alt+F1..F8 VT switch). Hold the renderer once,
    // IRQ-safe, and blit the held guard (INV-3, Rule 3.5.1).
    crate::graphics::with_renderer(|r| {
        let Some(r) = r else { return };
        r.clear(ANSI_COLORS[0]);
        let max_row = (r.fb.height / font::FONT_HEIGHT)
            .clamp(1, VGA_HEIGHT)
            .min(crate::input::vt::VT_CONSOLE_ROWS);
        let max_col = (r.fb.width / font::FONT_WIDTH)
            .clamp(1, VGA_WIDTH)
            .min(crate::input::vt::VT_CONSOLE_COLS);
        for row in 0..max_row {
            for col in 0..max_col {
                let ch = shadow.chars[row][col];
                if ch != 0 {
                    font::draw_char_on(
                        r, ch, col * font::FONT_WIDTH, row * font::FONT_HEIGHT,
                        ANSI_COLORS[7], ANSI_COLORS[0]);
                }
            }
        }
    });
}

fn console_max_row() -> usize {
    crate::graphics::with_renderer(|r| match r {
        Some(r) => (r.fb.height / font::FONT_HEIGHT).clamp(1, VGA_HEIGHT),
        None => VGA_HEIGHT,
    })
}


// ── Character output ──────────────────────────────────────────────────────
pub fn write_char(c: u8) {
    // Check ANSI parser state first
    if ANSI_STATE.load(Ordering::Relaxed) != ANSI_NORMAL {
        handle_ansi_byte(c);
        return;
    }

    // ESC starts an escape sequence
    if c == 0x1b {
        ANSI_STATE.store(ANSI_ESC, Ordering::Relaxed);
        return;
    }

    let mut r = ROW.load(Ordering::SeqCst);
    let mut col = COL.load(Ordering::SeqCst);

    match c {
        b'\n' => { r += 1; col = 0; }
        b'\r' => { col = 0; }
        b'\x08' => {
            col = col.saturating_sub(1);
            draw_char_at(b' ', r, col);
        }
        c => {
            draw_char_at(c, r, col);
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

fn draw_char_at(c: u8, row: usize, col: usize) {
    let x = col * font::FONT_WIDTH;
    let y = row * font::FONT_HEIGHT;
    let fg = current_fg_rgb();
    let bg = current_bg_rgb();
    font::draw_char(c, x, y, fg, bg);
    let act = crate::input::active_vt();
    if let Some(im) = crate::input::manager::input_manager_mut() {
        if row < crate::input::vt::VT_CONSOLE_ROWS && col < crate::input::vt::VT_CONSOLE_COLS {
            im.vt_shadow[act].chars[row][col] = c;
        }
    }
}

fn scroll() {
    crate::graphics::with_renderer(|r| {
        let Some(r) = r else { return };
        let fb = r.fb.base_address as *mut u32;
        let stride = r.fb.stride;
        let row_h = font::FONT_HEIGHT;
        let fb_height_pixels = r.fb.height;
        let visible_rows = fb_height_pixels / row_h;
        if visible_rows < 2 { return; }
        let rows_total = visible_rows * row_h;
        let bg = current_bg_rgb();

        unsafe {
            core::ptr::copy(
                fb.add(row_h * stride),
                fb,
                (rows_total - row_h) * stride,
            );

            let last = fb.add((rows_total - row_h) * stride);
            // Fill new blank row with background color
            crate::hal::raw::raw_rep_stosd(last, row_h * stride, bg);
        }
    });
}

pub fn print_str(s: &str) {
    for c in s.chars() {
        write_codepoint(c as u32);
    }
    crate::serial_print!("{}", s);
}

pub fn draw_cursor(visible: bool) {
    // Cursor blinking is driven from the BSP timer IRQ (`cursor_timer_tick`).
    // INV-3: an IRQ handler MUST NOT block on a spinlock that non-IRQ context
    // can hold with interrupts enabled. If the timer interrupts a thread inside
    // `scroll()`/`draw_char()` while it holds `RENDERER`, a blocking
    // `RENDERER.lock()` here would spin forever on the same CPU with interrupts
    // disabled and wedge the BSP (the observed system-wide freeze). Use
    // `try_lock` and skip this blink frame when the renderer is busy; the next
    // blink tick retries.
    let guard = match RENDERER.try_lock() {
        Some(g) => g,
        None => return,
    };
    let Some(r) = guard.as_ref() else { return };
    // Read the framebuffer geometry from the held guard instead of calling
    // `console_max_row`/`console_width`, which would take the lock again.
    let max_row = (r.fb.height / font::FONT_HEIGHT).clamp(1, VGA_HEIGHT);
    let max_col = (r.fb.width / font::FONT_WIDTH).clamp(1, VGA_WIDTH);
    let row = ROW.load(Ordering::SeqCst).min(max_row - 1);
    let col = COL.load(Ordering::SeqCst).min(max_col - 1);
    let x = col * font::FONT_WIDTH;
    let y = row * font::FONT_HEIGHT;
    if visible {
        font::draw_char_on(r, b'_', x, y, current_fg_rgb(), current_bg_rgb());
    } else {
        let bg = current_bg_rgb();
        font::draw_char_on(r, b' ', x, y, bg, bg);
    }
}

/// Enable or disable automatic cursor blinking.
/// When enabled, the blink state toggles on each timer tick.
pub fn set_cursor_blink(enabled: bool) {
    CURSOR_BLINK_ENABLED.store(enabled, Ordering::SeqCst);
    CURSOR_VISIBLE.store(true, Ordering::SeqCst);
    CURSOR_BLINK_COUNTER.store(0, Ordering::SeqCst);
    draw_cursor(true);
}

/// Called from the timer IRQ every tick.
/// Toggles cursor visibility when blink is enabled.
pub fn cursor_timer_tick() {
    if !CURSOR_BLINK_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let cnt = CURSOR_BLINK_COUNTER.fetch_add(1, Ordering::Relaxed);
    if cnt.is_multiple_of(CURSOR_BLINK_INTERVAL) {
        let visible = !CURSOR_VISIBLE.load(Ordering::Relaxed);
        CURSOR_VISIBLE.store(visible, Ordering::Relaxed);
        draw_cursor(visible);
    }
}

/// Check if cursor blink is currently enabled.
pub fn cursor_blink_enabled() -> bool {
    CURSOR_BLINK_ENABLED.load(Ordering::SeqCst)
}

pub fn clear_screen() {
    ROW.store(0, Ordering::SeqCst);
    COL.store(0, Ordering::SeqCst);
    crate::graphics::with_renderer(|r| {
        if let Some(r) = r {
            r.clear(current_bg_rgb());
        }
    });
}

fn console_width() -> usize {
    crate::graphics::with_renderer(|r| match r {
        Some(r) => (r.fb.width / font::FONT_WIDTH).clamp(1, VGA_WIDTH),
        None => VGA_WIDTH,
    })
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
