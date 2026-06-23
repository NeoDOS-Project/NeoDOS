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
static TYPE_HELP: &[u8] = b"::HELP::\
TYPE [drive:][path]filename\r\n\
  Display the contents of a text file on screen.\r\n\
  TYPE C:\\readme.txt   shows the readme file.\r\n\
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

fn print_usage() {
    write_str(b"\r\nUsage: TYPE [drive:][path]filename\r\n");
    write_str(b"  Display the contents of a text file.\r\n");
    write_str(b"  TYPE C:\\Programs\\test.txt\r\n");
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

    let path_args = if args.len() >= 2
        && ((args[0] == b'"' && args[args.len() - 1] == b'"')
            || (args[0] == b'\'' && args[args.len() - 1] == b'\''))
    {
        &args[1..args.len() - 1]
    } else {
        args
    };

    let normalized = normalize_path(path_args);
    let end = normalized.iter().position(|&b| b == 0).unwrap_or(normalized.len());
    let path = core::str::from_utf8(&normalized[..end]).unwrap_or("C:\\");
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(path, &mut ob_buf);
    let fd = match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\nFile not found\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; 512];
    loop {
        match syscall::sys_readfile(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = syscall::sys_write(1, &buf[..n]);
            }
            Err(e) => {
                write_err(b"\r\nError reading file: ");
                let err_str: &[u8] = match e {
                    -1 => b"EINVAL" as &[u8],
                    -2 => b"ENOENT" as &[u8],
                    -3 => b"ENOMEM" as &[u8],
                    -4 => b"EACCES" as &[u8],
                    -5 => b"EBADF" as &[u8],
                    _ => b"UNKNOWN" as &[u8],
                };
                write_err(err_str);
                write_err(b"\r\n");
                break;
            }
        }
    }

    write_str(b"\r\n");
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}
