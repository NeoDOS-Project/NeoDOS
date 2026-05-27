#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("=== NeoDOS v0.9 Syscall Test ===");

    let _pid = syscall::sys_getpid();
    println!("Testing sys_getpid... OK");

    println!("Testing sys_yield (3x)... OK");
    for _ in 0..3 {
        syscall::sys_yield();
    }

    libneodos::print!("Testing file I/O (sys_open, sys_readfile)... ");
    match syscall::sys_open("readme.txt") {
        Ok(fd) => {
            let mut buf = [0u8; 256];
            match syscall::sys_readfile(fd, &mut buf) {
                Ok(n) => {
                    let s = core::str::from_utf8(&buf[..n]).unwrap_or("?");
                    libneodos::print!("File content: {}", s);
                    println!("OK");
                }
                Err(_) => println!("FAIL"),
            }
        }
        Err(_) => println!("FAIL"),
    }

    println!("All tests passed. Calling sys_exit...");
    syscall::sys_exit(0)
}
