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

const ARGS_ADDR: u64 = 0x41F000;

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

fn write_result(path: &[u8]) {
    unsafe {
        let dst = ARGS_ADDR as *mut u8;
        core::ptr::write_bytes(dst, 0, 256);
        let len = path.len().min(255);
        core::ptr::copy_nonoverlapping(path.as_ptr(), dst, len);
        dst.add(len).write(0);
    }
}

fn normalize_path(input: &[u8]) -> [u8; 260] {
    let mut out = [0u8; 260];
    let mut pos = 0usize;
    let mut drive_len = 0usize;
    let mut start = 0usize;

    if input.len() >= 2 && input[1] == b':' {
        let drive = input[0];
        if pos < 259 {
            out[pos] = if drive >= b'a' && drive <= b'z' { drive - 32 } else { drive };
            pos += 1;
        }
        if pos < 259 {
            out[pos] = b':';
            pos += 1;
        }
        drive_len = 2;
        start = 2;
    }

    let mut parts: [&[u8]; 32] = [&[]; 32];
    let mut count = 0usize;
    let rest = &input[start..];

    let absolute = rest.starts_with(&[b'\\']) || rest.starts_with(&[b'/']);
    if !absolute {
        let mut cwd_buf = [0u8; 256];
        if let Ok(n) = syscall::sys_getcwd(&mut cwd_buf) {
            let cwd = libneodos::args::trim_ascii(&cwd_buf[..n]);
            let cwd_path = core::str::from_utf8(cwd).unwrap_or("C:\\");
            let cwd_bytes = cwd_path.as_bytes();
            if cwd_bytes.len() >= 2 && cwd_bytes[1] == b':' {
                out[0] = cwd_bytes[0];
                out[1] = b':';
                pos = 2;
            }
            let mut i = 2usize;
            while i < cwd_bytes.len() {
                let b = cwd_bytes[i];
                if (b == b'\\' || b == b'/') && pos > 0 && out[pos - 1] != b'\\' {
                    if pos < 259 {
                        out[pos] = b'\\';
                        pos += 1;
                    }
                } else if b != b':' {
                    if pos < 259 {
                        out[pos] = b;
                        pos += 1;
                    }
                }
                i += 1;
            }
        }
    }

    let mut i = 0usize;
    let mut comp_start = 0usize;
    while i <= rest.len() {
        let end = if i == rest.len() || rest[i] == b'\\' || rest[i] == b'/' {
            i
        } else {
            i += 1;
            continue;
        };

        if end > comp_start {
            let comp = &rest[comp_start..end];
            if comp == b"." {
                // skip
            } else if comp == b".." {
                while pos > drive_len && out[pos - 1] == b'\\' {
                    pos -= 1;
                }
                while pos > drive_len && out[pos - 1] != b'\\' {
                    pos -= 1;
                }
            } else if count < parts.len() {
                parts[count] = comp;
                count += 1;
            }
        }

        i += 1;
        comp_start = i;
    }

    for idx in 0..count {
        if pos > drive_len && out[pos - 1] != b'\\' {
            if pos < 259 {
                out[pos] = b'\\';
                pos += 1;
            }
        } else if pos == 0 {
            if pos < 259 {
                out[pos] = b'\\';
                pos += 1;
            }
        }

        for &b in parts[idx] {
            if pos < 259 {
                out[pos] = b;
                pos += 1;
            }
        }
    }

    if pos == 0 {
        out[0] = b'C';
        out[1] = b':';
        out[2] = b'\\';
        pos = 3;
    } else if out[pos - 1] != b'\\' {
        if pos < 259 {
            out[pos] = b'\\';
            pos += 1;
        }
    }

    if pos < 260 {
        out[pos] = 0;
    }

    out
}

fn validate_directory(path: &str) -> bool {
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(path, &mut ob_buf);
    match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => {
            let _ = syscall::sys_close(fd);
            true
        }
        Err(_) => false,
    }
}

fn print_usage() {
    write_str(b"\r\nUsage: CD [path]\r\n");
    write_str(b"       CD             shows current directory\r\n");
    write_str(b"       CD C:\\Path     changes the shell cwd\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw_args = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw_args);

    if args.is_empty() {
        let mut cwd_buf = [0u8; 256];
        if let Ok(n) = syscall::sys_getcwd(&mut cwd_buf) {
            write_result(libneodos::args::trim_ascii(&cwd_buf[..n]));
        } else {
            write_result(b"C:\\");
        }
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

    if !validate_directory(path) {
        write_err(b"\r\ncd: directory not found\r\n");
        syscall::sys_exit(1);
    }

    write_result(path.as_bytes());
    syscall::sys_exit(0);
}
