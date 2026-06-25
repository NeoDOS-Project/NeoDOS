#![no_std]
#![no_main]

use libneodos::syscall::{
    self, ob_access,
    sys_ob_open, sys_close, ObInfoClass,
};

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u64(v: u64) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = v;
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    write_str(&buf[i..]);
}

fn write_u64_hex(v: u64) {
    let mut buf = [0u8; 18];
    buf[0] = b'0';
    buf[1] = b'x';
    let mut n = v;
    let mut i = 18;
    while n > 0 {
        i -= 1;
        let d = (n & 0xF) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'A' + d - 10 };
        n >>= 4;
    }
    if i == 18 { buf[2] = b'0'; i = 2; }
    write_str(&buf[i..]);
}

#[used]
#[link_section = ".rodata"]
static MEM_HELP: &[u8] = b"::HELP::\
MEM\r\n\
  Show memory usage. Displays total, used, and free memory.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nMEM\r\n  Show memory usage. Displays total, used, and free memory.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match sys_ob_open("\\Global\\Info\\Memory", ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\nMemory info not available\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut info = syscall::MemInfo {
        phys_max: 0, total_kib: 0, usable_kib: 0,
        free_kib: 0, used_kib: 0, reserved_kib: 0,
    };
    let info_sz = core::mem::size_of::<syscall::MemInfo>();
    let buf = unsafe {
        core::slice::from_raw_parts_mut(&mut info as *mut syscall::MemInfo as *mut u8, info_sz)
    };
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Memory, buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = sys_close(fd);
            write_str(b"\r\nMemory info read failed\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    let _ = sys_close(fd);

    if n < info_sz {
        write_str(b"\r\nMemory info truncated\r\n\r\n");
        syscall::sys_exit(1);
    }

    let phys_max = info.phys_max;
    let total_kib = info.total_kib;
    let usable_kib = info.usable_kib;
    let free_kib = info.free_kib;
    let used_kib = info.used_kib;
    let reserved_kib = info.reserved_kib;

    write_str(b"\r\n");
    write_str(b"Physical max: 0x");
    write_u64_hex(phys_max);
    write_str(b"\r\n");
    write_str(b"Total:    ");
    write_u64(total_kib);
    write_str(b" KiB\r\n");
    write_str(b"Usable:   ");
    write_u64(usable_kib);
    write_str(b" KiB\r\n");
    write_str(b"Free:     ");
    write_u64(free_kib);
    write_str(b" KiB\r\n");
    write_str(b"Used:     ");
    write_u64(used_kib);
    write_str(b" KiB\r\n");
    write_str(b"Reserved: ");
    write_u64(reserved_kib);
    write_str(b" KiB\r\n\r\n");

    syscall::sys_exit(0)
}
