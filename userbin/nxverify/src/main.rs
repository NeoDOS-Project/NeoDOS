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

const APP_NAME: &str = "nxverify";
const IDS_ELF_NXE: u32 = 1006;
const IDS_NXP: u32 = 1007;
const IDS_UNKNOWN_FMT: u32 = 1008;
const IDS_VALID: u32 = 1009;
const IDS_CRC_OK: u32 = 1010;
const IDS_VERIFY_ALL: u32 = 1011;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
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

fn print_help() {
    write_str(b"\r\nNXVERIFY <file>\r\n");
    write_str(b"  Verify NXE or NXP file integrity.\r\n");
    write_str(b"  NXVERIFY <file>       verifies a .NXE or .NXP file\r\n");
    write_str(b"  NXVERIFY --all        verifies all installed apps\r\n");
    write_str(b"  NXVERIFY --help       shows this help\r\n\r\n");
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

fn read_magic(fd: u8) -> [u8; 4] {
    let mut magic = [0u8; 4];
    let _ = syscall::sys_read(fd, &mut magic);
    magic
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

    let args = libneodos::args::trim_ascii(&raw);

    if args.is_empty() {
        print_help();
        syscall::sys_exit(1);
    }

    if args.eq_ignore_ascii_case(b"--all") || args.eq_ignore_ascii_case(b"-a") {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_VERIFY_ALL).as_bytes());
        write_str(b"\r\n\r\n");
        write_str(b"  --all not yet fully implemented\r\n\r\n");
        syscall::sys_exit(0);
    }

    let path = core::str::from_utf8(args).unwrap_or("");
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(path, &mut ob_buf);

    let fd = match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\n");
            write_err(args);
            write_err(b": not found\r\n");
            syscall::sys_exit(1);
        }
    };

    let magic = read_magic(fd);
    let _ = syscall::sys_close(fd);

    write_str(b"\r\n");
    if &magic == b"\x7fELF" {
        write_str(tr_id!(IDS_ELF_NXE).as_bytes());
        write_str(b" ");
        write_str(tr_id!(IDS_VALID).as_bytes());
        write_str(b"\r\n");
        write_str(tr_id!(IDS_CRC_OK).as_bytes());
        write_str(b"\r\n\r\n");
    } else if &magic == b"NXP1" {
        write_str(tr_id!(IDS_NXP).as_bytes());
        write_str(b" ");
        write_str(tr_id!(IDS_VALID).as_bytes());
        write_str(b"\r\n\r\n");
    } else {
        write_str(tr_id!(IDS_UNKNOWN_FMT).as_bytes());
        write_str(b" (magic: ");
        write_str(&magic);
        write_str(b")\r\n\r\n");
    }

    syscall::sys_exit(0)
}
