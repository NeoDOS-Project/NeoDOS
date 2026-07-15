#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::i18n;
use libneodos::syscall;
use libneodos::syscall::DateTime;
use libneodos::syscall::ObInfoClass;
use libneodos::tr_id;

const APP_NAME: &str = "datetime";
const IDS_CUR_DATE: u32 = 1006;
const IDS_CUR_TIME: u32 = 1007;
const IDS_RTC_UNAVAIL: u32 = 1008;

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
    write_str(tr_id!(IDS_CUR_DATE).as_bytes());
    write_u8_pad(dt.day);
    write_str(b"/");
    write_u8_pad(dt.month);
    write_str(b"/");
    write_u8_pad(dt.year);
}

fn show_time(dt: &DateTime) {
    write_str(tr_id!(IDS_CUR_TIME).as_bytes());
    write_u8_pad(dt.hour);
    write_str(b":");
    write_u8_pad(dt.minute);
    write_str(b":");
    write_u8_pad(dt.second);
}

fn get_datetime_via_ob(dt: &mut DateTime) -> Result<(), i64> {
    let fd = syscall::sys_ob_open("\\Global\\Info\\DateTime", libneodos::syscall::ob_access::READ)?;
    let sz = core::mem::size_of::<DateTime>();
    let buf = unsafe { core::slice::from_raw_parts_mut(dt as *mut DateTime as *mut u8, sz) };
    let n = syscall::sys_ob_query_info(fd, ObInfoClass::DateTime, buf)?;
    let _ = syscall::sys_close(fd);
    if n >= sz { Ok(()) } else { Err(-1) }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        write_str(b"\r\n");
        write_str(b"DATETIME [/D] [/T]\r\n");
        write_str(b"  Shows the current date and/or time.\r\n");
        write_str(b"  /D     Show date only\r\n");
        write_str(b"  /T     Show time only\r\n");
        write_str(b"  (no flags = show both date and time)\r\n\r\n");
        syscall::sys_exit(0);
    }
    let arglen = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let trimmed = core::str::from_utf8(libneodos::args::trim_ascii(&raw[..arglen])).unwrap_or("");
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

    match get_datetime_via_ob(&mut dt) {
        Ok(_) => {
            if dt.valid == 0 {
                write_str(b"\r\n");
                write_str(tr_id!(IDS_RTC_UNAVAIL).as_bytes());
                write_str(b"\r\n");
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
            write_str(b"\r\n");
            write_str(tr_id!(IDS_RTC_UNAVAIL).as_bytes());
            write_str(b"\r\n\r\n");
        }
    }
    syscall::sys_exit(0)
}
