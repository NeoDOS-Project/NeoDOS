#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

// `datetime` links `libntp` (which uses `alloc`), so it needs a global
// allocator like the other time/net tools.
use core::alloc::{GlobalAlloc, Layout};

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = libneodos::mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

use libneodos::i18n;
use libneodos::syscall;
use libneodos::syscall::DateTime;
use libneodos::syscall::ObInfoClass;
use libneodos::tr_id;

const APP_NAME: &str = "datetime";
const IDS_CUR_DATE: u32 = 1006;
const IDS_CUR_TIME: u32 = 1007;
const IDS_RTC_UNAVAIL: u32 = 1008;
const IDS_SET_OK: u32 = 1009;
const IDS_ERR_INVALID: u32 = 1010;
const IDS_ERR_PERM: u32 = 1011;
const IDS_USAGE_LINE6: u32 = 1012;
const IDS_USAGE_LINE7: u32 = 1013;
const IDS_ERR_SET_FAIL: u32 = 1014;
const IDS_USAGE_LINE8: u32 = 1015;
const IDS_UTC_SUFFIX: u32 = 1016;

/// `SyscallError::Perm` maps to `-16` at the syscall boundary.
const EPERM: i64 = -16;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn as_utc(dt: &DateTime) -> libntp::UtcDateTime {
    libntp::UtcDateTime {
        second: dt.second,
        minute: dt.minute,
        hour: dt.hour,
        day: dt.day,
        month: dt.month,
        year: dt.year,
    }
}

fn show_date(dt: &DateTime) {
    let utc = as_utc(dt);
    let mut buf = [0u8; 16];
    let n = libntp::format_date(&utc, &mut buf);
    write_str(tr_id!(IDS_CUR_DATE).as_bytes());
    write_str(&buf[..n]);
}

fn show_time(dt: &DateTime) {
    let utc = as_utc(dt);
    let mut buf = [0u8; 16];
    let n = libntp::format_time(&utc, &mut buf);
    write_str(tr_id!(IDS_CUR_TIME).as_bytes());
    write_str(&buf[..n]);
}

fn get_datetime_via_ob(dt: &mut DateTime, class: ObInfoClass) -> Result<(), i64> {
    let fd = syscall::sys_ob_open("\\Global\\Info\\DateTime", libneodos::syscall::ob_access::READ)?;
    let sz = core::mem::size_of::<DateTime>();
    let buf = unsafe { core::slice::from_raw_parts_mut(dt as *mut DateTime as *mut u8, sz) };
    let n = syscall::sys_ob_query_info(fd, class, buf)?;
    let _ = syscall::sys_close(fd);
    if n >= sz { Ok(()) } else { Err(-1) }
}

/// Parse a decimal field (all digits, 1..=4 chars) into an integer.
fn parse_uint(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut n = 0u32;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (b - b'0') as u32;
    }
    Some(n)
}

/// Parse `DD/MM/YY` (also `-` or `.` separators, and a 4-digit 2000-2099 year)
/// into `(day, month, two_digit_year)`. Range validation is left to the kernel,
/// which is the single authority; this only rejects malformed input.
fn parse_date(s: &str) -> Option<(u8, u8, u8)> {
    let mut it = s.split(|c| c == '/' || c == '-' || c == '.');
    let ds = it.next()?;
    let ms = it.next()?;
    let ys = it.next()?;
    if it.next().is_some() {
        return None;
    }
    let day = parse_uint(ds)?;
    let month = parse_uint(ms)?;
    let year = if ys.len() == 4 {
        let y4 = parse_uint(ys)?;
        if !(2000..=2099).contains(&y4) {
            return None;
        }
        y4 - 2000
    } else {
        parse_uint(ys)?
    };
    if day > 31 || month > 12 || year > 99 {
        return None;
    }
    Some((day as u8, month as u8, year as u8))
}

/// Parse `HH:MM` or `HH:MM:SS` into `(hour, minute, second)`.
fn parse_time(s: &str) -> Option<(u8, u8, u8)> {
    let mut it = s.split(':');
    let hs = it.next()?;
    let ms = it.next()?;
    let ss = it.next().unwrap_or("0");
    if it.next().is_some() {
        return None;
    }
    let hour = parse_uint(hs)?;
    let minute = parse_uint(ms)?;
    let second = parse_uint(ss)?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some((hour as u8, minute as u8, second as u8))
}

fn print_usage() {
    write_str(b"\r\n");
    write_str(tr_id!(1001).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(1002).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(1003).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(1004).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(1005).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE6).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE7).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE8).as_bytes());
    write_str(b"\r\n");
}

fn set_datetime(date_tok: &str, time_tok: &str) -> ! {
    let (day, month, year) = match parse_date(date_tok) {
        Some(v) => v,
        None => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_INVALID).as_bytes());
            write_str(b"\r\n");
            syscall::sys_exit(1);
        }
    };
    let (hour, minute, second) = match parse_time(time_tok) {
        Some(v) => v,
        None => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_INVALID).as_bytes());
            write_str(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    let dt = DateTime { second, minute, hour, day, month, year, valid: 1 };
    match syscall::ob_set_datetime(&dt) {
        Ok(_) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_SET_OK).as_bytes());
            write_str(b"\r\n");
            syscall::sys_exit(0);
        }
        Err(e) if e == EPERM || e == syscall::EACCES => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_PERM).as_bytes());
            write_str(b"\r\n");
            syscall::sys_exit(1);
        }
        // The kernel is the range authority: it returns EINVAL for impossible
        // dates/times (Feb 30, hour 24, ...). Surface that as invalid input.
        Err(e) if e == syscall::EINVAL => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_INVALID).as_bytes());
            write_str(b"\r\n");
            syscall::sys_exit(1);
        }
        Err(_) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_SET_FAIL).as_bytes());
            write_str(b"\r\n");
            syscall::sys_exit(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_usage();
        syscall::sys_exit(0);
    }
    let arglen = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let trimmed = core::str::from_utf8(libneodos::args::trim_ascii(&raw[..arglen])).unwrap_or("");

    let mut show_d = false;
    let mut show_t = false;
    let mut show_utc = false;
    let mut set_mode = false;
    let mut date_tok = "";
    let mut time_tok = "";
    let mut positional = 0;
    for token in trimmed.split_whitespace() {
        let bytes = token.as_bytes();
        let is_flag = bytes.len() >= 2 && (bytes[0] == b'/' || bytes[0] == b'-');
        if is_flag {
            match bytes[1].to_ascii_uppercase() {
                b'D' => show_d = true,
                b'T' => show_t = true,
                b'U' => show_utc = true,
                b'S' => set_mode = true,
                _ => {}
            }
        } else {
            if positional == 0 {
                date_tok = token;
            } else if positional == 1 {
                time_tok = token;
            }
            positional += 1;
        }
    }

    if set_mode {
        if date_tok.is_empty() || time_tok.is_empty() {
            print_usage();
            syscall::sys_exit(1);
        }
        set_datetime(date_tok, time_tok);
    }

    if !show_d && !show_t { show_d = true; show_t = true; }

    let mut dt = DateTime {
        second: 0, minute: 0, hour: 0,
        day: 0, month: 0, year: 0, valid: 0,
    };

    // Local time is derived by the kernel from the authoritative UTC RTC.
    let class = if show_utc { ObInfoClass::DateTime } else { ObInfoClass::LocalDateTime };
    match get_datetime_via_ob(&mut dt, class) {
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
            if show_utc {
                write_str(tr_id!(IDS_UTC_SUFFIX).as_bytes());
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
