#![no_std]
#![no_main]

use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "hostname";
const IDS_USAGE: u32 = 1001;
const IDS_ERR_READ: u32 = 1002;
const IDS_USAGE_LINE2: u32 = 1003;
const IDS_USAGE_LINE3: u32 = 1004;
const IDS_USAGE_LINE4: u32 = 1005;
const IDS_SET_OK: u32 = 1006;
const IDS_ERR_SET: u32 = 1007;
const IDS_ERR_INVALID: u32 = 1008;
const IDS_ERR_PERM: u32 = 1009;

/// Kernel `SyscallError::Perm` (16) encoded at the syscall boundary.
const EPERM: i64 = -16;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE4).as_bytes());
    write_str(b"\r\n\r\n");
}

/// Print the current hostname. Returns the process exit code.
fn show_hostname() -> u32 {
    let mut buf = [0u8; 64];
    match syscall::sys_get_hostname(&mut buf) {
        Ok(n) if n > 0 => {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(n);
            write_str(b"\r\n");
            write_str(&buf[..end]);
            write_str(b"\r\n\r\n");
            0
        }
        _ => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_READ).as_bytes());
            write_str(b"\r\n\r\n");
            1
        }
    }
}

/// Persist a new hostname via `SetHostname`. The kernel validates the name
/// (RFC 952/1035 labels) and stores it in the Registry; errors are mapped to a
/// readable message. Returns the process exit code.
fn set_hostname(name: &str) -> u32 {
    match syscall::sys_set_hostname(name) {
        Ok(()) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_SET_OK).as_bytes());
            write_str(name.as_bytes());
            write_str(b"\r\n\r\n");
            0
        }
        Err(e) => {
            write_str(b"\r\n");
            if e == syscall::EINVAL {
                write_str(tr_id!(IDS_ERR_INVALID).as_bytes());
            } else if e == EPERM {
                write_str(tr_id!(IDS_ERR_PERM).as_bytes());
            } else {
                write_str(tr_id!(IDS_ERR_SET).as_bytes());
            }
            write_str(b"\r\n\r\n");
            1
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    let args = libneodos::args::trim_ascii(&raw);
    let name = core::str::from_utf8(args)
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("");

    let code = if name.is_empty() {
        show_hostname()
    } else {
        set_hostname(name)
    };
    syscall::sys_exit(code)
}
