//! Progress bars and spinner.

use core::sync::atomic::AtomicU32;
use crate::io::{cstr_len, u64_to_str, write_str};

// ── Progress bars ──────────────────────────────

const MAX_BARS: usize = 8;
const BAR_WIDTH: usize = 30;
const TITLE_MAX: usize = 64;
const MSG_MAX: usize = 128;

const BLOCK_FILLED: &[u8; 3] = b"\xe2\x96\x93";
const BLOCK_EMPTY: &[u8; 3]  = b"\xe2\x96\x91";

#[repr(C)]
struct ProgressBar {
    id: i32,
    title: [u8; TITLE_MAX],
    title_len: u8,
    message: [u8; MSG_MAX],
    msg_len: u8,
    current: u64,
    total: u64,
    last_pct: u8,
    active: bool,
}

const fn empty_bar() -> ProgressBar {
    ProgressBar {
        id: 0, title: [0; TITLE_MAX], title_len: 0,
        message: [0; MSG_MAX], msg_len: 0,
        current: 0, total: 0, last_pct: 0xFF, active: false,
    }
}

static mut BARS: [ProgressBar; MAX_BARS] = [
    empty_bar(), empty_bar(), empty_bar(), empty_bar(),
    empty_bar(), empty_bar(), empty_bar(), empty_bar(),
];
static NEXT_BAR_ID: AtomicU32 = AtomicU32::new(1);
static mut PREV_PROGRESS_ROWS: usize = 0;
static mut PROGRESS_DIRTY: bool = false;

fn bars_mut() -> &'static mut [ProgressBar; MAX_BARS] {
    unsafe { &mut *core::ptr::addr_of_mut!(BARS) }
}

fn bars_ref() -> &'static [ProgressBar; MAX_BARS] {
    unsafe { &*core::ptr::addr_of!(BARS) }
}

fn pct_of(current: u64, total: u64) -> u8 {
    if total == 0 { 100 }
    else { core::cmp::min((current as u128 * 100 / total as u128) as u8, 100) }
}

fn format_bar(current: u64, total: u64, buf: &mut [u8]) -> usize {
    let pct = pct_of(current, total);
    let filled = if total == 0 { BAR_WIDTH }
    else { core::cmp::min((current as u128 * BAR_WIDTH as u128 / total as u128) as usize, BAR_WIDTH) };
    let empty = BAR_WIDTH - filled;
    let mut pos = 0usize;
    if pos < buf.len() { buf[pos] = b'['; pos += 1; }
    for _ in 0..filled {
        if pos + 3 <= buf.len() { buf[pos..pos+3].copy_from_slice(BLOCK_FILLED); pos += 3; }
    }
    for _ in 0..empty {
        if pos + 3 <= buf.len() { buf[pos..pos+3].copy_from_slice(BLOCK_EMPTY); pos += 3; }
    }
    if pos < buf.len() { buf[pos] = b']'; pos += 1; }
    if pos < buf.len() { buf[pos] = b' '; pos += 1; }
    let mut pct_buf = [0u8; 4];
    let pct_len = u64_to_str(pct as u64, &mut pct_buf);
    if pos + pct_len < buf.len() {
        buf[pos..pos+pct_len].copy_from_slice(&pct_buf[..pct_len]);
        pos += pct_len;
    }
    if pos < buf.len() { buf[pos] = b'%'; pos += 1; }
    if total != 0 {
        if pos < buf.len() { buf[pos] = b' '; pos += 1; }
        if pos < buf.len() { buf[pos] = b'('; pos += 1; }
        let mut tmp = [0u8; 20];
        let cl = u64_to_str(current, &mut tmp);
        if pos + cl < buf.len() { buf[pos..pos+cl].copy_from_slice(&tmp[..cl]); pos += cl; }
        if pos < buf.len() { buf[pos] = b'/'; pos += 1; }
        let tl = u64_to_str(total, &mut tmp);
        if pos + tl < buf.len() { buf[pos..pos+tl].copy_from_slice(&tmp[..tl]); pos += tl; }
        if pos < buf.len() { buf[pos] = b')'; pos += 1; }
    }
    pos
}

fn progress_render() {
    if unsafe { !PROGRESS_DIRTY } { return; }
    let mut indices = [0usize; MAX_BARS];
    let mut count = 0;
    {
        let bars = bars_ref();
        for (i, bar) in bars.iter().enumerate() {
            if bar.active && bar.id > 0 { indices[count] = i; count += 1; }
        }
    }
    let mut new_rows = 0usize;
    {
        let bars = bars_ref();
        for &idx in indices[..count].iter() {
            new_rows += 2;
            if bars[idx].msg_len > 0 { new_rows += 1; }
        }
    }
    let prev = unsafe { PREV_PROGRESS_ROWS };
    for _ in 0..prev { write_str(b"\x1b[A"); }
    if new_rows < prev {
        let extra = prev - new_rows;
        for _ in 0..extra { write_str(b"\r\x1b[K\r\n"); }
        for _ in 0..extra { write_str(b"\x1b[A"); }
    }
    if count == 0 { unsafe { PREV_PROGRESS_ROWS = 0; PROGRESS_DIRTY = false; } return; }
    let bars = bars_ref();
    for &idx in indices[..count].iter() {
        let bar = &bars[idx];
        write_str(b"\r\x1b[K");
        write_str(&bar.title[..bar.title_len as usize]);
        write_str(b"\r\n");
        write_str(b"\r\x1b[K");
        let mut bar_buf = [0u8; 160];
        let blen = format_bar(bar.current, bar.total, &mut bar_buf);
        write_str(&bar_buf[..blen]);
        write_str(b"\r\n");
        if bar.msg_len > 0 {
            write_str(b"\r\x1b[K");
            write_str(&bar.message[..bar.msg_len as usize]);
            write_str(b"\r\n");
        }
    }
    unsafe { PREV_PROGRESS_ROWS = new_rows; PROGRESS_DIRTY = false; }
    let bars_m = bars_mut();
    for &idx in indices[..count].iter() {
        bars_m[idx].last_pct = pct_of(bars_m[idx].current, bars_m[idx].total);
    }
}

