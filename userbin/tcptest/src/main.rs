#![no_std]
#![no_main]

//! `tcptest` — minimal Ring 3 TCP client/server validation.
//!
//! Modes (command line):
//!   tcptest                      loopback client/server self-test (auto-run by neoinit)
//!   tcptest server <port>        listen, echo one connection's data back
//!   tcptest client <ip> <port>   connect, send a payload, verify the echo
//!
//! The self-test exercises the public socket ABI end-to-end in Ring 3:
//! create/bind/listen/connect/send/recv/close, plus the blocking socket wait
//! (`sys_ob_wait`) so the application never busy-polls.

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::syscall;
use libneodos::syscall::{
    ob_socket_bind, ob_socket_close, ob_socket_connect, ob_socket_create, ob_socket_listen,
    ob_socket_recv, ob_socket_send, sys_ob_query_info, sys_ob_wait, ObInfoClass,
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
/// Kernel `TcpState::Established as u32`.
const TCP_ESTABLISHED: u32 = 4;
const TCP_CLOSED: u32 = 0;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_dec(mut v: u32) {
    let mut b = [0u8; 10];
    let mut i = b.len();
    if v == 0 { write_str(b"0"); return; }
    while v > 0 && i > 0 {
        i -= 1;
        b[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&b[i..]);
}

fn fail(what: &[u8]) -> ! {
    write_str(b"[tcptest] FAIL: ");
    write_str(what);
    write_str(b"\r\n");
    syscall::sys_exit(1)
}

/// Allocate a unique socket Ob object (`\TcpTest-<n>`).
fn create_socket(prefix: &[u8]) -> u8 {
    let mut name = [0u8; 24];
    for n in 0..8u32 {
        name[..prefix.len()].copy_from_slice(prefix);
        let mut len = prefix.len();
        name[len] = b'-';
        len += 1;
        name[len] = b'0' + (n % 10) as u8;
        len += 1;
        let path = core::str::from_utf8(&name[..len]).unwrap_or("\\TcpTest-0");
        if let Ok(fd) = ob_socket_create(path, SOCK_TCP, 0) {
            return fd;
        }
    }
    fail(b"ob_socket_create")
}

fn tcp_state(fd: u8) -> u32 {
    let mut b = [0u8; 4];
    match sys_ob_query_info(fd, ObInfoClass::TcpStatus, &mut b) {
        Ok(4) => u32::from_ne_bytes(b),
        _ => u32::MAX,
    }
}

/// Block (via `ob_wait`) until the connection is Established. Bounded so a
/// protocol bug shows up as a test failure instead of a hung test run.
fn wait_established(fd: u8, what: &[u8]) -> bool {
    for _ in 0..500_000u32 {
        match tcp_state(fd) {
            TCP_ESTABLISHED => return true,
            TCP_CLOSED => return false,
            _ => {
                let _ = sys_ob_wait(fd);
            }
        }
    }
    write_str(b"[tcptest] wait timeout: ");
    write_str(what);
    write_str(b"\r\n");
    false
}

/// Receive at least `want` bytes, blocking on the socket wait primitive.
fn recv_exact(fd: u8, buf: &mut [u8], want: usize) -> bool {
    let mut got = 0usize;
    for _ in 0..500_000u32 {
        if got >= want {
            return true;
        }
        match ob_socket_recv(fd, &mut buf[got..]) {
            Ok(n) if n > 0 => got += n,
            _ => {
                let _ = sys_ob_wait(fd);
            }
        }
    }
    false
}

fn send_all(fd: u8, data: &[u8]) -> bool {
    let mut off = 0usize;
    for _ in 0..500_000u32 {
        if off >= data.len() {
            return true;
        }
        match ob_socket_send(fd, &data[off..]) {
            Ok(n) if n > 0 => off += n,
            _ => syscall::sys_yield(),
        }
    }
    false
}

fn run_self_test() -> bool {
    write_str(b"[tcptest] loopback client/server self-test\r\n");
    const PORT: u16 = 46000;
    const C2S: &[u8] = b"hello-from-client";
    const S2C: &[u8] = b"hello-from-server";

    let server = create_socket(b"\\TcpSelfSrv");
    ob_socket_bind(server, [127, 0, 0, 1], PORT).unwrap_or_else(|_| fail(b"bind server"));
    ob_socket_listen(server).unwrap_or_else(|_| fail(b"listen server"));

    let client = create_socket(b"\\TcpSelfCli");
    ob_socket_bind(client, [0, 0, 0, 0], 0).unwrap_or_else(|_| fail(b"bind client"));
    ob_socket_connect(client, [127, 0, 0, 1], PORT).unwrap_or_else(|_| fail(b"connect client"));

    if !wait_established(client, b"client") || !wait_established(server, b"server") {
        let _ = ob_socket_close(client);
        let _ = ob_socket_close(server);
        return false;
    }

    // client -> server
    if !send_all(client, C2S) { return false; }
    let mut buf = [0u8; 64];
    if !recv_exact(server, &mut buf, C2S.len()) || &buf[..C2S.len()] != C2S {
        write_str(b"[tcptest] c2s payload mismatch\r\n");
        return false;
    }
    write_str(b"[tcptest] client->server ok\r\n");

    // server -> client
    if !send_all(server, S2C) { return false; }
    let mut buf2 = [0u8; 64];
    if !recv_exact(client, &mut buf2, S2C.len()) || &buf2[..S2C.len()] != S2C {
        write_str(b"[tcptest] s2c payload mismatch\r\n");
        return false;
    }
    write_str(b"[tcptest] server->client ok\r\n");

    let _ = ob_socket_close(client);
    let _ = ob_socket_close(server);
    true
}

fn run_server(port: u16) -> bool {
    write_str(b"[tcptest] server listen :");
    write_dec(port as u32);
    write_str(b"\r\n");
    let server = create_socket(b"\\TcpSrv");
    ob_socket_bind(server, [0, 0, 0, 0], port).unwrap_or_else(|_| fail(b"server bind"));
    ob_socket_listen(server).unwrap_or_else(|_| fail(b"server listen"));
    if !wait_established(server, b"server") { return false; }
    write_str(b"[tcptest] connection established, echoing\r\n");
    let mut buf = [0u8; 256];
    if !recv_exact(server, &mut buf, 5) { return false; }
    let _ = send_all(server, &buf[..5]);
    let _ = ob_socket_close(server);
    true
}

fn run_client(ip: [u8; 4], port: u16) -> bool {
    write_str(b"[tcptest] client connect\r\n");
    let client = create_socket(b"\\TcpCli");
    ob_socket_bind(client, [0, 0, 0, 0], 0).unwrap_or_else(|_| fail(b"client bind"));
    ob_socket_connect(client, ip, port).unwrap_or_else(|_| fail(b"client connect"));
    if !wait_established(client, b"client") { return false; }
    if !send_all(client, b"PING!") { return false; }
    let mut buf = [0u8; 16];
    if !recv_exact(client, &mut buf, 5) { return false; }
    let ok = &buf[..5] == &b"PING!"[..];
    let _ = ob_socket_close(client);
    ok
}

fn parse_u16(s: &[u8]) -> Option<u16> {
    let mut v: u32 = 0;
    if s.is_empty() { return None; }
    for &c in s {
        if !c.is_ascii_digit() { return None; }
        v = v * 10 + (c - b'0') as u32;
        if v > 65535 { return None; }
    }
    Some(v as u16)
}

fn parse_ip(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut idx = 0usize;
    let mut cur: u32 = 0;
    let mut seen = 0u32;
    for &c in s {
        if c == b'.' {
            if idx >= 4 || seen == 0 { return None; }
            out[idx] = cur as u8;
            idx += 1;
            cur = 0;
            seen = 0;
        } else if c.is_ascii_digit() {
            cur = cur * 10 + (c - b'0') as u32;
            seen += 1;
            if cur > 255 { return None; }
        } else {
            return None;
        }
    }
    if idx != 3 || seen == 0 { return None; }
    out[3] = cur as u8;
    Some(out)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let args = libneodos::args::read_args();
    // Parse whitespace-separated tokens.
    let mut tok: [&[u8]; 4] = [&[]; 4];
    let mut ntok = 0usize;
    for word in args.split(|&b| b == b' ' || b == b'\t' || b == b'\0' || b == b'\r' || b == b'\n') {
        if word.is_empty() { continue; }
        if ntok < tok.len() { tok[ntok] = word; }
        ntok += 1;
    }

    let ok = if tok[0] == &b"server"[..] {
        match tok.get(1).and_then(|s| parse_u16(*s)) {
            Some(p) => run_server(p),
            None => fail(b"server <port>"),
        }
    } else if tok[0] == &b"client"[..] {
        match (tok.get(1).and_then(|s| parse_ip(*s)), tok.get(2).and_then(|s| parse_u16(*s))) {
            (Some(ip), Some(p)) => run_client(ip, p),
            _ => fail(b"client <ip> <port>"),
        }
    } else {
        run_self_test()
    };

    if ok {
        write_str(b"[tcptest] PASS\r\n");
        syscall::sys_exit(0)
    } else {
        write_str(b"[tcptest] FAIL\r\n");
        syscall::sys_exit(1)
    }
}
