#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let pid = syscall::sys_getpid();
    println!("CPUTEST: PID={} starting...", pid);

    let mut count: u64 = 0;
    loop {
        count += 1;
        if count % 50000000 == 0 {
            println!("CPUTEST: PID={} count={}", pid, count);
        }
        if count >= 200000000 {
            break;
        }
    }

    println!("CPUTEST: PID={} done (count={})", pid, count);
    syscall::sys_exit(0)
}
