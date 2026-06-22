#![no_std]
#![no_main]

use libneodos::syscall;

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

fn write_hex64(v: u64) {
    let chars = b"0123456789ABCDEF";
    let mut buf = [0u8; 18];
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..16 {
        buf[2 + i] = chars[((v >> (60 - i * 4)) & 0xF) as usize];
    }
    write_str(&buf);
}

fn error_str(e: u32) -> &'static str {
    match e {
        0 => "None",
        1 => "InitFailed",
        2 => "RegistrationFailed",
        3 => "BindFailed",
        4 => "SandboxRejected",
        5 => "CertificationFailed",
        6 => "OutOfMemory",
        7 => "PolicyViolation",
        8 => "LoadFailed",
        9 => "CapabilityDenied",
        10 => "UnloadFailed",
        11 => "UnloadTimeout",
        _ => "Unknown",
    }
}

#[used]
#[link_section = ".rodata"]
static NDREG_HELP: &[u8] = b"::HELP::\
NDREG [LIST|SHOW <name>|QUERY|RUNTIME]\r\n\
  NeoDOS Driver Registry - inspect driver metadata.\r\n\
  NDREG LIST            List all loaded drivers\r\n\
  NDREG SHOW <name>     Show full driver details\r\n\
  NDREG QUERY           Summarize driver registry\r\n\
  NDREG RUNTIME         Show runtime state snapshot\r\n\
::END::";

fn cmd_list() {
    write_str(b"\r\nLoaded drivers:\r\n");
    write_str(b"-----------------------------\r\n");
    let mut found = false;
    for i in 0..64 {
        match syscall::sys_driver_enum(i) {
            Ok(Some(info)) => {
                found = true;
                write_str(b"  ID:"); write_u32(info.id);
                write_str(b"  "); write_str(info.name_str().as_bytes());
                write_str(b"  ["); write_str(info.state_str().as_bytes());
                write_str(b"]");
                if info.last_error != 0 {
                    write_str(b"  ERR:"); write_str(error_str(info.last_error).as_bytes());
                }
                write_str(b"\r\n");
            }
            Ok(None) => break,
            Err(_) => { write_err(b"\r\nError enumerating drivers\r\n"); break; }
        }
    }
    if !found {
        write_str(b"  No drivers loaded.\r\n");
    }
    write_str(b"\r\n");
}

