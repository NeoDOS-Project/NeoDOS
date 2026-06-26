// src/console.rs
// ANSI-capable console driver: parses escape sequences, renders with 16-color palette

use core::fmt::{Write, Result, Arguments};
use crate::graphics::RENDERER;
use crate::font;

const VGA_WIDTH: usize = 160;
const VGA_HEIGHT: usize = 50;

use core::sync::atomic::{AtomicUsize, AtomicU8, AtomicBool, Ordering};

static ROW: AtomicUsize = AtomicUsize::new(0);
static COL: AtomicUsize = AtomicUsize::new(0);

// ── Cursor blink ──
static CURSOR_VISIBLE: AtomicBool = AtomicBool::new(true);
static CURSOR_BLINK_ENABLED: AtomicBool = AtomicBool::new(false);
static CURSOR_BLINK_COUNTER: AtomicUsize = AtomicUsize::new(0);
const CURSOR_BLINK_INTERVAL: usize = 18; // ticks (~every 18ms at 1KHz = ~55 Hz)

// ── ANSI color table (16 standard VGA/ANSI colors) ─────────────────────────
const ANSI_COLORS: [u32; 16] = [
    0x000000, //  0: Black
    0xAA0000, //  1: Red
    0x00AA00, //  2: Green
    0xAA5500, //  3: Brown
    0x0000AA, //  4: Blue
    0xAA00AA, //  5: Magenta
    0x00AAAA, //  6: Cyan
    0xAAAAAA, //  7: White (light gray)
    0x555555, //  8: Bright Black (gray)
    0xFF5555, //  9: Bright Red
    0x55FF55, // 10: Bright Green
    0xFFFF55, // 11: Bright Yellow
    0x5555FF, // 12: Bright Blue
    0xFF55FF, // 13: Bright Magenta
    0x55FFFF, // 14: Bright Cyan
    0xFFFFFF, // 15: Bright White
];

// ── ANSI parser state machine ─────────────────────────────────────────────
const ANSI_NORMAL: u8 = 0;
const ANSI_ESC: u8 = 1;
const ANSI_CSI: u8 = 2;

static ANSI_STATE: AtomicU8 = AtomicU8::new(ANSI_NORMAL);
static ANSI_FG: AtomicU8 = AtomicU8::new(7);    // default fg = white
static ANSI_BG: AtomicU8 = AtomicU8::new(0);    // default bg = black
static ANSI_BOLD: AtomicBool = AtomicBool::new(false);

// Parser transient state — only accessed when ANSI_STATE == CSI.
// Safe with same re-entrancy caveat as ROW/COL (interrupt handling could
// corrupt mid-sequence parsing; in practice ANSI sequences are short and
// interrupt handlers don't inject partial escape codes).
static mut ANSI_CSI_PARAM: u16 = 0;
static mut ANSI_CSI_PARAMS: [u16; 8] = [0; 8];
static mut ANSI_CSI_COUNT: usize = 0;
static mut ANSI_CSI_HAS_DIGIT: bool = false;

// ── Public helpers for tests ──────────────────────────────────────────────
pub fn get_row() -> usize { ROW.load(Ordering::SeqCst) }
pub fn get_col() -> usize { COL.load(Ordering::SeqCst) }
pub fn get_fg() -> u8 { ANSI_FG.load(Ordering::Relaxed) }
pub fn get_bg() -> u8 { ANSI_BG.load(Ordering::Relaxed) }
pub fn get_bold() -> bool { ANSI_BOLD.load(Ordering::Relaxed) }

fn current_fg_rgb() -> u32 {
    let idx = ANSI_FG.load(Ordering::Relaxed);
    let bold = ANSI_BOLD.load(Ordering::Relaxed);
    let actual = if bold && idx < 8 { idx + 8 } else { idx };
    ANSI_COLORS[actual as usize]
}

