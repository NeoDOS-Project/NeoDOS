#![no_std]
#![no_main]

use libneodos::fs::File;
use libneodos::syscall;

fn write_stdout(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}
fn write_stderr(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}
fn writeln(s: &[u8]) {
    write_stdout(s);
    write_stdout(b"\r\n");
}
fn write_str(s: &str) {
    write_stdout(s.as_bytes());
}
fn writeln_str(s: &str) {
    write_str(s);
    write_stdout(b"\r\n");
}
fn write_u64(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = 19;
    if v == 0 { write_stdout(b"0"); return; }
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if i == 0 { break; }
        i -= 1;
    }
    write_stdout(&buf[i + 1..=19]);
}

fn split_first_arg(args: &[u8]) -> (&[u8], &[u8]) {
    if let Some(pos) = args.iter().position(|&b| b == b' ') {
        let rest = libneodos::args::trim_ascii(&args[pos + 1..]);
        (&args[..pos], rest)
    } else {
        (args, &[])
    }
}

fn split_two_args(args: &[u8]) -> [&[u8]; 2] {
    let t = libneodos::args::trim_ascii(args);
    if let Some(pos) = t.iter().position(|&b| b == b' ') {
        let s2 = libneodos::args::trim_ascii(&t[pos + 1..]);
        [&t[..pos], s2]
    } else {
        [t, &[]]
    }
}

fn path_to_ob<'a>(vfs_path: &'a str, buf: &'a mut [u8; 512]) -> &'a str {
    let prefix = b"\\Global\\FileSystem\\";
    let vb = vfs_path.as_bytes();
    let total = prefix.len() + vb.len();
    if total > 510 { return vfs_path; }
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..total].copy_from_slice(vb);
    buf[total] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..total]) }
}

// ── NLT parsing ───────────────────────────────────────────────────────

const NLT_MAGIC: [u8; 4] = [b'N', b'L', b'T', 0];

#[derive(Clone, Copy)]
struct Entry {
    kstart: u16,
    klen: u16,
    vstart: u16,
    vlen: u16,
}

struct NltFile {
    data: [u8; 65536],
    len: usize,
    valid: bool,
    count: usize,
    entries: [Entry; 1024],
    errors: [u8; 2048],
    err_len: usize,
}

impl NltFile {
    fn new() -> Self {
        NltFile {
            data: [0; 65536], len: 0, valid: false, count: 0,
            entries: [Entry { kstart: 0, klen: 0, vstart: 0, vlen: 0 }; 1024],
            errors: [0; 2048], err_len: 0,
        }
    }

    fn add_err(&mut self, msg: &[u8]) {
        let rem = 2048 - self.err_len;
        if rem > 0 {
            let n = msg.len().min(rem - 1);
            self.errors[self.err_len..self.err_len + n].copy_from_slice(&msg[..n]);
            self.err_len += n;
            self.errors[self.err_len] = b'\n';
            self.err_len = self.err_len.min(2047);
        }
    }

    fn print_errors(&self) {
        write_stderr(&self.errors[..self.err_len]);
    }

    fn load(path: &str) -> Option<Self> {
        let mut file = File::open(path).ok()?;
        let mut nlt = NltFile::new();
        nlt.len = file.read(&mut nlt.data).ok()?;
        nlt.parse();
        Some(nlt)
    }

