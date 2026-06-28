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

fn parse_drive(args: &[u8]) -> Option<(u8, &[u8])> {
    let args = trim_ascii(args);
    if args.len() >= 2 && args[1] == b':' {
        let drive = args[0].to_ascii_uppercase();
        let rest = trim_ascii(&args[2..]);
        Some((drive, rest))
    } else {
        None
    }
}

#[used]
#[link_section = ".rodata"]
static LABEL_HELP: &[u8] = b"::HELP::\
LABEL [drive:][label]\r\n\
  Display or change the volume label of a drive.\r\n\
  LABEL C:          shows current label on C:\r\n\
  LABEL C:MYDISK    sets C: label to MYDISK\r\n\
  LABEL MYDISK      sets current drive label to MYDISK\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nLABEL [drive:][label]\r\n  Display or change the volume label of a drive.\r\n  LABEL C:          shows current label on C:\r\n  LABEL C:MYDISK    sets C: label to MYDISK\r\n  LABEL MYDISK      sets current drive label to MYDISK\r\n\r\n");
}



#[no_mangle]
pub extern "C" fn _start() -> ! {
    let args = read_args();
    if is_help_flag(&args) {
        print_help();
        syscall::sys_exit(0);
    }

    let arg_str = {
        let end = args.iter().position(|&b| b == 0).unwrap_or(0);
        &args[..end]
    };

    let arg_str = trim_ascii(arg_str);

    if arg_str.is_empty() {
        let drive = current_drive();
        show_label(drive);
        syscall::sys_exit(0);
    }

    if let Some((drive, rest)) = parse_drive(arg_str) {
        if drive < b'A' || drive > b'Z' {
            write_err(b"\r\nInvalid drive letter.\r\n");
            syscall::sys_exit(1);
        }
        if rest.is_empty() {
            show_label(drive);
        } else {
            set_label(drive, rest);
        }
    } else {
        let drive = current_drive();
        set_label(drive, arg_str);
    }

    syscall::sys_exit(0)
}

fn show_label(drive: u8) {
    let mut label_buf = [0u8; 64];
    let vfs_bytes = [drive, b':', b'\\'];
    let vfs_str = core::str::from_utf8(&vfs_bytes).unwrap_or("C:\\");
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(vfs_str, &mut ob_buf);
    match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => {
            match syscall::sys_ob_query_info(fd, libneodos::syscall::ObInfoClass::VolumeLabel, &mut label_buf) {
                Ok(n) if n > 0 => {
                    let actual = label_buf[..n].iter().position(|&b| b == 0).unwrap_or(n);
                    write_str(b"\r\n Volume in drive ");
                    write_str(&[drive]);
                    write_str(b" is ");
                    write_str(&label_buf[..actual]);
                    write_str(b"\r\n\r\n");
                }
                _ => {
                    write_str(b"\r\n Volume in drive ");
                    write_str(&[drive]);
                    write_str(b" has no label\r\n\r\n");
                }
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {
            write_str(b"\r\n Volume in drive ");
            write_str(&[drive]);
            write_str(b" has no label\r\n\r\n");
        }
    }
}

fn set_label(drive: u8, label: &[u8]) {
    let label = trim_ascii(label);
    if label.is_empty() {
        write_err(b"\r\nInvalid volume label.\r\n");
        return;
    }
    if label.len() > 11 {
        write_err(b"\r\nVolume label must be 11 characters or fewer.\r\n");
        return;
    }
    if !label.iter().all(|&b| b.is_ascii() && b >= 0x20) {
        write_err(b"\r\nInvalid volume label.\r\n");
        return;
    }

    let vfs_bytes = [drive, b':', b'\\'];
    let vfs_str = core::str::from_utf8(&vfs_bytes).unwrap_or("C:\\");
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(vfs_str, &mut ob_buf);
    match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => {
            match syscall::sys_ob_set_info(fd, libneodos::syscall::ob_set_info_class::SET_VOLUME_LABEL, label) {
                Ok(()) => {
                    write_str(b"\r\n Volume in drive ");
                    write_str(&[drive]);
                    write_str(b" is now ");
                    write_str(&label);
                    write_str(b"\r\n\r\n");
                }
                Err(_) => {
                    write_err(b"\r\n Error setting volume label.\r\n");
                }
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {
            write_err(b"\r\n Error setting volume label.\r\n");
        }
    }
}
