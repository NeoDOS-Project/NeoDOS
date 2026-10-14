//! Low-level console output helpers for NEOKEY.

use libneodos::syscall;

pub fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

pub fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

pub fn write_u32(mut v: u32) {
    if v == 0 { write_str(b"0"); return; }
    let mut buf = [0u8; 10];
    let mut i = 9;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

/// Print the set bits of `val` as space-separated `(name, bit)` labels,
/// or `(none)` when no bit is set.
pub fn format_bits(val: u8, names: &[(&[u8], u8)]) {
    let mut first = true;
    for (name, bit) in names {
        if val & bit != 0 {
            if !first { write_str(b" "); }
            write_str(name);
            first = false;
        }
    }
    if first { write_str(b"(none)"); }
}
