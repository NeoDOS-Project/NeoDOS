//! `certctl` — trust store administration.
//!
//! ```text
//! certctl list                      system (S) and user (U) anchors
//! certctl show <huella|índice>      details of one anchor
//! certctl import <fichero>          PEM (one or many) or DER; validates and dedups
//! certctl remove <huella>           user CAs only; system anchors are refused
//! certctl status                    system bundle version/date/count + user count
//! certctl repair                    drop keys left by an interrupted import
//! ```
//!
//! Only certificate *authorities* are accepted: importing a server certificate
//! is refused on purpose (`trusting one specific server certificate` is a
//! different feature). Nothing here can modify the image's bundle.

#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};

use libneodos::{args, mem, syscall};
use libtrust::store;
use libtrust::{Entry, Source};

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() {
            core::ptr::null_mut()
        } else {
            ptr
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

const USAGE: &[u8] = b"uso: certctl list | show <huella> | import <fichero> | remove <huella> | status | repair\r\n";

/// Largest import file we accept. A PEM bundle with several roots is far
/// smaller; the extra byte detects a file that does not fit instead of
/// trusting a truncated read.
const MAX_IMPORT_FILE: usize = 64 * 1024;

fn out(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}
fn err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}
fn fail(s: &str) -> ! {
    err(b"certctl: ");
    err(s.as_bytes());
    err(b"\r\n");
    syscall::sys_exit(1)
}
fn dec(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = 19;
    if v == 0 {
        out(b"0");
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
    out(&buf[i..=19]);
}

/// `YYYY-MM-DD` from Unix seconds (UTC).
fn date(unix: i64) {
    if unix <= 0 {
        out(b"?");
        return;
    }
    let days = unix.div_euclid(86_400);
    let (y, m, d) = civil(days);
    dec(y as u64);
    out(b"-");
    if m < 10 {
        out(b"0");
    }
    dec(m as u64);
    out(b"-");
    if d < 10 {
        out(b"0");
    }
    dec(d as u64);
}

/// Civil date from days since 1970 (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn print_entry(index: usize, e: &Entry) {
    dec(index as u64);
    out(if e.source == Source::System {
        b". [S] "
    } else {
        b". [U] "
    });
    let short = &e.fingerprint.as_bytes()[..16.min(e.fingerprint.len())];
    out(short);
    out(b"  ");
    out(e.subject.as_bytes());
    out(b"  ");
    date(e.not_before);
    out(b"..");
    date(e.not_after);
    out(b"\r\n");
}

fn read_file(path: &str) -> Result<alloc::vec::Vec<u8>, ()> {
    let mut ob = [0u8; 256];
    let mut len = 0usize;
    if !path.starts_with('\\') {
        let prefix = b"\\Global\\FileSystem\\";
        ob[..prefix.len()].copy_from_slice(prefix);
        len = prefix.len();
    }
    let bytes = path.as_bytes();
    if len + bytes.len() > ob.len() {
        return Err(());
    }
    ob[len..len + bytes.len()].copy_from_slice(bytes);
    len += bytes.len();
    let Ok(p) = core::str::from_utf8(&ob[..len]) else {
        return Err(());
    };
    let fd = syscall::sys_ob_open(p, syscall::ob_access::READ).map_err(|_| ())?;
    let mut buf = alloc::vec![0u8; MAX_IMPORT_FILE + 1];
    let n = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::ReadContent, &mut buf);
    let _ = syscall::sys_close(fd);
    let n = n.map_err(|_| ())? as usize;
    if n > MAX_IMPORT_FILE {
        return Err(());
    }
    buf.truncate(n);
    Ok(buf)
}

fn now_or_fail() -> i64 {
    match libclock::system::unix_seconds() {
        Ok(now) => now,
        Err(_) => fail("el reloj no es fiable; sincroniza la hora (ntpd) e inténtalo de nuevo"),
    }
}

fn cmd_list() {
    let system = store::system_entries();
    let user = store::user_entries();
    if system.is_empty() && user.is_empty() {
        out(b"certctl: no hay anclas de confianza\r\n");
        return;
    }
    out(b"[S] sistema  [U] usuario\r\n");
    let mut index = 1;
    for e in system.iter() {
        print_entry(index, e);
        index += 1;
    }
    for e in user.iter() {
        print_entry(index, e);
        index += 1;
    }
    out(b"total: ");
    dec(system.len() as u64);
    out(b" del sistema, ");
    dec(user.len() as u64);
    out(b" de usuario\r\n");
}

fn find(target: &str) -> Option<Entry> {
    let all: alloc::vec::Vec<Entry> = store::system_entries()
        .into_iter()
        .chain(store::user_entries())
        .collect();
    // By 1-based index, or by fingerprint prefix (>= 8 hex chars).
    if let Ok(n) = target.parse::<usize>() {
        return all.get(n.saturating_sub(1)).cloned();
    }
    let t = target.to_ascii_lowercase();
    if t.len() < 8 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    all.into_iter().find(|e| e.fingerprint.starts_with(&t))
}