fn progress_mark_dirty() {
    unsafe { PROGRESS_DIRTY = true; }
}

// ── Public API: progress bars ──────────────────

#[no_mangle]
pub unsafe extern "C" fn console_progress_begin(title: *const u8, total: u64) -> i32 {
    let id = {
        let bars = bars_mut();
        let mut slot = None;
        for (i, bar) in bars.iter().enumerate() { if !bar.active { slot = Some(i); break; } }
        let idx = match slot { Some(i) => i, None => return -1 };
        let id = NEXT_BAR_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed) as i32;
        let bar = &mut bars[idx];
        bar.id = id; bar.current = 0; bar.total = total; bar.msg_len = 0; bar.last_pct = 0xFF;
        bar.active = true;
        bar.title_len = {
            let len = core::cmp::min(cstr_len(title, TITLE_MAX - 1), TITLE_MAX - 1);
            unsafe { core::ptr::copy_nonoverlapping(title, bar.title.as_mut_ptr(), len); }
            bar.title[len] = 0; len as u8
        };
        id
    };
    progress_mark_dirty();
    progress_render();
    id
}

#[no_mangle]
pub extern "C" fn console_progress_update(id: i32, current: u64) {
    let mut changed = false;
    for bar in bars_mut().iter_mut() {
        if bar.active && bar.id == id {
            if bar.current != current {
                bar.current = current;
                let new_pct = pct_of(bar.current, bar.total);
                if new_pct != bar.last_pct { changed = true; }
            }
            break;
        }
    }
    if changed { progress_mark_dirty(); progress_render(); }
}

#[no_mangle]
pub extern "C" fn console_progress_finish(id: i32) {
    for bar in bars_mut().iter_mut() {
        if bar.active && bar.id == id { bar.current = bar.total; break; }
    }
    progress_mark_dirty();
    progress_render();
    for bar in bars_mut().iter_mut() { if bar.id == id { bar.active = false; break; } }
}

#[no_mangle]
pub unsafe extern "C" fn console_progress_set_message(id: i32, text: *const u8) {
    for bar in bars_mut().iter_mut() {
        if bar.active && bar.id == id {
            bar.msg_len = {
                let len = core::cmp::min(cstr_len(text, MSG_MAX - 1), MSG_MAX - 1);
                unsafe { core::ptr::copy_nonoverlapping(text, bar.message.as_mut_ptr(), len); }
                bar.message[len] = 0; len as u8
            };
            break;
        }
    }
    progress_mark_dirty();
    progress_render();
}

// ── Spinner ─────────────────────────────────────

const SPINNER_FRAMES: &[u8; 4] = b"|/-\\";

struct Spinner {
    active: bool,
    title: [u8; TITLE_MAX],
    title_len: u8,
    message: [u8; MSG_MAX],
    msg_len: u8,
    frame: u8,
}

static mut SPINNER: Spinner = Spinner {
    active: false,
    title: [0; TITLE_MAX],
    title_len: 0,
    message: [0; MSG_MAX],
    msg_len: 0,
    frame: 0,
};

fn spinner_emit() {
    let s = unsafe { &*core::ptr::addr_of!(SPINNER) };
    let mut buf = [0u8; 256];
    let mut pos = 0usize;
    buf[pos] = b'\r'; pos += 1;
    buf[pos] = 0x1B; pos += 1;
    buf[pos] = b'['; pos += 1;
    buf[pos] = b'K'; pos += 1;
    let title = &s.title[..s.title_len as usize];
    buf[pos..pos + title.len()].copy_from_slice(title);
    pos += title.len();
    if pos < buf.len() { buf[pos] = b' '; pos += 1; }
    let frame_idx = (s.frame as usize) % SPINNER_FRAMES.len();
    buf[pos] = SPINNER_FRAMES[frame_idx]; pos += 1;
    if s.msg_len > 0 {
        if pos < buf.len() { buf[pos] = b' '; pos += 1; }
        let msg = &s.message[..s.msg_len as usize];
        let mlen = core::cmp::min(msg.len(), buf.len().saturating_sub(pos));
        buf[pos..pos + mlen].copy_from_slice(&msg[..mlen]);
        pos += mlen;
    }
    write_str(&buf[..pos]);
}

// ── Public API: spinner ─────────────────────────

#[no_mangle]
pub unsafe extern "C" fn console_spinner_begin(title: *const u8) {
    let s = &mut SPINNER;
    s.active = true;
    s.frame = 0;
    s.msg_len = 0;
    s.title_len = {
        let len = core::cmp::min(cstr_len(title, TITLE_MAX - 1), TITLE_MAX - 1);
        core::ptr::copy_nonoverlapping(title, s.title.as_mut_ptr(), len);
        s.title[len] = 0; len as u8
    };
    spinner_emit();
}

#[no_mangle]
pub extern "C" fn console_spinner_update() {
    let s = unsafe { &mut SPINNER };
    if !s.active { return; }
    s.frame = s.frame.wrapping_add(1);
    spinner_emit();
}

#[no_mangle]
pub extern "C" fn console_spinner_finish() {
    let s = unsafe { &mut SPINNER };
    if !s.active { return; }
    s.active = false;
    let mut buf = [0u8; 4];
    buf[0] = b'\r';
    buf[1] = 0x1B;
    buf[2] = b'[';
    buf[3] = b'K';
    write_str(&buf[..4]);
}
