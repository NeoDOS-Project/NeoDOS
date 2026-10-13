#![no_std]
#![no_main]

//! `netsrv` — a minimal TCP echo server utility (Ring 3, public socket ABI).
//!
//! Usage:
//!   netsrv <port> [max_conns]   bind+listen, accept and echo each connection
//!   netsrv selftest             loopback accept/echo round-trip
//!
//! The listener stays listening across connections (accept model); each
//! connection is echoed until it goes idle past a finite timeout, then closed.

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::syscall;
use libneodos::syscall::{
    ob_socket_accept, ob_socket_bind, ob_socket_close, ob_socket_connect, ob_socket_create,
    ob_socket_listen, ob_socket_recv, ob_socket_send, sys_ob_query_info, sys_ob_wait_timeout,
    ObInfoClass,
};

struct SbrkAlloc;
unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = libneodos::mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

const SOCK_TCP: u32 = 1;
const TCP_ESTABLISHED: u32 = 4;
const TCP_CLOSED: u32 = 0;
const TIMEOUT_MS: u64 = 3000;

fn write_str(s: &[u8]) { let _ = syscall::sys_write(1, s); }
fn write_dec(mut v: u32) {
    let mut b = [0u8; 10]; let mut i = b.len();
    if v == 0 { write_str(b"0"); return; }
    while v > 0 && i > 0 { i -= 1; b[i] = b'0' + (v % 10) as u8; v /= 10; }
    write_str(&b[i..]);
}
fn fail(what: &[u8]) -> ! {
    write_str(b"[netsrv] FAIL: "); write_str(what); write_str(b"\r\n");
    syscall::sys_exit(1)
}

fn create_socket(prefix: &[u8]) -> u8 {
    let mut name = [0u8; 24];
    for n in 0..8u32 {
        name[..prefix.len()].copy_from_slice(prefix);
        let mut len = prefix.len();
        name[len] = b'-'; len += 1;
        name[len] = b'0' + (n % 10) as u8; len += 1;
        let path = core::str::from_utf8(&name[..len]).unwrap_or("\\NetSrv-0");
        if let Ok(fd) = ob_socket_create(path, SOCK_TCP, 0) { return fd; }
    }
    fail(b"create")
}

fn tcp_state(fd: u8) -> u32 {
    let mut b = [0u8; 4];
    match sys_ob_query_info(fd, ObInfoClass::TcpStatus, &mut b) {
        Ok(4) => u32::from_ne_bytes(b),
        _ => u32::MAX,
    }
}
fn wait_state(fd: u8, want: u32) -> bool {
    for _ in 0..200u32 {
        let st = tcp_state(fd);
        if st == want { return true; }
        if st == TCP_CLOSED { return false; }
        let _ = sys_ob_wait_timeout(fd, TIMEOUT_MS);
    }
    false
}
fn send_all(fd: u8, data: &[u8]) -> bool {
    let mut off = 0usize;
    for _ in 0..200u32 {
        if off >= data.len() { return true; }
        match ob_socket_send(fd, &data[off..]) {
            Ok(n) if n > 0 => off += n,
            _ => { let _ = sys_ob_wait_timeout(fd, 50); }
        }
    }
    false
}

/// Echo one connection until it is idle for `TIMEOUT_MS` (or the peer closes).
fn echo_connection(fd: u8) {
    let mut buf = [0u8; 512];
    loop {
        let mut got: Option<usize> = None;
        for _ in 0..20u32 {
            if let Ok(n) = ob_socket_recv(fd, &mut buf) {
                if n > 0 { got = Some(n); break; }
            }
            let _ = sys_ob_wait_timeout(fd, TIMEOUT_MS);
        }
        match got {
            Some(n) => { let _ = send_all(fd, &buf[..n]); }
            None => break,
        }
    }
}