fn current_bg_rgb() -> u32 {
    ANSI_COLORS[ANSI_BG.load(Ordering::Relaxed) as usize]
}

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
    pub fg: u8,
    pub bg: u8,
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
    if let Some(ref r) = *RENDERER.lock() { r.clear(ANSI_COLORS[0]); }
    for row in 0..console_max_row().min(crate::input::vt::VT_CONSOLE_ROWS) {
        for col in 0..console_width().min(crate::input::vt::VT_CONSOLE_COLS) {
            let ch = shadow.chars[row][col];
            if ch != 0 {
                font::draw_char(ch, col * font::FONT_WIDTH, row * font::FONT_HEIGHT, ANSI_COLORS[7], ANSI_COLORS[0]);
            }
        }
    }
}

fn console_max_row() -> usize {
    if let Some(ref r) = *RENDERER.lock() {
        let max_rows = r.fb.height / font::FONT_HEIGHT;
        max_rows.min(VGA_HEIGHT).max(1)
    } else {
        VGA_HEIGHT
    }
}

// ── ANSI escape handler ───────────────────────────────────────────────────
fn handle_ansi_byte(c: u8) {
    let state = ANSI_STATE.load(Ordering::Relaxed);
    match state {
        ANSI_ESC => {
            if c == b'[' {
                // Enter CSI — reset parameter accumulators
                ANSI_STATE.store(ANSI_CSI, Ordering::Relaxed);
                unsafe {
                    ANSI_CSI_PARAM = 0;
                    ANSI_CSI_COUNT = 0;
                    ANSI_CSI_HAS_DIGIT = false;
                }
            } else {
                // Not a CSI sequence; abort back to normal
                ANSI_STATE.store(ANSI_NORMAL, Ordering::Relaxed);
            }
        }
        ANSI_CSI => {
            match c {
                b'0'..=b'9' => {
                    unsafe {
                        ANSI_CSI_PARAM = ANSI_CSI_PARAM * 10 + (c - b'0') as u16;
                        ANSI_CSI_HAS_DIGIT = true;
                    }
                }
                b';' => {
                    unsafe {
                        if ANSI_CSI_COUNT < 8 {
                            ANSI_CSI_PARAMS[ANSI_CSI_COUNT] = ANSI_CSI_PARAM;
                            ANSI_CSI_COUNT += 1;
                        }
                        ANSI_CSI_PARAM = 0;
                        ANSI_CSI_HAS_DIGIT = false;
                    }
                }
                _ => {
                    // Command byte — finalise params and execute
                    unsafe {
                        if ANSI_CSI_HAS_DIGIT && ANSI_CSI_COUNT < 8 {
                            ANSI_CSI_PARAMS[ANSI_CSI_COUNT] = ANSI_CSI_PARAM;
                            ANSI_CSI_COUNT += 1;
                        } else if !ANSI_CSI_HAS_DIGIT && ANSI_CSI_COUNT == 0 {
                            // No params at all (e.g., ESC[H)
                        } else if !ANSI_CSI_HAS_DIGIT {
                            // Last was ';' — an empty trailing param counts as 0
                        }
                    }
                    execute_ansi_csi(c);
                    ANSI_STATE.store(ANSI_NORMAL, Ordering::Relaxed);
                }
            }
        }
        _ => {
            ANSI_STATE.store(ANSI_NORMAL, Ordering::Relaxed);
        }
    }
}

