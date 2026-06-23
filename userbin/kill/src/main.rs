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
static KILL_HELP: &[u8] = b"::HELP::\
KILL <pid>\r\n\
  Terminate a process by PID number.\r\n\
  Use PS to list running processes and their PIDs.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nKILL <pid>\r\n  Terminate a process by PID number.\r\n  Use PS to list running processes and their PIDs.\r\n\r\n");
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
        write_err(b"\r\nUsage: KILL <pid>\r\n");
        syscall::sys_exit(1);
    }

    let pid = match parse_u32(args) {
        Some(p) => p,
        None => {
            write_err(b"\r\nInvalid PID.\r\n");
            syscall::sys_exit(1);
        }
    };

    if pid == 0 {
        write_err(b"\r\nCannot kill the idle process.\r\n");
        syscall::sys_exit(1);
    }

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

    // ObSetInfo class 4 = ProcessTerminate
    match syscall::sys_ob_set_info(proc_fd, 4, &[]) {
        Ok(()) => {
            write_str(b"\r\nProcess ");
            write_u32(pid);
            write_str(b" terminated.\r\n");
        }
        Err(_) => {
            write_err(b"\r\nFailed to terminate process.\r\n");
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