fn run_server(port: u16, max_conns: u32) -> bool {
    write_str(b"[netsrv] listening :"); write_dec(port as u32); write_str(b"\r\n");
    let srv = create_socket(b"\\NetSrv");
    ob_socket_bind(srv, [0, 0, 0, 0], port).unwrap_or_else(|_| fail(b"bind"));
    ob_socket_listen(srv).unwrap_or_else(|_| fail(b"listen"));
    let mut served = 0u32;
    loop {
        if max_conns != 0 && served >= max_conns { break; }
        let conn = {
            let mut c = None;
            for _ in 0..2000u32 {
                if let Ok(fd) = ob_socket_accept(srv) { c = Some(fd); break; }
                let _ = sys_ob_wait_timeout(srv, TIMEOUT_MS);
            }
            match c { Some(fd) => fd, None => break }
        };
        if wait_state(conn, TCP_ESTABLISHED) {
            write_str(b"[netsrv] connection "); write_dec(served); write_str(b" established\r\n");
            echo_connection(conn);
        }
        let _ = ob_socket_close(conn);
        served += 1;
    }
    let _ = ob_socket_close(srv);
    true
}

fn run_selftest() -> bool {
    const PORT: u16 = 46102;
    write_str(b"[netsrv] selftest\r\n");
    let srv = create_socket(b"\\NetSrvSelf");
    ob_socket_bind(srv, [127, 0, 0, 1], PORT).unwrap_or_else(|_| fail(b"bind srv"));
    ob_socket_listen(srv).unwrap_or_else(|_| fail(b"listen srv"));
    let cli = create_socket(b"\\NetSrvCli");
    ob_socket_bind(cli, [0, 0, 0, 0], 0).unwrap_or_else(|_| fail(b"bind cli"));
    ob_socket_connect(cli, [127, 0, 0, 1], PORT).unwrap_or_else(|_| fail(b"connect cli"));
    let mut conn = None;
    for _ in 0..200u32 {
        if let Ok(fd) = ob_socket_accept(srv) { conn = Some(fd); break; }
        let _ = sys_ob_wait_timeout(srv, TIMEOUT_MS);
    }
    let conn = match conn { Some(c) => c, None => { write_str(b"[netsrv] accept timeout\r\n"); return false; } };
    if !wait_state(conn, TCP_ESTABLISHED) { return false; }
    let ok = if send_all(cli, b"PING!") {
        let mut b = [0u8; 8];
        let mut got = 0usize;
        for _ in 0..200u32 {
            if got >= 5 { break; }
            if let Ok(n) = ob_socket_recv(conn, &mut b[got..]) { if n > 0 { got += n; continue; } }
            let _ = sys_ob_wait_timeout(conn, TIMEOUT_MS);
        }
        got == 5 && &b[..5] == &b"PING!"[..]
    } else { false };
    let _ = ob_socket_close(cli);
    let _ = ob_socket_close(conn);
    let _ = ob_socket_close(srv);
    ok
}

fn parse_u16(s: &[u8]) -> Option<u16> {
    let mut v: u32 = 0;
    if s.is_empty() { return None; }
    for &c in s { if !c.is_ascii_digit() { return None; } v = v * 10 + (c - b'0') as u32; if v > 65535 { return None; } }
    Some(v as u16)
}
fn parse_u32(s: &[u8]) -> Option<u32> {
    let mut v: u32 = 0;
    if s.is_empty() { return None; }
    for &c in s { if !c.is_ascii_digit() { return None; } v = v.saturating_mul(10).saturating_add((c - b'0') as u32); }
    Some(v)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let args = libneodos::args::read_args();
    let mut tok: [&[u8]; 4] = [&[]; 4];
    let mut n = 0usize;
    for w in args.split(|&b| b == b' ' || b == b'\t' || b == b'\0' || b == b'\r' || b == b'\n') {
        if w.is_empty() { continue; }
        if n < tok.len() { tok[n] = w; }
        n += 1;
    }
    let ok = if n == 0 || tok[0] == &b"selftest"[..] {
        run_selftest()
    } else {
        match tok.get(0).and_then(|s| parse_u16(*s)) {
            Some(p) => {
                let max = tok.get(1).and_then(|s| parse_u32(*s)).unwrap_or(0);
                run_server(p, max)
            }
            // No readable args (spawned without a command line): self-test.
            None => run_selftest(),
        }
    };
    if ok { write_str(b"[netsrv] PASS\r\n"); syscall::sys_exit(0) }
    else { write_str(b"[netsrv] FAIL\r\n"); syscall::sys_exit(1) }
}
