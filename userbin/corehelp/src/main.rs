#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::DirEntry;

const PROGRAMS_DIR: &str = "C:\\Programs";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
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

fn entry_name(name_buf: &[u8; 260]) -> &str {
    let end = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
    core::str::from_utf8(&name_buf[..end]).unwrap_or("?")
}

fn is_nxe(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 4 { return false; }
    let ext = &b[b.len() - 4..];
    ext[0] == b'.' && (ext[1] == b'N' || ext[1] == b'n')
        && (ext[2] == b'X' || ext[2] == b'x')
        && (ext[3] == b'E' || ext[3] == b'e')
}

fn extract_help_desc(data: &[u8]) -> Option<&[u8]> {
    // Find "::HELP::" marker followed by "::END::" within 500 bytes
    let help_marker = b"::HELP::";
    let end_marker = b"::END::";
    let mut search_start = 0;
    loop {
        let pos = data[search_start..].windows(help_marker.len()).position(|w| w == help_marker)?;
        let abs_pos = search_start + pos;
        let after_marker = &data[abs_pos + help_marker.len()..];

        // Check if ::END:: exists within the next 500 bytes
        let after_end = after_marker.windows(end_marker.len()).position(|w| w == end_marker);
        match after_end {
            Some(end_pos) if end_pos < 500 => {
                let help_text = &after_marker[..end_pos];
                let trimmed = libneodos::args::trim_ascii(help_text);
                if trimmed.is_empty() {
                    return None;
                }
                let first_line_end = trimmed.iter().position(|&b| b == b'\r' || b == b'\n')
                    .unwrap_or(trimmed.len());
                return Some(&trimmed[..first_line_end]);
            }
            // False positive (::HELP:: in code), try next occurrence
            _ => {
                search_start = abs_pos + 1;
            }
        }
    }
}

