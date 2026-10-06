#![no_std]
#![no_main]
#![allow(clippy::missing_safety_doc)]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use core::arch::asm;

// ── Constants ──────────────────────────────────

pub(crate) const INPUT_MAX: usize = 256;
pub(crate) const HISTORY_MAX: usize = 32;
pub(crate) const HISTORY_LINE_MAX: usize = 128;

mod io;
mod history;
mod readline;
mod progress;

use io::*;
use history::*;
use readline::*;
use progress::*;

// ── Export table ───────────────────────────────

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
    pub completion_register: extern "C" fn(Option<CompletionFn>),
    pub progress_begin: unsafe extern "C" fn(*const u8, u64) -> i32,
    pub progress_update: extern "C" fn(i32, u64),
    pub progress_set_message: unsafe extern "C" fn(i32, *const u8),
    pub progress_finish: extern "C" fn(i32),
    pub spinner_begin: unsafe extern "C" fn(*const u8),
    pub spinner_update: extern "C" fn(),
    pub spinner_finish: extern "C" fn(),
    pub set_color_256: extern "C" fn(u8, u8),
    pub set_truecolor: extern "C" fn(u8, u8, u8, u8, u8, u8),
    _reserved: [u64; 4],
}

#[no_mangle]
#[link_section = ".export_table"]
pub static CONSOLE_EXPORT_TABLE: ConsoleAbiTable = ConsoleAbiTable {
    version: 3,
    readline: console_readline,
    read_byte: console_read_byte,
    write: console_write,
    write_line: console_write_line,
    set_color: console_set_color,
    reset_color: console_reset_color,
    clear_screen: console_clear_screen,
    cursor_home: console_cursor_home,
    history_add,
    history_prev,
    history_next,
    history_reset,
    history_get_count,
    history_get_entry,
    completion_register,
    progress_begin: console_progress_begin,
    progress_update: console_progress_update,
    progress_set_message: console_progress_set_message,
    progress_finish: console_progress_finish,
    spinner_begin: console_spinner_begin,
    spinner_update: console_spinner_update,
    spinner_finish: console_spinner_finish,
    set_color_256: console_set_color_256,
    set_truecolor: console_set_truecolor,
    _reserved: [0; 4],
};

// ── NXL boilerplate ────────────────────────────

#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { asm!("hlt"); } }
}

#[panic_handler]
fn nxl_panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("hlt"); } }
}
