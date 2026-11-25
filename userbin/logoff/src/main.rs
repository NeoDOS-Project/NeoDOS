//! `logoff` — close the caller's login session (#65).
//!
//! The shell ends after a successful logoff: its token is invalidated by the
//! kernel, so continuing to run it would only produce access denials.

#![no_std]
#![no_main]

use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_dec(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&buf[i..]);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let info = match syscall::ob_query_token_info() {
        Ok(info) => info,
        Err(_) => {
            write_str(b"\r\nlogoff: no se pudo consultar el token\r\n");
            syscall::sys_exit(1);
        }
    };
    if info.session_id == 0 {
        write_str(b"\r\nlogoff: la sesion actual no esta registrada\r\n");
        syscall::sys_exit(1);
    }
    let mut path = [0u8; 32];
    let mut p = 0usize;
    for &b in b"\\Session\\" {
        path[p] = b;
        p += 1;
    }
    let mut v = info.session_id as u64;
    let mut digits = [0u8; 10];
    let mut d = 0usize;
    if v == 0 {
        digits[0] = b'0';
        d = 1;
    }
    while v > 0 {
        digits[d] = b'0' + (v % 10) as u8;
        v /= 10;
        d += 1;
    }
    while d > 0 {
        d -= 1;
        path[p] = digits[d];
        p += 1;
    }
    let path = core::str::from_utf8(&path[..p]).unwrap_or("");
    let fd = match syscall::sys_ob_open(path, syscall::ob_access::READ) {
        Ok(fd) => fd,
        Err(_) => {
            write_str(b"\r\nlogoff: sesion no encontrada\r\n");
            syscall::sys_exit(1);
        }
    };
    match syscall::ob_session_logoff(fd) {
        Ok(()) => {
            write_str(b"\r\nCerrando sesion ");
            write_dec(info.session_id as u64);
            write_str(b"...\r\n");
        }
        Err(_) => {
            let _ = syscall::sys_close(fd);
            write_str(b"\r\nlogoff: no se pudo cerrar la sesion\r\n");
            syscall::sys_exit(1);
        }
    }
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0)
}
