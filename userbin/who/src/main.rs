//! `who` — list login sessions (#65).
//!
//! Sessions live in the Object Manager namespace (`\Session\{id}`), so the
//! listing is an `ObEnum` over `\Session` plus one `SessionInfo` query per
//! handle.

#![no_std]
#![no_main]

use libneodos::syscall::{self, ObEnumEntry};

const MAX_SESSIONS: usize = 16;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_dec(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&buf[i..]);
}

/// Format a binary SID (`rev, count, authority[6], sub[4]...`) into `out`.
fn format_sid(bytes: &[u8], len: usize, out: &mut [u8; 48]) -> usize {
    if len < 8 {
        return 0;
    }
    let mut p = 0usize;
    let push = |s: &[u8], out: &mut [u8; 48], p: &mut usize| {
        for &b in s {
            if *p < out.len() {
                out[*p] = b;
                *p += 1;
            }
        }
    };
    push(b"S-", out, &mut p);
    let mut rev = [0u8; 4];
    let mut rn = rev.len();
    let mut rv = bytes[0] as u64;
    if rv == 0 {
        rn -= 1;
        rev[rn] = b'0';
    }
    while rv > 0 {
        rn -= 1;
        rev[rn] = b'0' + (rv % 10) as u8;
        rv /= 10;
    }
    push(&rev[rn..], out, &mut p);
    push(b"-", out, &mut p);
    let authority = u64::from_be_bytes([
        0, 0, bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);
    let mut num = [0u8; 20];
    let mut n = num.len();
    let mut v = authority;
    if v == 0 {
        n -= 1;
        num[n] = b'0';
    }
    while v > 0 {
        n -= 1;
        num[n] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    push(&num[n..], out, &mut p);
    let count = (bytes[1] as usize).min((len.saturating_sub(8)) / 4);
    for i in 0..count {
        push(b"-", out, &mut p);
        let sub = u32::from_le_bytes([
            bytes[8 + i * 4],
            bytes[9 + i * 4],
            bytes[10 + i * 4],
            bytes[11 + i * 4],
        ]);
        let mut buf = [0u8; 20];
        let mut n = buf.len();
        let mut v = sub as u64;
        if v == 0 {
            n -= 1;
            buf[n] = b'0';
        }
        while v > 0 {
            n -= 1;
            buf[n] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        push(&buf[n..], out, &mut p);
    }
    p
}

fn state_name(state: u8) -> &'static [u8] {
    match state {
        0 => b"Active",
        1 => b"Locked",
        2 => b"Disconnected",
        _ => b"LoggedOff",
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let dir = match syscall::sys_ob_open("\\Session", syscall::ob_access::READ) {
        Ok(fd) => fd,
        Err(_) => {
            write_str(b"\r\nwho: no hay sesiones\r\n");
            syscall::sys_exit(1);
        }
    };
    let mut entries = [ObEnumEntry::EMPTY; MAX_SESSIONS];
    let count = match syscall::sys_ob_enum(dir, &mut entries) {
        Ok(n) => n,
        Err(_) => {
            let _ = syscall::sys_close(dir);
            write_str(b"\r\nwho: no se pudo enumerar \\Session\r\n");
            syscall::sys_exit(1);
        }
    };
    let _ = syscall::sys_close(dir);

    if count == 0 {
        write_str(b"\r\nNo hay sesiones activas\r\n");
        syscall::sys_exit(0);
    }

    write_str(b"\r\nID  Usuario          SID                          Estado       VT Proc Admin\r\n");
    write_str(b"--------------------------------------------------------------------------------\r\n");
    for entry in entries.iter().take(count) {
        let name = entry.name_str();
        let mut path = [0u8; 32];
        let mut p = 0usize;
        for &b in b"\\Session\\" {
            path[p] = b;
            p += 1;
        }
        for &b in name.as_bytes() {
            if p < 30 {
                path[p] = b;
                p += 1;
            }
        }
        let path = core::str::from_utf8(&path[..p]).unwrap_or("");
        let fd = match syscall::sys_ob_open(path, syscall::ob_access::READ) {
            Ok(fd) => fd,
            Err(_) => continue,
        };
        if let Ok(info) = syscall::ob_query_session_info(fd) {
            write_dec(info.session_id as u64);
            write_str(b"   ");
            let mut user = [b' '; 17];
            let ul = info.username.iter().position(|&b| b == 0).unwrap_or(32);
            for i in 0..ul.min(16) {
                user[i] = info.username[i];
            }
            write_str(&user);
            write_str(b" ");
            let mut sid = [0u8; 48];
            let sl = format_sid(&info.user_sid, info.user_sid_len as usize, &mut sid);
            let mut pad = 29usize.saturating_sub(sl);
            write_str(&sid[..sl]);
            while pad > 0 {
                write_str(b" ");
                pad -= 1;
            }
            let st = state_name(info.state);
            write_str(st);
            let mut pad = 13usize.saturating_sub(st.len());
            while pad > 0 {
                write_str(b" ");
                pad -= 1;
            }
            write_dec(info.vt_num as u64);
            write_str(b"  ");
            write_dec(info.process_count as u64);
            write_str(if info.is_admin != 0 { b"   si\r\n" } else { b"   no\r\n" });
        }
        let _ = syscall::sys_close(fd);
    }
    syscall::sys_exit(0)
}
