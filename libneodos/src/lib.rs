#![no_std]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

pub mod export;
pub mod syscall;
pub mod io;
pub mod fs;
pub mod mem;
pub mod macros;
pub mod args;
pub mod seh;
pub mod console;
pub mod keyboard;
pub mod i18n;
pub mod res;

// Re-export commonly used syscall helpers for convenience.
pub use syscall::{sys_cm_open_key, sys_cm_query_value, sys_close, sys_ob_open};

/// Load a shared library (NXL) from the filesystem.
/// Returns the base address where the NXL was loaded, which is also
/// the address of the NXL's export table.
pub fn loadlib(path: &str) -> Result<u64, i64> {
    syscall::sys_loadlib(path)
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    (export::get_table().sys_exit)(1)
}
