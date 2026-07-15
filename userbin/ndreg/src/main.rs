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

const APP_NAME: &str = "ndreg";
const IDS_HEADER_LOADED: u32 = 1007;
const IDS_NO_DRIVERS: u32 = 1008;
const IDS_ERR_ENUM: u32 = 1009;
const IDS_STATE: u32 = 1010;
const IDS_UNKNOWN_CMD: u32 = 1011;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn print_help() {
    write_str(b"\r\nNDREG [subcommand]\r\n");
    write_str(b"  NEM driver registry tool.\r\n");
    write_str(b"  NDREG list       lists loaded drivers\r\n");
    write_str(b"  NDREG info <n>   shows driver info\r\n");
    write_str(b"  NDREG state <n>  shows driver state\r\n");
    write_str(b"  NDREG help       shows this help\r\n\r\n");
}

fn is_cmd(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    if s.is_empty() { return None; }
    let mut n: u32 = 0;
    for &b in s {
        if b < b'0' || b > b'9' { return None; }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    Some(n)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DriverEntry {
    name: [u8; 64],
    state: u32,
    version: u32,
}

fn driver_state_str(state: u32) -> &'static [u8] {
    match state {
        0 => b"Stopped",
        1 => b"Loaded",
        2 => b"Running",
        3 => b"Error",
        _ => b"Unknown",
    }
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
    if args.is_empty() || is_cmd(args, b"list") || is_cmd(args, b"help") {
        if is_cmd(args, b"help") {
            print_help();
            syscall::sys_exit(0);
        }

        let fd = match syscall::sys_ob_open("\\Global\\Info\\Drivers", libneodos::syscall::ob_access::READ) {
            Ok(f) => f,
            Err(_) => {
                write_err(b"\r\n");
                write_err(tr_id!(IDS_ERR_ENUM).as_bytes());
                write_err(b"\r\n");
                syscall::sys_exit(1);
            }
        };

        let mut buf = [0u8; 64 * 32];
        let n = match syscall::sys_ob_query_info(fd, libneodos::syscall::ObInfoClass::Drivers, &mut buf) {
            Ok(n) => n,
            Err(_) => {
                let _ = syscall::sys_close(fd);
                write_err(b"\r\n");
                write_err(tr_id!(IDS_ERR_ENUM).as_bytes());
                write_err(b"\r\n");
                syscall::sys_exit(1);
            }
        };
        let _ = syscall::sys_close(fd);

        if n < core::mem::size_of::<DriverEntry>() {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_NO_DRIVERS).as_bytes());
            write_str(b"\r\n\r\n");
            syscall::sys_exit(0);
        }

        let count = n / core::mem::size_of::<DriverEntry>();
        let entries: &[DriverEntry] = unsafe {
            core::slice::from_raw_parts(buf.as_ptr() as *const DriverEntry, count)
        };

        write_str(b"\r\n");
        write_str(tr_id!(IDS_HEADER_LOADED).as_bytes());
        write_str(b"\r\n");
        for (i, entry) in entries.iter().enumerate() {
            if entry.name[0] == 0 { continue; }
            let name_end = entry.name.iter().position(|&b| b == 0).unwrap_or(64);
            write_str(b"  [");
            write_num(i as u64);
            write_str(b"] ");
            write_str(&entry.name[..name_end]);
            write_str(b"  ");
            write_str(tr_id!(IDS_STATE).as_bytes());
            write_str(driver_state_str(entry.state));
            write_str(b"\r\n");
        }
        write_str(b"\r\n");
        syscall::sys_exit(0);
    }

    if is_cmd(args, b"info") || is_cmd(args, b"state") {
        let rest = &args[4..];
        let rest = libneodos::args::trim_ascii(rest);
        let idx = match parse_u32(rest) {
            Some(i) => i,
            None => {
                print_help();
                syscall::sys_exit(1);
            }
        };

        let fd = match syscall::sys_ob_open("\\Global\\Info\\Drivers", libneodos::syscall::ob_access::READ) {
            Ok(f) => f,
            Err(_) => {
                write_err(b"\r\n");
                write_err(tr_id!(IDS_ERR_ENUM).as_bytes());
                write_err(b"\r\n");
                syscall::sys_exit(1);
            }
        };

        let mut entry: DriverEntry = DriverEntry {
            name: [0u8; 64],
            state: 0,
            version: 0,
        };
        let entry_size = core::mem::size_of::<DriverEntry>();
        let offset = idx as usize * entry_size;
        let buf = unsafe {
            core::slice::from_raw_parts_mut(&mut entry as *mut DriverEntry as *mut u8, entry_size)
        };
        match syscall::sys_ob_query_info(fd, libneodos::syscall::ObInfoClass::Drivers, &mut buf[..]) {
            Ok(n) if n >= entry_size => {}
            _ => {
                let _ = syscall::sys_close(fd);
                write_err(b"\r\nDriver not found\r\n");
                syscall::sys_exit(1);
            }
        };
        let _ = syscall::sys_close(fd);

        let name_end = entry.name.iter().position(|&b| b == 0).unwrap_or(64);
        write_str(b"\r\nDriver: ");
        write_str(&entry.name[..name_end]);
        write_str(b"\r\n");
        write_str(tr_id!(IDS_STATE).as_bytes());
        write_str(driver_state_str(entry.state));
        write_str(b"\r\n");
        write_str(b"Version: ");
        write_num(entry.version as u64);
        write_str(b"\r\n\r\n");
        syscall::sys_exit(0);
    }

    write_err(b"\r\n");
    write_err(tr_id!(IDS_UNKNOWN_CMD).as_bytes());
    write_err(b"\r\n");
    syscall::sys_exit(1)
}

fn write_num(mut v: u64) {
    if v == 0 { write_str(b"0"); return; }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&buf[i..]);
}
