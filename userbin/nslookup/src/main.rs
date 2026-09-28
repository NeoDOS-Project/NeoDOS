#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{i18n, mem, syscall, tr_id};

const APP_NAME: &str = "nslookup";

// NLT string IDs (must match data/locale/*/nslookup.toml).
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_SERVER: u32 = 1003;
const IDS_ADDRESS: u32 = 1004;
const IDS_NAME: u32 = 1005;
const IDS_ADDRESSES: u32 = 1006;
const IDS_LOCAL: u32 = 1007;
const IDS_ERR_NOCONFIG: u32 = 1010;
const IDS_ERR_NOSERVER: u32 = 1011;
const IDS_ERR_INVALID_HOST: u32 = 1012;
const IDS_ERR_TIMEOUT: u32 = 1013;
const IDS_ERR_UNREACHABLE: u32 = 1014;
const IDS_ERR_NXDOMAIN: u32 = 1015;
const IDS_ERR_MALFORMED: u32 = 1016;
const IDS_ERR_TRUNCATED: u32 = 1017;
const IDS_ERR_SERVER_FAILURE: u32 = 1018;
const IDS_ERR_NO_A: u32 = 1019;
const IDS_ERR_NETWORK: u32 = 1020;

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

/// Localized message for each distinct resolver failure.
fn error_message(err: libnet::dns::DnsError) -> &'static str {
    match err {
        libnet::dns::DnsError::NoConfig => tr_id!(IDS_ERR_NOCONFIG),
        libnet::dns::DnsError::NoServer => tr_id!(IDS_ERR_NOSERVER),
        libnet::dns::DnsError::InvalidHostname => tr_id!(IDS_ERR_INVALID_HOST),
        libnet::dns::DnsError::Timeout => tr_id!(IDS_ERR_TIMEOUT),
        libnet::dns::DnsError::ServerUnreachable => tr_id!(IDS_ERR_UNREACHABLE),
        libnet::dns::DnsError::NxDomain => tr_id!(IDS_ERR_NXDOMAIN),
        libnet::dns::DnsError::MalformedResponse => tr_id!(IDS_ERR_MALFORMED),
        libnet::dns::DnsError::Truncated => tr_id!(IDS_ERR_TRUNCATED),
        libnet::dns::DnsError::ServerFailure => tr_id!(IDS_ERR_SERVER_FAILURE),
        libnet::dns::DnsError::NoARecord => tr_id!(IDS_ERR_NO_A),
        libnet::dns::DnsError::Network => tr_id!(IDS_ERR_NETWORK),
    }
}

fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n\r\n");
}

/// Split whitespace-delimited arguments. Returns (hostname, optional server).
fn parse_args(arg_str: &str) -> (&str, Option<&str>) {
    let mut tokens = arg_str.split_ascii_whitespace();
    let hostname = tokens.next().unwrap_or("");
    let server = tokens.next();
    (hostname, server)
}

fn print_server(server: [u8; 4]) {
    write_str(tr_id!(IDS_SERVER).as_bytes());
    if server == [0, 0, 0, 0] {
        write_str(tr_id!(IDS_LOCAL).as_bytes());
    } else {
        write_ip(server);
    }
    write_str(b"\r\n");
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
    let (hostname, server_arg) = parse_args(arg_str);
    if hostname.is_empty() {
        print_help();
        syscall::sys_exit(1);
    }

    // Optional explicit DNS server: `nslookup <hostname> <dns-server>`.
    let explicit_server = match server_arg {
        Some(s) => match libnet::dns::parse_dotted_ip(s) {
            Some(ip) => Some(ip),
            None => {
                print_help();
                syscall::sys_exit(1);
            }
        },
        None => None,
    };

    let result = match explicit_server {
        Some(server) => libnet::dns::resolve_with_server(hostname, server),
        None => libnet::dns::resolve(hostname),
    };

    match result {
        Ok(result) => {
            write_str(b"\r\n");
            print_server(result.server);
            write_str(tr_id!(IDS_ADDRESS).as_bytes());
            if result.server == [0, 0, 0, 0] {
                write_str(b"127.0.0.1");
            } else {
                write_ip(result.server);
            }
            write_str(b"\r\n\r\n");

            write_str(tr_id!(IDS_NAME).as_bytes());
            write_str(hostname.as_bytes());
            write_str(b"\r\n");
            for (i, addr) in result.addresses.iter().enumerate() {
                if i == 0 {
                    write_str(tr_id!(IDS_ADDRESSES).as_bytes());
                } else {
                    // Align continuation lines under the first address.
                    write_str(b"           ");
                }
                write_ip(*addr);
                write_str(b"\r\n");
            }
            write_str(b"\r\n");
            syscall::sys_exit(0);
        }
        Err(err) => {
            write_err(b"\r\n");
            write_err(error_message(err).as_bytes());
            write_err(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    }
}
