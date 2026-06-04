#![no_std]

pub mod export;
pub mod syscall;
pub mod io;
pub mod fs;
pub mod mem;
pub mod macros;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    (export::get_table().sys_exit)(1)
}
