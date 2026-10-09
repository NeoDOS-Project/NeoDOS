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
use libneodos::i18n;
use libneodos::tr_id;

// ── String IDs (from TOML) ──
const IDS_FILE_NOT_FOUND: u32 = 1001;
const IDS_READ_ERROR: u32 = 1002;
const IDS_USAGE: u32 = 1003;

const APP_NAME: &str = "coretype";

use libneoutil::to_ob_path;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

#[used]
#[link_section = ".rodata"]
static TYPE_HELP: &[u8] = b"::HELP::\
TYPE [drive:][path]filename\r\n\
  Display the contents of a text file on screen.\r\n\
  TYPE C:\\readme.txt   shows the readme file.\r\n\
::END::";

use libneodos::path::normalize_path;

fn print_usage() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(b"  Display the contents of a text file.\r\n");
    write_str(b"  TYPE C:\\Programs\\test.txt\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let raw_args = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw_args);

    if args.is_empty() {
        print_usage();
        syscall::sys_exit(0);
    }

    if libneodos::args::is_help_flag(args) {
        print_usage();
        syscall::sys_exit(0);
    }

    let path_args = if args.len() >= 2
        && ((args[0] == b'"' && args[args.len() - 1] == b'"')
            || (args[0] == b'\'' && args[args.len() - 1] == b'\''))
    {
        &args[1..args.len() - 1]
    } else {
        args
    };

    let normalized = normalize_path(path_args);
    let end = normalized.iter().position(|&b| b == 0).unwrap_or(normalized.len());
    let path = core::str::from_utf8(&normalized[..end]).unwrap_or("C:\\");
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(path, &mut ob_buf);
    let fd = match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_FILE_NOT_FOUND).as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; 512];
    let mut failed = false;
    loop {
        match syscall::sys_ob_query_info(fd, libneodos::syscall::ObInfoClass::ReadContent, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = syscall::sys_write(1, &buf[..n]);
            }
            Err(e) => {
                write_err(b"\r\n");
                write_err(tr_id!(IDS_READ_ERROR).as_bytes());
                write_err(b": ");
                let err_str: &[u8] = match e {
                    -1 => b"EINVAL" as &[u8],
                    -2 => b"ENOENT" as &[u8],
                    -3 => b"ENOMEM" as &[u8],
                    -4 => b"EACCES" as &[u8],
                    -5 => b"EBADF" as &[u8],
                    _ => b"UNKNOWN" as &[u8],
                };
                write_err(err_str);
                write_err(b"\r\n");
                failed = true;
                break;
            }
        }
    }

    write_str(b"\r\n");
    let _ = syscall::sys_close(fd);
    if failed {
        syscall::sys_exit(1)
    } else {
        syscall::sys_exit(0)
    }
}
