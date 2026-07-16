#![no_std]
#![no_main]

use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "neolocale";
const IDS_TOOL_USAGE: u32 = 1001;
const IDS_TOOL_VALIDATE: u32 = 1002;
const IDS_TOOL_STATS: u32 = 1003;
const IDS_TOOL_DIFF: u32 = 1004;
const IDS_TOOL_CHECK: u32 = 1005;
const IDS_TOOL_CREATE: u32 = 1006;
const IDS_STATUS_VALID: u32 = 1007;
const IDS_STATUS_INVALID: u32 = 1008;
const IDS_ERROR_CANNOT_OPEN: u32 = 1009;
const IDS_ERROR_UNKNOWN_CMD: u32 = 1010;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn print_usage() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_TOOL_USAGE).as_bytes());
    write_str(b"\r\n\r\n");
    write_str(tr_id!(IDS_TOOL_VALIDATE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_TOOL_STATS).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_TOOL_DIFF).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_TOOL_CHECK).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_TOOL_CREATE).as_bytes());
    write_str(b"\r\n\r\n");
}

fn read_nlt_header(path: &str) -> Result<[u8; 32], ()> {
    let mut ob_buf = [0u8; 512];
    let prefix = b"\\Global\\FileSystem\\";
    let total = prefix.len() + path.len();
    if total > 511 { return Err(()); }
    ob_buf[..prefix.len()].copy_from_slice(prefix);
    ob_buf[prefix.len()..total].copy_from_slice(path.as_bytes());
    let ob_path = unsafe { core::str::from_utf8_unchecked(&ob_buf[..total]) };

    let fd = syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ).map_err(|_| ())?;
    let mut header = [0u8; 32];
    let n = syscall::sys_read(fd, &mut header).map_err(|_| ())?;
    let _ = syscall::sys_close(fd);
    if n < 32 { return Err(()); }
    Ok(header)
}

fn cmd_validate(path: &[u8]) {
    let path_str = core::str::from_utf8(path).unwrap_or("");
    if path_str.is_empty() {
        write_err(b"  ");
        write_err(tr_id!(IDS_ERROR_CANNOT_OPEN).as_bytes());
        write_err(b"\r\n");
        return;
    }

    match read_nlt_header(path_str) {
        Ok(header) => {
            let magic = &header[0..4];
            let valid = magic == b"NLT2"
                && u16::from_le_bytes([header[4], header[5]]) == 2
                && u16::from_le_bytes([header[6], header[7]]) >= 32;
            write_str(b"  ");
            if valid {
                write_str(tr_id!(IDS_STATUS_VALID).as_bytes());
            } else {
                write_str(tr_id!(IDS_STATUS_INVALID).as_bytes());
            }
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"  ");
            write_err(tr_id!(IDS_ERROR_CANNOT_OPEN).as_bytes());
            write_err(b"\r\n");
        }
    }
}

fn cmd_stats(path: &[u8]) {
    let path_str = core::str::from_utf8(path).unwrap_or("");
    if path_str.is_empty() {
        write_err(b"  ");
        write_err(tr_id!(IDS_ERROR_CANNOT_OPEN).as_bytes());
        write_err(b"\r\n");
        return;
    }

    match read_nlt_header(path_str) {
        Ok(header) => {
            let magic = &header[0..4];
            if magic != b"NLT2" {
                write_str(b"  ");
                write_str(tr_id!(IDS_STATUS_INVALID).as_bytes());
                write_str(b"\r\n");
                return;
            }
            let count = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
            let lang_id = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            let app_id = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);

            write_str(b"  Strings: ");
            write_u32(count);
            write_str(b", Language ID: ");
            write_u32(lang_id);
            write_str(b", App ID: ");
            write_u32(app_id);
            write_str(b"\r\n");
        }
        Err(_) => {
            write_str(b"  ");
            write_str(tr_id!(IDS_STATUS_INVALID).as_bytes());
            write_str(b"\r\n");
        }
    }
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

fn cmd_check(args: &[u8]) {
    write_str(b"  ");
    write_str(tr_id!(IDS_TOOL_CHECK).as_bytes());
    write_str(b"\r\n");
}

fn cmd_diff(args: &[u8]) {
    write_str(b"  ");
    write_str(tr_id!(IDS_TOOL_DIFF).as_bytes());
    write_str(b"\r\n");
}

fn cmd_create(args: &[u8]) {
    write_str(b"  ");
    write_str(tr_id!(IDS_TOOL_CREATE).as_bytes());
    write_str(b"\r\n");
}

fn is_cmd(a: &[u8], b: &[u8]) -> bool {
    if a.len() < b.len() { return false; }
    a[..b.len()].eq_ignore_ascii_case(b)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_usage();
        syscall::sys_exit(0);
    }

    let args = libneodos::args::trim_ascii(&raw);
    if args.is_empty() {
        print_usage();
        syscall::sys_exit(0);
    }

    let space = args.iter().position(|&b| b == b' ' || b == b'\t');
    let (cmd, rest) = if let Some(pos) = space {
        (&args[..pos], libneodos::args::trim_ascii(&args[pos + 1..]))
    } else {
        (args, &[][..])
    };

    if is_cmd(cmd, b"validate") {
        cmd_validate(rest);
    } else if is_cmd(cmd, b"stats") {
        cmd_stats(rest);
    } else if is_cmd(cmd, b"diff") {
        cmd_diff(rest);
    } else if is_cmd(cmd, b"check") {
        cmd_check(rest);
    } else if is_cmd(cmd, b"create") {
        cmd_create(rest);
    } else {
        write_err(b"\r\n");
        write_err(tr_id!(IDS_ERROR_UNKNOWN_CMD).as_bytes());
        write_err(b"\r\n\r\n");
        syscall::sys_exit(1);
    }

    syscall::sys_exit(0)
}
