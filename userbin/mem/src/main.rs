#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::MemInfo;

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
    let ptr = 0x41F000 as *const u8;
    let mut arg_buf = [0u8; 32];
    unsafe {
        let mut i = 0;
        while i < 31 {
            let b = ptr.add(i).read();
            arg_buf[i] = b;
            if b == 0 { break; }
            i += 1;
        }
    }
    let args = core::str::from_utf8(&arg_buf).unwrap_or("");
    let trimmed = args.trim();
    if trimmed == "/?" || trimmed == "-h" || trimmed == "--help" {
        print_help();
        syscall::sys_exit(0);
    }
    let mut info = MemInfo {
        phys_max: 0, total_kib: 0, usable_kib: 0,
        free_kib: 0, used_kib: 0, reserved_kib: 0,
    };

    match syscall::sys_get_meminfo(&mut info) {
        Ok(_) => {
            write_str(b"\r\n");
            write_str(b"Physical max: 0x");
            write_u64_hex(info.phys_max);
            write_str(b"\r\n");
            write_str(b"Total:    ");
            write_u64(info.total_kib);
            write_str(b" KiB\r\n");
            write_str(b"Usable:   ");
            write_u64(info.usable_kib);
            write_str(b" KiB\r\n");
            write_str(b"Free:     ");
            write_u64(info.free_kib);
            write_str(b" KiB\r\n");
            write_str(b"Used:     ");
            write_u64(info.used_kib);
            write_str(b" KiB\r\n");
            write_str(b"Reserved: ");
            write_u64(info.reserved_kib);
            write_str(b" KiB\r\n\r\n");
        }
        Err(_) => {
            write_str(b"\r\nMemory info not available\r\n\r\n");
        }
    }
    syscall::sys_exit(0)
}
