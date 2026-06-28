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

fn write_u32(mut v: u32) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 9;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

fn resolve_path(path: &[u8]) -> [u8; 256] {
    let mut full = [0u8; 256];
    let path = trim_ascii(path);

    if path.is_empty() {
        return full;
    }

    // If path has a drive letter (e.g. C:\) or starts with \, use as-is
    if (path.len() >= 2 && path[1] == b':') || path[0] == b'\\' {
        let copy_len = path.len().min(255);
        full[..copy_len].copy_from_slice(&path[..copy_len]);
        return full;
    }

    // Otherwise, prepend C:\System\Drivers\
    let prefix = b"C:\\System\\Drivers\\";
    full[..prefix.len()].copy_from_slice(prefix);
    let path_copy_len = path.len().min(255 - prefix.len());
    full[prefix.len()..prefix.len() + path_copy_len].copy_from_slice(&path[..path_copy_len]);

    full
}

#[used]
#[link_section = ".rodata"]
static LOADNEM_HELP: &[u8] = b"::HELP::\
LOADNEM <path> [/U]\r\n\
  Load or unload a NEM driver.\r\n\
  LOADNEM disk.nem          Load from C:\\System\\Drivers\\\r\n\
  LOADNEM C:\\path\\driver.nem Load from full path\r\n\
  LOADNEM driver.nem /U     Unload driver by name\r\n\
::END::";

fn cmd_load(path: &[u8]) {
    let full_path = resolve_path(path);
    let full_path_str = {
        let end = full_path.iter().position(|&b| b == 0).unwrap_or(0);
        unsafe { core::str::from_utf8_unchecked(&full_path[..end]) }
    };

    write_str(b"\r\nLoading driver: ");
    write_str(full_path_str.as_bytes());
    write_str(b"...\r\n");

    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(full_path_str, &mut ob_buf);
    match syscall::sys_ob_create(ob_path, libneodos::syscall::ob_type::DRIVER, None, 0) {
        Ok(fd) => {
            write_str(b"Driver loaded successfully, fd=");
            write_u32(fd as u32);
            write_str(b"\r\n");
        }
        Err(e) => {
            write_err(b"Failed to load driver (error ");
            write_u32((-e) as u32);
            write_err(b")\r\n");
        }
    }
    write_str(b"\r\n");
}

fn cmd_unload(name: &[u8]) {
    let name_str = unsafe { core::str::from_utf8_unchecked(name) };
    let name_str = name_str.trim();

    write_str(b"\r\nUnloading driver: ");
    write_str(name_str.as_bytes());
    write_str(b"...\r\n");

    match syscall::sys_driver_unload(name_str, false) {
        Ok(()) => {
            write_str(b"Driver unloaded successfully.\r\n");
        }
        Err(e) => {
            write_err(b"Failed to unload driver (error ");
            write_u32((-e) as u32);
            write_err(b")\r\n");
        }
    }
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let args = read_args();
    if is_help_flag(&args) {
        write_str(b"\r\nLOADNEM <path> [/U]\r\n  Load or unload a NEM driver.\r\n  LOADNEM disk.nem          Load from C:\\System\\Drivers\\\r\n  LOADNEM C:\\path\\driver.nem Load from full path\r\n  LOADNEM driver.nem /U     Unload driver by name\r\n\r\n");
        syscall::sys_exit(0);
    }

    let arg_str = {
        let end = args.iter().position(|&b| b == 0).unwrap_or(0);
        trim_ascii(&args[..end])
    };

    if arg_str.is_empty() {
        write_err(b"\r\nUsage: LOADNEM <path> [/U]\r\n\r\n");
        syscall::sys_exit(1);
    }

    // Check for /U flag (unload)
    let first_space = arg_str.iter().position(|&b| b == b' ' || b == b'\t').unwrap_or(arg_str.len());
    let cmd = &arg_str[..first_space];
    let rest = trim_ascii(&arg_str[first_space..]);

    // Check if rest is /U
    let is_unload = rest.len() >= 2 && (rest[0] == b'/' || rest[0] == b'-') && (rest[1] == b'U' || rest[1] == b'u');

    if is_unload {
        cmd_unload(cmd);
    } else {
        cmd_load(arg_str);
    }

    syscall::sys_exit(0)
}
