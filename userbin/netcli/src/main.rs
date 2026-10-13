#![no_std]
#![no_main]

//! `netcli` — a minimal TCP client utility (Ring 3, public socket ABI).
//!
//! Usage:
//!   netcli <ip> <port>     connect, send `PING!`, verify the echoed reply
//!   netcli selftest        loopback client/server round-trip (no NIC needed)
//!
//! Demonstrates: connect + accept readiness via `ob_wait`, finite wait
//! timeouts, a bidirectional exchange and clean closure.

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

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}
fn write_dec(mut v: u32) {
    let mut b = [0u8; 10];
    let mut i = b.len();
    if v == 0 { write_str(b"0"); return; }
    while v > 0 && i > 0 { i -= 1; b[i] = b'0' + (v % 10) as u8; v /= 10; }
    write_str(&b[i..]);
}
fn fail(what: &[u8]) -> ! {
    write_str(b"[netcli] FAIL: ");
    write_str(what);
    write_str(b"\r\n");
    syscall::sys_exit(1)
}

fn create_socket(prefix: &[u8]) -> u8 {
    let mut name = [0u8; 24];
    for n in 0..8u32 {
        name[..prefix.len()].copy_from_slice(prefix);
        let mut len = prefix.len();
        name[len] = b'-'; len += 1;
        name[len] = b'0' + (n % 10) as u8; len += 1;
        let path = core::str::from_utf8(&name[..len]).unwrap_or("\\NetCli-0");
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

/// Block (with a finite timeout) until `fd` reaches `want`.
fn wait_state(fd: u8, want: u32, what: &[u8]) -> bool {
    for _ in 0..200u32 {
        let st = tcp_state(fd);
        if st == want { return true; }
        if st == TCP_CLOSED { return false; }
        let _ = sys_ob_wait_timeout(fd, TIMEOUT_MS);
    }
    write_str(b"[netcli] timeout: "); write_str(what); write_str(b"\r\n");
    false
}

fn recv_exact(fd: u8, buf: &mut [u8], want: usize) -> bool {
    let mut got = 0usize;
    for _ in 0..200u32 {
        if got >= want { return true; }
        match ob_socket_recv(fd, &mut buf[got..]) {
            Ok(n) if n > 0 => got += n,
            _ => { let _ = sys_ob_wait_timeout(fd, TIMEOUT_MS); }
        }
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

fn run_client(ip: [u8; 4], port: u16) -> bool {
    write_str(b"[netcli] connecting\r\n");
    let c = create_socket(b"\\NetCli");
    ob_socket_bind(c, [0, 0, 0, 0], 0).unwrap_or_else(|_| fail(b"bind"));
    ob_socket_connect(c, ip, port).unwrap_or_else(|_| fail(b"connect"));
    if !wait_state(c, TCP_ESTABLISHED, b"connect") { let _ = ob_socket_close(c); return false; }
    if !send_all(c, b"PING!") { return false; }
    let mut buf = [0u8; 8];
    if !recv_exact(c, &mut buf, 5) { return false; }
    let ok = &buf[..5] == &b"PING!"[..];
    let _ = ob_socket_close(c);
    ok
}

/// Loopback client+server round-trip in one process (exercises accept).
fn run_selftest() -> bool {
    const PORT: u16 = 46101;
    write_str(b"[netcli] selftest\r\n");
    let srv = create_socket(b"\\NetCliSrv");
    ob_socket_bind(srv, [127, 0, 0, 1], PORT).unwrap_or_else(|_| fail(b"bind srv"));
    ob_socket_listen(srv).unwrap_or_else(|_| fail(b"listen srv"));
    let cli = create_socket(b"\\NetCliCli");
    ob_socket_bind(cli, [0, 0, 0, 0], 0).unwrap_or_else(|_| fail(b"bind cli"));
    ob_socket_connect(cli, [127, 0, 0, 1], PORT).unwrap_or_else(|_| fail(b"connect cli"));

    // Accept with a finite timeout.
    let mut conn = None;
    for _ in 0..200u32 {
        if let Ok(fd) = ob_socket_accept(srv) { conn = Some(fd); break; }
        let _ = sys_ob_wait_timeout(srv, TIMEOUT_MS);
    }
    let conn = match conn { Some(c) => c, None => { write_str(b"[netcli] accept timeout\r\n"); return false; } };
    if !wait_state(cli, TCP_ESTABLISHED, b"cli") || !wait_state(conn, TCP_ESTABLISHED, b"conn") {
        return false;
    }
    let ok = send_all(cli, b"PING!") && {
        let mut b = [0u8; 8];
        recv_exact(conn, &mut b, 5) && &b[..5] == &b"PING!"[..]
    };
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
fn parse_ip(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut idx = 0usize; let mut cur = 0u32; let mut seen = 0u32;
    for &c in s {
        if c == b'.' { if idx >= 4 || seen == 0 { return None; } out[idx] = cur as u8; idx += 1; cur = 0; seen = 0; }
        else if c.is_ascii_digit() { cur = cur * 10 + (c - b'0') as u32; seen += 1; if cur > 255 { return None; } }
        else { return None; }
    }
    if idx != 3 || seen == 0 { return None; }
    out[3] = cur as u8;
    Some(out)
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
        match (tok.get(0).and_then(|s| parse_ip(*s)), tok.get(1).and_then(|s| parse_u16(*s))) {
            (Some(ip), Some(p)) => run_client(ip, p),
            // No readable args (spawned without a command line): self-test.
            _ => run_selftest(),
        }
    };
    if ok { write_str(b"[netcli] PASS\r\n"); syscall::sys_exit(0) }
    else { write_str(b"[netcli] FAIL\r\n"); syscall::sys_exit(1) }
}