    fn parse(&mut self) {
        let len = self.len;
        if len < 12 {
            self.add_err(b"ERROR: file too small (<12 bytes)");
            return;
        }
        if self.data[..4] != NLT_MAGIC {
            self.add_err(b"ERROR: invalid magic (expected NLT\\0)");
            return;
        }
        let ver = u32::from_le_bytes([self.data[4], self.data[5], self.data[6], self.data[7]]);
        if ver != 1 {
            self.add_err(b"ERROR: unsupported version (expected 1)");
            return;
        }
        let count = u32::from_le_bytes([self.data[8], self.data[9], self.data[10], self.data[11]]) as usize;
        if count > 1024 {
            self.add_err(b"ERROR: too many entries (max 1024)");
            return;
        }
        let ko_off = 12;
        let vo_off = 12 + count * 4;
        let min_sz = vo_off + count * 4;
        if len < min_sz {
            self.add_err(b"ERROR: file too small for offset tables");
            return;
        }
        // Collect key/value strings and offsets, deferring error reporting
        // to avoid borrow conflicts with self.add_err.
        let mut local_ok = true;
        #[derive(Clone, Copy)]
        struct RawEntry { ko: u32, vo: u32, koff: u16, klen: u16, voff: u16, vlen: u16 }
        let mut raw = [RawEntry { ko: 0, vo: 0, koff: 0, klen: 0, voff: 0, vlen: 0 }; 1024];
        for i in 0..count {
            let kp = ko_off + i * 4;
            let vp = vo_off + i * 4;
            let ko = u32::from_le_bytes([self.data[kp], self.data[kp+1], self.data[kp+2], self.data[kp+3]]);
            let vo = u32::from_le_bytes([self.data[vp], self.data[vp+1], self.data[vp+2], self.data[vp+3]]);
            raw[i].ko = ko;
            raw[i].vo = vo;
            match null_str(&self.data[..len], ko) {
                Some(s) => {
                    raw[i].koff = (s.as_ptr() as u64 - self.data.as_ptr() as u64) as u16;
                    raw[i].klen = s.len() as u16;
                }
                None => { local_ok = false; }
            }
            match null_str(&self.data[..len], vo) {
                Some(s) => {
                    raw[i].voff = (s.as_ptr() as u64 - self.data.as_ptr() as u64) as u16;
                    raw[i].vlen = s.len() as u16;
                }
                None => { local_ok = false; }
            }
        }
        if !local_ok {
            self.add_err(b"ERROR: key or value offset out of range");
        }
        // Check duplicates (copy keys to stack first to avoid borrow conflicts)
        let mut dup_err = [0u8; 256];
        for i in 0..count {
            if raw[i].klen == 0 { continue; }
            for j in i+1..count {
                if raw[j].klen == 0 { continue; }
                if raw[i].klen == raw[j].klen {
                    let ki = &self.data[raw[i].koff as usize..][..raw[i].klen as usize];
                    let kj = &self.data[raw[j].koff as usize..][..raw[j].klen as usize];
                    if ki == kj {
                        let n = ki.len().min(250);
                        dup_err[..n].copy_from_slice(&ki[..n]);
                        self.add_err(b"ERROR: duplicate key");
                        self.add_err(&dup_err[..n]);
                        local_ok = false;
                    }
                }
            }
        }
        // Commit entries
        self.count = count;
        for i in 0..count {
            self.entries[i] = Entry {
                kstart: raw[i].koff, klen: raw[i].klen,
                vstart: raw[i].voff, vlen: raw[i].vlen,
            };
        }
        if count == 0 {
            self.add_err(b"WARNING: empty translation table");
        }
        self.valid = local_ok;
    }

    fn key_at(&self, i: usize) -> &[u8] {
        &self.data[self.entries[i].kstart as usize..][..self.entries[i].klen as usize]
    }
    fn val_at(&self, i: usize) -> &[u8] {
        &self.data[self.entries[i].vstart as usize..][..self.entries[i].vlen as usize]
    }
}

fn null_str<'a>(data: &'a [u8], off: u32) -> Option<&'a str> {
    let start = off as usize;
    if start >= data.len() { return None; }
    let end = data[start..].iter().position(|&b| b == 0)?;
    core::str::from_utf8(&data[start..start + end]).ok()
}

// ── Commands ──────────────────────────────────────────────────────────

fn cmd_validate(path: &[u8]) {
    let p = core::str::from_utf8(path).unwrap_or("");
    let nlt = match NltFile::load(p) {
        Some(n) => n,
        None => { writeln_str("ERROR: cannot open file"); return; }
    };
    if nlt.err_len > 0 {
        nlt.print_errors();
    }
    if nlt.valid {
        write_stdout(b"VALID  entries="); write_u64(nlt.count as u64);
        write_stdout(b" size="); write_u64(nlt.len as u64); writeln(b" bytes");
    } else {
        writeln_str("INVALID");
    }
}

