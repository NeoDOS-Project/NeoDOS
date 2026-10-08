//! `SNAPSHOT` — NeoFS v2 point-in-time snapshots (Ring 3, NeoShell).
//!
//! Usage:
//!   SNAPSHOT                          list snapshots of the current drive
//!   SNAPSHOT LIST [D:]                list snapshots
//!   SNAPSHOT CREATE [D:]              create a snapshot (prints its id)
//!   SNAPSHOT RESTORE <id> [D:]        roll the volume back to that snapshot
//!   SNAPSHOT DELETE <id> [D:]         delete one snapshot
//!   SNAPSHOT PURGE [D:]               delete all snapshots
//!   SNAPSHOT EXTRACT <id> <src> <dst> copy a file as of the snapshot to dst
//!   SNAPSHOT /?                       this help

#![no_std]
#![no_main]

use libneodos::args;
use libneodos::syscall::{self, ob_access};

const ENTRY_SIZE: usize = 24;

fn out(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn out_u64(mut n: u64) {
    if n == 0 {
        out(b"0");
        return;
    }
    let mut b = [0u8; 20];
    let mut i = 20usize;
    while n > 0 {
        i -= 1;
        b[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    out(&b[i..]);
}

fn current_drive() -> u8 {
    let mut buf = [0u8; 64];
    match syscall::sys_getcwd(&mut buf) {
        Ok(n) if n >= 2 && buf[1] == b':' => buf[0].to_ascii_uppercase(),
        _ => b'C',
    }
}

fn open_root(drive: u8) -> Result<u8, i64> {
    let prefix = b"\\Global\\FileSystem\\";
    let mut p = [0u8; 32];
    p[..prefix.len()].copy_from_slice(prefix);
    p[prefix.len()] = drive;
    p[prefix.len() + 1] = b':';
    p[prefix.len() + 2] = b'\\';
    let n = prefix.len() + 3;
    let s = core::str::from_utf8(&p[..n]).unwrap_or("\\Global\\FileSystem\\C:\\");
    syscall::sys_ob_open(s, ob_access::READ)
}

fn token<'a>(a: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    while *pos < a.len() && (a[*pos] == b' ' || a[*pos] == b'\t') {
        *pos += 1;
    }
    if *pos >= a.len() {
        return None;
    }
    let start = *pos;
    while *pos < a.len() && a[*pos] != b' ' && a[*pos] != b'\t' && a[*pos] != 0 {
        *pos += 1;
    }
    Some(&a[start..*pos])
}

fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.to_ascii_uppercase() == y.to_ascii_uppercase())
}

fn parse_id(s: &[u8]) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut n = 0u64;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (c - b'0') as u64;
    }
    Some(n)
}

fn parse_drive(s: &[u8]) -> Option<u8> {
    if s.len() == 2 && s[1] == b':' && s[0].is_ascii_alphabetic() {
        Some(s[0].to_ascii_uppercase())
    } else {
        None
    }
}

/// Drive prefix of a path (`X:\...` → X), if present.
fn drive_of_path(s: &[u8]) -> Option<u8> {
    if s.len() >= 2 && s[1] == b':' && s[0].is_ascii_alphabetic() {
        Some(s[0].to_ascii_uppercase())
    } else {
        None
    }
}

fn fail(msg: &[u8]) -> ! {
    out(b"\r\nSNAPSHOT: ");
    out(msg);
    out(b"\r\n\r\n");
    syscall::sys_exit(1);
}

fn help() {
    out(b"\r\nSNAPSHOT [LIST|CREATE|RESTORE|DELETE|PURGE|EXTRACT] [args] [drive:]\r\n");
    out(b"  LIST                    list snapshots (default)\r\n");
    out(b"  CREATE                  create a snapshot, prints its id\r\n");
    out(b"  RESTORE <id>            roll the volume back to snapshot <id>\r\n");
    out(b"  DELETE <id>             delete snapshot <id>\r\n");
    out(b"  PURGE                   delete all snapshots\r\n");
    out(b"  EXTRACT <id> <src> <dst>  copy <src> as of <id> to <dst>\r\n\r\n");
}

fn do_list(drive: u8) -> ! {
    let fd = match open_root(drive) {
        Ok(fd) => fd,
        Err(_) => fail(b"cannot open volume"),
    };
    let mut buf = [0u8; 64 * ENTRY_SIZE];
    match syscall::sys_ob_snapshot_list(fd, &mut buf) {
        Ok(n) => {
            out(b"\r\nID   TIMESTAMP   ROOT\r\n");
            for i in 0..n.min(64) {
                let off = i * ENTRY_SIZE;
                let id = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
                let ts = u64::from_le_bytes(buf[off + 8..off + 16].try_into().unwrap());
                let root = u64::from_le_bytes(buf[off + 16..off + 24].try_into().unwrap());
                out_u64(id);
                out(b"   ");
                out_u64(ts);
                out(b"   ");
                out_u64(root);
                out(b"\r\n");
            }
            out(b"\r\n");
            let _ = syscall::sys_close(fd);
            syscall::sys_exit(0)
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            fail(b"list failed")
        }
    }
}