fn cmd_show(name: &[u8]) {
    let mut found = false;
    for i in 0..64 {
        match syscall::sys_driver_enum(i) {
            Ok(Some(info)) => {
                if info.name_str().as_bytes().eq_ignore_ascii_case(name) {
                    found = true;
                    write_str(b"\r\n========================================\r\n");
                    write_str(b"  Driver: "); write_str(info.name_str().as_bytes()); write_str(b"\r\n");
                    write_str(b"========================================\r\n");
                    write_str(b"  ID:              "); write_u32(info.id); write_str(b"\r\n");
                    write_str(b"  State:           "); write_str(info.state_str().as_bytes()); write_str(b"\r\n");
                    write_str(b"  Category:        "); write_str(info.category_str().as_bytes()); write_str(b"\r\n");
                    write_str(b"  Type:            "); write_u32(info.driver_type as u32); write_str(b"\r\n");
                    write_str(b"  API Version:     "); write_u32(info.api_version as u32); write_str(b"\r\n");
                    write_str(b"  ABI:             "); write_u32(info.abi_min as u32); write_str(b"-"); write_u32(info.abi_target as u32); write_str(b"-"); write_u32(info.abi_max as u32); write_str(b"\r\n");
                    write_str(b"  Caps:            "); write_hex64(info.caps); write_str(b"\r\n");
                    write_str(b"  Last Error:      "); write_str(error_str(info.last_error).as_bytes()); write_str(b"\r\n");
                    write_str(b"  Isolation:       "); write_u32(info.isolation_mode as u32); write_str(b"\r\n");
                    write_str(b"  Events Received: "); write_u64(info.events_received); write_str(b"\r\n");
                    write_str(b"  Tick Count:      "); write_u64(info.tick_count); write_str(b"\r\n");
                    write_str(b"  Registered at:   "); write_u64(info.registered_at_tick); write_str(b"\r\n");
                    write_str(b"\r\n");
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    if !found {
        write_str(b"\r\nDriver '");
        write_str(name);
        write_str(b"' not found.\r\n\r\n");
    }
}

fn cmd_query() {
    let mut total = 0u32;
    let mut active = 0u32;
    let mut loaded = 0u32;
    let mut faulted = 0u32;
    let mut unloaded = 0u32;

    for i in 0..64 {
        match syscall::sys_driver_enum(i) {
            Ok(Some(info)) => {
                total += 1;
                match info.state {
                    4 => active += 1,
                    0 | 1 | 2 | 3 => loaded += 1,
                    5 => faulted += 1,
                    6 | 7 => unloaded += 1,
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    write_str(b"\r\nDriver Registry Summary:\r\n");
    write_str(b"  Total:  "); write_u32(total); write_str(b"\r\n");
    write_str(b"  Active: "); write_u32(active); write_str(b"\r\n");
    write_str(b"  Loaded: "); write_u32(loaded); write_str(b"\r\n");
    write_str(b"  Faulted:"); write_u32(faulted); write_str(b"\r\n");
    write_str(b"  Unloaded:"); write_u32(unloaded); write_str(b"\r\n");
    write_str(b"\r\n");
}

fn cmd_runtime() {
    write_str(b"\r\nRuntime Snapshot:\r\n");
    write_str(b"-----------------------------\r\n");
    for i in 0..64 {
        match syscall::sys_driver_enum(i) {
            Ok(Some(info)) => {
                write_str(b"  ["); write_str(info.state_str().as_bytes());
                write_str(b"] "); write_str(info.name_str().as_bytes());
                write_str(b" (ID:"); write_u32(info.id); write_str(b", ");
                write_str(info.category_str().as_bytes());
                if info.isolation_mode > 0 {
                    write_str(b", ISO:");
                    write_u32(info.isolation_mode as u32);
                }
                write_str(b")\r\n");
            }
            Ok(None) => break,
            Err(_) => { write_err(b"Error reading driver info\r\n"); break; }
        }
    }
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let args = read_args();
    if is_help_flag(&args) {
        write_str(b"\r\nNDREG [LIST|SHOW <name>|QUERY|RUNTIME]\r\n  NeoDOS Driver Registry - inspect driver metadata.\r\n  NDREG LIST            List all loaded drivers\r\n  NDREG SHOW <name>     Show full driver details\r\n  NDREG QUERY           Summarize driver registry\r\n  NDREG RUNTIME         Show runtime state snapshot\r\n\r\n");
        syscall::sys_exit(0);
    }

    let arg_str = {
        let end = args.iter().position(|&b| b == 0).unwrap_or(0);
        trim_ascii(&args[..end])
    };

    if arg_str.is_empty() {
        cmd_list();
        syscall::sys_exit(0);
    }

    // Split first word and rest
    let first_space = arg_str.iter().position(|&b| b == b' ' || b == b'\t').unwrap_or(arg_str.len());
    let cmd = &arg_str[..first_space];
    let rest = trim_ascii(&arg_str[first_space..]);

    if cmd.eq_ignore_ascii_case(b"LIST") {
        cmd_list();
    } else if cmd.eq_ignore_ascii_case(b"SHOW") {
        if rest.is_empty() {
            write_err(b"\r\nUsage: NDREG SHOW <name>\r\n\r\n");
        } else {
            cmd_show(rest);
        }
    } else if cmd.eq_ignore_ascii_case(b"QUERY") {
        cmd_query();
    } else if cmd.eq_ignore_ascii_case(b"RUNTIME") {
        cmd_runtime();
    } else {
        write_err(b"\r\nUnknown NDREG subcommand. Try: NDREG LIST, SHOW, QUERY, or RUNTIME\r\n\r\n");
    }

    syscall::sys_exit(0)
}
