#![no_std]
#![no_main]

use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "hostname";
const IDS_USAGE: u32 = 1001;
const IDS_ERR_READ: u32 = 1002;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let mut buf = [0u8; 64];
    match syscall::sys_get_hostname(&mut buf) {
        Ok(n) if n > 0 => {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(n);
            write_str(b"\r\n");
            write_str(&buf[..end]);
            write_str(b"\r\n\r\n");
        }
        _ => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_ERR_READ).as_bytes());
            write_str(b"\r\n\r\n");
        }
    }
    syscall::sys_exit(0)
}
