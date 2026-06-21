#![no_std]
#![no_main]

use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn make_ascii_uppercase(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        if *b >= b'a' && *b <= b'z' {
            *b -= 32;
        }
    }
}

#[used]
#[link_section = ".rodata"]
static KEYB_HELP: &[u8] = b"::HELP::\
KEYB US|SP\r\n\
  Change keyboard layout.\r\n\
  US = English (United States)\r\n\
  SP = Spanish\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nKEYB US|SP\r\n  Change keyboard layout.\r\n  US = English (United States)\r\n  SP = Spanish\r\n\r\n");
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
    if args.is_empty() {
        write_str(b"\r\nUsage: KEYB US|SP\r\n");
        write_str(b"  US = English (United States)\r\n");
        write_str(b"  SP = Spanish\r\n\r\n");
        syscall::sys_exit(0);
    }

    let mut layout_buf = [0u8; 4];
    let layout_len = args.len().min(3);
    layout_buf[..layout_len].copy_from_slice(&args[..layout_len]);
    make_ascii_uppercase(&mut layout_buf[..layout_len]);

    let layout = match &layout_buf[..layout_len] {
        b"US" => 0u8,
        b"SP" => 1u8,
        _ => {
            write_err(b"\r\nInvalid layout. Use US or SP.\r\n");
            syscall::sys_exit(1)
        }
    };

    match syscall::sys_set_keyboard_layout(layout) {
        Ok(()) => {
            let name = if layout == 0 { "US" } else { "SP" };
            write_str(b"\r\nKeyboard layout changed to ");
            write_str(name.as_bytes());
            write_str(b".\r\n");
        }
        Err(_) => {
            write_err(b"\r\nError: failed to change keyboard layout.\r\n");
        }
    }

    syscall::sys_exit(0)
}
