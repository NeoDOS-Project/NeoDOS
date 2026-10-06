//! Command history state and API.

use crate::io::cstr_len;
use crate::{INPUT_MAX, HISTORY_MAX, HISTORY_LINE_MAX};

// ── History state ──────────────────────────────

pub(crate) struct History {
    pub(crate) entries: [[u8; HISTORY_LINE_MAX]; HISTORY_MAX],
    pub(crate) count: u16,
    pub(crate) browse_pos: i16,
    pub(crate) head: u16,
    pub(crate) pending: [u8; INPUT_MAX],
    pub(crate) pending_len: u16,
}

pub(crate) static mut HIST: History = History {
    entries: [[0; HISTORY_LINE_MAX]; HISTORY_MAX],
    count: 0,
    browse_pos: -1,
    head: 0,
    pending: [0; INPUT_MAX],
    pending_len: 0,
};

pub(crate) fn entry_index(logical: u16) -> usize {
    unsafe {
        if HIST.count < HISTORY_MAX as u16 {
            logical as usize
        } else {
            ((HIST.head + 1 + logical) % HISTORY_MAX as u16) as usize
        }
    }
}
// ── History API ────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn history_add(text: *const u8) {
    let len = core::cmp::min(cstr_len(text, HISTORY_LINE_MAX - 1), HISTORY_LINE_MAX - 1);
    if len == 0 { return; }
    unsafe {
        if HIST.count > 0 {
            let last = &HIST.entries[HIST.head as usize];
            let same = (0..len).all(|i| i < HISTORY_LINE_MAX && last[i] == *text.add(i)) && last[len] == 0;
            if same { return; }
        }
        let idx = (HIST.head as usize + 1) % HISTORY_MAX;
        let entry = &mut HIST.entries[idx];
        entry.fill(0);
        core::ptr::copy_nonoverlapping(text, entry.as_mut_ptr(), len);
        HIST.head = idx as u16;
        if HIST.count < HISTORY_MAX as u16 { HIST.count += 1; }
        HIST.browse_pos = -1;
    }
}

#[no_mangle]
pub extern "C" fn history_prev() -> *const u8 {
    unsafe {
        if HIST.count == 0 { return core::ptr::null(); }
        if HIST.browse_pos == -1 {
            HIST.browse_pos = (HIST.count - 1) as i16;
        } else if HIST.browse_pos > 0 {
            HIST.browse_pos -= 1;
        } else { return core::ptr::null(); }
        let idx = entry_index(HIST.browse_pos as u16);
        HIST.entries[idx].as_ptr()
    }
}

#[no_mangle]
pub extern "C" fn history_next() -> *const u8 {
    unsafe {
        if HIST.count == 0 || HIST.browse_pos == -1 { return core::ptr::null(); }
        if HIST.browse_pos < (HIST.count - 1) as i16 {
            HIST.browse_pos += 1;
            let idx = entry_index(HIST.browse_pos as u16);
            HIST.entries[idx].as_ptr()
        } else {
            HIST.browse_pos = -1;
            core::ptr::null()
        }
    }
}

#[no_mangle]
pub extern "C" fn history_reset() {
    unsafe { HIST.browse_pos = -1; }
}

#[no_mangle]
pub extern "C" fn history_get_count() -> i32 {
    unsafe { HIST.count as i32 }
}

#[no_mangle]
pub extern "C" fn history_get_entry(idx: i32) -> *const u8 {
    unsafe {
        if idx < 0 || idx >= HIST.count as i32 { return core::ptr::null(); }
        let i = entry_index(idx as u16);
        HIST.entries[i].as_ptr()
    }
}
