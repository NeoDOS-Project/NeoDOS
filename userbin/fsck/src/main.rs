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
use libneodos::tr_id;

const APP_NAME: &str = "fsck";
const IDS_ERR_OPEN: u32 = 1004;
const IDS_ERR_GENERIC: u32 = 1005;
const IDS_INVALID: u32 = 1006;
const IDS_CHECKING: u32 = 1007;
const IDS_HEADER: u32 = 1008;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn current_drive() -> u8 {
    let mut buf = [0u8; 64];
    match syscall::sys_getcwd(&mut buf) {
        Ok(n) if n >= 2 && buf[1] == b':' => buf[0],
        _ => b'C',
    }
}

fn read_args() -> [u8; 256] {
    let ptr = 0x41F000 as *const u8;
    let mut buf = [0u8; 256];
    unsafe {
        let mut i = 0;
        while i < 255 {
            let b = ptr.add(i).read();
            buf[i] = b;
            if b == 0 { break; }
            i += 1;
        }
    }
    buf
}

fn is_help_flag(buf: &[u8; 256]) -> bool {
    let s = unsafe { core::str::from_utf8_unchecked(buf) };
    let s = s.trim();
    s.eq_ignore_ascii_case("/?") || s.eq_ignore_ascii_case("-h") || s.eq_ignore_ascii_case("--help")
}

fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\t') {
        start += 1;
    }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t') {
        end -= 1;
    }
    &s[start..end]
}

fn to_ob_path<'a>(vfs: &'a str, buf: &'a mut [u8; 512]) -> &'a str {
    let prefix = b"\\Global\\FileSystem\\";
    let vfs_bytes = vfs.as_bytes();
    let total = prefix.len() + vfs_bytes.len();
    if total > 510 { return vfs; }
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..total].copy_from_slice(vfs_bytes);
    buf[total] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..total]) }
}

fn print_help() {
    write_str(b"\r\nFSCK [drive:] [/F]\r\n  Check filesystem integrity on a NeoDOS volume.\r\n  Without /F, only checks and reports errors.\r\n  With /F, attempts to repair detected issues.\r\n\r\n  FSCK C:             check-only on C:\r\n  FSCK C: /F          check and repair C:\r\n\r\n");
}

fn write_u32(mut v: u32) {
    if v == 0 { write_str(b"0"); return; }
    let mut buf = [0u8; 10];
    let mut i = 9;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

fn print_report(stats: &libneodos::syscall::FsckStats, drive: u8) {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_CHECKING).as_bytes());
    write_str(&[drive, b':']);
    write_str(b"\r\n\r\n");
    write_str(tr_id!(IDS_HEADER).as_bytes());
    write_str(b"\r\n");
    write_str(b"  Blocks: ");
    write_u32(stats.total_blocks as u32);
    write_str(b" total, ");
    write_u32(stats.used_blocks as u32);
    write_str(b" used, ");
    write_u32(stats.free_blocks as u32);
    write_str(b" free\r\n");
    write_str(b"  Nodes: ");
    write_u32(stats.total_nodes as u32);
    write_str(b" total, ");
    write_u32(stats.total_dirs as u32);
    write_str(b" dirs, ");
    write_u32(stats.total_files as u32);
    write_str(b" files\r\n");
    if stats.errors > 0 || stats.warnings > 0 {
        write_str(b"  Errors: ");
        write_u32(stats.errors);
        write_str(b" errors, ");
        write_u32(stats.warnings);
        write_str(b" warnings, ");
        write_u32(stats.repaired);
        write_str(b" repaired\r\n");
    }
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let args = read_args();
    if is_help_flag(&args) {
        print_help();
        syscall::sys_exit(0);
    }

    let arg_str = {
        let end = args.iter().position(|&b| b == 0).unwrap_or(0);
        trim_ascii(&args[..end])
    };

    let drive = if !arg_str.is_empty() && arg_str.len() >= 2 && arg_str[1] == b':' {
        arg_str[0].to_ascii_uppercase()
    } else {
        current_drive()
    };

    let repair = arg_str.iter().any(|&b| b == b'/' || b == b'-')
        && arg_str.iter().any(|&b| b == b'F' || b == b'f');

    let drive_str = [drive, b':', b'\\'];
    let vfs_str = core::str::from_utf8(&drive_str).unwrap_or("C:\\");
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(vfs_str, &mut ob_buf);

    let fd = match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_OPEN).as_bytes());
            write_err(&[drive]);
            write_err(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    if repair {
        let _ = syscall::ob_fsck_repair(fd, true);
    }

    match syscall::ob_fsck_status(fd) {
        Ok(stats) => {
            print_report(&stats, drive);
        }
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_GENERIC).as_bytes());
            write_err(tr_id!(IDS_INVALID).as_bytes());
            write_err(b"\r\n\r\n");
        }
    }

    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0)
}
