#![no_std]
#![no_main]

use libneodos::syscall;

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

fn print_help() {
    write_str(b"\r\nECHO [text]\r\n  Print text.\r\n  ECHO               prints a blank line.\r\n  ECHO Hello world   prints \"Hello world\".\r\n\r\n");
}

fn args_to_slice(buf: &[u8; 256]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }
    let args = args_to_slice(&raw);
    write_str(b"\r\n");
    if !args.is_empty() {
        write_str(args);
    }
    write_str(b"\r\n");
    syscall::sys_exit(0)
}
