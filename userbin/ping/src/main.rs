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
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
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
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
    write_str(b"\r\n\r\n");
}

/// Parse `[host] [/n count] [/t]`. Accepts `/` (NT style) and `-` prefixes,
/// plus the legacy positional `ping <host> <count>`. Returns (host, count, on).
fn parse_args(arg_str: &str) -> (&str, u32, bool) {
    let mut host: &str = "";
    let mut count: u32 = 4;
    let mut continuous = false;
    let mut tokens = arg_str.split_ascii_whitespace();
    let mut positional_count_seen = false;
    while let Some(tok) = tokens.next() {
        if tok.eq_ignore_ascii_case("/n") || tok.eq_ignore_ascii_case("/c")
            || tok.eq_ignore_ascii_case("-n") || tok.eq_ignore_ascii_case("-c")
        {
            if let Some(v) = tokens.next() {
                count = v.parse().unwrap_or(count);
            }
        } else if tok.eq_ignore_ascii_case("/t") || tok.eq_ignore_ascii_case("-t") {
            continuous = true;
        } else if host.is_empty() {
            host = tok;
        } else if !positional_count_seen {
            // Legacy: second positional token is the count.
            count = tok.parse().unwrap_or(count);
            positional_count_seen = true;
        }
    }
    (host, count, continuous)
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
    let (host, count, continuous) = parse_args(arg_str);
    if host.is_empty() {
        print_help();
        syscall::sys_exit(1);
    }

    let dest_ip = match parse_ip_address(host) {
        Some(ip) => ip,
        None => {
            // Numeric IP only for now; a hostname would need libnet DNS.
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_INVALID_IP).as_bytes());
            write_err(host.as_bytes());
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    };

    write_str(b"\r\n");
    write_str(tr_id!(IDS_PINGING).as_bytes());
    write_ip(dest_ip);
    write_str(b" with 32 bytes of data:\r\n\r\n");

    let mut sent: u32 = 0;
    let mut received: u32 = 0;
    let mut rtt_min: u64 = u64::MAX;
    let mut rtt_max: u64 = 0;
    let mut rtt_sum: u64 = 0;
    loop {
        if !continuous && sent >= count {
            break;
        }
        sent += 1;
        let rtt = syscall::sys_icmp_ping(dest_ip);
        if rtt > 0 {
            received += 1;
            rtt_sum = rtt_sum.saturating_add(rtt);
            if rtt < rtt_min { rtt_min = rtt; }
            if rtt > rtt_max { rtt_max = rtt; }
            write_str(tr_id!(IDS_REPLY).as_bytes());
            write_ip(dest_ip);
            write_str(b": bytes=32 time=");
            write_dec_u64(rtt / 1000);
            write_str(b"ms TTL=64\r\n");
        } else {
            write_str(tr_id!(IDS_TIMEOUT).as_bytes());
            write_str(b"\r\n");
        }
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_COMPLETE).as_bytes());
    write_str(b" ");
    write_dec_u64(sent as u64);
    write_str(b" sent, ");
    write_dec_u64(received as u64);
    write_str(b" received, ");
    let lost = sent.saturating_sub(received);
    let loss_pct = if sent == 0 { 0 } else { lost * 100 / sent };
    write_dec_u64(loss_pct as u64);
    write_str(b"% loss");
    if received > 0 {
        write_str(b" (min/avg/max ms=");
        write_dec_u64(rtt_min / 1000);
        write_str(b"/");
        write_dec_u64(rtt_sum / received as u64 / 1000);
        write_str(b"/");
        write_dec_u64(rtt_max / 1000);
        write_str(b")");
    }
    write_str(b"\r\n\r\n");
    syscall::sys_exit(0)
}
