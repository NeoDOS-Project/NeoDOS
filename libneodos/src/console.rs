//! Console NXL wrapper — readline, history, completion, progress bars.
//!
//! Loaded lazily on first use via `sys_loadlib`.
//! ```ignore
//! let input = console::readline("C:\\> ");
//! ```

use core::sync::atomic::{AtomicU64, Ordering};

const CONSOLE_NXL_PATH: &str = "C:\\System\\Libraries\\console.nxl\0";
const EXPORT_TABLE_OFFSET: u64 = 0x00;

static CONSOLE_BASE: AtomicU64 = AtomicU64::new(0);

/// Completion handler type.
/// Receives (input_text, cursor_pos, candidates_buffer, max_len) and returns
/// number of candidates written (null-separated) into the buffer.
pub type CompletionHandler = extern "C" fn(*const u8, i32, *mut u8, i32) -> i32;

#[repr(C)]
pub struct ConsoleAbiTable {
    pub version: u32,
    pub readline: extern "C" fn(*const u8, *mut u8, i32) -> i32,
    pub read_byte: extern "C" fn() -> i32,
    pub write: extern "C" fn(*const u8, i32) -> i32,
    pub write_line: extern "C" fn(*const u8, i32) -> i32,
    pub set_color: extern "C" fn(u8, u8),
    pub reset_color: extern "C" fn(),
    pub clear_screen: extern "C" fn(),
    pub cursor_home: extern "C" fn(),
    pub history_add: extern "C" fn(*const u8),
    pub history_prev: extern "C" fn() -> *const u8,
    pub history_next: extern "C" fn() -> *const u8,
    pub history_reset: extern "C" fn(),
    pub history_get_count: extern "C" fn() -> i32,
    pub history_get_entry: extern "C" fn(i32) -> *const u8,
    pub completion_register: extern "C" fn(Option<CompletionHandler>),
    pub progress_create: extern "C" fn(*const u8, u64) -> i32,
    pub progress_update: extern "C" fn(i32, u64),
    pub progress_set_message: extern "C" fn(i32, *const u8),
    pub progress_finish: extern "C" fn(i32),
    _reserved: [u64; 8],
}

fn get_table() -> Option<&'static ConsoleAbiTable> {
    let base = CONSOLE_BASE.load(Ordering::Relaxed);
    if base != 0 {
        return Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const ConsoleAbiTable) });
    }
    match crate::loadlib(CONSOLE_NXL_PATH) {
        Ok(base) => {
            CONSOLE_BASE.store(base, Ordering::Relaxed);
            Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const ConsoleAbiTable) })
        }
        Err(_) => None,
    }
}

/// Check whether console.nxl is currently loaded.
pub fn is_loaded() -> bool {
    CONSOLE_BASE.load(Ordering::Relaxed) != 0
}

// ── Input ──────────────────────────────────────

/// Read a line into a buffer with editing support.
/// Returns the length of the input, or 0 / -1 on error.
pub fn readline_buf(prompt: &str, buf: &mut [u8]) -> i32 {
    let table = match get_table() {
        Some(t) => t,
        None => return -1,
    };
    let mut pbuf = [0u8; 128];
    let plen = core::cmp::min(prompt.len(), 127);
    pbuf[..plen].copy_from_slice(&prompt.as_bytes()[..plen]);
    pbuf[plen] = 0;
    let maxlen = buf.len() as i32;
    (table.readline)(pbuf.as_ptr(), buf.as_mut_ptr(), maxlen)
}

/// Read a single key byte from the console.
pub fn read_byte() -> i32 {
    match get_table() {
        Some(t) => (t.read_byte)(),
        None => -1,
    }
}

// ── Output ─────────────────────────────────────

/// Write text to stdout.
pub fn write(text: &[u8]) -> i32 {
    match get_table() {
        Some(t) => (t.write)(text.as_ptr(), text.len() as i32),
        None => -1,
    }
}

