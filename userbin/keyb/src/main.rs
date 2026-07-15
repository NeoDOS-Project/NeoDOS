#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "keyb";
const IDS_LAYOUT: u32 = 1005;
const IDS_ERR_READING: u32 = 1006;
const IDS_ERR_INVALID: u32 = 1007;
const IDS_ERR_CHANGE: u32 = 1008;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn print_help() {
    write_str(b"\r\nKEYB [layout]\r\n  Display or set the keyboard layout.\r\n  KEYB                shows current layout\r\n  KEYB es             sets layout to Spanish\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    let args = libneodos::args::trim_ascii(&raw);
    if args.is_empty() {
        write_str(tr_id!(IDS_LAYOUT).as_bytes());
        let fd = match syscall::sys_ob_open("\\Global\\Info\\Keyboard", libneodos::syscall::ob_access::READ) {
            Ok(f) => f,
            Err(_) => {
                write_str(b"?\r\n");
                syscall::sys_exit(1);
            }
        };
        let mut buf = [0u8; 64];
        let n = match syscall::sys_ob_query_info(fd, libneodos::syscall::ObInfoClass::KeyboardLayout, &mut buf) {
            Ok(n) => n,
            Err(_) => {
                let _ = syscall::sys_close(fd);
                write_str(b"?\r\n");
                syscall::sys_exit(1);
            }
        };
        let _ = syscall::sys_close(fd);
        let end = buf[..n].iter().position(|&b| b == 0).unwrap_or(n);
        write_str(&buf[..end]);
        write_str(b"\r\n");
        syscall::sys_exit(0);
    }

    let fd = match syscall::sys_ob_open("\\Global\\Info\\Keyboard", libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_READING).as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; 12];
    let layout_bytes = args;
    buf[..layout_bytes.len().min(11)].copy_from_slice(&layout_bytes[..layout_bytes.len().min(11)]);
    match syscall::sys_ob_set_info(fd, syscall::ObSetInfoClass::KeyboardLayout, &buf) {
        Ok(_) => {
            let _ = syscall::sys_close(fd);
            write_str(b"\r\nOK\r\n");
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_CHANGE).as_bytes());
            write_err(b"\r\n");
        }
    }
    syscall::sys_exit(0)
}
