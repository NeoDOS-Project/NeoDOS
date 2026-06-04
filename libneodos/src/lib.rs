#![no_std]

pub mod export;
pub mod syscall;
pub mod io;
pub mod fs;
pub mod mem;
pub mod macros;

/// Load a shared library (DLL) from the filesystem.
/// Returns the base address where the DLL was loaded, which is also
/// the address of the DLL's export table.
pub fn loadlib(path: &str) -> Result<u64, i64> {
    syscall::sys_loadlib(path)
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    (export::get_table().sys_exit)(1)
}
