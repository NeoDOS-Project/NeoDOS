#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

extern crate alloc;
use core::alloc::{GlobalAlloc, Layout};
use alloc::vec::Vec;
use libneodos::{i18n, mem, syscall, tr_id};

const APP_NAME: &str = "ping";
const IDS_ERR_INVALID_IP: u32 = 1004;
const IDS_PINGING: u32 = 1005;
const IDS_REPLY: u32 = 1006;
const IDS_TIMEOUT: u32 = 1007;
const IDS_COMPLETE: u32 = 1008;

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

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
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

fn parse_ip_address(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 { return None; }
    let mut ip: u32 = 0;
    for part in &parts {
        let octet: u32 = part.parse().ok()?;
        if octet > 255 { return None; }
        ip = (ip << 8) | octet;
    }
    Some(ip)
}

fn print_help() {
    write_str(b"\r\nping <ip> [count]\r\n  Ping a network address.\r\n\r\n");
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

    let arg_str = core::str::from_utf8(args).unwrap_or("");
    let (ip_str, count) = if let Some(space) = arg_str.find(' ') {
        let c: u32 = arg_str[space + 1..].trim().parse().unwrap_or(4);
        (&arg_str[..space], c)
    } else {
        (arg_str, 4)
    };

    let dest_ip = match parse_ip_address(ip_str) {
        Some(ip) => ip,
        None => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_INVALID_IP).as_bytes());
            write_err(ip_str.as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    write_str(b"\r\n");
    write_str(tr_id!(IDS_PINGING).as_bytes());
    write_ip(dest_ip);
    write_str(b" with 32 bytes of data:\r\n\r\n");

    let mut lost = 0u32;
    for _ in 0..count {
        let rtt = syscall::sys_icmp_ping(dest_ip);
        if rtt > 0 {
            write_str(tr_id!(IDS_REPLY).as_bytes());
            write_ip(dest_ip);
            write_str(b": bytes=32 time=");
            write_dec_u64(rtt / 1000);
            write_str(b"ms TTL=64\r\n");
        } else {
            write_str(tr_id!(IDS_TIMEOUT).as_bytes());
            write_str(b"\r\n");
            lost += 1;
        }
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_COMPLETE).as_bytes());
    write_str(b" ");
    write_dec_u64(count as u64);
    write_str(b" sent, ");
    write_dec_u64((count - lost) as u64);
    write_str(b" received, ");
    write_dec_u64((lost * 100 / count) as u64);
    write_str(b"% loss\r\n\r\n");
    syscall::sys_exit(0)
}
