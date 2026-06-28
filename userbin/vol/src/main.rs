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

#[used]
#[link_section = ".rodata"]
static VOL_HELP: &[u8] = b"::HELP::\
VOL [drive:]\r\n\
  Show the volume label of the specified drive,\r\n\
  or the current drive if none given.\r\n\
::END::";

fn current_drive() -> u8 {
    let mut buf = [0u8; 64];
    match syscall::sys_getcwd(&mut buf) {
        Ok(n) if n >= 2 && buf[1] == b':' => buf[0],
        _ => b'C',
    }
}

fn parse_drive_from_args() -> Option<u8> {
    let ptr = ARGS_ADDR as *const u8;
    let mut buf = [0u8; 32];
    let mut len = 0usize;
    unsafe {
        while len < 31 {
            let b = ptr.add(len).read();
            if b == 0 { break; }
            buf[len] = b;
            len += 1;
        }
    }
    if len == 0 {
        return None;
    }
    let s = core::str::from_utf8(&buf[..len]).ok()?;
    let s = s.trim();
    if s.len() == 2 && s.as_bytes()[1] == b':' {
        return Some(s.as_bytes()[0].to_ascii_uppercase());
    }
    if s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic() {
        return Some(s.as_bytes()[0].to_ascii_uppercase());
    }
    None
}

fn print_help() {
    write_str(b"\r\nVOL [drive:]\r\n  Show the volume label of the specified drive.\r\n  If no drive is specified, shows the current drive.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }
    let drive = parse_drive_from_args().unwrap_or_else(|| current_drive());

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

    syscall::sys_exit(0)
}
