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

#[used]
#[link_section = ".rodata"]
static FSCK_HELP: &[u8] = b"::HELP::\
FSCK [drive:] [/F]\r\n\
  Check filesystem integrity on a NeoDOS volume.\r\n\
  Without /F, only checks and reports errors.\r\n\
  With /F, attempts to repair detected issues.\r\n\
  FSCK C:             check-only on C:\r\n\
  FSCK C: /F          check and repair C:\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nFSCK [drive:] [/F]\r\n  Check filesystem integrity on a NeoDOS volume.\r\n  Without /F, only checks and reports errors.\r\n  With /F, attempts to repair detected issues.\r\n\r\n  FSCK C:             check-only on C:\r\n  FSCK C: /F          check and repair C:\r\n\r\n");
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

fn write_u64(mut v: u64) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 19;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

fn print_report(stats: &syscall::FsckStats) {
    write_str(b"\r\n");
    write_str(b"========================================\r\n");
    write_str(b"  NeoDOS  FSCK   Report\r\n");
    write_str(b"========================================\r\n");
    write_str(b"\r\n");

    write_str(b"  Summary:\r\n");
    write_str(b"    Total blocks: "); write_u64(stats.total_blocks); write_str(b"\r\n");
    write_str(b"    Used blocks:  "); write_u64(stats.used_blocks); write_str(b"\r\n");
    write_str(b"    Free blocks:  "); write_u64(stats.free_blocks); write_str(b"\r\n");
    write_str(b"\r\n");

    write_str(b"  B-tree nodes:\r\n");
    write_str(b"    Total:   "); write_u64(stats.total_nodes); write_str(b"\r\n");
    write_str(b"    Dirs:    "); write_u64(stats.total_dirs); write_str(b"\r\n");
    write_str(b"    Files:   "); write_u64(stats.total_files); write_str(b"\r\n");
    write_str(b"\r\n");

    write_str(b"  Errors:      "); write_u32(stats.errors); write_str(b"\r\n");
    write_str(b"  Warnings:    "); write_u32(stats.warnings); write_str(b"\r\n");

    write_str(b"\r\n");
    if stats.errors == 0 && stats.warnings == 0 {
        write_str(b"  STATUS: OK -- No errors found.\r\n");
    } else if stats.repaired != 0 {
        write_str(b"  STATUS: "); write_u32(stats.errors + stats.warnings); write_str(b" issue(s) repaired.\r\n");
    } else {
        write_str(b"  STATUS: "); write_u32(stats.errors + stats.warnings); write_str(b" issue(s) found (use /F to repair).\r\n");
    }
    write_str(b"========================================\r\n");
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

    // Parse drive and /F flag
    let (drive, repair) = if arg_str.is_empty() {
        (current_drive(), false)
    } else {
        let repair = arg_str.iter().any(|&b| b == b'/')
            || arg_str.iter().any(|&b| b == b'-');
        if arg_str.len() >= 2 && arg_str[1] == b':' {
            (arg_str[0].to_ascii_uppercase(), repair)
        } else {
            (current_drive(), repair)
        }
    };

    if drive < b'A' || drive > b'Z' {
        write_err(b"\r\nInvalid drive letter.\r\n");
        syscall::sys_exit(1);
    }

    if repair {
        write_str(b"\r\nFSCK /F: Checking and repairing drive "); write_str(&[drive]); write_str(b"...\r\n");
    } else {
        write_str(b"\r\nFSCK: Checking drive "); write_str(&[drive]); write_str(b"... (use /F to repair errors)\r\n");
    }

    match syscall::sys_fsck(drive, repair) {
        Ok(stats) => {
            print_report(&stats);
        }
        Err(e) => {
            write_err(b"\r\nFSCK error: ");
            write_u32((-e) as u32);
            write_err(b"\r\n");
        }
    }

    syscall::sys_exit(0)
}
