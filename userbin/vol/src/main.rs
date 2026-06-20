#![no_std]
#![no_main]

use libneodos::syscall;

const ARGS_ADDR: u64 = 0x41F000;

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

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let drive = parse_drive_from_args().unwrap_or_else(|| current_drive());

    let mut label_buf = [0u8; 64];
    match syscall::sys_get_volume_label(drive, &mut label_buf) {
        Ok(n) if n > 0 => {
            write_str(b"\r\n Volume in drive ");
            write_str(&[drive]);
            write_str(b" is ");
            write_str(&label_buf[..n]);
            write_str(b"\r\n\r\n");
        }
        _ => {
            write_str(b"\r\n Volume in drive ");
            write_str(&[drive]);
            write_str(b" has no label\r\n\r\n");
        }
    }

    syscall::sys_exit(0)
}
