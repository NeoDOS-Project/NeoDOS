#![no_std]

pub mod syscall;
pub mod io;
pub mod fs;
pub mod mem;
pub mod macros;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    syscall::sys_exit(1)
}
