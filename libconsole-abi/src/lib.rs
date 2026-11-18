//! `console.nxl` export table — the single declaration of the ABI layout.
//!
//! `libconsole-nxl` (the producer) and `libneodos::console` (the consumer) used
//! to keep byte-for-byte copies of this struct. Adding a field on one side only
//! shifted every function pointer after it, so a call landed on the wrong
//! function with no compile error. Both sides now import this definition, and
//! the consumer refuses a table whose [`CONSOLE_ABI_VERSION`] it does not know.
//!
//! The layout is version-first: `version: u32` followed by four padding bytes,
//! then the function pointers, in the order below. Bump the version and append
//! new pointers at the end (`_reserved` absorbs small additions first) — never
//! insert or reorder.

#![no_std]

/// ABI revision of [`ConsoleAbiTable`].
pub const CONSOLE_ABI_VERSION: u32 = 3;

/// Completion handler type.
/// Receives (input_text, cursor_pos, candidates_buffer, max_len) and returns
/// the number of candidates written (null-separated) into the buffer.
pub type CompletionHandler = extern "C" fn(*const u8, i32, *mut u8, i32) -> i32;

/// Export table published by `console.nxl` at the base of its NXL image.
#[repr(C)]
pub struct ConsoleAbiTable {
    pub version: u32,
    pub readline: unsafe extern "C" fn(*const u8, *mut u8, i32) -> i32,
    pub read_byte: extern "C" fn() -> i32,
    pub write: unsafe extern "C" fn(*const u8, i32) -> i32,
    pub write_line: unsafe extern "C" fn(*const u8, i32) -> i32,
    pub set_color: extern "C" fn(u8, u8),
    pub reset_color: extern "C" fn(),
    pub clear_screen: extern "C" fn(),
    pub cursor_home: extern "C" fn(),
    pub history_add: unsafe extern "C" fn(*const u8),
    pub history_prev: extern "C" fn() -> *const u8,
    pub history_next: extern "C" fn() -> *const u8,
    pub history_reset: extern "C" fn(),
    pub history_get_count: extern "C" fn() -> i32,
    pub history_get_entry: extern "C" fn(i32) -> *const u8,
    pub completion_register: extern "C" fn(Option<CompletionHandler>),
    pub progress_begin: unsafe extern "C" fn(*const u8, u64) -> i32,
    pub progress_update: extern "C" fn(i32, u64),
    pub progress_set_message: unsafe extern "C" fn(i32, *const u8),
    pub progress_finish: extern "C" fn(i32),
    pub spinner_begin: unsafe extern "C" fn(*const u8),
    pub spinner_update: extern "C" fn(),
    pub spinner_finish: extern "C" fn(),
    pub set_color_256: extern "C" fn(u8, u8),
    pub set_truecolor: extern "C" fn(u8, u8, u8, u8, u8, u8),
    pub _reserved: [u64; 4],
}
