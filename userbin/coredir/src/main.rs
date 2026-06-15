#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::DirEntry;

const MODE_DIR: u16 = 0x4000;
const PAGE_LINES: usize = 23;

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

fn entry_name(name_buf: &[u8; 260]) -> &str {
    let end = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
    core::str::from_utf8(&name_buf[..end]).unwrap_or("?")
}

fn raw_name(e: &DirEntry) -> &str {
    let end = e.name.iter().position(|&b| b == 0).unwrap_or(0);
    core::str::from_utf8(&e.name[..end]).unwrap_or("?")
}

fn read_key() {
    let mut key = [0u8; 1];
    let _ = syscall::sys_read(0, &mut key);
}

fn spaces(n: usize) {
    for _ in 0..n { write_str(b" "); }
}

fn cell_writer(marker: &[u8; 5], name: &str, cell_w: usize) {
    let mut buf = [b' '; 32];
    buf[..5].copy_from_slice(marker);
    buf[5] = b' ';
    let name_bytes = name.as_bytes();
    let max_name = cell_w.saturating_sub(6).min(18);
    let copy_len = name_bytes.len().min(max_name);
    buf[6..6 + copy_len].copy_from_slice(&name_bytes[..copy_len]);
    write_str(&buf[..cell_w]);
}

#[derive(Clone, Copy)]
struct Info {
    name: [u8; 260],
    dir: bool,
}

fn list_directory(dir_path: &str, wide: bool, pause: bool) {
    write_str(b"\r\n Volume in drive ");
    write_str(&[dir_path.as_bytes()[0]]);
    write_str(b" is NEODOS\r\n\r\n Directory of ");
    write_str(dir_path.as_bytes());
    write_str(b"\r\n\r\n");

    match syscall::sys_open(dir_path) {
        Ok(fd) => {
            let mut entries: [Info; 256] = [Info { name: [0u8; 260], dir: false }; 256];
            let mut count = 0usize;

            let mut raw = DirEntry { inode: 0, mode: 0, size: 0, name: [0u8; 260] };

            loop {
                match syscall::sys_readdir(fd, &mut raw) {
                    Ok(1) => {
                        let n = raw_name(&raw);
                        if n.is_empty() || n == "." || n == ".." { continue; }
                        if count >= 256 { break; }
                        let mut nb = [0u8; 260];
                        let b = n.as_bytes();
                        let cl = b.len().min(259);
                        nb[..cl].copy_from_slice(&b[..cl]);
                        entries[count] = Info { name: nb, dir: is_dir(raw.mode) };
                        count += 1;
                    }
                    Ok(0) => break,
                    Err(_) => { write_str(b"readdir error\r\n"); break; }
                    _ => break,
                }
            }
            let _ = syscall::sys_close(fd);

            let mut line_count = 0usize;

            if wide {
                // Wide mode: 5 columns, just names
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
                // Default: 3 columns with [DIR] markers
                // Entries flow top-to-bottom, left-to-right
                let cols = 3;
                let cell_w: usize = 25;
                let rows = (count + cols - 1) / cols;
                for r in 0..rows {
                    for c in 0..cols {
                        let idx = r + c * rows;
                        if idx < count {
                            let e = &entries[idx];
                            let n = entry_name(&e.name);
                            let marker = if e.dir { b"<DIR>" } else { b"     " };
                            cell_writer(marker, n, cell_w);
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

// Help text marker for external HELP command.
// neoshell's HELP reads the .NXE binary, finds ::HELP::, and
// displays everything up to ::END::.
#[used]
#[link_section = ".rodata"]
static DIR_HELP: &[u8] = b"::HELP::\
DIR [path] [/W] [/P]\r\n\
  Lists directory contents.\r\n\
  path   Directory to list (default: current dir)\r\n\
  /W     Wide format: 5 columns, names only\r\n\
  /P     Pause after each screenful\r\n\
::END::";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Parse the command line for arguments.
    // Currently NeoDOS does not pass argc/argv to user-mode processes.
    // When arg passing is available, the binary will see args like:
    //   DIR.NXE /W
    //   DIR.NXE C:\SYSTEM /P
    //   DIR.NXE /W /P

    let wide = false;
    let pause = false;

    let mut path_buf = [0u8; 260];
    let path_len = {
        let mut cwd_buf = [0u8; 256];
        match syscall::sys_getcwd(&mut cwd_buf) {
            Ok(n) if n > 0 => {
                let mut pos = 0;
                for &b in &cwd_buf[..n - 1] {
                    if pos < 259 { path_buf[pos] = b; pos += 1; }
                }
                pos
            }
            _ => {
                path_buf[..3].copy_from_slice(b"C:\\");
                3
            }
        }
    };

    let dir_path = core::str::from_utf8(&path_buf[..path_len]).unwrap_or("C:\\");
    list_directory(dir_path, wide, pause);
    syscall::sys_exit(0)
}
