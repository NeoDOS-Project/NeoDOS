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

const APP_NAME: &str = "coreren";
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_ERR_MISSING_NAME: u32 = 1004;
const IDS_ERR_CANNOT_RENAME: u32 = 1005;
const IDS_ERR_FILE_NOT_FOUND: u32 = 1006;
const IDS_ERR_EINVAL: u32 = 1007;
const IDS_ERR_ENOENT: u32 = 1008;
const IDS_ERR_EACCES: u32 = 1009;
const IDS_ERR_EEXIST: u32 = 1010;
const IDS_ERR_EIO: u32 = 1011;
const IDS_ERR_UNKNOWN: u32 = 1012;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
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

fn normalize_path(input: &[u8]) -> [u8; 260] {
    let path_str = core::str::from_utf8(input).unwrap_or("");
    if path_str.is_empty() {
        return [0u8; 260];
    }
    let bytes = path_str.as_bytes();
    let mut buf = [0u8; 260];
    if bytes[0] == b'\\' || bytes.contains(&b':') {
        let n = bytes.len().min(259);
        buf[..n].copy_from_slice(&bytes[..n]);
    } else {
        let mut cwd_buf = [0u8; 256];
        let mut pos = 0;
        match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 => {
                for &b in &cwd_buf[..n - 1] {
                    if pos < 259 { buf[pos] = b; pos += 1; }
                }
                if pos > 0 && buf[pos - 1] != b'\\' {
                    if pos < 259 { buf[pos] = b'\\'; pos += 1; }
                }
            }
            _ => {
                buf[..3].copy_from_slice(b"C:\\");
                pos = 3;
            }
        }
        for &b in bytes {
            if pos < 259 { buf[pos] = b; pos += 1; }
        }
    }
    buf
}

fn split_first_token(args: &[u8]) -> (&[u8], &[u8]) {
    let trimmed = libneodos::args::trim_ascii(args);
    if trimmed.is_empty() {
        return (b"", b"");
    }
    let mut idx = 0;
    while idx < trimmed.len() && trimmed[idx] != b' ' && trimmed[idx] != b'\t' {
        idx += 1;
    }
    let first = &trimmed[..idx];
    let rest = libneodos::args::trim_ascii(&trimmed[idx..]);
    (first, rest)
}

fn print_usage() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw_args = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw_args);

    if args.is_empty() {
        print_usage();
        syscall::sys_exit(0);
    }

    if libneodos::args::is_help_flag(args) {
        print_usage();
        syscall::sys_exit(0);
    }

    let (old_token, new_token) = split_first_token(args);

    if old_token.is_empty() || new_token.is_empty() {
        write_err(b"\r\n");
        write_err(tr_id!(IDS_ERR_MISSING_NAME).as_bytes());
        write_err(b"\r\n");
        syscall::sys_exit(1);
    }

    let normalized_old = normalize_path(old_token);
    let end_old = normalized_old.iter().position(|&b| b == 0).unwrap_or(normalized_old.len());
    let old_path = core::str::from_utf8(&normalized_old[..end_old]).unwrap_or("C:\\");

    let normalized_new = normalize_path(new_token);
    let end_new = normalized_new.iter().position(|&b| b == 0).unwrap_or(normalized_new.len());
    let new_path = core::str::from_utf8(&normalized_new[..end_new]).unwrap_or("C:\\");

    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(old_path, &mut ob_buf);
    match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => {
            let new_bytes = new_path.as_bytes();
            match syscall::sys_ob_set_info(fd, syscall::ObSetInfoClass::VfsRename, new_bytes) {
                Ok(_) => {
                    let _ = syscall::sys_close(fd);
                    write_str(b"\r\n");
                    syscall::sys_exit(0);
                }
                Err(e) => {
                    let _ = syscall::sys_close(fd);
                    write_err(b"\r\n");
                    write_err(tr_id!(IDS_ERR_CANNOT_RENAME).as_bytes());
                    let err_str: &[u8] = match e {
                        -1 => tr_id!(IDS_ERR_EINVAL).as_bytes(),
                        -2 => tr_id!(IDS_ERR_ENOENT).as_bytes(),
                        -4 => tr_id!(IDS_ERR_EACCES).as_bytes(),
                        -10 => tr_id!(IDS_ERR_EEXIST).as_bytes(),
                        -13 => tr_id!(IDS_ERR_EIO).as_bytes(),
                        _ => tr_id!(IDS_ERR_UNKNOWN).as_bytes(),
                    };
                    write_err(err_str);
                    write_err(b"\r\n");
                    syscall::sys_exit(1);
                }
            }
        }
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_FILE_NOT_FOUND).as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    }
}
