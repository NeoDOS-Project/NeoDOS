#![no_std]
#![no_main]

use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[used]
#[link_section = ".rodata"]
static POWEROFF_HELP: &[u8] = b"::HELP::\
POWEROFF\r\n\
  Power off the system.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nPOWEROFF\r\n  Power off the system.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }
    write_str(b"\r\npowering off...\r\n");
    syscall::sys_poweroff()
}
