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
use libneodos::syscall::{self, ObEnumEntry, ObProcessInfo};
use libneodos::tr_id;

const APP_NAME: &str = "neotop";
const IDS_TITLE: u32 = 1005;
const IDS_COL_PID: u32 = 1006;
const IDS_COL_NAME: u32 = 1007;
const IDS_COL_STATE: u32 = 1008;
const IDS_COL_PRI: u32 = 1009;
const IDS_COL_THR: u32 = 1010;
const IDS_ERR_OPEN: u32 = 1011;

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

fn print_help() {
    write_str(b"\r\n");
    write_str(b"NEOTOP\r\n");
    write_str(b"  Display system monitor with process list.\r\n");
    write_str(b"  Shows processes sorted by CPU usage.\r\n");
    write_str(b"  Press Q to quit.\r\n\r\n");
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

    let dir_fd = match syscall::sys_ob_open("\\Process", libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_OPEN).as_bytes());
            write_str(b"\r\n");
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
            let _ = syscall::sys_close(dir_fd);
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_OPEN).as_bytes());
            write_str(b"\r\n");
            syscall::sys_exit(1);
        }
    };
    let _ = syscall::sys_close(dir_fd);

    write_str(b"\r\n");
    write_str(tr_id!(IDS_TITLE).as_bytes());
    write_str(b"\r\n\r\n");

    write_str(tr_id!(IDS_COL_PID).as_bytes());
    write_str(b"  ");
    write_str(tr_id!(IDS_COL_NAME).as_bytes());
    write_str(b"          ");
    write_str(tr_id!(IDS_COL_STATE).as_bytes());
    write_str(b"  ");
    write_str(tr_id!(IDS_COL_PRI).as_bytes());
    write_str(b" ");
    write_str(tr_id!(IDS_COL_THR).as_bytes());
    write_str(b"\r\n");
    write_str(b"---  ----            -----    --- ---\r\n");

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

        if written < 20 { continue; }

        let info: ObProcessInfo = unsafe { core::ptr::read(info_buf.as_ptr() as *const ObProcessInfo) };

        write_u32_right(info.pid, 3);
        write_str(b"  ");
        let n = pad_right(name_str.as_bytes(), 14);
        write_str(&n[..14]);
        write_str(b" ");
        let state = info.state_str();
        write_str(state.as_bytes());
        for _ in 0..(8 - state.len()) {
            write_str(b" ");
        }
        let prio = info.priority_str();
        write_str(prio.as_bytes());
        for _ in 0..(4 - prio.len()) {
            write_str(b" ");
        }
        write_u32_right(info.thread_count, 3);
        write_str(b"\r\n");
    }

    write_str(b"\r\n");
    syscall::sys_exit(0)
}
