#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::{ObEnumEntry};

const MODE_DIR: u16 = 0x40;
const PERM_R: u16 = 0x0001;
const PERM_W: u16 = 0x0002;
const PERM_X: u16 = 0x0004;
const PERM_S: u16 = 0x0008;
const PERM_D: u16 = 0x0010;
const PAGE_LINES: usize = 23;
const ARGS_ADDR: u64 = 0x41F000;

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

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u64(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = 19;
    if v == 0 { write_str(b"0"); return; }
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if i == 0 { break; }
        i -= 1;
    }
    write_str(&buf[i + 1..=19]);
}

fn is_dir(mode: u16) -> bool {
    (mode & MODE_DIR) != 0
}

fn fmt_perms(mode: u16) -> [u8; 5] {
    let mut p = [b'-'; 5];
    if mode & PERM_R != 0 { p[0] = b'R'; }
    if mode & PERM_W != 0 { p[1] = b'W'; }
    if mode & PERM_X != 0 { p[2] = b'X'; }
    if mode & PERM_S != 0 { p[3] = b'S'; }
    if mode & PERM_D != 0 { p[4] = b'D'; }
    p
}

fn entry_name(name_buf: &[u8; 260]) -> &str {
    let end = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
    core::str::from_utf8(&name_buf[..end]).unwrap_or("?")
}

fn ob_entry_name(e: &ObEnumEntry) -> &str {
    let end = e.name.iter().position(|&b| b == 0).unwrap_or(32);
    core::str::from_utf8(&e.name[..end]).unwrap_or("?")
}

fn read_key() {
    let mut key = [0u8; 1];
    let _ = syscall::sys_read(0, &mut key);
}

fn spaces(n: usize) {
    for _ in 0..n { write_str(b" "); }
}

/// Read args from the shared buffer at 0x41F000.
/// Returns (path, wide, pause).
fn parse_args() -> ([u8; 260], bool, bool) {
    let mut arg_buf = [0u8; 256];
    unsafe {
        core::ptr::copy_nonoverlapping(ARGS_ADDR as *const u8, arg_buf.as_mut_ptr(), 256);
    }
    let arg_slice = trim_ascii(&arg_buf);

    let mut path = [0u8; 260];
    let mut wide = false;
    let mut pause = false;

    if arg_slice.is_empty() {
        return (path, wide, pause);
    }

    let mut tokens: [(usize, usize); 8] = [(0, 0); 8];
    let mut tok_count = 0usize;
    let mut i = 0usize;
    while i < arg_slice.len() && tok_count < 8 {
        while i < arg_slice.len() && (arg_slice[i] == b' ' || arg_slice[i] == b'\t') { i += 1; }
        if i >= arg_slice.len() { break; }
        let start = i;
        while i < arg_slice.len() && arg_slice[i] != b' ' && arg_slice[i] != b'\t' { i += 1; }
            tokens[tok_count] = (start, i);
        tok_count += 1;
    }

    let mut path_tokens: [(usize, usize); 8] = [(0, 0); 8];
    let mut ptok_count = 0usize;

    for t in 0..tok_count {
        let (start, end) = tokens[t];
        let token = &arg_slice[start..end];
        if token == b"/W" || token == b"/w" || token == b"-W" || token == b"-w" {
            wide = true;
        } else if token == b"/P" || token == b"/p" || token == b"-P" || token == b"-p" {
            pause = true;
        } else {
            path_tokens[ptok_count] = (start, end);
            ptok_count += 1;
        }
    }

    if ptok_count > 0 {
        let mut pos = 0usize;
        for t in 0..ptok_count {
            if t > 0 {
                if pos < 259 { path[pos] = b' '; pos += 1; }
            }
            let (start, end) = path_tokens[t];
            for &b in &arg_slice[start..end] {
                if pos < 259 { path[pos] = b; pos += 1; }
            }
        }
    }

    (path, wide, pause)
}

fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\t' || s[start] == b'\0') {
        start += 1;
    }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t' || s[end - 1] == b'\0') {
        end -= 1;
    }
    &s[start..end]
}

fn resolve_path(path_buf: &[u8; 260]) -> [u8; 260] {
    let path_str = entry_name(path_buf);
    if path_str.is_empty() {
        let mut buf = [0u8; 260];
        let mut cwd_buf = [0u8; 256];
        match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 => {
                let mut pos = 0;
                for &b in &cwd_buf[..n - 1] {
                    if pos < 259 { buf[pos] = b; pos += 1; }
                }
                if pos < 259 { buf[pos] = 0; }
            }
            _ => {
                buf[..3].copy_from_slice(b"C:\\");
            }
        }
        return buf;
    }

    let bytes = path_str.as_bytes();
    let mut buf = [0u8; 260];
    if bytes[0] == b'\\' || bytes.contains(&b':') {
        let n = bytes.len().min(259);
        buf[..n].copy_from_slice(&bytes[..n]);
    } else {
        let mut cwd_buf = [0u8; 256];
        let mut pos = 0;
        match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 => {
                for &b in &cwd_buf[..n - 1] {
                    if pos < 259 { buf[pos] = b; pos += 1; }
                }
                if pos > 0 && buf[pos - 1] != b'\\' {
                    if pos < 259 { buf[pos] = b'\\'; pos += 1; }
                }
            }
            _ => {
                buf[..3].copy_from_slice(b"C:\\");
                pos = 3;
            }
        }
        for &b in bytes {
            if pos < 259 { buf[pos] = b; pos += 1; }
        }
    }
    buf
}

