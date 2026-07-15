#![no_std]
#![no_main]

use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "poweroff";
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_MSG: u32 = 1003;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE).as_bytes());
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(0);
    }
    write_str(b"\r\n");
    write_str(tr_id!(IDS_MSG).as_bytes());
    write_str(b"\r\n");
    syscall::ob_power_shutdown()
}
