#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

const PROGRESS_MASK: u64 = 0x3FFFF; // ~262k iterations between prints
const MAX_COUNT: u64 = 5_000_000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let pid = syscall::sys_getpid();
    println!("CPUTEST: PID={} starting, max={}", pid, MAX_COUNT);

    let mut count: u64 = 0;
    while count < MAX_COUNT {
        count += 1;
        if count & PROGRESS_MASK == 0 {
            println!("CPUTEST: PID={} count={}", pid, count);
            syscall::sys_yield();
        }
    }

    println!("CPUTEST: PID={} done (count={})", pid, count);
    syscall::sys_exit(0)
}
