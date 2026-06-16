#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let pid = syscall::sys_getpid();
    println!("CPUTEST[{}]: inicio", pid);

    let mut i: u64 = 0;
    while i < 100 {
        i += 1;
        if i % 10 == 0 {
            println!("CPUTEST[{}]: iter={}", pid, i);
            syscall::sys_yield();
        }
    }

    println!("CPUTEST[{}]: fin", pid);
    syscall::sys_exit(0)
}