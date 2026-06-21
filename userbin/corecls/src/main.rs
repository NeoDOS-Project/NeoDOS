#![no_std]
#![no_main]

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

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\x1b[2J\x1b[H");
    syscall::sys_exit(0)
}
