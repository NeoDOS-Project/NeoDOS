#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::DirEntry;

const MODE_DIR: u16 = 0x40;
const ARGS_ADDR: u64 = 0x41F000;
const MAX_DEPTH: usize = 6;
const MAX_ENTRIES: usize = 64;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn is_dir(mode: u16) -> bool {
    (mode & MODE_DIR) != 0
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

fn read_args() -> [u8; 260] {
    let mut arg_buf = [0u8; 256];
    unsafe {
        core::ptr::copy_nonoverlapping(ARGS_ADDR as *const u8, arg_buf.as_mut_ptr(), 256);
    }
    let arg_slice = trim_ascii(&arg_buf);
    let mut path = [0u8; 260];
    if !arg_slice.is_empty() {
        let n = arg_slice.len().min(259);
        path[..n].copy_from_slice(&arg_slice[..n]);
    }
    path
}

fn entry_name(name: &[u8; 260]) -> &str {
    let end = name.iter().position(|&b| b == 0).unwrap_or(0);
    core::str::from_utf8(&name[..end]).unwrap_or("?")
}

fn resolve_path(path_buf: &[u8; 260]) -> [u8; 260] {
    let path_str = entry_name(path_buf);
    let mut buf = [0u8; 260];

    if path_str.is_empty() {
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
struct Entry {
    name: [u8; 260],
    is_directory: bool,
}

fn collect_entries(dir_path: &str, entries: &mut [Entry; MAX_ENTRIES]) -> usize {
    match syscall::sys_open(dir_path) {
        Ok(fd) => {
            let mut count = 0;
            let mut raw = DirEntry { inode: 0, mode: 0, size: 0, name: [0u8; 260] };

            loop {
                match syscall::sys_readdir(fd, &mut raw) {
                    Ok(1) => {
                        let name_str = raw.name_str();
                        if name_str.is_empty() || name_str == "." || name_str == ".." {
                            continue;
                        }
                        if count >= MAX_ENTRIES { break; }
                        let mut nb = [0u8; 260];
                        let b = name_str.as_bytes();
                        let cl = b.len().min(259);
                        nb[..cl].copy_from_slice(&b[..cl]);
                        entries[count] = Entry {
                            name: nb,
                            is_directory: is_dir(raw.mode),
                        };
                        count += 1;
                    }
                    Ok(0) => break,
                    Err(_) => { write_str(b"\r\nreaddir error\r\n"); break; }
                    _ => break,
                }
            }
            let _ = syscall::sys_close(fd);
            count
        }
        Err(_) => 0,
    }
}

fn sort_entries(entries: &mut [Entry], count: usize) {
    for i in 1..count {
        let mut j = i;
        while j > 0 {
            let a_is_dir = entries[j - 1].is_directory;
            let b_is_dir = entries[j].is_directory;

            let a_name = {
                let end = entries[j - 1].name.iter().position(|&b| b == 0).unwrap_or(0);
                &entries[j - 1].name[..end]
            };
            let b_name = {
                let end = entries[j].name.iter().position(|&b| b == 0).unwrap_or(0);
                &entries[j].name[..end]
            };

            let should_swap = if a_is_dir != b_is_dir {
                b_is_dir && !a_is_dir
            } else {
                let mut cmp_result = false;
                let mut determined = false;
                let mut k = 0;
                loop {
                    let ac = if k < a_name.len() { a_name[k] } else { break; };
                    let bc = if k < b_name.len() { b_name[k] } else { break; };
                    let au = if ac >= b'a' && ac <= b'z' { ac - 32 } else { ac };
                    let bu = if bc >= b'a' && bc <= b'z' { bc - 32 } else { bc };
                    if au != bu {
                        cmp_result = au > bu;
                        determined = true;
                        break;
                    }
                    k += 1;
                }
                if !determined {
                    a_name.len() > b_name.len()
                } else {
                    cmp_result
                }
            };

            if should_swap {
                entries.swap(j - 1, j);
                j -= 1;
            } else {
                break;
            }
        }
    }
}

const TEE: &[u8] = &[0xE2, 0x94, 0x9C, 0xE2, 0x94, 0x80, 0xE2, 0x94, 0x80, 0x20];
const CORNER: &[u8] = &[0xE2, 0x94, 0x94, 0xE2, 0x94, 0x80, 0xE2, 0x94, 0x80, 0x20];
const PIPE: &[u8] = &[0xE2, 0x94, 0x82, 0x20, 0x20, 0x20];
const EMPTY: &[u8] = b"    ";

fn print_tree(dir_path: &str, prefix: &[u8], depth: usize) {
    let mut entries = [Entry { name: [0u8; 260], is_directory: false }; MAX_ENTRIES];
    let count = collect_entries(dir_path, &mut entries);
    if count == 0 { return; }

    sort_entries(&mut entries, count);

    for i in 0..count {
        let is_last = i == count - 1;
        let connector = if is_last { CORNER } else { TEE };
        let name = entry_name(&entries[i].name);

        write_str(prefix);
        write_str(connector);
        write_str(name.as_bytes());

        if entries[i].is_directory {
            write_str(b"\r\n");

            if depth + 1 < MAX_DEPTH && count > 0 {
                let mut new_prefix = [0u8; 160];
                let mut pos = 0;
                for &b in prefix {
                    if pos >= 159 { break; }
                    new_prefix[pos] = b;
                    pos += 1;
                }
                let indent = if is_last { EMPTY } else { PIPE };
                for &b in indent {
                    if pos >= 159 { break; }
                    new_prefix[pos] = b;
                    pos += 1;
                }

                let mut sub_path = [0u8; 260];
                let mut sp = 0;
                for &b in dir_path.as_bytes() {
                    if sp >= 259 { break; }
                    sub_path[sp] = b;
                    sp += 1;
                }
                if sp > 0 && sub_path[sp - 1] != b'\\' && sp < 259 {
                    sub_path[sp] = b'\\';
                    sp += 1;
                }
                for &b in name.as_bytes() {
                    if sp >= 259 { break; }
                    sub_path[sp] = b;
                    sp += 1;
                }

                let sub_path_str = core::str::from_utf8(&sub_path[..sp]).unwrap_or("");
                print_tree(sub_path_str, &new_prefix[..pos], depth + 1);
            }
        } else {
            write_str(b"\r\n");
        }
    }
}

#[used]
#[link_section = ".rodata"]
static TREE_HELP: &[u8] = b"::HELP::\
TREE [drive:][path]\r\n\
  Display directory tree.\r\n\
  TREE            shows tree of current directory\r\n\
  TREE C:\\        shows tree of C:\\\r\n\
  TREE \\Programs  shows tree of \\Programs\r\n\
::END::";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let path_buf = read_args();
    let arg_slice = {
        let mut arg_buf = [0u8; 256];
        unsafe {
            core::ptr::copy_nonoverlapping(ARGS_ADDR as *const u8, arg_buf.as_mut_ptr(), 256);
        }
        let s = trim_ascii(&arg_buf);
        let mut buf = [0u8; 260];
        let n = s.len().min(259);
        buf[..n].copy_from_slice(&s[..n]);
        buf
    };

    let arg_slice_ref = trim_ascii(&arg_slice);
    if arg_slice_ref == b"/?" || arg_slice_ref == b"-h" || arg_slice_ref == b"--help" {
        write_str(b"\r\nTREE [drive:][path]\r\n  Display directory tree.\r\n  TREE            shows tree of current directory\r\n  TREE C:\\        shows tree of C:\\\r\n  TREE \\Programs  shows tree of \\Programs\r\n\r\n");
        syscall::sys_exit(0);
    }

    let resolved = resolve_path(&path_buf);
    let path_end = resolved.iter().position(|&b| b == 0).unwrap_or(resolved.len());
    let dir_path = core::str::from_utf8(&resolved[..path_end]).unwrap_or("C:\\");

    write_str(b"\r\n");
    write_str(dir_path.as_bytes());
    write_str(b"\r\n");

    print_tree(dir_path, b"", 0);

    write_str(b"\r\n");
    syscall::sys_exit(0)
}
