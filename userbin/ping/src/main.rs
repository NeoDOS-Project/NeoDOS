#![no_std]
#![no_main]

extern crate alloc;
use core::alloc::{GlobalAlloc, Layout};
use alloc::vec::Vec;
use libneodos::{mem, syscall};

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
    }
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_dec_u64(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = 19;
    if v == 0 {
        buf[i] = b'0';
        write_str(&buf[i..=i]);
        return;
    }
    while v > 0 && i > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..=19]);
}

fn write_ip(ip: u32) {
    let octets = ip.to_be_bytes();
    for (i, &o) in octets.iter().enumerate() {
        if i > 0 { write_str(b"."); }
        let mut buf = [0u8; 3];
        let mut n = 0;
        let mut v = o as u32;
        loop {
            buf[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 { break; }
        }
        for j in (0..n).rev() {
            write_str(&buf[j..=j]);
        }
    }
}

fn parse_ip(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 { return None; }
    let mut ip = 0u32;
    for part in &parts {
        let octet: u32 = part.parse().ok()?;
        if octet > 255 { return None; }
        ip = (ip << 8) | octet;
    }
    Some(ip)
}

fn read_args() -> [u8; 256] {
    let mut buf = [0u8; 256];
    let _ = syscall::sys_read(0, &mut buf);
    buf
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");

    // Read command line (first argument after "ping ")
    let args = read_args();
    let args_str = core::str::from_utf8(&args).unwrap_or("");
    let target = args_str.trim().split_whitespace().nth(1).unwrap_or("");

    if target.is_empty() {
        write_str(b"Usage: ping <ip>\r\n");
        loop { syscall::sys_yield(); }
    }

    let dest_ip = match parse_ip(target) {
        Some(ip) => ip,
        None => {
            write_str(b"Ping: invalid IP address: ");
            write_str(target.as_bytes());
            write_str(b"\r\n");
            loop { syscall::sys_yield(); }
        }
    };

    write_str(b"Pinging ");
    write_ip(dest_ip);
    write_str(b" with 56 bytes of data:\r\n");

    for _seq in 1..=4 {
        let rtt = syscall::sys_icmp_ping(dest_ip);
        if rtt > 0 {
            write_str(b"Reply from ");
            write_ip(dest_ip);
            write_str(b": bytes=56 time=");
            write_dec_u64(rtt);
            write_str(b"us TTL=64\r\n");
        } else {
            write_str(b"Request timed out.\r\n");
        }

        // Wait between pings
        for _ in 0..100_000 {
            syscall::sys_yield();
        }
    }

    write_str(b"\r\nPing complete.\r\n");
    loop { syscall::sys_yield(); }
}
