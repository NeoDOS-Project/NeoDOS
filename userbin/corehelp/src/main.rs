#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::syscall;
use libneodos::syscall::ObEnumEntry;
use libneodos::i18n;
use libneodos::tr;

const APP_NAME: &str = "corehelp";
const PROGRAMS_DIR: &str = "C:\\Programs";

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

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn writeln(s: &[u8]) {
    write_str(s);
    write_str(b"\r\n");
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

fn is_nxe(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 4 { return false; }
    let ext = &b[b.len() - 4..];
    ext[0] == b'.' && (ext[1] == b'N' || ext[1] == b'n')
        && (ext[2] == b'X' || ext[2] == b'x')
        && (ext[3] == b'E' || ext[3] == b'e')
}

fn extract_help_desc(data: &[u8]) -> Option<&[u8]> {
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
                let first_line_end = trimmed.iter().position(|&b| b == b'\r' || b == b'\n')
                    .unwrap_or(trimmed.len());
                return Some(&trimmed[..first_line_end]);
            }
            _ => {
                search_start = abs_pos + 1;
            }
        }
    }
}

fn cmd_list_all() {
    write_str(b"\r\n");
    writeln(tr!("help.header").as_bytes());
    write_str(b"------------------\r\n\r\n");

    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(PROGRAMS_DIR, &mut ob_buf);
    match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => {
            let mut count = 0u64;

            let mut names: [[u8; 32]; 128] = [[0u8; 32]; 128];
            let mut name_lens: [usize; 128] = [0; 128];
            let mut name_count = 0usize;

            let mut ob_entries: [ObEnumEntry; 128] = core::array::from_fn(|_| ObEnumEntry {
                id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
            });
            match syscall::sys_ob_enum(fd, &mut ob_entries) {
                Ok(n) => {
                    for i in 0..n {
                        let raw = &ob_entries[i];
                        let n = raw.name_str();
                        if n.is_empty() || n == "." || n == ".." { continue; }
                        if !is_nxe(n) { continue; }
                        if name_count >= 128 { break; }
                        let bytes = n.as_bytes();
                        let len = bytes.len().min(31);
                        names[name_count][..len].copy_from_slice(&bytes[..len]);
                        name_lens[name_count] = len;
                        name_count += 1;
                    }
                }
                Err(_) => { writeln(tr!("error.reading_dir").as_bytes()); }
            }
            let _ = syscall::sys_close(fd);

            for i in 0..name_count {
                let name = &names[i][..name_lens[i]];

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

                let mut ob_buf2 = [0u8; 512];
                let ob_path2 = to_ob_path(path_str, &mut ob_buf2);
                if let Ok(nxe_fd) = syscall::sys_ob_open(ob_path2, libneodos::syscall::ob_access::READ) {
                    let mut accumulated = [0u8; 32768];
                    let mut total = 0usize;
                    loop {
                        let mut chunk = [0u8; 4096];
                        match syscall::sys_ob_query_info(nxe_fd, libneodos::syscall::ObInfoClass::ReadContent, &mut chunk) {
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
                        let dlen = help_line.len().min(79);
                        desc[..dlen].copy_from_slice(&help_line[..dlen]);
                        desc_len = dlen;
                    } else {
                        desc_len = 0;
                    }
                } else {
                    desc_len = 0;
                }

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
                    write_str(tr!("tooltip.no_description").as_bytes());
                }
                write_str(b"\r\n");
                count += 1;
            }

            write_str(b"\r\n");
            write_u64(count);
            writeln(tr!("help.commands_suffix").as_bytes());
            writeln(tr!("help.type_for_details").as_bytes());
            writeln(tr!("help.example").as_bytes());
            write_str(b"\r\n");
        }
        Err(_) => {
            writeln(tr!("error.no_programs_dir").as_bytes());
            writeln(tr!("error.create_programs_dir").as_bytes());
            write_str(b"\r\n");
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
    let mut ob_buf3 = [0u8; 512];
    let ob_path3 = to_ob_path(path_str, &mut ob_buf3);
    if let Ok(fd) = syscall::sys_ob_open(ob_path3, libneodos::syscall::ob_access::READ) {
        let mut total = 0usize;
        loop {
            let mut chunk = [0u8; 4096];
            match syscall::sys_ob_query_info(fd, libneodos::syscall::ObInfoClass::ReadContent, &mut chunk) {
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
    let mut fds = [0u64; 2];
    if syscall::sys_ob_create("\\Pipe\\help_capture", 4, Some(&mut fds), 0).is_ok() {
        let read_fd = fds[0] as u8;
        let write_fd = fds[1] as u8;

        let packed = (0xFFu64) | ((write_fd as u64) << 8) | ((0xFFu64) << 16);
        let mut ob_buf2 = [0u8; 512];
        let ob_cmd_path = to_ob_path(path_str, &mut ob_buf2);
        match syscall::sys_ob_create(ob_cmd_path, libneodos::syscall::ob_type::PROCESS, None, packed) {
            Ok(proc_fd) => {
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
                let _ = syscall::sys_ob_wait(proc_fd);
                let _ = syscall::sys_close(proc_fd);
                write_str(b"\r\n");
                return;
            }
            Err(_) => {
                let _ = syscall::sys_close(read_fd);
                let _ = syscall::sys_close(write_fd);
            }
        }
    }

    let mut content = [0u8; 32768];
    let total = read_file_content(path_str, &mut content);
    if total > 0 {
        if let Some(full_help) = extract_full_help(&content[..total]) {
            write_str(full_help);
            write_str(b"\r\n");
        } else {
            write_err(tr!("error.no_help").as_bytes());
            write_str(b"\r\n");
        }
    } else {
        write_err(tr!("error.cmd_not_found").as_bytes());
        write_str(b"\r\n");
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
    write_str(b"\r\n");
    writeln(tr!("help.usage").as_bytes());
    writeln(tr!("help.usage_desc1").as_bytes());
    writeln(tr!("help.usage_desc2").as_bytes());
    writeln(tr!("help.usage_desc3").as_bytes());
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

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
