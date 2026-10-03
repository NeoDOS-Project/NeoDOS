//! Low-level console I/O: syscall wrappers, string helpers and output API.

use core::arch::asm;

// ── Syscall wrappers ───────────────────────────

pub(crate) unsafe fn sys_write(fd: u8, buf: *const u8, len: usize) -> i64 {
    let r: i64;
    asm!(
        "push rbx", "push rcx", "push rdx",
        "mov rax, 20",
        "mov rbx, {fd}",
        "mov rcx, {buf}",
        "mov rdx, {len}",
        "int 0x80",
        "pop rdx", "pop rcx", "pop rbx",
        fd = in(reg) fd as u64,
        buf = in(reg) buf as u64,
        len = in(reg) len as u64,
        out("rax") r,
    );
    r
}

pub(crate) fn write_str(s: &[u8]) {
    if !s.is_empty() {
        unsafe { sys_write(1, s.as_ptr(), s.len()); }
    }
}

pub(crate) fn read_byte() -> i32 {
    let mut c: u8 = 0;
    let r: u64;
    unsafe {
        asm!(
            "push rbx", "push rcx", "push rdx",
            "mov rax, 21",
            "mov rbx, 0",
            "mov rcx, {buf}",
            "mov rdx, 1",
            "int 0x80",
            "pop rdx", "pop rcx", "pop rbx",
            buf = in(reg) (&mut c as *mut u8),
            out("rax") r,
        );
    }
    if r & 0x8000_0000_0000_0000 != 0 { -1 } else { c as i32 }
}
pub(crate) fn cstr_len(s: *const u8, max: usize) -> usize {
    unsafe {
        for i in 0..max {
            if *s.add(i) == 0 { return i; }
        }
    }
    max
}

pub(crate) fn u64_to_str(mut n: u64, buf: &mut [u8]) -> usize {
    if n == 0 {
        if !buf.is_empty() { buf[0] = b'0'; return 1; }
        return 0;
    }
    let mut digits = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        if i < 20 { digits[i] = b'0' + (n % 10) as u8; }
        n /= 10;
        i += 1;
    }
    for j in 0..i {
        if j < buf.len() { buf[j] = digits[i - 1 - j]; }
    }
    core::cmp::min(i, buf.len())
}

pub(crate) fn trim_input(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\t') { start += 1; }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t') { end -= 1; }
    &s[start..end]
}
// ── Output API ─────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn console_write(text: *const u8, len: i32) -> i32 {
    if len <= 0 { return 0; }
    unsafe { sys_write(1, text, len as usize) as i32 }
}

#[no_mangle]
pub unsafe extern "C" fn console_write_line(text: *const u8, len: i32) -> i32 {
    if len > 0 { unsafe { sys_write(1, text, len as usize); } }
    write_str(b"\r\n");
    0
}

#[no_mangle]
pub extern "C" fn console_set_color(fg: u8, bg: u8) {
    let mut buf = [0u8; 16];
    buf[0] = 0x1B; buf[1] = b'[';
    let mut pos = 2;
    let fg_code = (fg & 0x0F).saturating_add(30);
    let bg_code = (bg & 0x0F).saturating_add(40);
    let mut tmp = [0u8; 4];
    let fl = u64_to_str(fg_code as u64, &mut tmp);
    buf[pos..pos+fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b';'; pos += 1;
    let bl = u64_to_str(bg_code as u64, &mut tmp);
    buf[pos..pos+bl].copy_from_slice(&tmp[..bl]); pos += bl;
    buf[pos] = b'm'; pos += 1;
    write_str(&buf[..pos]);
}

#[no_mangle]
pub extern "C" fn console_reset_color() { write_str(b"\x1b[0m"); }

#[no_mangle]
pub extern "C" fn console_set_color_256(fg: u8, bg: u8) {
    let mut buf = [0u8; 24];
    let mut tmp = [0u8; 4];
    // foreground: ESC[38;5;Fm
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'3'; buf[3] = b'8'; buf[4] = b';';
    buf[5] = b'5'; buf[6] = b';';
    let fl = u64_to_str(fg as u64, &mut tmp);
    buf[7..7 + fl].copy_from_slice(&tmp[..fl]);
    buf[7 + fl] = b'm';
    write_str(&buf[..8 + fl]);
    // background: ESC[48;5;Bm
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'4'; buf[3] = b'8'; buf[4] = b';';
    buf[5] = b'5'; buf[6] = b';';
    let bl = u64_to_str(bg as u64, &mut tmp);
    buf[7..7 + bl].copy_from_slice(&tmp[..bl]);
    buf[7 + bl] = b'm';
    write_str(&buf[..8 + bl]);
}

#[no_mangle]
pub extern "C" fn console_set_truecolor(fg_r: u8, fg_g: u8, fg_b: u8, bg_r: u8, bg_g: u8, bg_b: u8) {
    let mut buf = [0u8; 48];
    let mut tmp = [0u8; 4];
    // foreground: ESC[38;2;R;G;Bm
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'3'; buf[3] = b'8'; buf[4] = b';';
    buf[5] = b'2'; buf[6] = b';';
    let mut pos = 7;
    let fl = u64_to_str(fg_r as u64, &mut tmp);
    buf[pos..pos + fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b';'; pos += 1;
    let fl = u64_to_str(fg_g as u64, &mut tmp);
    buf[pos..pos + fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b';'; pos += 1;
    let fl = u64_to_str(fg_b as u64, &mut tmp);
    buf[pos..pos + fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b'm'; pos += 1;
    write_str(&buf[..pos]);
    // background: ESC[48;2;R;G;Bm
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'4'; buf[3] = b'8'; buf[4] = b';';
    buf[5] = b'2'; buf[6] = b';';
    pos = 7;
    let fl = u64_to_str(bg_r as u64, &mut tmp);
    buf[pos..pos + fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b';'; pos += 1;
    let fl = u64_to_str(bg_g as u64, &mut tmp);
    buf[pos..pos + fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b';'; pos += 1;
    let fl = u64_to_str(bg_b as u64, &mut tmp);
    buf[pos..pos + fl].copy_from_slice(&tmp[..fl]); pos += fl;
    buf[pos] = b'm'; pos += 1;
    write_str(&buf[..pos]);
}

#[no_mangle]
pub extern "C" fn console_clear_screen() { write_str(b"\x1b[2J"); }

#[no_mangle]
pub extern "C" fn console_cursor_home() { write_str(b"\x1b[H"); }

#[no_mangle]
pub extern "C" fn console_read_byte() -> i32 { read_byte() }
