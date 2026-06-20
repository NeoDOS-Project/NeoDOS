#![no_std]
#![no_main]

use libneodos::syscall;

const ARGS_ADDR: u64 = 0x41F000;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[used]
#[link_section = ".rodata"]
static ECHO_HELP: &[u8] = b"::HELP::\
ECHO [text]\r\n\
  Print text.\r\n\
  ECHO               prints a blank line.\r\n\
  ECHO Hello world   prints \"Hello world\".\r\n\
::END::";

fn get_args<'a>() -> &'a [u8] {
    unsafe {
        let ptr = ARGS_ADDR as *const u8;
        let mut len = 0usize;
        while len < 256 && *ptr.add(len) != 0 {
            len += 1;
        }
        core::slice::from_raw_parts(ptr, len)
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let args = get_args();
    write_str(b"\r\n");
    if !args.is_empty() {
        write_str(args);
    }
    write_str(b"\r\n");
    syscall::sys_exit(0)
}
