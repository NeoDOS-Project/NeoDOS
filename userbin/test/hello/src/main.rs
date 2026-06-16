#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello from Ring 3! (NeoDOS v0.7)");

    let _pid = syscall::sys_getpid();
    println!("sys_getpid returned successfully.");

    for _ in 0..3 {
        syscall::sys_yield();
    }

    println!("Goodbye from user space! Calling sys_exit...");
    syscall::sys_exit(0)
}
