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

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = args::read_args();
    let raw = trim_ascii(&raw);
    let mode_runas = raw
        .split(|b| *b == b' ' || *b == b'\t')
        .next()
        .map(|word| word.eq_ignore_ascii_case(b"runas"))
        .unwrap_or(false);

    write_str(b"\r\n");
    write_str(if mode_runas {
        b"NeoDOS runas - elevacion a Administrator\r\n"
    } else {
        b"NeoDOS Logon\r\n"
    });
    write_str(b"----------------------------------------\r\n");

    let default_user = if mode_runas { "Administrator" } else { "NeoDOS" };
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
        let user_len = prompt(b"Usuario: ", &mut user_buf, false, default_user);
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
        fail(
            b"neologon: autenticacion fallida\r\n",
            Some(session_fd),
        );
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

    let shell = match syscall::ob_spawn_in_session(
        core::str::from_utf8(SHELL_PATH).unwrap_or(""),
        session_fd,
    ) {
        Ok(fd) => fd,
        Err(_) => fail(b"neologon: no se pudo iniciar el shell\r\n", Some(session_fd)),
    };
    let _ = syscall::sys_ob_wait(shell);
    let _ = syscall::sys_close(shell);
    let _ = syscall::ob_session_logoff(session_fd);
    let _ = syscall::sys_close(session_fd);
    write_str(b"neologon: sesion finalizada\r\n");
    syscall::sys_exit(0)
}
