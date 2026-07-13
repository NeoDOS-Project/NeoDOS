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

// ── NLTv2 parsing (only format supported) ─────────────────────────────
//
// NLTv2 Header (32 bytes):
//   [0..4)   Magic: "NLT2"
//   [4..6)   Version: u16 = 2
//   [6..8)   HeaderSize: u16 = 32
//   [8..12)  LanguageID: u32 LE
//   [12..16) ApplicationID: u32 LE
//   [16..20) StringCount: u32 LE
//   [20..24) Flags: u32 LE
//   [24..28) Checksum: u32 LE
//   [28..32) Reserved: u32
//   [32..)   IndexTable: { id: u32, offset: u32 }[N]
//   [..)     StringData: UTF-8 null-terminated

const NLT2_MAGIC: [u8; 4] = [b'N', b'L', b'T', b'2'];
const NLT2_HEADER_SIZE: usize = 32;

fn crc32_ne(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

#[derive(Clone, Copy)]
struct Nltv2Entry {
    id: u32,
    offset: u32,
    klen: u16,
    vlen: u16,
}

struct Nltv2File {
    data: [u8; 65536],
    len: usize,
    valid: bool,
    count: usize,
    lang_id: u32,
    app_id: u32,
    entries: [Nltv2Entry; 2048],
    errors: [u8; 2048],
    err_len: usize,
}

impl Nltv2File {
    fn new() -> Self {
        Nltv2File {
            data: [0; 65536], len: 0, valid: false, count: 0,
            lang_id: 0, app_id: 0,
            entries: [Nltv2Entry { id: 0, offset: 0, klen: 0, vlen: 0 }; 2048],
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
        let mut nlt = Nltv2File::new();
        nlt.len = file.read(&mut nlt.data).ok()?;
        nlt.parse();
        Some(nlt)
    }

    fn parse(&mut self) {
        let len = self.len;
        if len < NLT2_HEADER_SIZE {
            self.add_err(b"ERROR: file too small (<32 bytes for NLTv2 header)");
            return;
        }
        if self.data[..4] != NLT2_MAGIC {
            self.add_err(b"ERROR: invalid magic (expected NLT2)");
            return;
        }
        let ver = u16::from_le_bytes([self.data[4], self.data[5]]);
        if ver != 2 {
            self.add_err(b"ERROR: unsupported version (expected 2)");
            return;
        }
        self.lang_id = u32::from_le_bytes([self.data[8], self.data[9], self.data[10], self.data[11]]);
        self.app_id = u32::from_le_bytes([self.data[12], self.data[13], self.data[14], self.data[15]]);
        let count = u32::from_le_bytes([self.data[16], self.data[17], self.data[18], self.data[19]]);
        let flags = u32::from_le_bytes([self.data[20], self.data[21], self.data[22], self.data[23]]);
        let stored_crc = u32::from_le_bytes([self.data[24], self.data[25], self.data[26], self.data[27]]);

        if count > 2048 {
            self.add_err(b"ERROR: too many entries (max 2048)");
            return;
        }
        if count == 0 {
            self.add_err(b"WARNING: empty translation table");
        }

        // Verify CRC32
        let mut crc_data = [0u8; 65536];
        let crc_len = len.min(65536);
        crc_data[..crc_len].copy_from_slice(&self.data[..crc_len]);
        // Zero out checksum field for CRC calculation
        crc_data[24..28].copy_from_slice(&[0, 0, 0, 0]);
        let actual_crc = crc32_ne(&crc_data[..crc_len]);
        if stored_crc != 0 && stored_crc != actual_crc {
            self.add_err(b"ERROR: CRC32 mismatch (file may be corrupt)");
            // Continue parsing for additional errors
        }

        // Validate size
        let min_sz = NLT2_HEADER_SIZE + count as usize * 8;
        if len < min_sz {
            self.add_err(b"ERROR: file too small for index table");
            return;
        }

        // Parse index table and collect strings
        let mut local_ok = true;
        #[derive(Clone, Copy)]
        struct RawEntry { id: u32, offset: u32, klen: u16, vlen: u16 }
        let mut raw = [RawEntry { id: 0, offset: 0, klen: 0, vlen: 0 }; 2048];

        let index_start = NLT2_HEADER_SIZE;
        for i in 0..count as usize {
            let off = index_start + i * 8;
            let sid = u32::from_le_bytes([self.data[off], self.data[off+1], self.data[off+2], self.data[off+3]]);
            let str_off = u32::from_le_bytes([self.data[off+4], self.data[off+5], self.data[off+6], self.data[off+7]]);
            raw[i].id = sid;
            raw[i].offset = str_off;

            if str_off as usize >= len {
                self.add_err(b"ERROR: string offset out of range");
                local_ok = false;
                continue;
            }
            let end = self.data[str_off as usize..].iter().position(|&b| b == 0).unwrap_or(len - str_off as usize);
            raw[i].klen = 0; // NLTv2 has no key string, just ID
            raw[i].vlen = end as u16;
        }

        // Check for duplicate IDs
        for i in 0..count as usize {
            if raw[i].id == 0 && raw[i].offset == 0 { continue; }
            for j in i + 1..count as usize {
                if raw[j].id == 0 && raw[j].offset == 0 { continue; }
                if raw[i].id == raw[j].id {
                    self.add_err(b"ERROR: duplicate string ID");
                    local_ok = false;
                }
            }
        }

        // Check ID ordering (should be sorted for binary search)
        let mut prev_id = 0u32;
        for i in 0..count as usize {
            if raw[i].id < prev_id {
                self.add_err(b"WARNING: IDs not sorted (will degrade binary search)");
                break;
            }
            prev_id = raw[i].id;
        }

        // Check flags for unknown bits
        const KNOWN_FLAGS: u32 = 0;
        if flags & !KNOWN_FLAGS != 0 {
            self.add_err(b"WARNING: unknown flags set");
        }

        self.count = count as usize;
        for i in 0..count as usize {
            self.entries[i] = Nltv2Entry {
                id: raw[i].id,
                offset: raw[i].offset,
                klen: raw[i].klen,
                vlen: raw[i].vlen,
            };
        }
        self.valid = local_ok;
    }

    fn str_at(&self, i: usize) -> &[u8] {
        let off = self.entries[i].offset as usize;
        let max_len = self.len - off;
        let end = self.data[off..].iter().position(|&b| b == 0).unwrap_or(max_len);
        &self.data[off..off + end]
    }

    fn id_at(&self, i: usize) -> u32 {
        self.entries[i].id
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn lang_id_to_name(id: u32) -> &'static str {
    match id {
        1 => "en-US", 2 => "es-ES", 3 => "fr-FR", 4 => "de-DE",
        5 => "it-IT", 6 => "pt-PT", 7 => "pt-BR", 8 => "ca-ES",
        9 => "eu-ES", 10 => "gl-ES", 11 => "en-GB", 12 => "ja-JP",
        13 => "zh-CN", 14 => "ru-RU", 15 => "ar-SA", 16 => "nl-NL",
        17 => "pl-PL", 18 => "sv-SE", 19 => "da-DK", 20 => "fi-FI",
        21 => "nb-NO", 22 => "ko-KR", 23 => "tr-TR", 24 => "cs-CZ",
        25 => "hu-HU", _ => "unknown",
    }
}

// ── Commands ──────────────────────────────────────────────────────────

fn cmd_validate(path: &[u8]) {
    let p = core::str::from_utf8(path).unwrap_or("");
    let nlt = match Nltv2File::load(p) {
        Some(n) => n,
        None => { writeln_str("ERROR: cannot open file"); return; }
    };
    if nlt.err_len > 0 {
        nlt.print_errors();
    }
    if nlt.valid {
        write_stdout(b"VALID  entries="); write_u64(nlt.count as u64);
        write_stdout(b" lang="); write_str(lang_id_to_name(nlt.lang_id));
        write_stdout(b" size="); write_u64(nlt.len as u64); writeln(b" bytes");
    } else {
        writeln_str("INVALID");
    }
}

fn cmd_stats(path: &[u8]) {
    let p = core::str::from_utf8(path).unwrap_or("");
    let nlt = match Nltv2File::load(p) {
        Some(n) => n,
        None => { writeln_str("ERROR: cannot open file"); return; }
    };
    if nlt.err_len > 0 { nlt.print_errors(); }
    if !nlt.valid { return; }

    let mut total_v = 0usize;
    for i in 0..nlt.count {
        total_v += nlt.entries[i].vlen as usize;
    }
    writeln_str("=== NLTv2 Statistics ===");
    write_stdout(b"  File:        "); write_stdout(path); writeln(b"");
    write_stdout(b"  Format:      NLTv2"); writeln(b"");
    write_stdout(b"  Size:        "); write_u64(nlt.len as u64); writeln(b" bytes");
    write_stdout(b"  Language:    "); write_str(lang_id_to_name(nlt.lang_id)); writeln(b"");
    write_stdout(b"  App ID:      "); write_u64(nlt.app_id as u64); writeln(b"");
    write_stdout(b"  Entries:     "); write_u64(nlt.count as u64); writeln(b"");
    write_stdout(b"  Value bytes: "); write_u64(total_v as u64); writeln(b"");
    if nlt.count > 0 {
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
    let a = match Nltv2File::load(p1) { Some(n) => n, None => { writeln_str("ERROR: cannot open first file"); return; } };
    let b = match Nltv2File::load(p2) { Some(n) => n, None => { writeln_str("ERROR: cannot open second file"); return; } };

    let mut changed = 0u64;
    let mut only_a = 0u64;
    let mut only_b = 0u64;

    for i in 0..a.count {
        let id_a = a.id_at(i);
        let va = a.str_at(i);
        let mut found = false;
        for j in 0..b.count {
            if id_a == b.id_at(j) {
                found = true;
                if va != b.str_at(j) {
                    write_stdout(b"  ~ ID "); write_u64(id_a as u64); writeln(b"");
                    write_stdout(b"    - "); write_stdout(va); writeln(b"");
                    write_stdout(b"    + "); write_stdout(b.str_at(j)); writeln(b"");
                    changed += 1;
                }
                break;
            }
        }
        if !found {
            write_stdout(b"  - ID "); write_u64(id_a as u64);
            write_stdout(b"  (only in first)"); writeln(b"");
            only_a += 1;
        }
    }
    for j in 0..b.count {
        let id_b = b.id_at(j);
        let mut found = false;
        for i in 0..a.count {
            if id_b == a.id_at(i) { found = true; break; }
        }
        if !found {
            write_stdout(b"  + ID "); write_u64(id_b as u64);
            write_stdout(b"  (only in second)"); writeln(b"");
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

    if app.is_empty() {
        writeln_str("Usage: neolocale create <app> [locale]");
        writeln_str("  Creates an empty NLTv2 scaffold (stdout).");
        return;
    }

    // Write a minimal NLTv2 header with 0 entries
    let mut hdr = [0u8; 32];
    hdr[..4].copy_from_slice(b"NLT2");
    hdr[4..6].copy_from_slice(&2u16.to_le_bytes());     // version = 2
    hdr[6..8].copy_from_slice(&32u16.to_le_bytes());    // header_size = 32
    hdr[8..12].copy_from_slice(&1u32.to_le_bytes());    // lang = en-US
    // app_id at 12..16 is 0
    // count at 16..20 is 0
    // flags at 20..24 is 0
    // checksum at 24..28 is 0
    write_stdout(&hdr);

    write_stderr(b"Created empty NLTv2 scaffold. Use nltc to add entries.\r\n");
}

fn cmd_check(args: &[u8]) {
    let locale_dir = if args.is_empty() { b"C:\\System\\Locale" } else { args };
    let dir_str = core::str::from_utf8(locale_dir).unwrap_or("C:\\System\\Locale");
    writeln_str("Checking locale directory: ");
    writeln_str(dir_str);

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

fn build_sub_path(dir: &str, sub: &str) -> [u8; 260] {
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
    writeln_str("NeoLocale v0.2 — NLTv2 translation file tool");
    writeln_str("");
    writeln_str("Usage:");
    writeln_str("  neolocale validate <file.nlt>     Validate NLTv2 format");
    writeln_str("  neolocale stats    <file.nlt>     Show entry statistics");
    writeln_str("  neolocale diff     <f1> <f2>      ID-by-ID differences");
    writeln_str("  neolocale check    [dir]          Cross-locale missing check");
    writeln_str("  neolocale create   <app>          Empty NLTv2 scaffold");
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
