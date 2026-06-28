#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::syscall;
use libneodos::syscall::{ObInfoClass, ob_access};

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[used]
#[link_section = ".rodata"]
static DRIVES_HELP: &[u8] = b"::HELP::\
DRIVES\r\n\
  Lists all mounted drives.\r\n\
::END::";

#[repr(C)]
#[derive(Clone, Copy)]
struct DriveInfo {
    letter: u8,
    present: u8,
    fs_type: [u8; 16],
    label: [u8; 32],
    total_sectors: u64,
}

fn fs_type_str(fs_type: &[u8; 16]) -> &str {
    let end = fs_type.iter().position(|&b| b == 0).unwrap_or(16);
    core::str::from_utf8(&fs_type[..end]).unwrap_or("Unknown")
}

fn label_str(label: &[u8; 32]) -> &str {
    let end = label.iter().position(|&b| b == 0).unwrap_or(32);
    if end == 0 { return "(no label)"; }
    core::str::from_utf8(&label[..end]).unwrap_or("")
}

fn write_num(n: u64) {
    if n == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut v = n;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&buf[i..]);
}

fn write_size(sectors: u64) {
    let bytes = sectors * 512;
    if bytes >= 1024 * 1024 * 1024 {
        let gb = bytes / (1024 * 1024 * 1024);
        let rem = (bytes % (1024 * 1024 * 1024)) * 100 / (1024 * 1024 * 1024);
        write_num(gb);
        write_str(b".");
        if rem < 10 { write_str(b"0"); }
        write_num(rem);
        write_str(b" GB");
    } else if bytes >= 1024 * 1024 {
        let mb = bytes / (1024 * 1024);
        write_num(mb);
        write_str(b" MB");
    } else if bytes >= 1024 {
        let kb = bytes / 1024;
        write_num(kb);
        write_str(b" KB");
    } else {
        write_str(b"0 B");
    }
}

fn print_help() {
    write_str(b"\r\nDRIVES\r\n  Lists all mounted drives.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match syscall::sys_ob_open("\\Global\\Info\\Drives", ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\nError listing drives\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; 58 * 26]; // 26 drives max
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Drives, &mut buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = syscall::sys_close(fd);
            write_str(b"\r\nError listing drives\r\n\r\n");
            syscall::sys_exit(1);
        }
    };
    let _ = syscall::sys_close(fd);

    if n == 0 {
        write_str(b"\r\nNo drives found\r\n\r\n");
        syscall::sys_exit(0);
    }

    let entry_size = core::mem::size_of::<DriveInfo>();
    let count = n / entry_size;

    write_str(b"\r\nMounted drives:\r\n");
    let drives = unsafe {
        core::slice::from_raw_parts(buf.as_ptr() as *const DriveInfo, count)
    };
    for d in drives {
        if d.present == 0 { continue; }
        let letter = d.letter as char;
        let fstype = fs_type_str(&d.fs_type);
        let label = label_str(&d.label);

        write_str(b"  ");
        write_str(&[letter as u8, b':']);
        write_str(b"  ");
        write_str(fstype.as_bytes());
        write_str(b"  ");
        write_str(label.as_bytes());
        write_str(b"  ");
        write_size(d.total_sectors);
        write_str(b"\r\n");
    }
    write_str(b"\r\n");
    syscall::sys_exit(0)
}
