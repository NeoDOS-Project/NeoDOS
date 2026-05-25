#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello from libneodos! (Ring 3)");

    let pid = syscall::sys_getpid();
    println!("PID: {}", pid);

    syscall::sys_yield();
    println!("After yield");

    if let Ok(file) = libneodos::fs::File::open("C:\\README.TXT") {
        let mut buf = [0u8; 256];
        if let Ok(n) = file.read(&mut buf) {
            println!("Read {} bytes from README.TXT", n);
        }
    }

    syscall::sys_exit(0)
}
