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

const APP_NAME: &str = "nxres";
const IDS_ERR_APP_REQUIRED: u32 = 1006;
const IDS_ERR_NOT_FOUND: u32 = 1007;
const IDS_ERR_RES_NOT_FOUND: u32 = 1008;
const IDS_LOCALES: u32 = 1009;
const IDS_NO_LOCALE_RES: u32 = 1010;
const IDS_NOT_IMPL: u32 = 1011;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn print_help() {
    write_str(b"\r\nNXRES <app> [resource]\r\n");
    write_str(b"  NXE resource viewer.\r\n");
    write_str(b"  NXRES <app>            lists resources of app\r\n");
    write_str(b"  NXRES <app> <res>     shows resource content\r\n");
    write_str(b"  NXRES <app> --locales  shows available locales\r\n\r\n");
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
        write_err(b"\r\n");
        write_err(tr_id!(IDS_ERR_APP_REQUIRED).as_bytes());
        write_err(b"\r\n");
        syscall::sys_exit(1);
    }

    let first_space = args.iter().position(|&b| b == b' ' || b == b'\t');
    let (app_name, rest) = if let Some(pos) = first_space {
        (&args[..pos], libneodos::args::trim_ascii(&args[pos + 1..]))
    } else {
        (args, &[][..])
    };

    let mut path_buf = [0u8; 512];
    let prefix = b"\\Global\\FileSystem\\C:\\Programs\\";
    {
        let total = prefix.len() + app_name.len() + 10;
        path_buf[..prefix.len()].copy_from_slice(prefix);
        path_buf[prefix.len()..prefix.len() + app_name.len()].copy_from_slice(app_name);
        let off = prefix.len() + app_name.len();
        path_buf[off..off + 10].copy_from_slice(b"\\resources");
    }
    let resources_dir = unsafe { core::str::from_utf8_unchecked(&path_buf[..prefix.len() + app_name.len() + 10]) };

    let fd = match syscall::sys_ob_open(resources_dir, libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_NOT_FOUND).as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    if rest.is_empty() {
        let mut entries: [syscall::ObEnumEntry; 64] = core::array::from_fn(|_| syscall::ObEnumEntry {
            id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
        });
        match syscall::sys_ob_enum(fd, &mut entries) {
            Ok(n) if n > 0 => {
                write_str(b"\r\n");
                write_str(app_name);
                write_str(b" resources:\r\n");
                for i in 0..n {
                    let name = entries[i].name_str();
                    if name == "." || name == ".." { continue; }
                    write_str(b"  ");
                    write_str(name.as_bytes());
                    write_str(b"\r\n");
                }
                write_str(b"\r\n");
            }
            _ => {
                write_str(b"\r\n");
                write_str(tr_id!(IDS_ERR_NOT_FOUND).as_bytes());
                write_str(b"\r\n");
            }
        }
    } else if rest == b"--locales" || rest == b"-l" {
        let mut locale_buf = [0u8; 512];
        let lprefix = b"\\Global\\FileSystem\\C:\\Programs\\";
        {
            let total = lprefix.len() + app_name.len() + 16;
            locale_buf[..lprefix.len()].copy_from_slice(lprefix);
            locale_buf[lprefix.len()..lprefix.len() + app_name.len()].copy_from_slice(app_name);
            let off = lprefix.len() + app_name.len();
            locale_buf[off..off + 16].copy_from_slice(b"\\resources\\locale");
        }
        let locale_dir = unsafe { core::str::from_utf8_unchecked(&locale_buf[..lprefix.len() + app_name.len() + 16]) };

        let locale_fd = match syscall::sys_ob_open(locale_dir, libneodos::syscall::ob_access::READ) {
            Ok(f) => f,
            Err(_) => {
                write_str(b"\r\n");
                write_str(tr_id!(IDS_NO_LOCALE_RES).as_bytes());
                write_str(b"\r\n");
                let _ = syscall::sys_close(fd);
                syscall::sys_exit(0);
            }
        };

        let mut entries: [syscall::ObEnumEntry; 64] = core::array::from_fn(|_| syscall::ObEnumEntry {
            id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
        });
        match syscall::sys_ob_enum(locale_fd, &mut entries) {
            Ok(n) if n > 0 => {
                write_str(b"\r\n");
                write_str(tr_id!(IDS_LOCALES).as_bytes());
                write_str(b"\r\n");
                for i in 0..n {
                    let name = entries[i].name_str();
                    if name == "." || name == ".." { continue; }
                    write_str(b"  ");
                    write_str(name.as_bytes());
                    write_str(b"\r\n");
                }
                write_str(b"\r\n");
            }
            _ => {
                write_str(b"\r\n");
                write_str(tr_id!(IDS_NO_LOCALE_RES).as_bytes());
                write_str(b"\r\n");
            }
        }
        let _ = syscall::sys_close(locale_fd);
    } else {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_NOT_IMPL).as_bytes());
        write_str(b"\r\n\r\n");
    }

    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0)
}
