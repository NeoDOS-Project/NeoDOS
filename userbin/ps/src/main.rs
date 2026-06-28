#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::syscall::{self, ObEnumEntry, ObProcessInfo};

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u32_right(v: u32, width: usize) {
    let mut buf = [0u8; 12];
    let mut i = 11;
    let mut n = v;
    if n == 0 {
        buf[i] = b'0';
        if i == 0 { write_str(&buf[0..1]); return; }
        i -= 1;
    } else {
        while n > 0 {
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if i == 0 { break; }
            i -= 1;
        }
    }
    let digits_end = 12;
    let digits_start = i + 1;
    let digits_len = digits_end - digits_start;
    if digits_len >= width {
        write_str(&buf[digits_start..digits_end]);
    } else {
        for _ in 0..(width - digits_len) {
            write_str(b" ");
        }
        write_str(&buf[digits_start..digits_end]);
    }
}

fn pad_right(s: &[u8], width: usize) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let len = s.len().min(width);
    buf[..len].copy_from_slice(&s[..len]);
    buf
}

/// Build an Ob namespace path like "\\Ob\\Process\\eproc/N" into a fixed buffer.
fn build_proc_path(pid: u32, buf: &mut [u8; 128]) -> &str {
    let prefix = b"\\Process\\";
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
static PS_HELP: &[u8] = b"::HELP::\
PS\r\n\
  Show process list.\r\n\
  Displays PID, parent, priority, thread count, and state.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nPS\r\n  Show process list.\r\n  Displays PID, parent, priority, thread count, and state.\r\n\r\n");
}

fn parse_pid_from_name(name: &str) -> Option<u32> {
    let num_part = if let Some(pos) = name.find('/') {
        &name[pos + 1..]
    } else {
        name
    };
    let mut n: u32 = 0;
    for &b in num_part.as_bytes() {
        if b < b'0' || b > b'9' { return None; }
        n = n * 10 + (b - b'0') as u32;
    }
    Some(n)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    let dir_fd = match syscall::sys_ob_open("\\Process", libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\nPS: cannot open process list\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut entries: [ObEnumEntry; 64] = core::array::from_fn(|_| ObEnumEntry {
        id: 0,
        obj_type: 0,
        name: [0u8; 32],
        mode: 0,
        _pad: [0u8; 2],
        size: 0,
    });

    let count = match syscall::sys_ob_enum(dir_fd, &mut entries) {
        Ok(c) => c,
        Err(_) => {
            write_str(b"\r\nPS: enumeration failed\r\n");
            let _ = syscall::sys_close(dir_fd);
            syscall::sys_exit(1);
        }
    };

    let _ = syscall::sys_close(dir_fd);

    if count == 0 {
        write_str(b"\r\nNo processes\r\n");
        syscall::sys_exit(0);
    }

    write_str(b"\r\n");
    write_str(b"PID  PPID PRI THR STATE      NAME\r\n");
    write_str(b"---- ---- --- --- ---------- ------------------------\r\n");

    let mut path_buf = [0u8; 128];

    for i in 0..count.min(64) {
        let e = &entries[i];
        let name_str = e.name_str();
        let pid = match parse_pid_from_name(&name_str) {
            Some(p) => p,
            None => continue,
        };

        let proc_path = build_proc_path(pid, &mut path_buf);
        let proc_fd = match syscall::sys_ob_open(proc_path, libneodos::syscall::ob_access::READ) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let mut info_buf = [0u8; 20];
        let written = match syscall::sys_ob_query_info(proc_fd,
            libneodos::syscall::ObInfoClass::Process, &mut info_buf) {
            Ok(w) => w,
            Err(_) => { let _ = syscall::sys_close(proc_fd); continue; }
        };

        let _ = syscall::sys_close(proc_fd);

        if written < 20 {
            continue;
        }

        // Interpret as ObProcessInfo
        let info: ObProcessInfo = unsafe { core::ptr::read(info_buf.as_ptr() as *const ObProcessInfo) };

        write_str(b" ");
        write_u32_right(info.pid, 3);
        write_str(b"  ");
        write_u32_right(info.parent_pid, 3);
        write_str(b"  ");
        let prio = info.priority_str();
        write_str(prio.as_bytes());
        write_str(b" ");
        write_u32_right(info.thread_count, 2);
        write_str(b" ");
        let state = info.state_str();
        write_str(state.as_bytes());
        for _ in 0..(10 - state.len()) {
            write_str(b" ");
        }
        let n = pad_right(name_str.as_bytes(), 24);
        write_str(&n[..24]);
        write_str(b"\r\n");
    }

    write_str(b"\r\n");
    syscall::sys_exit(0)
}