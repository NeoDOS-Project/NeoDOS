//! ANSI colour handling, parser state and CSI execution.

use core::sync::atomic::{AtomicU8, AtomicU32, AtomicBool, Ordering};
use crate::graphics::RENDERER;
use crate::font;
use crate::console::{ROW, COL, console_max_row, console_width};

// ── ANSI color table (16 standard VGA/ANSI colors) ─────────────────────────
pub(crate) const ANSI_COLORS: [u32; 16] = [
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
pub(crate) const ANSI_NORMAL: u8 = 0;
pub(crate) const ANSI_ESC: u8 = 1;
const ANSI_CSI: u8 = 2;

pub(crate) static ANSI_STATE: AtomicU8 = AtomicU8::new(ANSI_NORMAL);
// Color encoding: bits [31:24] = mode (0=ANSI 16-color, 1=256-color, 2=truecolor)
//                 bits [23:0]  = value (index for 0/1, RGB for truecolor)
pub(crate) const CM_ANSI: u32 = 0;
pub(crate) const CM_256: u32 = 1;
pub(crate) const CM_TRUECOLOR: u32 = 2;
const CM_SHIFT: u32 = 24;

pub(crate) fn enc_color(mode: u32, val: u32) -> u32 { (mode << CM_SHIFT) | (val & 0x00FF_FFFF) }
pub(crate) fn dec_mode(c: u32) -> u32 { (c >> CM_SHIFT) & 0x3 }
pub(crate) fn dec_val(c: u32) -> u32 { c & 0x00FF_FFFF }

pub(crate) static ANSI_FG: AtomicU32 = AtomicU32::new(7);    // default fg = white (ANSI index 7)
pub(crate) static ANSI_BG: AtomicU32 = AtomicU32::new(0);    // default bg = black (ANSI index 0)
pub(crate) static ANSI_BOLD: AtomicBool = AtomicBool::new(false);

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
pub fn get_fg() -> u32 { ANSI_FG.load(Ordering::Relaxed) }
pub fn get_bg() -> u32 { ANSI_BG.load(Ordering::Relaxed) }
pub fn get_bold() -> bool { ANSI_BOLD.load(Ordering::Relaxed) }

pub(crate) fn xterm_256_to_rgb(index: u8) -> u32 {
    if index < 16 {
        ANSI_COLORS[index as usize]
    } else if index < 232 {
        let i = index - 16;
        let r = (i / 36) % 6;
        let g = (i / 6) % 6;
        let b = i % 6;
        let to_byte = |v: u8| -> u8 { if v == 0 { 0 } else { v * 40 + 55 } };
        (to_byte(r as u8) as u32) << 16
            | (to_byte(g as u8) as u32) << 8
            | to_byte(b as u8) as u32
    } else {
        let s = ((index - 232) * 10 + 8) as u32;
        (s << 16) | (s << 8) | s
    }
}

fn resolve_color(enc: u32, is_fg: bool) -> u32 {
    let mode = dec_mode(enc);
    let val = dec_val(enc) as u8;
    match mode {
        CM_ANSI => {
            if is_fg {
                let bold = ANSI_BOLD.load(Ordering::Relaxed);
                let idx = if bold && val < 8 { val + 8 } else { val };
                ANSI_COLORS[idx as usize]
            } else {
                ANSI_COLORS[val as usize]
            }
        }
        CM_256 => xterm_256_to_rgb(val),
        CM_TRUECOLOR => enc & 0x00FF_FFFF,
        _ => if is_fg { ANSI_COLORS[7] } else { ANSI_COLORS[0] },
    }
}

pub(crate) fn current_fg_rgb() -> u32 {
    resolve_color(ANSI_FG.load(Ordering::Relaxed), true)
}

pub(crate) fn current_bg_rgb() -> u32 {
    resolve_color(ANSI_BG.load(Ordering::Relaxed), false)
}

// ── ANSI escape handler ───────────────────────────────────────────────────
pub(crate) fn handle_ansi_byte(c: u8) {
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
                    ANSI_FG.store(7, Ordering::Relaxed);
                    ANSI_BG.store(0, Ordering::Relaxed);
                    ANSI_BOLD.store(false, Ordering::Relaxed);
                }
                let mut i = 0;
                while i < count {
                    let p = params[i];
                    match p {
                        0 => {
                            ANSI_FG.store(enc_color(CM_ANSI, 7), Ordering::Relaxed);
                            ANSI_BG.store(enc_color(CM_ANSI, 0), Ordering::Relaxed);
                            ANSI_BOLD.store(false, Ordering::Relaxed);
                        }
                        1 => { ANSI_BOLD.store(true, Ordering::Relaxed); }
                        22 => { ANSI_BOLD.store(false, Ordering::Relaxed); }
                        30..=37 => { ANSI_FG.store(enc_color(CM_ANSI, (p - 30) as u32), Ordering::Relaxed); }
                        38 => {
                            i += 1;
                            if i < count && params[i] == 5 && i + 1 < count {
                                i += 1;
                                ANSI_FG.store(enc_color(CM_256, params[i] as u32), Ordering::Relaxed);
                            } else if i < count && params[i] == 2 && i + 3 < count {
                                let r = params[i + 1] as u8;
                                let g = params[i + 2] as u8;
                                let b = params[i + 3] as u8;
                                ANSI_FG.store(enc_color(CM_TRUECOLOR,
                                    (r as u32) << 16 | (g as u32) << 8 | b as u32), Ordering::Relaxed);
                                i += 3;
                            }
                        }
                        39 => { ANSI_FG.store(enc_color(CM_ANSI, 7), Ordering::Relaxed); }
                        40..=47 => { ANSI_BG.store(enc_color(CM_ANSI, (p - 40) as u32), Ordering::Relaxed); }
                        48 => {
                            i += 1;
                            if i < count && params[i] == 5 && i + 1 < count {
                                i += 1;
                                ANSI_BG.store(enc_color(CM_256, params[i] as u32), Ordering::Relaxed);
                            } else if i < count && params[i] == 2 && i + 3 < count {
                                let r = params[i + 1] as u8;
                                let g = params[i + 2] as u8;
                                let b = params[i + 3] as u8;
                                ANSI_BG.store(enc_color(CM_TRUECOLOR,
                                    (r as u32) << 16 | (g as u32) << 8 | b as u32), Ordering::Relaxed);
                                i += 3;
                            }
                        }
                        49 => { ANSI_BG.store(enc_color(CM_ANSI, 0), Ordering::Relaxed); }
                        90..=97 => { ANSI_FG.store(enc_color(CM_ANSI, (p - 90 + 8) as u32), Ordering::Relaxed); }
                        100..=107 => { ANSI_BG.store(enc_color(CM_ANSI, (p - 100 + 8) as u32), Ordering::Relaxed); }
                        _ => {}
                    }
                    i += 1;
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
