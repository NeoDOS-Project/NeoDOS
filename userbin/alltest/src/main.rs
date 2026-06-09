#![no_std]
#![no_main]

use libneodos::println;
use libneodos::syscall;

fn check<T, E: core::fmt::Debug>(result: Result<T, E>, ok_msg: &str, fail_msg: &str) -> T {
    match result {
        Ok(val) => {
            println!("{}", ok_msg);
            val
        }
        Err(e) => {
            println!("{} (err: {:?})", fail_msg, e);
            syscall::sys_exit(1)
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("=== ALL Syscall Test ===");

    // sys_yield 3x
    for _ in 0..3 {
        syscall::sys_yield();
    }
    println!("sys_yield: OK");

    // sys_getpid
    let pid = syscall::sys_getpid();
    if pid != 0 {
        println!("sys_getpid: OK");
    } else {
        println!("sys_getpid: FAIL");
        syscall::sys_exit(1);
    }

    // sys_open
    let fd = check(
        syscall::sys_open("C:\\HELLO.NXE"),
        "sys_open: OK",
        "sys_open: FAIL",
    );

    // sys_readfile
    let mut buf = [0u8; 16];
    check(
        syscall::sys_readfile(fd, &mut buf),
        "sys_readfile: OK",
        "sys_readfile: FAIL",
    );

    // sys_close
    let _ = syscall::sys_close(0);
    println!("sys_close: OK");

    // sys_chdir
    check(
        syscall::sys_chdir("C:\\"),
        "sys_chdir: OK",
        "sys_chdir: FAIL",
    );

    // sys_getcwd
    let mut cwd_buf = [0u8; 64];
    check(
        syscall::sys_getcwd(&mut cwd_buf),
        "sys_getcwd: OK",
        "sys_getcwd: FAIL",
    );

    // sys_brk
    match syscall::sys_brk(0) {
        Ok(current) => match syscall::sys_brk(current + 4096) {
            Ok(_) => {
                let ptr = (current + 4095) as *mut u8;
                unsafe {
                    *ptr = 0x42;
                }
                println!("sys_brk: OK");
            }
            Err(_) => {
                println!("sys_brk: FAIL");
                syscall::sys_exit(1);
            }
        },
        Err(_) => {
            println!("sys_brk: FAIL");
            syscall::sys_exit(1);
        }
    }

    println!("ALL_TESTS_PASSED");
    syscall::sys_exit(0)
}