fn cmd_stats(path: &[u8]) {
    let p = core::str::from_utf8(path).unwrap_or("");
    let nlt = match NltFile::load(p) {
        Some(n) => n,
        None => { writeln_str("ERROR: cannot open file"); return; }
    };
    if nlt.err_len > 0 { nlt.print_errors(); }
    if !nlt.valid { return; }

    let mut total_k = 0usize;
    let mut total_v = 0usize;
    for i in 0..nlt.count {
        total_k += nlt.entries[i].klen as usize;
        total_v += nlt.entries[i].vlen as usize;
    }
    writeln_str("=== NLT Statistics ===");
    write_stdout(b"  File:        "); write_stdout(path); writeln(b"");
    write_stdout(b"  Size:        "); write_u64(nlt.len as u64); writeln(b" bytes");
    write_stdout(b"  Entries:     "); write_u64(nlt.count as u64); writeln(b"");
    write_stdout(b"  Key bytes:   "); write_u64(total_k as u64); writeln(b"");
    write_stdout(b"  Value bytes: "); write_u64(total_v as u64); writeln(b"");
    if nlt.count > 0 {
        write_stdout(b"  Avg key:     "); write_u64((total_k / nlt.count) as u64); writeln(b" bytes");
        write_stdout(b"  Avg value:   "); write_u64((total_v / nlt.count) as u64); writeln(b" bytes");
    }
}

fn cmd_diff(args: &[u8]) {
    let parts = split_two_args(args);
    if parts[0].is_empty() || parts[1].is_empty() {
        writeln_str("Usage: neolocale diff <file1.nlt> <file2.nlt>");
        return;
    }
    let p1 = core::str::from_utf8(parts[0]).unwrap_or("");
    let p2 = core::str::from_utf8(parts[1]).unwrap_or("");
    let a = match NltFile::load(p1) { Some(n) => n, None => { writeln_str("ERROR: cannot open first file"); return; } };
    let b = match NltFile::load(p2) { Some(n) => n, None => { writeln_str("ERROR: cannot open second file"); return; } };

    let mut changed = 0u64;
    let mut only_a = 0u64;
    let mut only_b = 0u64;

    for i in 0..a.count {
        let ka = a.key_at(i);
        let va = a.val_at(i);
        let mut found = false;
        for j in 0..b.count {
            if ka == b.key_at(j) {
                found = true;
                if va != b.val_at(j) {
                    write_stdout(b"  ~ "); write_stdout(ka); writeln(b"");
                    write_stdout(b"    - "); write_stdout(va); writeln(b"");
                    write_stdout(b"    + "); write_stdout(b.val_at(j)); writeln(b"");
                    changed += 1;
                }
                break;
            }
        }
        if !found {
            write_stdout(b"  - "); write_stdout(ka); writeln(b"  (only in first)");
            only_a += 1;
        }
    }
    for j in 0..b.count {
        let kb = b.key_at(j);
        let mut found = false;
        for i in 0..a.count {
            if kb == a.key_at(i) { found = true; break; }
        }
        if !found {
            write_stdout(b"  + "); write_stdout(kb); writeln(b"  (only in second)");
            only_b += 1;
        }
    }

    if changed == 0 && only_a == 0 && only_b == 0 {
        writeln_str("  (identical)");
    } else {
        write_stdout(b"  "); write_u64(changed); writeln(b" changed");
        write_stdout(b"  "); write_u64(only_a); writeln(b" only in first");
        write_stdout(b"  "); write_u64(only_b); writeln(b" only in second");
    }
}

fn cmd_create(args: &[u8]) {
    let parts = split_two_args(args);
    let app = parts[0];
    let _locale = if parts[1].is_empty() { b"en-US" } else { parts[1] };

    if app.is_empty() {
        writeln_str("Usage: neolocale create <app> [locale]");
        writeln_str("  Creates an empty NLT scaffold (stdout). Redirect to file:");
        writeln_str("    neolocale create myapp > myapp.nlt");
        return;
    }

    let mut hdr = [0u8; 12];
    hdr[..4].copy_from_slice(b"NLT\0");
    hdr[4..8].copy_from_slice(&1u32.to_le_bytes());
    hdr[8..12].copy_from_slice(&0u32.to_le_bytes());
    write_stdout(&hdr);

    write_stderr(b"Created empty NLT scaffold. Use gen_nlt.py to add entries.\r\n");
}

