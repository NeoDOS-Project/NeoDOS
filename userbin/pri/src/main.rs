#![no_std]
#![no_main]

use libneodos::syscall;

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

#[used]
#[link_section = ".rodata"]
static PRI_HELP: &[u8] = b"::HELP::\
PRI <pid> <priority>\r\n\
  Set process scheduling priority.\r\n\
  Priority levels:\r\n\
    0 = HIGH (400 ticks)\r\n\
    1 = ABOVE_NORMAL (200 ticks)\r\n\
    2 = NORMAL (100 ticks) - default\r\n\
    3 = IDLE (50 ticks)\r\n\
  PRI 2 0   boosts PID 2 to HIGH priority.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nPRI <pid> <priority>\r\n  Set process scheduling priority.\r\n  Priority levels: 0=HIGH, 1=ABOVE_NORMAL, 2=NORMAL, 3=IDLE\r\n  PRI 2 0   boosts PID 2 to HIGH priority.\r\n\r\n");
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
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }
    let args = args_to_slice(&raw);
    if args.is_empty() {
        write_err(b"\r\nUsage: PRI <pid> <priority>\r\n");
        syscall::sys_exit(1);
    }

    let (pid_str, pri_str) = split_whitespace(args);
    if pid_str.is_empty() || pri_str.is_empty() {
        write_err(b"\r\nUsage: PRI <pid> <priority>\r\n");
        syscall::sys_exit(1);
    }

    let pid = match parse_u32(pid_str) {
        Some(p) => p,
        None => {
            write_err(b"\r\nInvalid PID.\r\n");
            syscall::sys_exit(1);
        }
    };

    let priority = match parse_u32(pri_str) {
        Some(p) if p <= 3 => p as u8,
        _ => {
            write_err(b"\r\nInvalid priority. Use 0-3.\r\n");
            syscall::sys_exit(1);
        }
    };

    // Open the process via Ob namespace
    let mut path_buf = [0u8; 128];
    let proc_path = build_proc_path(pid, &mut path_buf);

    let proc_fd = match syscall::sys_ob_open(proc_path, libneodos::syscall::ob_access::WRITE) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\nProcess not found.\r\n");
            syscall::sys_exit(1);
        }
    };

    // ObSetInfo class 0 = ProcessPriority
    let priority_bytes = [priority as u8, 0, 0, 0];
    match syscall::sys_ob_set_info(proc_fd, 0, &priority_bytes) {
        Ok(()) => {
            let names: &[&[u8]] = &[b"HIGH", b"ABOVE_NORMAL", b"NORMAL", b"IDLE"];
            write_str(b"\r\nProcess ");
            write_u32(pid);
            write_str(b" priority set to ");
            write_u32(priority as u32);
            write_str(b" (");
            write_str(names[priority as usize]);
            write_str(b")\r\n");
        }
        Err(_) => {
            write_err(b"\r\nFailed to set priority.\r\n");
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