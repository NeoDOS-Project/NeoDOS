#![no_std]
#![no_main]

use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "nxlocale";
const IDS_CURRENT: u32 = 1006;
const IDS_AVAILABLE: u32 = 1007;
const IDS_SET_SUCCESS: u32 = 1008;
const IDS_ERR_UNKNOWN: u32 = 1010;
const IDS_ERR_SET: u32 = 1011;
const IDS_ERR_UNKNOWN_LOCALE: u32 = 1012;
const IDS_NO_LOCALES: u32 = 1013;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn print_help() {
    write_str(b"\r\n");
    write_str(b"NXLOCALE [subcommand]\r\n");
    write_str(b"  Locale management tool.\r\n");
    write_str(b"  NXLOCALE                  shows current locale\r\n");
    write_str(b"  NXLOCALE list             lists available locales\r\n");
    write_str(b"  NXLOCALE set <locale>     changes system locale\r\n\r\n");
}

fn is_cmd(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let raw = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw);

    if args.is_empty() || libneodos::args::is_help_flag(&raw) {
        if libneodos::args::is_help_flag(&raw) {
            print_help();
            syscall::sys_exit(0);
        }

        write_str(b"\r\n");
        write_str(tr_id!(IDS_CURRENT).as_bytes());
        write_str(i18n::i18n_language().as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(0);
    }

    if is_cmd(args, b"list") || is_cmd(args, b"-l") || is_cmd(args, b"--list") {
        let locales = i18n::i18n_available_locales();
        write_str(b"\r\n");
        write_str(tr_id!(IDS_AVAILABLE).as_bytes());
        write_str(b"\r\n");

        if locales.is_empty() {
            write_str(b"  ");
            write_str(tr_id!(IDS_NO_LOCALES).as_bytes());
            write_str(b"\r\n");
        } else {
            for locale in locales.split(';') {
                if !locale.is_empty() {
                    write_str(b"  ");
                    write_str(locale.as_bytes());
                    write_str(b"\r\n");
                }
            }
        }
        write_str(b"\r\n");
        syscall::sys_exit(0);
    }

    if is_cmd(args, b"set") || is_cmd(args, b"-s") || is_cmd(args, b"--set") {
        let rest = &args[3..];
        let rest = libneodos::args::trim_ascii(rest);

        if rest.is_empty() {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_UNKNOWN_LOCALE).as_bytes());
            write_err(b"\r\n\r\n");
            syscall::sys_exit(1);
        }

        let locale_str = core::str::from_utf8(rest).unwrap_or("");
        let key = "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Locale";
        match syscall::sys_cm_open_key(key) {
            Ok(fd) => {
                match syscall::sys_cm_set_value(fd, "Language", syscall::REG_SZ, locale_str.as_bytes()) {
                    Ok(_) => {
                        let _ = syscall::sys_close(fd);
                        i18n::i18n_reload_all();
                        write_str(b"\r\n");
                        write_str(tr_id!(IDS_SET_SUCCESS).as_bytes());
                        write_str(b"\r\n\r\n");
                    }
                    Err(_) => {
                        let _ = syscall::sys_close(fd);
                        write_err(b"\r\n");
                        write_err(tr_id!(IDS_ERR_SET).as_bytes());
                        write_err(b"\r\n\r\n");
                        syscall::sys_exit(1);
                    }
                }
            }
            Err(_) => {
                write_err(b"\r\n");
                write_err(tr_id!(IDS_ERR_UNKNOWN).as_bytes());
                write_err(b"\r\n\r\n");
                syscall::sys_exit(1);
            }
        }
        syscall::sys_exit(0);
    }

    write_err(b"\r\n");
    write_err(tr_id!(IDS_ERR_UNKNOWN).as_bytes());
    write_err(b"\r\n\r\n");
    syscall::sys_exit(1)
}
