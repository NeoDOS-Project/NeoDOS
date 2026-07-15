use libneodos::syscall;
use core;

use crate::shell::{make_ascii_uppercase, trim_ascii, first_token, to_ob_path};

pub static BUILTINS: &[&[u8]] = &[
    b"CWD", b"SET", b"EXIT", b"CALL",
];

static mut COMPL_PATH: [u8; 256] = [0; 256];
static mut COMPL_PATH_LEN: usize = 0;

pub fn set_completion_ctx(_drive: u8, path: &[u8]) {
    unsafe {
        let n = path.len().min(255);
        COMPL_PATH[..n].copy_from_slice(&path[..n]);
        COMPL_PATH_LEN = n;
    }
}

pub extern "C" fn shell_complete(input: *const u8, cursor: i32, cand: *mut u8, max: i32) -> i32 {
    let t = unsafe { trim_ascii(core::slice::from_raw_parts(input, cursor.max(0) as usize)) };
    if t.is_empty() { return 0; }
    let word = first_token(t);
    let wup = { let mut b = [0u8; 32]; let n = word.len().min(31); b[..n].copy_from_slice(&word[..n]); make_ascii_uppercase(&mut b[..n]); b };
    let wlen = word.len().min(31);
    let max = max as usize;
    let mut p = 0usize; let mut cnt = 0i32;
    let mut add = |name: &[u8]| {
        let n = name.len().min(max - p - 1);
        if n > 0 && p + n + 1 <= max {
            unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), cand.add(p), n); *cand.add(p + n) = 0; }
            p += n + 1; cnt += 1;
        }
    };
    for builtin in BUILTINS {
        if builtin.len() >= wlen && builtin[..wlen] == wup[..wlen] { add(builtin); }
    }
    let path_val = unsafe {
        let ptr = core::ptr::addr_of!(COMPL_PATH) as *const u8;
        core::slice::from_raw_parts(ptr, COMPL_PATH_LEN)
    };
    let store = if path_val.is_empty() { b"\\Programs" } else { path_val };
    let drive_byte = b'C';
    let mut s = 0usize;
    loop {
        while s < store.len() && store[s] == b';' { s += 1; }
        if s >= store.len() { break; }
        let mut e = s;
        while e < store.len() && store[e] != b';' { e += 1; }
        let dir = &store[s..e];
        let mut dp = [0u8; 260]; let mut dp_pos = 0;
        let is_abs = dir.len() >= 2 && dir[1] == b':'
            && ((dir[0] >= b'A' && dir[0] <= b'Z') || (dir[0] >= b'a' && dir[0] <= b'z'));
        if is_abs {
            for &b in dir { if dp_pos < 255 { dp[dp_pos] = b; dp_pos += 1; } }
        } else {
            dp[dp_pos] = drive_byte; dp_pos += 1; dp[dp_pos] = b':'; dp_pos += 1;
            for &b in dir { if dp_pos < 255 { dp[dp_pos] = b; dp_pos += 1; } }
        }
        let ds = core::str::from_utf8(&dp[..dp_pos]).unwrap_or("");
        let mut ob = [0u8; 512];
        let obp = to_ob_path(ds, &mut ob);
        if let Ok(fd) = syscall::sys_ob_open(obp, syscall::ob_access::READ) {
            let mut ents: [syscall::ObEnumEntry; 64] = core::array::from_fn(|_| syscall::ObEnumEntry {
                id:0, obj_type:0, name:[0;32], mode:0, _pad:[0;2], size:0,
            });
            if let Ok(n) = syscall::sys_ob_enum(fd, &mut ents) {
                for i in 0..n {
                    let nm = ents[i].name_str(); let nb = nm.as_bytes();
                    if nb.len() > 4 && nb[nb.len()-4..].eq_ignore_ascii_case(b".NXE") && nb.len()-4 >= wlen {
                        let su = { let mut b=[0u8;32]; let n=(nb.len()-4).min(31); b[..n].copy_from_slice(&nb[..n]); make_ascii_uppercase(&mut b[..n]); b };
                        if su[..wlen] == wup[..wlen] { add(&nb[..nb.len()-4]); }
                    }
                }
            }
            let _ = syscall::sys_close(fd);
        }
        s = e + 1;
    }
    cnt
}
