//! `neologon` — interactive logon (#40) and `runas`-style elevation (#67).
//!
//! Usage:
//! - `neologon` — authenticate as any SAM account and open a shell in the new
//!   session.
//! - `neologon runas` — authenticate as an administrator and open an elevated
//!   shell. The shell aliases `LOGON` and `RUNAS` invoke both modes.
//!
//! Credentials are authenticated by the kernel against SAM
//! (`ObSetInfoClass::SessionLogon`); the issued token is bound to the session
//! and handed to the child shell by `ObCreate(Process)` attrs bit 33.

#![no_std]
#![no_main]

use libneodos::args::{self, trim_ascii};
use libneodos::syscall;

const SHELL_PATH: &[u8] = b"\\Global\\FileSystem\\C:\\NeoDOS\\System\\neoshell.nxe";
const MAX_ATTEMPTS: u32 = 3;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

/// Read one line from stdin, echoing characters (or `*` when `mask` is set).
fn read_line(buf: &mut [u8], mask: bool) -> usize {
    let mut len = 0usize;
    loop {
        let mut byte = [0u8; 1];
        match syscall::sys_read(0, &mut byte) {
            Ok(1) => {}
            _ => continue,
        }
        match byte[0] {
            b'\r' | b'\n' => {
                write_str(b"\r\n");
                return len;
            }
            0x08 | 0x7F => {
                if len > 0 {
                    len -= 1;
                    write_str(b"\x08 \x08");
                }
            }
            c if c >= 0x20 => {
                if len + 1 < buf.len() {
                    buf[len] = c;
                    len += 1;
                    if mask {
                        write_str(b"*");
                    } else {
                        let echo = [c];
                        write_str(&echo);
                    }
                }
            }
            _ => {}
        }
    }
}

fn prompt(label: &[u8], buf: &mut [u8], mask: bool, default: &str) -> usize {
    write_str(label);
    let len = read_line(buf, mask);
    if len == 0 && !default.is_empty() {
        let bytes = default.as_bytes();
        let n = bytes.len().min(buf.len() - 1);
        buf[..n].copy_from_slice(&bytes[..n]);
        return n;
    }
    len
}

fn as_str(buf: &[u8], len: usize) -> &str {
    core::str::from_utf8(&buf[..len]).unwrap_or("")
}

fn fail(message: &[u8], session: Option<u8>) -> ! {
    write_str(message);
    if let Some(fd) = session {
        let _ = syscall::ob_session_logoff(fd);
        let _ = syscall::sys_close(fd);
    }
    syscall::sys_exit(1)
}

fn tokenize<'a>(raw: &'a [u8], out: &mut [&'a [u8]; 16]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < raw.len() && n < out.len() {
        while i < raw.len() && (raw[i] == b' ' || raw[i] == b'\t') {
            i += 1;
        }
        if i >= raw.len() {
            break;
        }
        let start = i;
        while i < raw.len() && raw[i] != b' ' && raw[i] != b'\t' {
            i += 1;
        }
        out[n] = &raw[start..i];
        n += 1;
    }
    n
}

fn resolve_command(word: &[u8]) -> Option<[u8; 260]> {
    const DIRS: [&[u8]; 2] = [b"C:\\NeoDOS\\System\\", b"C:\\NeoDOS\\System\\Tools\\"];
    let mut buf = [0u8; 260];
    let has_ext = word.len() >= 4 && word[word.len() - 4..].eq_ignore_ascii_case(b".nxe");
    let is_path = word.contains(&b':') || word.contains(&b'\\');
    if is_path || has_ext {
        let mut p = 0;
        for &b in word {
            if p < 255 {
                buf[p] = b;
                p += 1;
            }
        }
        if !has_ext && p + 4 < 260 {
            buf[p..p + 4].copy_from_slice(b".nxe");
            p += 4;
        }
        buf[p] = 0;
        return Some(buf);
    }
    for dir in DIRS {
        let mut p = 0;
        for &b in dir {
            if p < 255 {
                buf[p] = b;
                p += 1;
            }
        }
        for &b in word {
            if p < 255 {
                buf[p] = if b.is_ascii_uppercase() { b + 32 } else { b };
                p += 1;
            }
        }
        if p + 4 < 260 {
            buf[p..p + 4].copy_from_slice(b".nxe");
            p += 4;
        }
        buf[p] = 0;
        let path = core::str::from_utf8(&buf[..p]).unwrap_or("");
        let mut ob = [0u8; 300];
        let ob_path = libneoutil_path(path, &mut ob);
        if syscall::sys_ob_open(ob_path, syscall::ob_access::READ).is_ok() {
            return Some(buf);
        }
    }
    None
}

