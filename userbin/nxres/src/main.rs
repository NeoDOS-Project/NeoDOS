#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::ObEnumEntry;

fn write_stdout(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn writeln(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
    let _ = syscall::sys_write(1, b"\r\n");
}

fn write_str(s: &str) {
    write_stdout(s.as_bytes());
}

fn help() {
    writeln(b"Usage: nxres <command> [options]");
    writeln(b"  list    <app>         List resources");
    writeln(b"  cat     <app> <path>  Display resource content");
    writeln(b"  tree    <app>         Tree view");
    writeln(b"  find    <app> <pat>   Search resources");
    writeln(b"  locale  <app>         Show available locales");
}

fn split_args(args: &[u8]) -> [&[u8]; 4] {
    let mut parts = [&[][..], &[][..], &[][..], &[][..]];
    let mut pi = 0usize;
    let mut i = 0usize;
    while i < args.len() && pi < 4 {
        while i < args.len() && (args[i] == b' ' || args[i] == b'\t') { i += 1; }
        if i >= args.len() { break; }
        let start = i;
        while i < args.len() && args[i] != b' ' && args[i] != b'\t' { i += 1; }
        if i > start {
            parts[pi] = &args[start..i];
            pi += 1;
        }
    }
    parts
}

fn args_cmp(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    for i in 0..a.len() {
        let ca = if a[i] >= b'a' && a[i] <= b'z' { a[i] } else if a[i] >= b'A' && a[i] <= b'Z' { a[i] + 32 } else { a[i] };
        let cb = if b[i] >= b'a' && b[i] <= b'z' { b[i] } else if b[i] >= b'A' && b[i] <= b'Z' { b[i] + 32 } else { b[i] };
        if ca != cb { return false; }
    }
    true
}

fn build_res_path(app: &[u8], file_path: &[u8]) -> ([u8; 256], usize) {
    let prefix = b"\\Global\\FileSystem\\C:\\Programs\\";
    let mid = b"\\resources\\";
    let total = prefix.len() + app.len() + mid.len() + file_path.len();
    let mut buf = [0u8; 256];
    let mut pos = 0;
    if total > 255 { return (buf, 0); }
    buf[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    buf[pos..pos + app.len()].copy_from_slice(app);
    pos += app.len();
    buf[pos..pos + mid.len()].copy_from_slice(mid);
    pos += mid.len();
    buf[pos..pos + file_path.len()].copy_from_slice(file_path);
    pos += file_path.len();
    (buf, pos)
}

pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        help();
        syscall::sys_exit(0);
    }

    let parts = split_args(&raw);
    let cmd = parts[0];
    let app = parts[1];
    let extra = parts[2];

    if cmd.is_empty() || args_cmp(cmd, b"help") {
        help();
        syscall::sys_exit(0);
    }

    if parts[1].is_empty() && !args_cmp(cmd, b"help") {
        writeln(b"Error: app name required");
        help();
        syscall::sys_exit(1);
    }

    if args_cmp(cmd, b"list") || args_cmp(cmd, b"ls") {
        cmd_list(app);
    } else if args_cmp(cmd, b"cat") || args_cmp(cmd, b"show") {
        cmd_cat(app, extra);
    } else if args_cmp(cmd, b"locale") || args_cmp(cmd, b"locales") {
        cmd_locale(app);
    } else if args_cmp(cmd, b"tree") || args_cmp(cmd, b"find") {
        write_stdout(cmd);
        writeln(b": not yet implemented");
    } else {
        writeln(b"Unknown command");
        help();
    }

    syscall::sys_exit(0);
}

fn cmd_list(app: &[u8]) {
    let (path_buf, path_len) = build_res_path(app, b"");
    if path_len == 0 { writeln(b"Error: path too long"); return; }
    let path_str = unsafe { core::str::from_utf8_unchecked(&path_buf[..path_len]) };

    match syscall::sys_ob_open(path_str, syscall::ob_access::READ) {
        Ok(fd) => {
            let mut entries: [ObEnumEntry; 64] = core::array::from_fn(|_| ObEnumEntry {
                id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
            });
            if let Ok(n) = syscall::sys_ob_enum(fd, &mut entries) {
                for i in 0..n {
                    let name = entries[i].name_str();
                    write_stdout(b"  ");
                    write_str(name);
                    writeln(b"");
                }
            } else {
                writeln(b"(empty or error reading)");
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {
            writeln(b"App not found or no resources");
        }
    }
}

fn cmd_cat(app: &[u8], file_path: &[u8]) {
    if file_path.is_empty() {
        writeln(b"Usage: nxres cat <app> <path>");
        return;
    }
    let (path_buf, path_len) = build_res_path(app, file_path);
    if path_len == 0 { writeln(b"Error: path too long"); return; }
    let path_str = unsafe { core::str::from_utf8_unchecked(&path_buf[..path_len]) };

    match syscall::sys_ob_open(path_str, syscall::ob_access::READ) {
        Ok(fd) => {
            loop {
                let mut buf = [0u8; 4096];
                match syscall::sys_read(fd, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => { write_stdout(&buf[..n]); }
                }
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {
            writeln(b"Resource not found");
        }
    }
}

fn cmd_locale(app: &[u8]) {
    let (mut path_buf, mut path_len) = build_res_path(app, b"locale");
    if path_len == 0 { writeln(b"Error: path too long"); return; }
    let path_str = unsafe { core::str::from_utf8_unchecked(&path_buf[..path_len]) };

    match syscall::sys_ob_open(path_str, syscall::ob_access::READ) {
        Ok(fd) => {
            writeln(b"Available locales:");
            let mut entries: [ObEnumEntry; 16] = core::array::from_fn(|_| ObEnumEntry {
                id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
            });
            if let Ok(n) = syscall::sys_ob_enum(fd, &mut entries) {
                for i in 0..n {
                    let name = entries[i].name_str();
                    write_stdout(b"  ");
                    write_str(name);
                    writeln(b"");
                }
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {
            writeln(b"No locale resources found for this app");
        }
    }
}
