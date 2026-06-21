#![no_std]
#![no_main]

use libneodos::syscall::{self, KObjEntryRaw, sys_kobj_enum};

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

#[used]
#[link_section = ".rodata"]
static PS_HELP: &[u8] = b"::HELP::\
PS\r\n\
  Show process/thread list.\r\n\
  Displays TID, PID, state, priority, and native ID.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nPS\r\n  Show process/thread list.\r\n  Displays TID, PID, state, priority, and native ID.\r\n\r\n");
}

const KOBJ_TYPE_PROCESS: u32 = 1;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    let mut entries = [KObjEntryRaw {
        id: 0,
        obj_type: 0,
        padding: 0,
        name: [0u8; 24],
        refcount: 0,
        native_id: 0,
    }; 64];

    match sys_kobj_enum(&mut entries) {
        Ok(count) if count > 0 => {
            let mut proc_count = 0usize;
            for i in 0..count.min(64) {
                if entries[i].obj_type == KOBJ_TYPE_PROCESS {
                    proc_count += 1;
                }
            }

            if proc_count == 0 {
                write_str(b"\r\nNo processes\r\n");
                syscall::sys_exit(0);
            }

            write_str(b"\r\nTID  PID  STATE       PRI  NAME\r\n");
            write_str(b"---- ---- ----------- ---  ------------------------\r\n");
            for i in 0..count.min(64) {
                let e = &entries[i];
                if e.obj_type != KOBJ_TYPE_PROCESS {
                    continue;
                }
                let pid = e.native_id as u32;
                let name_str = e.name_str();
                write_str(b" ");
                write_u32_right(e.id as u32, 3);
                write_str(b"  ");
                write_u32_right(pid, 3);
                write_str(b"  ");
                write_str(b"Ready     ");
                write_str(b" N   ");
                let n = pad_right(name_str.as_bytes(), 24);
                write_str(&n[..24]);
                write_str(b"\r\n");
            }
            write_str(b"\r\n");
        }
        Ok(_) => {
            write_str(b"\r\nNo processes.\r\n");
        }
        Err(_) => {
            write_str(b"\r\nPS: syscall failed\r\n");
        }
    }

    syscall::sys_exit(0)
}