fn execute_ansi_csi(cmd: u8) {
    unsafe {
        let count = ANSI_CSI_COUNT;
        let params = &ANSI_CSI_PARAMS[..count];

        match cmd {
            b'm' => { // SGR — Select Graphic Rendition
                if count == 0 {
                    // No params = reset (ESC[m ≣ ESC[0m)
                    ANSI_FG.store(7, Ordering::Relaxed);
                    ANSI_BG.store(0, Ordering::Relaxed);
                    ANSI_BOLD.store(false, Ordering::Relaxed);
                }
                for &p in params {
                    match p {
                        0 => {
                            ANSI_FG.store(7, Ordering::Relaxed);
                            ANSI_BG.store(0, Ordering::Relaxed);
                            ANSI_BOLD.store(false, Ordering::Relaxed);
                        }
                        1 => { ANSI_BOLD.store(true, Ordering::Relaxed); }
                        22 => { ANSI_BOLD.store(false, Ordering::Relaxed); }
                        30..=37 => { ANSI_FG.store((p - 30) as u8, Ordering::Relaxed); }
                        38 => {} // extended fg — not implemented
                        39 => { ANSI_FG.store(7, Ordering::Relaxed); }
                        40..=47 => { ANSI_BG.store((p - 40) as u8, Ordering::Relaxed); }
                        48 => {} // extended bg — not implemented
                        49 => { ANSI_BG.store(0, Ordering::Relaxed); }
                        90..=97 => { ANSI_FG.store((p - 90 + 8) as u8, Ordering::Relaxed); }
                        100..=107 => { ANSI_BG.store((p - 100 + 8) as u8, Ordering::Relaxed); }
                        _ => {}
                    }
                }
            }

            b'H' | b'f' => { // CUP / HVP — cursor position
                let row = if count >= 1 { (params[0].max(1) - 1) as usize } else { 0 };
                let col = if count >= 2 { (params[1].max(1) - 1) as usize } else { 0 };
                ROW.store(row, Ordering::SeqCst);
                COL.store(col, Ordering::SeqCst);
            }

            b'J' => { // ED — erase in display
                let mode = if count >= 1 { params[0] } else { 0 };
                if mode == 2 {
                    // Clear entire screen
                    ROW.store(0, Ordering::SeqCst);
                    COL.store(0, Ordering::SeqCst);
                    if let Some(ref r) = *RENDERER.lock() {
                        r.clear(current_bg_rgb());
                    }
                }
                // mode 0 and 1 are not implemented
            }

            b'A' => { // CUU — cursor up
                let n = if count >= 1 { params[0].max(1) as usize } else { 1 };
                let row = ROW.load(Ordering::SeqCst);
                ROW.store(row.saturating_sub(n), Ordering::SeqCst);
            }

            b'B' => { // CUD — cursor down
                let n = if count >= 1 { params[0].max(1) as usize } else { 1 };
                let row = ROW.load(Ordering::SeqCst);
                let max_row = console_max_row();
                ROW.store(core::cmp::min(row.saturating_add(n), max_row - 1), Ordering::SeqCst);
            }

            b'C' => { // CUF — cursor right
                let n = if count >= 1 { params[0].max(1) as usize } else { 1 };
                let col = COL.load(Ordering::SeqCst);
                let max_col = console_width();
                COL.store(core::cmp::min(col.saturating_add(n), max_col - 1), Ordering::SeqCst);
            }

            b'D' => { // CUB — cursor left
                let n = if count >= 1 { params[0].max(1) as usize } else { 1 };
                let col = COL.load(Ordering::SeqCst);
                COL.store(col.saturating_sub(n), Ordering::SeqCst);
            }

            b'G' => { // CHA — cursor horizontal absolute
                let col = if count >= 1 { params[0].max(1).saturating_sub(1) as usize } else { 0 };
                let max_col = console_width();
                COL.store(core::cmp::min(col, max_col - 1), Ordering::SeqCst);
            }

            b'K' => { // EL — erase in line
                let mode = if count >= 1 { params[0] } else { 0 };
                if mode == 0 || mode == 2 {
                    let row = ROW.load(Ordering::SeqCst).min(console_max_row() - 1);
                    let start_col = if mode == 0 { COL.load(Ordering::SeqCst) } else { 0 };
                    let max_col = console_width();
                    let bg = current_bg_rgb();
                    for c in start_col..max_col {
                        let x = c * font::FONT_WIDTH;
                        let y = row * font::FONT_HEIGHT;
                        font::draw_char(b' ', x, y, bg, bg);
                    }
                }
            }

            _ => {} // unsupported — silently ignore
        }

        // Reset parser accumulators
        ANSI_CSI_PARAM = 0;
        ANSI_CSI_PARAMS = [0; 8];
        ANSI_CSI_COUNT = 0;
        ANSI_CSI_HAS_DIGIT = false;
    }
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
            if col > 0 { col -= 1; }
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
    if let Some(ref r) = *RENDERER.lock() {
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
    }
}

pub fn print_str(s: &str) {
    for c in s.chars() {
        write_codepoint(c as u32);
    }
    crate::serial_print!("{}", s);
}

