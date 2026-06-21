#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::DateTime;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u8_pad(v: u8) {
    let hi = v / 10;
    let lo = v % 10;
    let buf = [b'0' + hi, b'0' + lo];
    write_str(&buf);
}

fn show_date(dt: &DateTime) {
    write_str(b"Current date: ");
    write_u8_pad(dt.day);
    write_str(b"/");
    write_u8_pad(dt.month);
    write_str(b"/");
    write_u8_pad(dt.year);
}

fn show_time(dt: &DateTime) {
    write_str(b"Current time: ");
    write_u8_pad(dt.hour);
    write_str(b":");
    write_u8_pad(dt.minute);
    write_str(b":");
    write_u8_pad(dt.second);
}

#[used]
#[link_section = ".rodata"]
static DATETIME_HELP: &[u8] = b"::HELP::\
DATETIME [/D] [/T]\r\n\
  Shows the current date and/or time.\r\n\
  /D     Show date only\r\n\
  /T     Show time only\r\n\
  (no flags = show both date and time)\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nDATETIME [/D] [/T]\r\n  Shows the current date and/or time.\r\n  /D     Show date only\r\n  /T     Show time only\r\n  (no flags = show both date and time)\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let ptr = 0x41F000 as *const u8;
    let mut arg_buf = [0u8; 64];
    unsafe {
        let mut i = 0;
        while i < 63 {
            let b = ptr.add(i).read();
            arg_buf[i] = b;
            if b == 0 { break; }
            i += 1;
        }
    }
    let args = core::str::from_utf8(&arg_buf).unwrap_or("");
    let trimmed = args.trim();
    if trimmed == "/?" || trimmed == "-h" || trimmed == "--help" {
        print_help();
        syscall::sys_exit(0);
    }
    // Parse flags from command line
    let mut show_d = false;
    let mut show_t = false;
    for token in trimmed.split_whitespace() {
        let bytes = token.as_bytes();
        if bytes.len() >= 2 {
            let flag = bytes[0] == b'/' || bytes[0] == b'-';
            if flag {
                match bytes[1].to_ascii_uppercase() {
                    b'D' => show_d = true,
                    b'T' => show_t = true,
                    _ => {}
                }
            }
        }
    }
    if !show_d && !show_t { show_d = true; show_t = true; }

    let mut dt = DateTime {
        second: 0, minute: 0, hour: 0,
        day: 0, month: 0, year: 0, valid: 0,
    };

    match syscall::sys_get_datetime(&mut dt) {
        Ok(_) => {
            if dt.valid == 0 {
                write_str(b"\r\nRTC not available\r\n");
                syscall::sys_exit(1);
            }

            write_str(b"\r\n");
            if show_d && show_t {
                show_date(&dt);
                write_str(b"\r\n");
                show_time(&dt);
            } else if show_d {
                show_date(&dt);
            } else if show_t {
                show_time(&dt);
            }
            write_str(b"\r\n\r\n");
        }
        Err(_) => {
            write_str(b"\r\nRTC not available\r\n\r\n");
        }
    }
    syscall::sys_exit(0)
}