/// Write text + CRLF to stdout.
pub fn write_line(text: &[u8]) -> i32 {
    match get_table() {
        Some(t) => (t.write_line)(text.as_ptr(), text.len() as i32),
        None => -1,
    }
}

// ── Colors / Display ───────────────────────────

/// Set foreground and background color (ANSI index 0-15).
pub fn set_color(fg: u8, bg: u8) {
    if let Some(t) = get_table() {
        (t.set_color)(fg, bg);
    }
}

/// Reset colors to defaults.
pub fn reset_color() {
    if let Some(t) = get_table() {
        (t.reset_color)();
    }
}

/// Clear entire screen.
pub fn clear_screen() {
    if let Some(t) = get_table() {
        (t.clear_screen)();
    }
}

/// Move cursor to home position (0,0).
pub fn cursor_home() {
    if let Some(t) = get_table() {
        (t.cursor_home)();
    }
}

// ── History ────────────────────────────────────

/// Add a raw C-string to the history.
pub fn history_add_raw(text: *const u8) {
    if let Some(t) = get_table() {
        (t.history_add)(text);
    }
}

/// Add a command to the history.
pub fn history_add(text: &str) {
    if let Some(t) = get_table() {
        let mut buf = [0u8; 128];
        let len = core::cmp::min(text.len(), 127);
        buf[..len].copy_from_slice(&text.as_bytes()[..len]);
        buf[len] = 0;
        (t.history_add)(buf.as_ptr());
    }
}

/// Get previous history entry (for up-arrow browsing).
/// Returns null pointer when at oldest entry.
pub fn history_prev() -> *const u8 {
    match get_table() {
        Some(t) => (t.history_prev)(),
        None => core::ptr::null(),
    }
}

/// Get next history entry (for down-arrow browsing).
/// Returns null pointer when back at current input.
pub fn history_next() -> *const u8 {
    match get_table() {
        Some(t) => (t.history_next)(),
        None => core::ptr::null(),
    }
}

/// Reset history browse position.
pub fn history_reset() {
    if let Some(t) = get_table() {
        (t.history_reset)();
    }
}

/// Get number of history entries.
pub fn history_count() -> i32 {
    match get_table() {
        Some(t) => (t.history_get_count)(),
        None => 0,
    }
}

/// Get a history entry by index (0 = oldest).
/// Returns null pointer if index is out of range.
pub fn history_entry(index: i32) -> *const u8 {
    match get_table() {
        Some(t) => (t.history_get_entry)(index),
        None => core::ptr::null(),
    }
}

// ── Completion ─────────────────────────────────

/// Register a TAB completion handler.
pub fn register_completion(handler: Option<CompletionHandler>) {
    if let Some(t) = get_table() {
        (t.completion_register)(handler);
    }
}

// ── Progress bars ──────────────────────────────

/// Create a new progress bar.
pub fn progress_create(title: &str, total: u64) -> i32 {
    let table = match get_table() {
        Some(t) => t,
        None => return -1,
    };
    let mut buf = [0u8; 64];
    let len = core::cmp::min(title.len(), 63);
    buf[..len].copy_from_slice(&title.as_bytes()[..len]);
    buf[len] = 0;
    (table.progress_create)(buf.as_ptr(), total)
}

/// Update progress bar position.
pub fn progress_update(id: i32, current: u64) {
    if let Some(t) = get_table() {
        (t.progress_update)(id, current);
    }
}

/// Set or update the status message below the bar.
pub fn progress_set_message(id: i32, text: &str) {
    if let Some(t) = get_table() {
        let mut buf = [0u8; 128];
        let len = core::cmp::min(text.len(), 127);
        buf[..len].copy_from_slice(&text.as_bytes()[..len]);
        buf[len] = 0;
        (t.progress_set_message)(id, buf.as_ptr());
    }
}

/// Mark progress as complete (100%).
pub fn progress_finish(id: i32) {
    if let Some(t) = get_table() {
        (t.progress_finish)(id);
    }
}
