#![no_std]
#![no_main]

use libneodos::syscall;

const ARGS_ADDR: u64 = 0x41F000;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

#[used]
#[link_section = ".rodata"]
static DEL_HELP: &[u8] = b"::HELP::\
DEL [drive:][path]filename\r\n\
  Delete a file.\r\n\
  DEL C:\\file.txt\r\n\
::END::";

fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && matches!(s[start], b' ' | b'\t' | b'\r' | b'\n' | 0) {
        start += 1;
    }
    let mut end = s.len();
    while end > start && matches!(s[end - 1], b' ' | b'\t' | b'\r' | b'\n' | 0) {
        end -= 1;
    }
    &s[start..end]
}

fn read_args() -> [u8; 256] {
    let mut buf = [0u8; 256];
    unsafe {
        core::ptr::copy_nonoverlapping(ARGS_ADDR as *const u8, buf.as_mut_ptr(), buf.len());
    }
    buf
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

fn print_usage() {
    write_str(b"\r\nUsage: DEL [drive:][path]filename\r\n");
    write_str(b"  Delete a file.\r\n");
    write_str(b"  DEL C:\\file.txt\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw_args = read_args();
    let args = trim_ascii(&raw_args);

    if args.is_empty() {
        print_usage();
        syscall::sys_exit(0);
    }

    if args == b"/?" || args == b"-h" || args == b"--help" {
        print_usage();
        syscall::sys_exit(0);
    }

    let normalized = normalize_path(args);
    let end = normalized.iter().position(|&b| b == 0).unwrap_or(normalized.len());
    let path = core::str::from_utf8(&normalized[..end]).unwrap_or("C:\\");

    match syscall::sys_unlink(path) {
        Ok(_) => {
            write_str(b"\r\n");
            syscall::sys_exit(0);
        }
        Err(e) => {
            write_err(b"\r\nDEL: cannot delete: ");
            let err_str: &[u8] = match e {
                -1 => b"EINVAL",
                -2 => b"ENOENT",
                -4 => b"EACCES",
                -11 => b"ENOTDIR",
                -12 => b"EISDIR",
                -13 => b"EIO",
                _ => b"UNKNOWN",
            };
            write_err(err_str);
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    }
}
