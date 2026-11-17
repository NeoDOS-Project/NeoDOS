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
use libneodos::syscall::{ObInfoClass, ob_access};
use libneodos::tr_id;

const APP_NAME: &str = "drives";
const IDS_ERR_LISTING: u32 = 1003;
const IDS_ERR_NONE: u32 = 1004;
const IDS_HEADER: u32 = 1005;
const IDS_NO_LABEL: u32 = 1006;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

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
    if end == 0 { return tr_id!(IDS_NO_LABEL); }
    core::str::from_utf8(&label[..end]).unwrap_or("")
}

/// Format a sector count as a human-readable size via the shared units
/// service in `math.nxl` (`libmath`).
fn write_size(sectors: u64) {
    let mut buf = [0u8; 32];
    let n = libmath::format_size(sectors.saturating_mul(512), &mut buf);
    if n > 0 {
        write_str(&buf[..n]);
    }
}

fn print_help() {
    write_str(b"\r\n");
    write_str(b"DRIVES\r\n");
    write_str(b"  Lists all mounted drives.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match syscall::sys_ob_open("\\Global\\Info\\Drives", ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_LISTING).as_bytes());
            write_str(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; 58 * 26];
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Drives, &mut buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = syscall::sys_close(fd);
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_LISTING).as_bytes());
            write_str(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    };
    let _ = syscall::sys_close(fd);

    if n == 0 {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_ERR_NONE).as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(0);
    }

    let entry_size = core::mem::size_of::<DriveInfo>();
    let count = n / entry_size;

    write_str(b"\r\n");
    write_str(tr_id!(IDS_HEADER).as_bytes());
    write_str(b"\r\n");
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