#[derive(Clone, Copy)]
struct Info {
    name: [u8; 260],
    dir: bool,
    mode: u16,
    size: u32,
}

fn list_directory(dir_path: &[u8], wide: bool, pause: bool) {
    write_str(b"\r\n Directory of ");
    write_str(dir_path);
    write_str(b"\r\n\r\n");

    let path_str = core::str::from_utf8(dir_path).unwrap_or("C:\\");
    let mut path_end = path_str.len();
    while path_end > 0 && (dir_path[path_end - 1] == b' ' || dir_path[path_end - 1] == 0) {
        path_end -= 1;
    }
    let clean_path = core::str::from_utf8(&dir_path[..path_end]).unwrap_or("C:\\");

    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(clean_path, &mut ob_buf);
    match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => {
            let mut entries: [Info; 256] = [Info { name: [0u8; 260], dir: false, mode: 0, size: 0 }; 256];
            let mut count = 0usize;

            let mut ob_entries: [ObEnumEntry; 256] = core::array::from_fn(|_| ObEnumEntry {
                id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
            });

            match syscall::sys_ob_enum(fd, &mut ob_entries) {
                Ok(n) => {
                    for i in 0..n {
                        let raw = &ob_entries[i];
                        let n = ob_entry_name(raw);
                        if n.is_empty() || n == "." || n == ".." { continue; }
                        if count >= 256 { break; }
                        let mut nb = [0u8; 260];
                        let b = n.as_bytes();
                        let cl = b.len().min(259);
                        nb[..cl].copy_from_slice(&b[..cl]);
                        let is_dir = raw.obj_type == 11; // ObType::Directory
                        entries[count] = Info { name: nb, dir: is_dir, mode: raw.mode, size: raw.size };
                        count += 1;
                    }
                }
                Err(_) => { write_str(b"enum error\r\n"); }
            }
            let _ = syscall::sys_close(fd);

            let mut line_count = 0usize;

            if wide {
                let cols = 5;
                let cell_w: usize = 15;
                let rows = (count + cols - 1) / cols;
                for r in 0..rows {
                    for c in 0..cols {
                        let idx = r + c * rows;
                        if idx < count {
                            let n = entry_name(&entries[idx].name);
                            write_str(n.as_bytes());
                            spaces(cell_w.saturating_sub(n.len()));
                        } else {
                            spaces(cell_w);
                        }
                    }
                    write_str(b"\r\n");
                    line_count += 1;
                    if pause && line_count >= PAGE_LINES {
                        write_str(b"Press any key to continue...");
                        read_key();
                        write_str(b"\r\n");
                        line_count = 0;
                    }
                }
            } else {
                for i in 0..count {
                    let e = &entries[i];
                    let n = entry_name(&e.name);
                    let perms = fmt_perms(e.mode);

                    let mut line_buf = [b' '; 40];
                    let name_len = n.len().min(12);
                    line_buf[..name_len].copy_from_slice(&n.as_bytes()[..name_len]);

                    let type_str: &[u8] = if e.dir { b"<DIR>" } else { b"     " };
                    line_buf[13..18].copy_from_slice(type_str);

                    line_buf[19..24].copy_from_slice(&perms);

                    let mut v = e.size as u64;
                    let mut si = 19usize;
                    let mut tmp = [0u8; 20];
                    if v == 0 {
                        tmp[19] = b'0';
                        si = 18;
                    } else {
                        while v > 0 {
                            tmp[si] = b'0' + (v % 10) as u8;
                            v /= 10;
                            if si == 0 { break; }
                            si -= 1;
                        }
                    }
                    let size_src = &tmp[si + 1..=19];
                    let size_start = 25 + (10 - size_src.len());
                    line_buf[size_start..size_start + size_src.len()].copy_from_slice(size_src);

                    write_str(b"  ");
                    write_str(&line_buf[..35]);
                    write_str(b"\r\n");

                    line_count += 1;
                    if pause && line_count >= PAGE_LINES {
                        write_str(b"Press any key to continue...");
                        read_key();
                        write_str(b"\r\n");
                        line_count = 0;
                    }
                }
            }

            write_str(b"\r\n");
            write_u64(count as u64);
            write_str(b" File(s)\r\n");
        }
        Err(_) => {
            write_str(b"Path not found\r\n");
        }
    }
}

#[used]
#[link_section = ".rodata"]
static DIR_HELP: &[u8] = b"::HELP::\
DIR [path] [/W] [/P]\r\n\
  Lists directory contents.\r\n\
  path   Directory to list (default: current dir)\r\n\
  /W     Wide format: 5 columns, names only\r\n\
  /P     Pause after each screenful\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nDIR [path] [/W] [/P]\r\n  Lists directory contents.\r\n  path   Directory to list (default: current dir)\r\n  /W     Wide format: 5 columns, names only\r\n  /P     Pause after each screenful\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }
    let (path_buf, wide, pause) = parse_args();
    let dir_path = resolve_path(&path_buf);
    let path_end = dir_path.iter().position(|&b| b == 0).unwrap_or(dir_path.len());
    list_directory(&dir_path[..path_end], wide, pause);
    syscall::sys_exit(0)
}
