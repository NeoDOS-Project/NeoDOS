#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("=== NeoDOS File I/O Test ===");

    match syscall::sys_open("C:\\readme.txt") {
        Ok(fd) => {
            let data = b"Hola FILETEST!";
            let _ = syscall::sys_writefile(fd, data);
            println!("sys_write: OK");

            let mut buf = [0u8; 64];
            let _ = syscall::sys_readfile(fd, &mut buf);
            println!("sys_read: OK");
        }
        Err(_) => {}
    }

    println!("File test complete!");
    syscall::sys_exit(0)
}
