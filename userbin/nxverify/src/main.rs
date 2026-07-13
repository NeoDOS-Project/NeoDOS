#![no_std]
#![no_main]

use libneodos::syscall;
use libneodos::syscall::ObEnumEntry;

static CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = 0xEDB88320 ^ (crc >> 1);
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

fn crc32_calc(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc = CRC32_TABLE[((crc as u8) ^ byte) as usize] ^ (crc >> 8);
    }
    !crc
}

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
    writeln(b"Usage: nxverify <command> [options]");
    writeln(b"  file    <path>   Verify single NXE/NXP file");
    writeln(b"  app     <name>   Verify installed app");
    writeln(b"  all              Verify all installed apps");
    writeln(b"  package <nxp>    Verify NXP package CRC32");
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

fn read_file(path: &str, buf: &mut [u8]) -> Result<usize, i64> {
    let fd = syscall::sys_ob_open(path, syscall::ob_access::READ)?;
    let mut total = 0usize;
    loop {
        let remaining = buf.len() - total;
        if remaining == 0 { break; }
        match syscall::sys_read(fd, &mut buf[total..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => total += n,
        }
    }
    let _ = syscall::sys_close(fd);
    Ok(total)
}

fn build_vfs_path(path: &[u8]) -> ([u8; 256], usize) {
    let prefix = b"\\Global\\FileSystem\\C:\\";
    let total = prefix.len() + path.len();
    let mut buf = [0u8; 256];
    if total >= 255 { return (buf, 0); }
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..total].copy_from_slice(path);
    (buf, total)
}

pub extern "C" fn _start() -> ! {
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

    if args_cmp(cmd, b"file") {
        cmd_file(arg1);
    } else if args_cmp(cmd, b"app") {
        cmd_app(arg1);
    } else if args_cmp(cmd, b"all") {
        cmd_all();
    } else if args_cmp(cmd, b"package") || args_cmp(cmd, b"nxp") {
        cmd_package(arg1);
    } else {
        writeln(b"Unknown command");
        help();
    }

    syscall::sys_exit(0);
}

fn cmd_file(path: &[u8]) {
    if path.is_empty() { writeln(b"Usage: nxverify file <path>"); return; }
    let (vfs_path, vfs_len) = build_vfs_path(path);
    if vfs_len == 0 { writeln(b"Path too long"); return; }
    let vfs_str = unsafe { core::str::from_utf8_unchecked(&vfs_path[..vfs_len]) };

    let mut buf = [0u8; 65536];
    match read_file(vfs_str, &mut buf) {
        Ok(size) => {
            let data = &buf[..size];
            if size >= 4 && &data[..4] == b"\x7fELF" {
                write_stdout(b"ELF NXE: ");
                write_stdout(path);
                writeln(b" VALID");
            } else if size >= 4 && data[..4] == [0x4E, 0x58, 0x50, 0x31] {
                write_stdout(b"NXP package: ");
                write_stdout(path);
                writeln(b"");
                cmd_package(path);
            } else {
                writeln(b"Unknown format");
            }
        }
        Err(_) => {
            writeln(b"Error reading file");
        }
    }
}

fn cmd_app(name: &[u8]) {
    if name.is_empty() { writeln(b"Usage: nxverify app <name>"); return; }
    // Build path: Programs/<name>.nxe
    let prefix = b"Programs\\";
    let ext = b".nxe";
    let total = prefix.len() + name.len() + ext.len();
    let mut path = [0u8; 256];
    if total >= 255 { writeln(b"Path too long"); return; }
    path[..prefix.len()].copy_from_slice(prefix);
    path[prefix.len()..prefix.len() + name.len()].copy_from_slice(name);
    let ext_start = prefix.len() + name.len();
    path[ext_start..ext_start + ext.len()].copy_from_slice(ext);
    cmd_file(&path[..total]);
}

fn cmd_all() {
    writeln(b"Verifying all installed apps...");
    let dir_path = "\\Global\\FileSystem\\C:\\Programs";
    match syscall::sys_ob_open(dir_path, syscall::ob_access::READ) {
        Ok(fd) => {
            let mut entries: [ObEnumEntry; 128] = core::array::from_fn(|_| ObEnumEntry {
                id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
            });
            if let Ok(n) = syscall::sys_ob_enum(fd, &mut entries) {
                for i in 0..n {
                    let name = entries[i].name_str();
                    if name.len() < 4 || !name[name.len()-4..].eq_ignore_ascii_case(".nxe") {
                        continue;
                    }
                    let app_name = &name[..name.len() - 4];
                    write_stdout(b"  ");
                    write_str(app_name);
                    write_stdout(b".nxe ... ");
                    cmd_app(app_name.as_bytes());
                }
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => {
            writeln(b"Cannot open Programs directory");
        }
    }
}

fn cmd_package(path: &[u8]) {
    if path.is_empty() { writeln(b"Usage: nxverify package <file.nxp>"); return; }
    let (vfs_path, vfs_len) = build_vfs_path(path);
    if vfs_len == 0 { writeln(b"Path too long"); return; }
    let vfs_str = unsafe { core::str::from_utf8_unchecked(&vfs_path[..vfs_len]) };

    let mut buf = [0u8; 65536];
    match read_file(vfs_str, &mut buf) {
        Ok(size) => {
            let data = &buf[..size];
            if size < 32 || &data[..4] != &[0x4E, 0x58, 0x50, 0x31] {
                writeln(b"Not a valid NXP file");
                syscall::sys_exit(1);
            }
            let hdr_crc = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let actual_hdr_crc = crc32_calc(&data[8..32]);
            if hdr_crc != actual_hdr_crc {
                writeln(b"Header CRC32: MISMATCH");
                syscall::sys_exit(1);
            }
            writeln(b"NXP Package: VALID");
            writeln(b"Header CRC32: OK");
        }
        Err(_) => {
            writeln(b"Error reading package");
        }
    }
}