pub fn draw_cursor(visible: bool) {
    let max_row = console_max_row();
    let max_col = console_width();
    let r = ROW.load(Ordering::SeqCst).min(max_row - 1);
    let c = COL.load(Ordering::SeqCst).min(max_col - 1);
    let fg = current_fg_rgb();
    let bg = current_bg_rgb();
    if visible {
        font::draw_char(b'_', c * font::FONT_WIDTH, r * font::FONT_HEIGHT, fg, bg);
    } else {
        font::draw_char(b' ', c * font::FONT_WIDTH, r * font::FONT_HEIGHT, bg, bg);
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
    if cnt % CURSOR_BLINK_INTERVAL == 0 {
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
    if let Some(ref r) = *RENDERER.lock() {
        r.clear(current_bg_rgb());
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

// ── ANSI tests ────────────────────────────────────────────────────────────
pub fn register_ansi_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("ansi_color_foreground", {
        let saved_fg = ANSI_FG.load(Ordering::Relaxed);
        let saved_bg = ANSI_BG.load(Ordering::Relaxed);
        let saved_bold = ANSI_BOLD.load(Ordering::Relaxed);

        // ESC[31m — red foreground
        print_str("\x1b[31m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 1);

        // ESC[41m — red background
        print_str("\x1b[41m");
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 1);

        // ESC[1m — bold
        print_str("\x1b[1m");
        test_true!(ANSI_BOLD.load(Ordering::Relaxed));

        // ESC[0m — reset
        print_str("\x1b[0m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 7);
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 0);
        test_eq!(ANSI_BOLD.load(Ordering::Relaxed), false);

        // ESC[91m — bright red foreground
        print_str("\x1b[91m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 9);

        // ESC[107m — bright white background
        print_str("\x1b[107m");
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 15);

        // ESC[1;32m — bold + green
        print_str("\x1b[0m");
        print_str("\x1b[1;32m");
        test_true!(ANSI_BOLD.load(Ordering::Relaxed));
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 2);

        // ESC[39m — default fg
        print_str("\x1b[39m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 7);

        // ESC[49m — default bg
        print_str("\x1b[49m");
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 0);

        // Restore
        ANSI_FG.store(saved_fg, Ordering::Relaxed);
        ANSI_BG.store(saved_bg, Ordering::Relaxed);
        ANSI_BOLD.store(saved_bold, Ordering::Relaxed);
    });

    test_case!("ansi_cursor_position", {
        let saved_row = ROW.load(Ordering::SeqCst);
        let saved_col = COL.load(Ordering::SeqCst);

        // ESC[10;20H — cursor to row 10, col 20
        print_str("\x1b[10;20H");
        test_eq!(ROW.load(Ordering::SeqCst), 9);
        test_eq!(COL.load(Ordering::SeqCst), 19);

        // ESC[H — cursor home
        print_str("\x1b[H");
        test_eq!(ROW.load(Ordering::SeqCst), 0);
        test_eq!(COL.load(Ordering::SeqCst), 0);

        // ESC[1;1H — explicit home
        print_str("\x1b[1;1H");
        test_eq!(ROW.load(Ordering::SeqCst), 0);
        test_eq!(COL.load(Ordering::SeqCst), 0);

        // ESC[f — alternative home
        print_str("\x1b[5;15f");
        test_eq!(ROW.load(Ordering::SeqCst), 4);
        test_eq!(COL.load(Ordering::SeqCst), 14);

        // Restore
        ROW.store(saved_row, Ordering::SeqCst);
        COL.store(saved_col, Ordering::SeqCst);
    });

    test_case!("ansi_clear_screen", {
        let saved_row = ROW.load(Ordering::SeqCst);
        let saved_col = COL.load(Ordering::SeqCst);

        // Move cursor to known position
        print_str("\x1b[5;10H");
        test_eq!(ROW.load(Ordering::SeqCst), 4);
        test_eq!(COL.load(Ordering::SeqCst), 9);

        // ESC[2J — clear entire screen (resets cursor to home)
        print_str("\x1b[2J");
        test_eq!(ROW.load(Ordering::SeqCst), 0);
        test_eq!(COL.load(Ordering::SeqCst), 0);

        // Restore
        ROW.store(saved_row, Ordering::SeqCst);
        COL.store(saved_col, Ordering::SeqCst);
    });
}
