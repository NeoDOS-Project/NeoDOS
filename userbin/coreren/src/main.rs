#![no_std]
#![no_main]

use libneodos::syscall;

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

#[used]
#[link_section = ".rodata"]
static REN_HELP: &[u8] = b"::HELP::\
REN [drive:][path]oldname [drive:][path]newname\r\n\
  Rename a file or directory.\r\n\
  REN C:\\old.txt C:\\new.txt\r\n\
::END::";

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
    write_str(b"\r\nUsage: REN [drive:][path]oldname [drive:][path]newname\r\n");
    write_str(b"  Rename a file or directory.\r\n");
    write_str(b"  REN C:\\old.txt C:\\new.txt\r\n");
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

    let (old_token, new_token) = split_first_token(args);

    if old_token.is_empty() || new_token.is_empty() {
        write_err(b"\r\nREN: missing old or new name\r\n");
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
            match syscall::sys_ob_set_info(fd, 6, new_bytes) {
                Ok(_) => {
                    let _ = syscall::sys_close(fd);
                    write_str(b"\r\n");
                    syscall::sys_exit(0);
                }
                Err(e) => {
                    let _ = syscall::sys_close(fd);
                    write_err(b"\r\nREN: cannot rename: ");
                    let err_str: &[u8] = match e {
                        -1 => b"EINVAL",
                        -2 => b"ENOENT",
                        -4 => b"EACCES",
                        -10 => b"EEXIST",
                        -13 => b"EIO",
                        _ => b"UNKNOWN",
                    };
                    write_err(err_str);
                    write_err(b"\r\n");
                    syscall::sys_exit(1);
                }
            }
        }
        Err(_) => {
            write_err(b"\r\nREN: file not found\r\n");
            syscall::sys_exit(1);
        }
    }
}