fn cmd_show(target: &str) {
    let Some(e) = find(target) else {
        fail("no encuentro esa huella")
    };
    out(b"huella:    ");
    out(e.fingerprint.as_bytes());
    out(b"\r\ntipo:      ");
    out(if e.is_ca { b"CA" } else { b"certificado de servidor" });
    out(b"\r\norigen:    ");
    out(e.source.as_str().as_bytes());
    out(b"\r\nsujeto:    ");
    out(e.subject.as_bytes());
    out(b"\r\nemisor:    ");
    out(e.issuer.as_bytes());
    out(b"\r\nvalidez:   ");
    date(e.not_before);
    out(b" .. ");
    date(e.not_after);
    out(b"\r\nubicacion: ");
    out(e.location.as_bytes());
    out(b"\r\n");
}

fn cmd_import(path: &str) {
    let Ok(bytes) = read_file(path) else {
        fail("no puedo leer el fichero")
    };
    let now = now_or_fail();
    let results = store::import_all(&bytes, now);
    let mut installed = 0;
    let mut present = 0;
    for r in results {
        match r {
            Ok(store::Imported::Installed(fp)) => {
                out(b"instalada ");
                out(&fp.as_bytes()[..16]);
                out(b" (usuario)\r\n");
                installed += 1;
            }
            Ok(store::Imported::AlreadyUser(fp)) => {
                out(b"ya instalada ");
                out(&fp.as_bytes()[..16]);
                out(b" (usuario)\r\n");
                present += 1;
            }
            Ok(store::Imported::AlreadySystem(fp)) => {
                out(b"ya es una CA del sistema ");
                out(&fp.as_bytes()[..16]);
                out(b" (no se duplica)\r\n");
                present += 1;
            }
            Err(e) => {
                err(b"certctl: rechazado: ");
                err(e.as_str().as_bytes());
                err(b"\r\n");
            }
        }
    }
    if installed == 0 && present == 0 {
        syscall::sys_exit(1);
    }
    out(b"hecho: ");
    dec(installed as u64);
    out(b" instalada(s), ");
    dec(present as u64);
    out(b" ya presente(s)\r\n");
}

fn cmd_remove(target: &str) {
    // A corrupt entry may be omitted from `list`, so accept a full fingerprint
    // even when `find` cannot build an Entry for it.
    let fingerprint = match find(target) {
        Some(e) => {
            if e.source == Source::System {
                fail("es una CA del sistema: no se puede eliminar (solo las importadas)");
            }
            e.fingerprint
        }
        None => {
            let t = target.to_ascii_lowercase();
            if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
                t
            } else {
                fail("no encuentro esa huella")
            }
        }
    };
    match store::remove(&fingerprint) {
        Ok(()) => {
            out(b"eliminada ");
            out(&fingerprint.as_bytes()[..16]);
            out(b"\r\n");
        }
        Err(_) => fail("no se pudo eliminar"),
    }
}

fn cmd_status() {
    let (info, _) = store::system_bundle();
    out(b"bundle del sistema: ");
    out(if info.count == 0 {
        b"ausente"
    } else {
        b"presente"
    });
    out(b"\r\n  version: ");
    dec(info.version as u64);
    out(b"\r\n  construido: ");
    date(info.built_unix);
    out(b"\r\n  anclas: ");
    dec(info.count as u64);
    out(b"\r\nCAs de usuario: ");
    dec(store::user_entries().len() as u64);
    out(b"\r\n");

    // Diagnostic view: entries that were skipped because they are corrupt,
    // expired, not a CA, etc. `repair` only removes structural corruption.
    let now = libclock::system::trusted_now_unix()
        .or_else(|_| libclock::system::unix_seconds())
        .unwrap_or(0);
    let report = store::anchor_report(now);
    out(b"anclas utilizables: ");
    dec(report.roots.len() as u64);
    out(b"\r\n");
    if !report.rejected.is_empty() {
        out(b"ignoradas: ");
        dec(report.rejected.len() as u64);
        out(b"\r\n");
        for r in report.rejected.iter().take(16) {
            out(b"  ");
            out(&r.fingerprint.as_bytes()[..16.min(r.fingerprint.len())]);
            out(b"  ");
            out(r.issue.as_str().as_bytes());
            out(b"\r\n");
        }
    }
}

fn run(argv: &[&[u8]]) -> i32 {
    if argv.is_empty() {
        err(USAGE);
        return 2;
    }
    let cmd = argv[0];
    let arg = argv.get(1).copied().unwrap_or(b"");
    match cmd {
        b"list" => cmd_list(),
        b"status" => cmd_status(),
        b"repair" => {
            let n = store::repair();
            out(b"claves incompletas eliminadas: ");
            dec(n as u64);
            out(b"\r\n");
        }
        b"show" | b"remove" | b"import" => {
            let Ok(a) = core::str::from_utf8(arg) else {
                fail("argumento inválido")
            };
            if a.is_empty() {
                err(USAGE);
                return 2;
            }
            match cmd {
                b"show" => cmd_show(a),
                b"remove" => cmd_remove(a),
                _ => cmd_import(a),
            }
        }
        _ => {
            err(USAGE);
            return 2;
        }
    }
    0
}

/// Split the argument line on spaces (quotes are not supported, like the shell).
fn split_args(line: &[u8]) -> alloc::vec::Vec<&[u8]> {
    let mut out = alloc::vec::Vec::new();
    for token in line.split(|b| *b == b' ' || *b == b'\t') {
        if !token.is_empty() {
            out.push(token);
        }
    }
    out
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = args::read_args();
    let argv = split_args(args::trim_ascii(&raw));
    let code = run(&argv);
    syscall::sys_exit(code as u32)
}
