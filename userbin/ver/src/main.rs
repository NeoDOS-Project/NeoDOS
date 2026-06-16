#![no_std]
#![no_main]

use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[used]
#[link_section = ".rodata"]
static VER_HELP: &[u8] = b"::HELP::\
VER\r\n\
  Shows the NeoDOS kernel version.\r\n\
::END::";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 128];
    match syscall::sys_get_version(&mut buf) {
        Ok(n) => {
            let len = n.min(buf.len());
            write_str(b"\r\n");
            write_str(&buf[..len]);
            write_str(b"\r\n\r\n");
        }
        Err(_) => {
            write_str(b"\r\nNeoDOS Kernel\r\n\r\n");
        }
    }
    syscall::sys_exit(0)
}