fn cmd_list_all() {
    write_str(b"\r\n");
    write_str(b"NeoDOS Core Tools\r\n");
    write_str(b"------------------\r\n\r\n");

    match syscall::sys_open(PROGRAMS_DIR) {
        Ok(fd) => {
            let mut raw = DirEntry {
                inode: 0, mode: 0, size: 0, name: [0u8; 260],
            };
            let mut count = 0u64;

            // Collect .NXE file names
            let mut names: [[u8; 32]; 128] = [[0u8; 32]; 128];
            let mut name_lens: [usize; 128] = [0; 128];
            let mut name_count = 0usize;

            loop {
                match syscall::sys_readdir(fd, &mut raw) {
                    Ok(1) => {
                        let n = entry_name(&raw.name);
                        if n.is_empty() || n == "." || n == ".." { continue; }
                        if !is_nxe(n) { continue; }
                        if name_count >= 128 { break; }
                        let bytes = n.as_bytes();
                        let len = bytes.len().min(31);
                        names[name_count][..len].copy_from_slice(&bytes[..len]);
                        name_lens[name_count] = len;
                        name_count += 1;
                    }
                    Ok(0) => break,
                    Err(_) => { write_str(b"  (error reading directory)\r\n"); break; }
                    _ => break,
                }
            }
            let _ = syscall::sys_close(fd);

            // For each .NXE, open and read help description
            for i in 0..name_count {
                let name = &names[i][..name_lens[i]];

                // Build path: C:\Programs\NAME.NXE
                let mut path_buf = [0u8; 260];
                let mut pos = 0;
                for &b in PROGRAMS_DIR.as_bytes() {
                    if pos < 259 { path_buf[pos] = b; pos += 1; }
                }
                if pos < 259 { path_buf[pos] = b'\\'; pos += 1; }
                for &b in name {
                    if pos < 259 { path_buf[pos] = b; pos += 1; }
                }
                let path_str = core::str::from_utf8(&path_buf[..pos]).unwrap_or("");

                let mut desc = [0u8; 80];
                let desc_len: usize;

                if let Ok(nxe_fd) = syscall::sys_open(path_str) {
                    // Read file in 4096-byte chunks (kernel max per sys_readfile)
                    let mut accumulated = [0u8; 32768];
                    let mut total = 0usize;
                    loop {
                        let mut chunk = [0u8; 4096];
                        match syscall::sys_readfile(nxe_fd, &mut chunk) {
                            Ok(0) => break,
                            Ok(n) => {
                                let copy = n.min(accumulated.len() - total);
                                accumulated[total..total+copy].copy_from_slice(&chunk[..copy]);
                                total += copy;
                            }
                            Err(_) => break,
                        }
                        if total >= accumulated.len() { break; }
                    }
                    let _ = syscall::sys_close(nxe_fd);

                    if let Some(help_line) = extract_help_desc(&accumulated[..total]) {
                        // Extract first 60 bytes of description
                        let dlen = help_line.len().min(79);
                        desc[..dlen].copy_from_slice(&help_line[..dlen]);
                        desc_len = dlen;
                    } else {
                        desc_len = 0;
                    }
                } else {
                    desc_len = 0;
                }

                // Strip .NXE extension for display (case-insensitive)
                let display_name = if name_lens[i] >= 4 {
                    let ext = &name[name_lens[i]-4..name_lens[i]];
                    let is_nxe_ext = ext.len() == 4
                        && (ext[0] == b'.' || ext[0] == b'.')
                        && (ext[1] == b'N' || ext[1] == b'n')
                        && (ext[2] == b'X' || ext[2] == b'x')
                        && (ext[3] == b'E' || ext[3] == b'e');
                    if is_nxe_ext {
                        &name[..name_lens[i]-4]
                    } else {
                        name
                    }
                } else {
                    name
                };
                let dlen = display_name.len();

                // Print: "  CMDNAME      description"
                write_str(b"  ");
                let mut n_upper = [0u8; 32];
                let ulen = dlen.min(31);
                n_upper[..ulen].copy_from_slice(&display_name[..ulen]);
                for b in n_upper[..ulen].iter_mut() {
                    if *b >= b'a' && *b <= b'z' { *b -= 32; }
                }
                write_str(&n_upper[..ulen]);
                for _ in ulen..15 { write_str(b" "); }

                if desc_len > 0 {
                    write_str(&desc[..desc_len]);
                } else {
                    write_str(b"(no description)");
                }
                write_str(b"\r\n");
                count += 1;
            }

            // Remove ".NXE" suffix for display
            write_str(b"\r\n");
            write_u64(count);
            write_str(b" command(s) available\r\n");
            write_str(b"\r\n");
            write_str(b"Type HELP <command> for details on a specific command.\r\n");
            write_str(b"  Example: HELP CLS\r\n\r\n");
        }
        Err(_) => {
            write_str(b"\r\nNo Programs directory found.\r\n");
            write_str(b"Create C:\\Programs with .NXE tools.\r\n\r\n");
        }
    }
}

fn extract_full_help(data: &[u8]) -> Option<&[u8]> {
    let help_marker = b"::HELP::";
    let end_marker = b"::END::";
    let mut search_start = 0;
    loop {
        let pos = data[search_start..].windows(help_marker.len()).position(|w| w == help_marker)?;
        let abs_pos = search_start + pos;
        let after_marker = &data[abs_pos + help_marker.len()..];

        let after_end = after_marker.windows(end_marker.len()).position(|w| w == end_marker);
        match after_end {
            Some(end_pos) if end_pos < 500 => {
                let help_text = &after_marker[..end_pos];
                let trimmed = libneodos::args::trim_ascii(help_text);
                if trimmed.is_empty() {
                    return None;
                }
                return Some(trimmed);
            }
            _ => {
                search_start = abs_pos + 1;
            }
        }
    }
}

