#![no_std]
#![no_main]

use libneodos::syscall::{self, ObEnumEntry};

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u64(mut v: u64) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 19;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

fn pad_right(s: &[u8], width: usize) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let len = s.len().min(width);
    buf[..len].copy_from_slice(&s[..len]);
    buf
}

fn type_str(obj_type: u32) -> &'static [u8] {
    match obj_type {
        1 => b"PROCESS  ",
        2 => b"DRIVER   ",
        3 => b"DEVICE   ",
        4 => b"PIPE     ",
        5 => b"EVENTBUS ",
        6 => b"BLOCKDEV ",
        7 => b"FILESYSTEM",
        8 => b"MEMREGION",
        9 => b"SYMLINK  ",
        10 => b"MOUNTPOINT",
        11 => b"DIRECTORY",
        12 => b"REGKEY   ",
        13 => b"EVENT    ",
        14 => b"SEMAPHORE",
        15 => b"TIMER    ",
        _ => b"UNKNOWN  ",
    }
}

#[used]
#[link_section = ".rodata"]
static KOBJ_HELP: &[u8] = b"::HELP::\
KOBJ\r\n\
  Lists all objects in the Ob namespace.\r\n\
  Shows ID, type, and name.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nKOBJ\r\n  Lists all objects in the Ob namespace.\r\n  Shows ID, type, and name.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match syscall::sys_ob_open("\\Ob", libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\nKOBJ: cannot open Ob namespace\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut entries: [ObEnumEntry; 64] = core::array::from_fn(|_| ObEnumEntry {
        id: 0,
        obj_type: 0,
        name: [0u8; 32],
    });

    match syscall::sys_ob_enum(fd, &mut entries) {
        Ok(count) if count > 0 => {
            write_str(b"\r\n");
            write_str(b" ID   TYPE         NAME\r\n");
            write_str(b" ---- ------------ ------------------------------\r\n");
            for i in 0..count.min(64) {
                let e = &entries[i];
                let name_str = e.name_str();
                write_u64(e.id);
                write_str(b"  ");
                write_str(type_str(e.obj_type));
                write_str(b" ");
                let n = pad_right(name_str.as_bytes(), 30);
                write_str(&n[..30]);
                write_str(b"\r\n");
            }
            write_str(b"\r\nTotal: ");
            write_u64(count as u64);
            write_str(b" entries\r\n\r\n");
        }
        Ok(_) => {
            write_str(b"\r\nNo entries in Ob namespace.\r\n\r\n");
        }
        Err(_) => {
            write_str(b"\r\nKOBJ: syscall failed\r\n\r\n");
        }
    }

    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0)
}