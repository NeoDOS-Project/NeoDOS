#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::DirEntry;

const BIN_DIR: &str = "C:\\Programs";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u64(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = 19;
    if v == 0 {
        write_str(b"0");
        return;
    }
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if i == 0 {
            break;
        }
        i -= 1;
    }
    write_str(&buf[i + 1..=19]);
}

fn entry_name(name_buf: &[u8; 260]) -> &str {
    let end = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
    core::str::from_utf8(&name_buf[..end]).unwrap_or("?")
}

fn is_nxe(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 4 {
        return false;
    }
    let ext = &b[b.len() - 4..];
    ext[0] == b'.'
        && (ext[1] == b'N' || ext[1] == b'n')
        && (ext[2] == b'X' || ext[2] == b'x')
        && (ext[3] == b'E' || ext[3] == b'e')
}

#[used]
#[link_section = ".rodata"]
static HELP_TEXT: &[u8] = b"::HELP::\
HELP\r\n\
  Lists all available core tools in C:\\BIN.\r\n\
  Scans .NXE files from the core tools directory.\r\n\
::END::";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");
    write_str(b"NeoDOS Core Tools\r\n");
    write_str(b"------------------\r\n\r\n");

    match syscall::sys_open(BIN_DIR) {
        Ok(fd) => {
            let mut raw = DirEntry {
                inode: 0,
                mode: 0,
                size: 0,
                name: [0u8; 260],
            };
            let mut count = 0u64;

            loop {
                match syscall::sys_readdir(fd, &mut raw) {
                    Ok(1) => {
                        let n = entry_name(&raw.name);
                        if n.is_empty() || n == "." || n == ".." {
                            continue;
                        }
                        if !is_nxe(n) {
                            continue;
                        }
                        write_str(b"  ");
                        write_str(n.as_bytes());
                        write_str(b"\r\n");
                        count += 1;
                    }
                    Ok(0) => break,
                    Err(_) => {
                        write_str(b"  (error reading directory)\r\n");
                        break;
                    }
                    _ => break,
                }
            }
            let _ = syscall::sys_close(fd);

            write_str(b"\r\n");
            write_u64(count);
            write_str(b" core tool(s) available\r\n");
        }
        Err(_) => {
            write_str(b"No core tools directory found.\r\n");
            write_str(b"Create C:\\Programs with .NXE tools.\r\n");
        }
    }

    write_str(b"\r\n");
    syscall::sys_exit(0)
}
