#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::{ObEnumEntry, ObInfoClass};
use libneodos::i18n;

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
    writeln(b"Usage: nxlocale <command> [options]");
    writeln(b"  list                    List installed locales");
    writeln(b"  current                 Show current locale");
    writeln(b"  set     <locale>        Change system locale");
    writeln(b"  check   [app]           Check translation coverage");
    writeln(b"  stats   [app]           Translation statistics");
    writeln(b"  show    <app>           Show app's loaded strings");
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
        let ca = if a[i] >= b'a' && a[i] <= b'z' { a[i] }
            else if a[i] >= b'A' && a[i] <= b'Z' { a[i] + 32 }
            else { a[i] };
        let cb = if b[i] >= b'a' && b[i] <= b'z' { b[i] }
            else if b[i] >= b'A' && b[i] <= b'Z' { b[i] + 32 }
            else { b[i] };
        if ca != cb { return false; }
    }
    true
}

fn write_u64(n: u64) {
    let mut buf = [0u8; 20];
    let mut i = 19;
    let mut v = n;
    if v == 0 { write_stdout(b"0"); return; }
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_stdout(&buf[i + 1..]);
}

pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load("nxlocale");

    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        help();
        syscall::sys_exit(0);
    }

    let parts = split_args(&raw);
    let cmd = parts[0];
    let arg1 = parts[1];

    if cmd.is_empty() || args_cmp(cmd, b"help") {
        help();
        syscall::sys_exit(0);
    }

    if args_cmp(cmd, b"list") || args_cmp(cmd, b"ls") {
        cmd_list();
    } else if args_cmp(cmd, b"current") || args_cmp(cmd, b"cur") {
        cmd_current();
    } else if args_cmp(cmd, b"set") {
        cmd_set(arg1);
    } else if args_cmp(cmd, b"check") {
        cmd_check(arg1);
    } else if args_cmp(cmd, b"stats") {
        cmd_stats();
    } else if args_cmp(cmd, b"show") {
        cmd_show(arg1);
    } else {
        writeln(b"Unknown command");
        help();
    }

    syscall::sys_exit(0);
}

fn cmd_list() {
    writeln(b"Available locales:");
    let locale_path = "\\Global\\FileSystem\\C:\\System\\Locale";
    match syscall::sys_ob_open(locale_path, syscall::ob_access::READ) {
        Ok(fd) => {
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
            writeln(b"No locales found (check C:\\System\\Locale)");
        }
    }
}

fn cmd_current() {
    write_stdout(b"Current locale: ");
    write_str(i18n::i18n_language());
    writeln(b"");
}

fn cmd_set(locale: &[u8]) {
    if locale.is_empty() {
        writeln(b"Usage: nxlocale set <locale>");
        return;
    }

    let reg_path = "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Locale";
    match syscall::sys_cm_open_key(reg_path) {
        Ok(fd) => {
            let name = "Language";
            let val_type = 1u32;
            match syscall::sys_cm_set_value(fd, name, val_type, locale) {
                Ok(_) => {
                    write_stdout(b"Locale changed to: ");
                    write_stdout(locale);
                    writeln(b"");
                    writeln(b"Restart applications to apply.");
                }
                Err(_) => {
                    writeln(b"Error setting locale");
                }
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {
            writeln(b"Cannot open Registry");
        }
    }
}

fn cmd_check(_app: &[u8]) {
    writeln(b"check: not yet fully implemented");
}

fn cmd_stats() {
    let count = i18n::i18n_loaded_count();
    write_stdout(b"Loaded NLT tables: ");
    write_u64(count as u64);
    writeln(b"");
    write_stdout(b"Current locale: ");
    write_str(i18n::i18n_language());
    writeln(b"");
}

fn cmd_show(app: &[u8]) {
    if app.is_empty() {
        writeln(b"Usage: nxlocale show <app>");
        return;
    }
    let app_str = unsafe { core::str::from_utf8_unchecked(app) };
    match i18n::i18n_load(app_str) {
        Ok(_) => {
            write_stdout(b"Translations loaded for: ");
            write_stdout(app);
            writeln(b"");
        }
        Err(_) => {
            write_stdout(b"No translations available for: ");
            write_stdout(app);
            writeln(b"");
        }
    }
}
