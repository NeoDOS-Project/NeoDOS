#![no_std]
#![no_main]

use libneodos::syscall;

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

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

#[used]
#[link_section = ".rodata"]
static COPY_HELP: &[u8] = b"::HELP::\
COPY [drive:][path]src [drive:][path]dst\r\n\
  Copy a file from source to destination.\r\n\
  COPY C:\\file.txt D:\\backup.txt\r\n\
::END::";

fn normalize_path(input: &[u8]) -> [u8; 260] {
    let path_str = core::str::from_utf8(input).unwrap_or("");
    if path_str.is_empty() {
        let mut buf = [0u8; 260];
        let mut cwd_buf = [0u8; 256];
        match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 => {
                let mut pos = 0;
                for &b in &cwd_buf[..n - 1] {
                    if pos < 259 { buf[pos] = b; pos += 1; }
                }
                if pos < 259 { buf[pos] = 0; }
            }
            _ => {
                buf[..3].copy_from_slice(b"C:\\");
            }
        }
        return buf;
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
    write_str(b"\r\nUsage: COPY [drive:][path]src [drive:][path]dst\r\n");
    write_str(b"  Copy a file.\r\n");
    write_str(b"  COPY C:\\file.txt D:\\backup.txt\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
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

    let (src_token, dst_token) = split_first_token(args);

    if src_token.is_empty() || dst_token.is_empty() {
        write_err(b"\r\nCOPY: missing source or destination\r\n");
        syscall::sys_exit(1);
    }

    let normalized_src = normalize_path(src_token);
    let end_src = normalized_src.iter().position(|&b| b == 0).unwrap_or(normalized_src.len());
    let src_path = core::str::from_utf8(&normalized_src[..end_src]).unwrap_or("C:\\");

    let normalized_dst = normalize_path(dst_token);
    let end_dst = normalized_dst.iter().position(|&b| b == 0).unwrap_or(normalized_dst.len());
    let dst_path = core::str::from_utf8(&normalized_dst[..end_dst]).unwrap_or("C:\\");

    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(src_path, &mut ob_buf);
    let src_fd = match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\nCOPY: source file not found\r\n");
            syscall::sys_exit(1);
        }
    };

    // Remove destination if it exists (via Ob)
    {
        let mut ob_buf2 = [0u8; 512];
        let ob_dst = to_ob_path(dst_path, &mut ob_buf2);
        if let Ok(old_fd) = syscall::sys_ob_open(ob_dst, libneodos::syscall::ob_access::READ) {
            let _ = syscall::ob_file_delete(old_fd);
            let _ = syscall::sys_close(old_fd);
        }
    }

    let dst_fd = match syscall::ob_file_create(dst_path) {
        Ok(f) => f,
        Err(e) => {
            write_err(b"\r\nCOPY: cannot create destination: ");
            let err_str: &[u8] = match e {
                -1 => b"EINVAL",
                -2 => b"ENOENT",
                -4 => b"EACCES",
                -5 => b"EBADF",
                -13 => b"EIO",
                _ => b"UNKNOWN",
            };
            write_err(err_str);
            write_err(b"\r\n");
            let _ = syscall::sys_close(src_fd);
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; 4096];
    loop {
        match syscall::sys_ob_query_info(src_fd, libneodos::syscall::ObInfoClass::ReadContent, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if syscall::sys_ob_set_info(dst_fd, libneodos::syscall::ob_set_info_class::WRITE_CONTENT, &buf[..n]).is_err() {
                    write_err(b"\r\nCOPY: write error\r\n");
                    break;
                }
            }
            Err(e) => {
                write_err(b"\r\nCOPY: read error code=");
                let err_str: &[u8] = match e {
                    -1 => b"-1",
                    -2 => b"-2",
                    -3 => b"-3",
                    -4 => b"-4",
                    -5 => b"-5",
                    -6 => b"-6",
                    -7 => b"-7",
                    -8 => b"-8",
                    -13 => b"-13",
                    n if n < 0 => b"<-1",
                    _ => b"UNKN",
                };
                write_err(err_str);
                write_err(b"\r\n");
                break;
            }
        }
    }

    let _ = syscall::sys_close(src_fd);
    let _ = syscall::sys_close(dst_fd);
    syscall::sys_exit(0)
}
