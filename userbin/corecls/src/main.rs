#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[used]
#[link_section = ".rodata"]
static CLS_HELP: &[u8] = b"::HELP::\
CLS\r\n\
  Clear the screen.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nCLS\r\n  Clear the screen.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }
    write_str(b"\x1b[2J\x1b[H");
    syscall::sys_exit(0)
}