fn cmd_check(args: &[u8]) {
    let locale_dir = if args.is_empty() { b"C:\\System\\Locale" } else { args };
    let dir_str = core::str::from_utf8(locale_dir).unwrap_or("C:\\System\\Locale");
    writeln_str("Checking locale directory: ");
    writeln_str(dir_str);

    // List locale subdirectories via Ob
    let mut ob_buf = [0u8; 512];
    let ob_path = path_to_ob(dir_str, &mut ob_buf);
    let dir_fd = match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => fd,
        Err(_) => { writeln_str("ERROR: cannot open locale directory"); return; }
    };

    let mut locales: [[u8; 16]; 16] = [[0; 16]; 16];
    let mut loc_lens: [usize; 16] = [0; 16];
    let mut loc_count = 0usize;

    let mut entries: [libneodos::syscall::ObEnumEntry; 64] = core::array::from_fn(|_|
        libneodos::syscall::ObEnumEntry { id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0 }
    );
    if let Ok(n) = syscall::sys_ob_enum(dir_fd, &mut entries) {
        for i in 0..n {
            let name = entries[i].name_str();
            if name.is_empty() || name == "." || name == ".." { continue; }
            if (entries[i].mode & 0x10) == 0 { continue; }
            if loc_count >= 16 { break; }
            let b = name.as_bytes();
            let len = b.len().min(15);
            locales[loc_count][..len].copy_from_slice(b);
            loc_lens[loc_count] = len;
            loc_count += 1;
        }
    }
    let _ = syscall::sys_close(dir_fd);

    if loc_count == 0 {
        writeln_str("No locale subdirectories found.");
        return;
    }
    write_stdout(b"Found "); write_u64(loc_count as u64); writeln(b" locale(s)");
    for li in 0..loc_count {
        let loc = unsafe { core::str::from_utf8_unchecked(&locales[li][..loc_lens[li]]) };
        // List .nlt files in this locale dir
        let sub_path_buf = build_sub_path(dir_str, loc);
        let mut ob_buf2 = [0u8; 512];
        let ob_sub = path_to_ob(unsafe { core::str::from_utf8_unchecked(&sub_path_buf) }, &mut ob_buf2);
        if let Ok(fd2) = syscall::sys_ob_open(ob_sub, libneodos::syscall::ob_access::READ) {
            let mut nlt_e: [libneodos::syscall::ObEnumEntry; 64] = core::array::from_fn(|_|
                libneodos::syscall::ObEnumEntry { id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0 }
            );
            let mut apps_in_loc = 0u64;
            if let Ok(m) = syscall::sys_ob_enum(fd2, &mut nlt_e) {
                for j in 0..m {
                    let n = nlt_e[j].name_str();
                    if n.len() < 5 || !n.ends_with(".nlt") { continue; }
                    if n == "." || n == ".." { continue; }
                    apps_in_loc += 1;
                }
            }
            write_stdout(b"  "); write_stdout(loc.as_bytes());
            write_stdout(b": "); write_u64(apps_in_loc); writeln(b" app(s)");
            let _ = syscall::sys_close(fd2);
        }
    }

    // Cross-locale missing key check
    if loc_count > 1 {
        writeln_str("");
        writeln_str("Checking for missing translations...");
        // Compare first locale (en-US reference) against others
        let ref_loc = unsafe { core::str::from_utf8_unchecked(&locales[0][..loc_lens[0]]) };
        let ref_path = build_sub_path(dir_str, ref_loc);

        let mut ref_apps: [[u8; 32]; 64] = [[0; 32]; 64];
        let mut ref_app_lens: [usize; 64] = [0; 64];
        let mut ref_count = 0usize;

        let mut ob_buf3 = [0u8; 512];
        let ob_ref = path_to_ob(unsafe { core::str::from_utf8_unchecked(&ref_path) }, &mut ob_buf3);
        if let Ok(fd_ref) = syscall::sys_ob_open(ob_ref, libneodos::syscall::ob_access::READ) {
            let mut e: [libneodos::syscall::ObEnumEntry; 64] = core::array::from_fn(|_|
                libneodos::syscall::ObEnumEntry { id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0 }
            );
            if let Ok(m) = syscall::sys_ob_enum(fd_ref, &mut e) {
                for j in 0..m {
                    let n = e[j].name_str();
                    if n.len() < 5 || !n.ends_with(".nlt") { continue; }
                    if n == "." || n == ".." { continue; }
                    let app_part = &n[..n.len() - 4];
                    let b = app_part.as_bytes();
                    let blen = b.len().min(31);
                    if ref_count < 64 {
                        ref_apps[ref_count][..blen].copy_from_slice(b);
                        ref_app_lens[ref_count] = blen;
                        ref_count += 1;
                    }
                }
            }
            let _ = syscall::sys_close(fd_ref);
        }

        for li in 1..loc_count {
            let loc = unsafe { core::str::from_utf8_unchecked(&locales[li][..loc_lens[li]]) };
            let loc_path = build_sub_path(dir_str, loc);
            let mut ob_buf4 = [0u8; 512];
            let ob_loc = path_to_ob(unsafe { core::str::from_utf8_unchecked(&loc_path) }, &mut ob_buf4);
            if let Ok(fd_loc) = syscall::sys_ob_open(ob_loc, libneodos::syscall::ob_access::READ) {
                let mut e2: [libneodos::syscall::ObEnumEntry; 64] = core::array::from_fn(|_|
                    libneodos::syscall::ObEnumEntry { id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0 }
                );
                let mut loc_apps: [bool; 64] = [false; 64];
                if let Ok(m) = syscall::sys_ob_enum(fd_loc, &mut e2) {
                    for j in 0..m {
                        let n = e2[j].name_str();
                        if n.len() < 5 || !n.ends_with(".nlt") { continue; }
                        let app_part = &n[..n.len() - 4];
                        for ai in 0..ref_count {
                            let ra = unsafe { core::str::from_utf8_unchecked(&ref_apps[ai][..ref_app_lens[ai]]) };
                            if app_part == ra { loc_apps[ai] = true; break; }
                        }
                    }
                }
                let _ = syscall::sys_close(fd_loc);

                for ai in 0..ref_count {
                    if !loc_apps[ai] {
                        let ra = unsafe { core::str::from_utf8_unchecked(&ref_apps[ai][..ref_app_lens[ai]]) };
                        write_stdout(b"  MISSING: "); write_stdout(ra.as_bytes());
                        write_stdout(b" in "); write_stdout(loc.as_bytes()); writeln(b"");
                    }
                }
            }
        }
    }
    writeln_str("CHECK COMPLETE");
}