fn read_file_content(path_str: &str, buf: &mut [u8]) -> usize {
    if let Ok(fd) = syscall::sys_open(path_str) {
        let mut total = 0usize;
        loop {
            let mut chunk = [0u8; 4096];
            match syscall::sys_readfile(fd, &mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let copy = n.min(buf.len() - total);
                    buf[total..total+copy].copy_from_slice(&chunk[..copy]);
                    total += copy;
                    if total >= buf.len() { break; }
                }
                Err(_) => break,
            }
        }
        let _ = syscall::sys_close(fd);
        total
    } else {
        0
    }
}

fn cmd_show_detail(cmd_name: &str) {
    let mut upper = [0u8; 32];
    let cmd_len = cmd_name.len().min(31);
    for (i, &b) in cmd_name.as_bytes().iter().enumerate() {
        if i >= cmd_len { break; }
        upper[i] = if b >= b'a' && b <= b'z' { b - 32 } else { b };
    }

    // Build path: C:\Programs\CMD.NXE
    let mut path_buf = [0u8; 260];
    let mut pos = 0;
    for &b in PROGRAMS_DIR.as_bytes() {
        if pos < 259 { path_buf[pos] = b; pos += 1; }
    }
    if pos < 259 { path_buf[pos] = b'\\'; pos += 1; }
    for &b in &upper[..cmd_len] {
        if pos < 259 { path_buf[pos] = b; pos += 1; }
    }
    if pos + 4 < 260 {
        path_buf[pos] = b'.'; pos += 1;
        path_buf[pos] = b'N'; pos += 1;
        path_buf[pos] = b'X'; pos += 1;
        path_buf[pos] = b'E'; pos += 1;
    }
    let path_str = core::str::from_utf8(&path_buf[..pos]).unwrap_or("");

    write_str(b"\r\n");
    // First try to spawn CMD.NXE /? with pipe capture
    let mut fds = [0u64; 2];
    if syscall::sys_pipe(&mut fds).is_ok() {
        let read_fd = fds[0] as u8;
        let write_fd = fds[1] as u8;

        match syscall::sys_spawn(path_str, 0xFF, write_fd, 0xFF) {
            Ok(pid) => {
                let _ = syscall::sys_close(write_fd);
                let mut buf = [0u8; 512];
                loop {
                    match syscall::sys_read(read_fd, &mut buf) {
                        Ok(0) => break,
                        Ok(n) => { let _ = syscall::sys_write(1, &buf[..n]); }
                        Err(_) => break,
                    }
                }
                let _ = syscall::sys_close(read_fd);
                let _ = syscall::sys_waitpid(pid);
                write_str(b"\r\n");
                return;
            }
            Err(_) => {
                let _ = syscall::sys_close(read_fd);
                let _ = syscall::sys_close(write_fd);
                // Fall through to direct file read
            }
        }
    }

    // Fallback: read help text directly from the binary
    let mut content = [0u8; 32768];
    let total = read_file_content(path_str, &mut content);
    if total > 0 {
        if let Some(full_help) = extract_full_help(&content[..total]) {
            write_str(full_help);
            write_str(b"\r\n");
        } else {
            write_err(b"No help available for this command.\r\n");
        }
    } else {
        write_err(b"HELP: command not found\r\n");
    }
    write_str(b"\r\n");
}

#[used]
#[link_section = ".rodata"]
static HELP_HELP: &[u8] = b"::HELP::\
HELP [command]\r\n\
  Lists available commands with descriptions.\r\n\
  HELP <command>     Shows detailed help for a specific command.\r\n\
  HELP              Lists all commands.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nHELP [command]\r\n");
    write_str(b"  Lists available commands with descriptions.\r\n");
    write_str(b"  HELP CLS          Shows help for the CLS command.\r\n");
    write_str(b"  HELP              Lists all commands.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw_args = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw_args);

    if args == b"/?" || args == b"-h" || args == b"--help" {
        print_help();
        syscall::sys_exit(0);
    }

    if args.is_empty() {
        cmd_list_all();
    } else {
        let cmd_str = core::str::from_utf8(args).unwrap_or("");
        cmd_show_detail(cmd_str);
    }

    syscall::sys_exit(0)
}