fn do_create(drive: u8) -> ! {
    let fd = match open_root(drive) {
        Ok(fd) => fd,
        Err(_) => fail(b"cannot open volume"),
    };
    match syscall::sys_ob_snapshot_create(fd) {
        Ok(id) => {
            out(b"\r\nSnapshot ");
            out_u64(id);
            out(b" created.\r\n\r\n");
            let _ = syscall::sys_close(fd);
            syscall::sys_exit(0)
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            fail(b"create failed")
        }
    }
}

fn do_restore(drive: u8, id: u64) -> ! {
    let fd = match open_root(drive) {
        Ok(fd) => fd,
        Err(_) => fail(b"cannot open volume"),
    };
    match syscall::sys_ob_snapshot_restore(fd, id) {
        Ok(()) => {
            out(b"\r\nRestored to snapshot ");
            out_u64(id);
            out(b".\r\n\r\n");
            let _ = syscall::sys_close(fd);
            syscall::sys_exit(0)
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            fail(b"restore failed (unknown id?)")
        }
    }
}

fn do_delete(drive: u8, id: u64) -> ! {
    let fd = match open_root(drive) {
        Ok(fd) => fd,
        Err(_) => fail(b"cannot open volume"),
    };
    match syscall::sys_ob_snapshot_delete(fd, id) {
        Ok(()) => {
            out(b"\r\nSnapshot ");
            out_u64(id);
            out(b" deleted.\r\n\r\n");
            let _ = syscall::sys_close(fd);
            syscall::sys_exit(0)
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            fail(b"delete failed (unknown id?)")
        }
    }
}

fn do_purge(drive: u8) -> ! {
    let fd = match open_root(drive) {
        Ok(fd) => fd,
        Err(_) => fail(b"cannot open volume"),
    };
    match syscall::sys_ob_snapshot_purge(fd) {
        Ok(()) => {
            out(b"\r\nAll snapshots purged.\r\n\r\n");
            let _ = syscall::sys_close(fd);
            syscall::sys_exit(0)
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            fail(b"purge failed")
        }
    }
}

fn do_extract(drive: u8, id: u64, src: &[u8], dst: &[u8]) -> ! {
    let src = match core::str::from_utf8(src) {
        Ok(s) => s,
        Err(_) => fail(b"invalid src path"),
    };
    let dst = match core::str::from_utf8(dst) {
        Ok(s) => s,
        Err(_) => fail(b"invalid dst path"),
    };
    let fd = match open_root(drive) {
        Ok(fd) => fd,
        Err(_) => fail(b"cannot open volume"),
    };
    match syscall::sys_ob_snapshot_extract(fd, id, src, dst) {
        Ok(bytes) => {
            out(b"\r\nExtracted ");
            out_u64(bytes);
            out(b" bytes from snapshot ");
            out_u64(id);
            out(b".\r\n\r\n");
            let _ = syscall::sys_close(fd);
            syscall::sys_exit(0)
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            fail(b"extract failed (unknown id or file?)")
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = args::read_args();
    let a = args::trim_ascii(&raw);
    if a.is_empty() {
        do_list(current_drive());
    }
    if args::is_help_flag(a) {
        help();
        syscall::sys_exit(0);
    }

    let mut toks: [&[u8]; 6] = [&[]; 6];
    let mut nt = 0usize;
    let mut pos = 0usize;
    while nt < 6 {
        match token(a, &mut pos) {
            Some(t) => {
                toks[nt] = t;
                nt += 1;
            }
            None => break,
        }
    }
    let cmd = toks[0];

    if eq_ci(cmd, b"LIST") {
        let drive = parse_drive(toks[1]).unwrap_or_else(current_drive);
        do_list(drive);
    } else if eq_ci(cmd, b"CREATE") {
        let drive = parse_drive(toks[1]).unwrap_or_else(current_drive);
        do_create(drive);
    } else if eq_ci(cmd, b"PURGE") {
        let drive = parse_drive(toks[1]).unwrap_or_else(current_drive);
        do_purge(drive);
    } else if eq_ci(cmd, b"RESTORE") {
        let id = match parse_id(toks[1]) {
            Some(id) => id,
            None => fail(b"RESTORE needs <id>"),
        };
        let drive = parse_drive(toks[2]).unwrap_or_else(current_drive);
        do_restore(drive, id);
    } else if eq_ci(cmd, b"DELETE") {
        let id = match parse_id(toks[1]) {
            Some(id) => id,
            None => fail(b"DELETE needs <id>"),
        };
        let drive = parse_drive(toks[2]).unwrap_or_else(current_drive);
        do_delete(drive, id);
    } else if eq_ci(cmd, b"EXTRACT") {
        let id = match parse_id(toks[1]) {
            Some(id) => id,
            None => fail(b"EXTRACT needs <id> <src> <dst>"),
        };
        if toks[2].is_empty() || toks[3].is_empty() {
            fail(b"EXTRACT needs <id> <src> <dst>");
        }
        let drive = drive_of_path(toks[2]).or_else(|| drive_of_path(toks[3])).unwrap_or_else(current_drive);
        do_extract(drive, id, toks[2], toks[3]);
    } else {
        help();
        syscall::sys_exit(0);
    }
}