fn libneoutil_path<'a>(vfs: &str, buf: &'a mut [u8; 300]) -> &'a str {
    const PREFIX: &[u8] = b"\\Global\\FileSystem\\";
    buf[..PREFIX.len()].copy_from_slice(PREFIX);
    let vb = vfs.as_bytes();
    let n = vb.len().min(buf.len() - PREFIX.len() - 1);
    buf[PREFIX.len()..PREFIX.len() + n].copy_from_slice(&vb[..n]);
    let total = PREFIX.len() + n;
    buf[total] = 0;
    core::str::from_utf8(&buf[..total]).unwrap_or("")
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = args::read_args();
    let raw = trim_ascii(&raw);
    let mut toks: [&[u8]; 16] = [b""; 16];
    let ntok = tokenize(raw, &mut toks);
    let mut ti = 0;
    let mode_runas = ntok > 0 && toks[0].eq_ignore_ascii_case(b"runas");
    if mode_runas {
        ti = 1;
    }
    let mut target = if mode_runas { "Administrator" } else { "NeoDOS" };
    let mut target_buf = [0u8; 32];
    if ti < ntok {
        let t = toks[ti];
        if t.len() > 6 && t[..6].eq_ignore_ascii_case(b"/user:") {
            let name = &t[6..];
            let n = name.len().min(31);
            target_buf[..n].copy_from_slice(&name[..n]);
            target = core::str::from_utf8(&target_buf[..n]).unwrap_or(target);
            ti += 1;
        }
    }
    let command = if ti < ntok { Some(toks[ti]) } else { None };

    write_str(b"\r\n");
    write_str(if mode_runas {
        b"NeoDOS runas - ejecutar como otro usuario\r\n"
    } else {
        b"NeoDOS Logon\r\n"
    });
    write_str(b"----------------------------------------\r\n");

    let session_fd = match syscall::ob_create_session() {
        Ok(fd) => fd,
        Err(_) => fail(b"neologon: no se pudo crear la sesion\r\n", None),
    };

    let mut authenticated = false;
    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            write_str(b"Credenciales invalidas, intentelo de nuevo.\r\n");
        }
        let mut user_buf = [0u8; 32];
        let mut pass_buf = [0u8; 64];
        let user_len = prompt(b"Usuario: ", &mut user_buf, false, target);
        let pass_len = prompt(b"Password: ", &mut pass_buf, true, "");
        let user = as_str(&user_buf, user_len);
        let pass = as_str(&pass_buf, pass_len);
        if user.is_empty() {
            continue;
        }
        if syscall::ob_session_logon(session_fd, user, pass).is_ok() {
            authenticated = true;
            break;
        }
    }
    if !authenticated {
        fail(b"neologon: autenticacion fallida\r\n", Some(session_fd));
    }

    if mode_runas {
        match syscall::ob_query_session_info(session_fd) {
            Ok(info) if info.is_admin != 0 => {}
            _ => fail(
                b"runas: la cuenta no es administrador\r\n",
                Some(session_fd),
            ),
        }
    }

    let child = if let Some(cmd) = command {
        let path = match resolve_command(cmd) {
            Some(p) => p,
            None => fail(b"runas: comando no encontrado\r\n", Some(session_fd)),
        };
        let mut argbuf = [0u8; 256];
        let mut alen = 0usize;
        let mut first = true;
        for word in &toks[ti + 1..ntok] {
            if !first && alen < 255 {
                argbuf[alen] = b' ';
                alen += 1;
            }
            first = false;
            for &b in *word {
                if alen < 255 {
                    argbuf[alen] = b;
                    alen += 1;
                }
            }
        }
        args::write_args(&argbuf[..alen]);
        let path = core::str::from_utf8(&path[..path.iter().position(|&b| b == 0).unwrap_or(0)])
            .unwrap_or("");
        let mut ob = [0u8; 300];
        let ob_path = libneoutil_path(path, &mut ob);
        let attrs = 0xFFu64
            | (0xFFu64 << 8)
            | (0xFFu64 << 16)
            | (1u64 << 33)
            | ((session_fd as u64) << 40);
        match syscall::sys_ob_create(ob_path, syscall::ob_type::PROCESS, None, attrs) {
            Ok(fd) => fd,
            Err(_) => fail(b"runas: no se pudo ejecutar el comando\r\n", Some(session_fd)),
        }
    } else {
        let shell = SHELL_PATH;
        let shell = core::str::from_utf8(shell).unwrap_or("");
        match syscall::ob_spawn_in_session(shell, session_fd) {
            Ok(fd) => fd,
            Err(_) => fail(
                b"neologon: no se pudo iniciar el shell\r\n",
                Some(session_fd),
            ),
        }
    };

    let _ = syscall::sys_ob_wait(child);
    let _ = syscall::sys_close(child);
    let _ = syscall::ob_session_logoff(session_fd);
    let _ = syscall::sys_close(session_fd);
    syscall::sys_exit(0)
}