fn build_sub_path<'a>(dir: &str, sub: &str) -> [u8; 260] {
    let mut buf = [0u8; 260];
    let db = dir.as_bytes();
    let sb = sub.as_bytes();
    let total = db.len() + 1 + sb.len();
    if total > 259 { return buf; }
    buf[..db.len()].copy_from_slice(db);
    buf[db.len()] = b'\\';
    buf[db.len() + 1..total].copy_from_slice(sb);
    buf
}

fn print_usage() {
    writeln_str("");
    writeln_str("NeoLocale v0.1 — NLT translation file tool");
    writeln_str("");
    writeln_str("Usage:");
    writeln_str("  neolocale validate <file.nlt>     Validate format and structure");
    writeln_str("  neolocale stats    <file.nlt>     Show entry statistics");
    writeln_str("  neolocale diff     <f1> <f2>      Key-by-key differences");
    writeln_str("  neolocale check    [dir]          Cross-locale missing check");
    writeln_str("  neolocale create   <app> [locale] Empty NLT scaffold (stdout)");
    writeln_str("");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw);

    if args.is_empty() || libneodos::args::is_help_flag(args) {
        print_usage();
        syscall::sys_exit(0);
    }

    let (cmd, rest) = split_first_arg(args);

    match cmd {
        b"validate" => cmd_validate(rest),
        b"stats"    => cmd_stats(rest),
        b"diff"     => cmd_diff(rest),
        b"check"    => cmd_check(rest),
        b"create"   => cmd_create(rest),
        _ => {
            write_stderr(b"Unknown command: ");
            write_stderr(cmd);
            write_stderr(b"\r\n");
            print_usage();
        }
    }

    syscall::sys_exit(0)
}
