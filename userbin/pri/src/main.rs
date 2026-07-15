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
use libneodos::tr_id;

const APP_NAME: &str = "pri";
const IDS_ERR_INVALID_PID: u32 = 1008;
const IDS_ERR_INVALID_PRI: u32 = 1009;
const IDS_ERR_PROCESS_NOT_FOUND: u32 = 1010;
const IDS_MSG_SET: u32 = 1011;
const IDS_MSG_TO: u32 = 1012;
const IDS_PRI_HIGH: u32 = 1013;
const IDS_PRI_ABOVE_NORMAL: u32 = 1014;
const IDS_PRI_NORMAL: u32 = 1015;
const IDS_PRI_IDLE: u32 = 1016;
const IDS_ERR_FAILED: u32 = 1017;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for &b in s {
        if b < b'0' || b > b'9' {
            return None;
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    Some(n)
}

fn build_proc_path(pid: u32, buf: &mut [u8; 128]) -> &str {
    let prefix = b"\\Ob\\Process\\eproc/";
    let plen = prefix.len();
    buf[..plen].copy_from_slice(prefix);
    let mut i = plen;
    let mut n = pid;
    if n == 0 {
        buf[i] = b'0';
        i += 1;
    } else {
        let mut digits = [0u8; 10];
        let mut di = 10;
        while n > 0 {
            di -= 1;
            digits[di] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        while di < 10 {
            buf[i] = digits[di];
            i += 1;
            di += 1;
        }
    }
    buf[i] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..i]) }
}

fn print_help() {
    write_str(b"\r\n");
    write_str(b"PRI <pid> <priority>\r\n");
    write_str(b"  Set process scheduling priority.\r\n");
    write_str(b"  Priority levels: 0=HIGH, 1=ABOVE_NORMAL, 2=NORMAL, 3=IDLE\r\n");
    write_str(b"  PRI 2 0   boosts PID 2 to HIGH priority.\r\n\r\n");
}

fn args_to_slice(buf: &[u8; 256]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

fn split_whitespace(s: &[u8]) -> (&[u8], &[u8]) {
    let trimmed = {
        let mut start = 0;
        while start < s.len() && (s[start] == b' ' || s[start] == b'\t') {
            start += 1;
        }
        let mut end = s.len();
        while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t') {
            end -= 1;
        }
        &s[start..end]
    };
    if trimmed.is_empty() {
        return (&[], &[]);
    }
    let mut split = 0;
    while split < trimmed.len() && trimmed[split] != b' ' && trimmed[split] != b'\t' {
        split += 1;
    }
    let first = &trimmed[..split];
    let rest = {
        let mut s = split;
        while s < trimmed.len() && (trimmed[s] == b' ' || trimmed[s] == b'\t') {
            s += 1;
        }
        if s < trimmed.len() { &trimmed[s..] } else { &[] }
    };
    (first, rest)
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
    let args = args_to_slice(&raw);
    if args.is_empty() {
        print_help();
        syscall::sys_exit(1);
    }

    let (pid_str, pri_str) = split_whitespace(args);
    if pid_str.is_empty() || pri_str.is_empty() {
        print_help();
        syscall::sys_exit(1);
    }

    let pid = match parse_u32(pid_str) {
        Some(p) => p,
        None => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_INVALID_PID).as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    let priority = match parse_u32(pri_str) {
        Some(p) if p <= 3 => p as u8,
        _ => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_INVALID_PRI).as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut path_buf = [0u8; 128];
    let proc_path = build_proc_path(pid, &mut path_buf);

    let proc_fd = match syscall::sys_ob_open(proc_path, libneodos::syscall::ob_access::WRITE) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_PROCESS_NOT_FOUND).as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    let priority_bytes = [priority as u8, 0, 0, 0];
    match syscall::sys_ob_set_info(proc_fd, syscall::ObSetInfoClass::ProcessPriority, &priority_bytes) {
        Ok(()) => {
            let names: &[&[u8]] = &[
                tr_id!(IDS_PRI_HIGH).as_bytes(),
                tr_id!(IDS_PRI_ABOVE_NORMAL).as_bytes(),
                tr_id!(IDS_PRI_NORMAL).as_bytes(),
                tr_id!(IDS_PRI_IDLE).as_bytes(),
            ];
            write_str(b"\r\nProcess ");
            write_u32(pid);
            write_str(tr_id!(IDS_MSG_SET).as_bytes());
            write_u32(priority as u32);
            write_str(tr_id!(IDS_MSG_TO).as_bytes());
            write_str(names[priority as usize]);
            write_str(b")\r\n");
        }
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_FAILED).as_bytes());
            write_err(b"\r\n");
        }
    }

    let _ = syscall::sys_close(proc_fd);
    syscall::sys_exit(0)
}

fn write_u32(mut v: u32) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 9;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}
