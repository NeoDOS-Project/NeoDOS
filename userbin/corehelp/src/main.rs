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
use libneodos::tr_id;

// ── String IDs (from TOML) ──
const IDS_HEADER: u32 = 1001;
const IDS_COMMANDS_SUFFIX: u32 = 1002;
const IDS_TYPE_FOR_DETAILS: u32 = 1003;
const IDS_EXAMPLE: u32 = 1004;
const IDS_NO_PROGRAMS_DIR: u32 = 1005;
const IDS_CREATE_PROGRAMS_DIR: u32 = 1006;
const IDS_ERROR_READING_DIR: u32 = 1007;
const IDS_CMD_NOT_FOUND: u32 = 1010;
const IDS_HELP_USAGE: u32 = 1011;
const IDS_USAGE_DESC1: u32 = 1012;
const IDS_USAGE_DESC2: u32 = 1013;
const IDS_USAGE_DESC3: u32 = 1014;

const APP_NAME: &str = "corehelp";

const DEFAULT_PATHS: &[&str] = &["C:\\Programs"];
const MAX_PATH_DIRS: usize = 8;

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

fn get_path_dirs(buf: &mut [[u8; 260]; MAX_PATH_DIRS]) -> usize {
    let mut count = 0usize;
    if let Ok(fd) = syscall::sys_cm_open_key(
        "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Session Manager\\Environment"
    ) {
        let mut rb = [0u8; 256];
        if let Ok(total) = syscall::sys_cm_query_value(fd, "PATH", &mut rb) {
            if total >= 8 {
                let dlen = u32::from_le_bytes([rb[4], rb[5], rb[6], rb[7]]) as usize;
                let dmax = total.saturating_sub(8);
                let n = dlen.min(dmax);
                if n > 0 {
                    let data = &rb[8..8 + n];
                    let trimmed = if data.last() == Some(&0) { &data[..n.saturating_sub(1)] } else { data };
                    let mut s = 0usize;
                    while s < trimmed.len() && count < MAX_PATH_DIRS {
                        while s < trimmed.len() && trimmed[s] == b';' { s += 1; }
                        if s >= trimmed.len() { break; }
                        let mut e = s;
                        while e < trimmed.len() && trimmed[e] != b';' { e += 1; }
                        let entry = &trimmed[s..e];
                        let elen = entry.len().min(259);
                        let d = &mut buf[count];
                        let is_abs = elen >= 2 && entry[1] == b':'
                            && ((entry[0] >= b'A' && entry[0] <= b'Z') || (entry[0] >= b'a' && entry[0] <= b'z'));
                        if is_abs {
                            d[..elen].copy_from_slice(&entry[..elen]);
                        } else {
                            d[0] = b'C'; d[1] = b':';
                            d[2..2+elen].copy_from_slice(&entry[..elen]);
                        }
                        count += 1;
                        s = e + 1;
                    }
                }
            }
        }
        let _ = syscall::sys_close(fd);
    }
    if count == 0 {
        for (i, def) in DEFAULT_PATHS.iter().enumerate() {
            let blen = def.len().min(259);
            buf[i][..blen].copy_from_slice(def.as_bytes());
            count += 1;
        }
    }
    count
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

fn cmd_list_all() {
    write_str(b"\r\n");
    writeln(tr_id!(IDS_HEADER).as_bytes());
    write_str(b"------------------\r\n\r\n");

    let mut path_dirs: [[u8; 260]; MAX_PATH_DIRS] = [[0u8; 260]; MAX_PATH_DIRS];
    let ndirs = get_path_dirs(&mut path_dirs);

    let mut count = 0u64;
    let mut names: [[u8; 32]; 128] = [[0u8; 32]; 128];
    let mut name_lens: [usize; 128] = [0; 128];
    let mut name_count = 0usize;

    for di in 0..ndirs {
        let dir_bytes = &path_dirs[di];
        let dir_len = dir_bytes.iter().position(|&b| b == 0).unwrap_or(dir_bytes.len());
        let dir_str = core::str::from_utf8(&dir_bytes[..dir_len]).unwrap_or("");

        let mut ob_buf = [0u8; 512];
        let ob_path = to_ob_path(dir_str, &mut ob_buf);
        match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
            Ok(fd) => {
                let mut ob_entries: [ObEnumEntry; 128] = core::array::from_fn(|_| ObEnumEntry {
                    id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
                });
                match syscall::sys_ob_enum(fd, &mut ob_entries) {
                    Ok(n) => {
                        for i in 0..n {
                            let raw = &ob_entries[i];
                            let n2 = raw.name_str();
                            if n2.is_empty() || n2 == "." || n2 == ".." { continue; }
                            if !is_nxe(n2) { continue; }
                            if name_count >= 128 { break; }
                            let base_name = if n2.len() >= 4 {
                                &n2[..n2.len()-4]
                            } else {
                                n2
                            };
                            let mut already = false;
                            for j in 0..name_count {
                                if name_lens[j] == base_name.len()
                                    && &names[j][..name_lens[j]] == base_name.as_bytes()
                                {
                                    already = true;
                                    break;
                                }
                            }
                            if already { continue; }
                            let bytes = n2.as_bytes();
                            let len = bytes.len().min(31);
                            names[name_count][..len].copy_from_slice(&bytes[..len]);
                            name_lens[name_count] = len;
                            name_count += 1;
                        }
                    }
                    Err(_) => { writeln(tr_id!(IDS_ERROR_READING_DIR).as_bytes()); }
                }
                let _ = syscall::sys_close(fd);
            }
            Err(_) => {}
        }
    }

    if name_count == 0 {
        writeln(tr_id!(IDS_NO_PROGRAMS_DIR).as_bytes());
        writeln(tr_id!(IDS_CREATE_PROGRAMS_DIR).as_bytes());
        write_str(b"\r\n");
        return;
    }

    let mut display: [[u8; 32]; 128] = [[0u8; 32]; 128];
    let mut dcount = 0usize;
    for i in 0..name_count {
        let name = &names[i][..name_lens[i]];
        let dn = if name_lens[i] >= 4
            && name[name_lens[i]-4..].eq_ignore_ascii_case(b".NXE") {
            &name[..name_lens[i]-4]
        } else {
            name
        };
        let blen = dn.len().min(31);
        let mut upper = [0u8; 32];
        upper[..blen].copy_from_slice(dn);
        for b in &mut upper[..blen] { if *b >= b'a' && *b <= b'z' { *b -= 32; } }
        display[dcount][..blen].copy_from_slice(&upper[..blen]);
        dcount += 1;
    }

    for i in 0..dcount {
        for j in i+1..dcount {
            let a = &display[i];
            let b2 = &display[j];
            let la = a.iter().position(|&x| x == 0).unwrap_or(32);
            let lb = b2.iter().position(|&x| x == 0).unwrap_or(32);
            let minl = if la < lb { la } else { lb };
            let mut cmp = 0;
            for k in 0..minl {
                if a[k] != b2[k] { cmp = a[k] as i32 - b2[k] as i32; break; }
            }
            if cmp == 0 && la != lb { cmp = la as i32 - lb as i32; }
            if cmp > 0 {
                let tmp = display[i]; display[i] = display[j]; display[j] = tmp;
            }
        }
    }

    const COLS: usize = 5;
    const CW: usize = 15;
    let rows = (dcount + COLS - 1) / COLS;

    for r in 0..rows {
        write_str(b"  ");
        for c in 0..COLS {
            let idx = c * rows + r;
            if idx < dcount {
                let name = &display[idx];
                let nlen = name.iter().position(|&x| x == 0).unwrap_or(32);
                write_str(&name[..nlen]);
                for _ in nlen..CW { write_str(b" "); }
            }
        }
        write_str(b"\r\n");
        count += 1;
    }


    write_str(b"\r\n");
    write_u64(count);
    writeln(tr_id!(IDS_COMMANDS_SUFFIX).as_bytes());
    writeln(tr_id!(IDS_TYPE_FOR_DETAILS).as_bytes());
    writeln(tr_id!(IDS_EXAMPLE).as_bytes());
    write_str(b"\r\n");
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

    write_str(b"\r\n");

    let mut path_dirs: [[u8; 260]; MAX_PATH_DIRS] = [[0u8; 260]; MAX_PATH_DIRS];
    let ndirs = get_path_dirs(&mut path_dirs);

    for di in 0..ndirs {
        let dir_bytes = &path_dirs[di];
        let dir_len = dir_bytes.iter().position(|&b| b == 0).unwrap_or(dir_bytes.len());
        if dir_len == 0 { continue; }

        let mut path_buf = [0u8; 260];
        let mut pos = 0;
        for &b in &dir_bytes[..dir_len] {
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
                write_str(b"\r\n\r\n");
                return;
            }
        }
    }

    write_err(tr_id!(IDS_CMD_NOT_FOUND).as_bytes());
    write_str(b"\r\n\r\n");
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
    writeln(tr_id!(IDS_HELP_USAGE).as_bytes());
    writeln(tr_id!(IDS_USAGE_DESC1).as_bytes());
    writeln(tr_id!(IDS_USAGE_DESC2).as_bytes());
    writeln(tr_id!(IDS_USAGE_DESC3).as_bytes());
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
