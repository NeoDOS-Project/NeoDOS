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
use libconsole_abi::ConsoleAbiTable;
use history::*;
use readline::*;
use progress::*;

// ── Export table ───────────────────────────────

#[no_mangle]
#[link_section = ".export_table"]
pub static CONSOLE_EXPORT_TABLE: ConsoleAbiTable = ConsoleAbiTable {
    version: libconsole_abi::CONSOLE_ABI_VERSION,
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
