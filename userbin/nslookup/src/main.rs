#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{i18n, mem, syscall};

const APP_NAME: &str = "nslookup";

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

fn write_ip(ip: [u8; 4]) {
    for (i, &o) in ip.iter().enumerate() {
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

fn read_reg_dword(fd: u8, name: &str) -> u32 {
    let mut buf = [0u8; 16];
    let r = syscall::sys_cm_query_value(fd, name, &mut buf);
    match r {
        Ok(n) if n >= 12 => {
            let t = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            if t == syscall::REG_DWORD {
                return u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
            }
        }
        _ => {}
    }
    0
}

fn ip_to_str(ip: u32, buf: &mut [u8; 16]) -> usize {
    let o = ip.to_be_bytes();
    let mut pos = 0;
    for (idx, &b) in o.iter().enumerate() {
        if idx > 0 { buf[pos] = b'.'; pos += 1; }
        if b >= 100 { buf[pos] = b'0' + b / 100; pos += 1; }
        if b >= 10  { buf[pos] = b'0' + (b / 10) % 10; pos += 1; }
        buf[pos] = b'0' + (b % 10); pos += 1;
    }
    pos
}

fn print_help() {
    write_str(b"\r\nnslookup <hostname>\r\n  Resolve a hostname to an IP address.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    let args = libneodos::args::trim_ascii(&raw);
    if args.is_empty() {
        print_help();
        syscall::sys_exit(1);
    }

    let hostname = core::str::from_utf8(args).unwrap_or("").trim();
    if hostname.is_empty() {
        print_help();
        syscall::sys_exit(1);
    }

    // Read DNS server from registry for display
    let reg_path = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";
    let dns_ip = match syscall::sys_cm_open_key(reg_path) {
        Ok(fd) => {
            let dns = read_reg_dword(fd, "DnsServer");
            let _ = syscall::sys_close(fd);
            dns
        }
        Err(_) => 0,
    };

    write_str(b"\r\n");
    if dns_ip != 0 {
        write_str(b"Server: ");
        let mut ip_buf = [0u8; 16];
        let n = ip_to_str(dns_ip, &mut ip_buf);
        write_str(&ip_buf[..n]);
        write_str(b"\r\n\r\n");
    }

    write_str(b"Name:\r\n    ");
    write_str(hostname.as_bytes());
    write_str(b"\r\n\r\n");

    match libnet::dns_resolve(hostname) {
        Some(addr) => {
            write_str(b"Address:\r\n    ");
            write_ip(addr);
            write_str(b"\r\n\r\n");
            syscall::sys_exit(0);
        }
        None => {
            write_str(b"Address:\r\n    ");
            write_str(b"*** unresolved\r\n\r\n");
            syscall::sys_exit(1);
        }
    }
}
