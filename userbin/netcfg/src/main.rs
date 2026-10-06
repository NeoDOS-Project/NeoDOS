#![no_std]
#![no_main]

//! `netcfg` — one-shot network configuration CLI (#319/#365).
//!
//! Reads/validates the interface configuration, writes it through the shared
//! `libnet::config` backend (#363) and/or applies it explicitly, then exits.
//! It is never resident and never a service: the continuous application role
//! belongs to the `NetApplier` service.

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{i18n, mem, syscall, tr_id};
use libnet::config::{self, NetConfig};

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

const APP_NAME: &str = "netcfg";
const IDS_ERR_KEY: u32 = 1001;
const IDS_STATIC: u32 = 1002;
const IDS_SET_DNS_USAGE: u32 = 1006;
const IDS_SET_DNS_OK: u32 = 1008;
const IDS_SET_DNS_FLUSH: u32 = 1009;
const IDS_SET_DNS_NOFLUSH: u32 = 1010;
const IDS_UNKNOWN: u32 = 1020;
const IDS_INVALID_ADDR: u32 = 1021;
const IDS_SETIP_OK: u32 = 1022;
const IDS_SETMASK_OK: u32 = 1023;
const IDS_SETGW_OK: u32 = 1024;
const IDS_DHCP_ON: u32 = 1025;
const IDS_DHCP_OFF: u32 = 1026;
const IDS_RESET_OK: u32 = 1027;
const IDS_RESETDNS_OK: u32 = 1028;
const IDS_STATUS_HEADER: u32 = 1029;
const IDS_STATUS_APPLIED: u32 = 1030;
const IDS_STATUS_PENDING: u32 = 1031;
const IDS_TEST_OK: u32 = 1032;
const IDS_TEST_WARN: u32 = 1033;

const CRLF: &[u8] = b"\r\n";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_ip(ip: u32) {
    let mut b = [0u8; 16];
    let n = config::format_ip(ip, &mut b);
    write_str(&b[..n]);
}

fn print_help() {
    write_str(b"\r\nnetcfg - network configuration CLI\r\n\r\n");
    write_str(b"  netcfg                          apply the Registry config (same as /apply)\r\n");
    write_str(b"  netcfg /apply                  apply the Registry config to the NIC\r\n");
    write_str(b"  netcfg /setip <ip> <mask> [gw]  set static IPv4 (disables DHCP)\r\n");
    write_str(b"  netcfg /setmask <mask>          set the subnet mask\r\n");
    write_str(b"  netcfg /setgateway <gw>         set the default gateway\r\n");
    write_str(b"  netcfg /setdns <s1> [s2] [s3]   set the DNS servers\r\n");
    write_str(b"  netcfg /dhcp on|off             enable/disable DHCP\r\n");
    write_str(b"  netcfg /reset                   clear IP/mask/gw/DNS; enable DHCP\r\n");
    write_str(b"  netcfg /resetdns                clear the DNS servers\r\n");
    write_str(b"  netcfg /status                  show Registry vs NIC\r\n");
    write_str(b"  netcfg /test                    validate the config (dry run)\r\n");
    write_str(b"  netcfg /? | help                show this help\r\n\r\n");
}

fn usage() -> ! {
    print_help();
    syscall::sys_exit(2);
}

fn invalid_addr(tok: &str) -> ! {
    write_str(tr_id!(IDS_INVALID_ADDR).as_bytes());
    write_str(tok.as_bytes());
    write_str(CRLF);
    syscall::sys_exit(2);
}

/// Load the interface configuration (shared backend). Exits on a missing key.
fn iface_config() -> NetConfig {
    match config::load(0) {
        Some(cfg) => cfg,
        None => {
            write_str(tr_id!(IDS_ERR_KEY).as_bytes());
            write_str(CRLF);
            syscall::sys_exit(1);
        }
    }
}

/// `netcfg [apply]` — apply the Registry config to the NIC.
fn cmd_apply() -> ! {
    let cfg = iface_config();
    if cfg.dhcp_enabled {
        write_str(tr_id!(IDS_DHCP_ON).as_bytes());
        write_str(CRLF);
    } else {
        if cfg.has_ip() { config::apply(0, &cfg); }
        write_str(tr_id!(IDS_STATIC).as_bytes());
        write_ip(cfg.ip);
        write_str(CRLF);
    }
    syscall::sys_exit(0);
}

/// `netcfg /setdns <server> [server2] [server3]`
fn cmd_setdns(rest: &str) -> ! {
    let mut servers = [0u32; 3];
    let mut count = 0usize;

    for tok in rest.split_ascii_whitespace() {
        if count >= 3 { break; }
        match config::parse_ip(tok) {
            Some(ip) => { servers[count] = ip; count += 1; }
            None => invalid_addr(tok),
        }
    }
    if count == 0 {
        write_str(tr_id!(IDS_SET_DNS_USAGE).as_bytes());
        write_str(CRLF);
        syscall::sys_exit(2);
    }

    let mut cfg = iface_config();
    cfg.dns = servers;
    let flush_ok = config::store(0, &cfg).is_ok();

    write_str(tr_id!(IDS_SET_DNS_OK).as_bytes());
    for i in 0..count {
        if i > 0 { write_str(b", "); }
        write_ip(servers[i]);
    }
    write_str(CRLF);
    let msg = if flush_ok { IDS_SET_DNS_FLUSH } else { IDS_SET_DNS_NOFLUSH };
    write_str(tr_id!(msg).as_bytes());
    write_str(CRLF);
    syscall::sys_exit(if flush_ok { 0 } else { 1 })
}

/// `netcfg /setip <ip> <mask> [gateway]` — DHCP off + apply.
fn cmd_setip(rest: &str) -> ! {
    let mut it = rest.split_ascii_whitespace();
    let ip = match it.next() {
        Some(t) => match config::parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let mask = match it.next() {
        Some(t) => match config::parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let gw = match it.next() {
        Some(t) => match config::parse_ip(t) { Some(v) => Some(v), None => invalid_addr(t) },
        None => None,
    };

    let mut cfg = iface_config();
    cfg.ip = ip;
    cfg.mask = mask;
    cfg.dhcp_enabled = false;
    cfg.dhcp_bound = false;
    if let Some(g) = gw { cfg.gateway = g; }
    let _ = config::store(0, &cfg);

    // Apply IP/mask always; the gateway only when the caller provided one.
    let apply_cfg = NetConfig { gateway: gw.unwrap_or(0), ..cfg };
    config::apply(0, &apply_cfg);

    write_str(tr_id!(IDS_SETIP_OK).as_bytes());
    write_ip(ip);
    write_str(b"/");
    write_ip(mask);
    if let Some(g) = gw { write_str(b" gw "); write_ip(g); }
    write_str(CRLF);
    syscall::sys_exit(0);
}

/// `netcfg /setmask <mask>`
fn cmd_setmask(rest: &str) -> ! {
    let mask = match rest.split_ascii_whitespace().next() {
        Some(t) => match config::parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let mut cfg = iface_config();
    cfg.mask = mask;
    let _ = config::store(0, &cfg);
    config::apply_current(0);
    write_str(tr_id!(IDS_SETMASK_OK).as_bytes());
    write_ip(mask);
    write_str(CRLF);
    syscall::sys_exit(0);
}

/// `netcfg /setgateway <gateway>`
fn cmd_setgateway(rest: &str) -> ! {
    let gw = match rest.split_ascii_whitespace().next() {
        Some(t) => match config::parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let mut cfg = iface_config();
    cfg.gateway = gw;
    let _ = config::store(0, &cfg);
    config::apply_current(0);
    write_str(tr_id!(IDS_SETGW_OK).as_bytes());
    write_ip(gw);
    write_str(CRLF);
    syscall::sys_exit(0);
}

/// `netcfg /dhcp on|off`
fn cmd_dhcp(rest: &str) -> ! {
    let arg = rest.split_ascii_whitespace().next().unwrap_or("");
    if !arg.eq_ignore_ascii_case("on") && !arg.eq_ignore_ascii_case("off") {
        usage();
    }

    let mut cfg = iface_config();
    cfg.dhcp_bound = false;
    if arg.eq_ignore_ascii_case("on") {
        cfg.dhcp_enabled = true;
        let _ = config::store(0, &cfg);
        write_str(tr_id!(IDS_DHCP_ON).as_bytes());
        write_str(CRLF);
    } else {
        cfg.dhcp_enabled = false;
        let _ = config::store(0, &cfg);
        config::apply_current(0);
        write_str(tr_id!(IDS_DHCP_OFF).as_bytes());
        write_str(CRLF);
    }
    syscall::sys_exit(0);
}

/// `netcfg /reset` — clear IP/mask/gateway/DNS and enable DHCP.
fn cmd_reset() -> ! {
    let mut cfg = iface_config();
    cfg.ip = 0;
    cfg.mask = 0;
    cfg.gateway = 0;
    cfg.dns = [0; 3];
    cfg.dhcp_enabled = true;
    cfg.dhcp_bound = false;
    let _ = config::store(0, &cfg);
    write_str(tr_id!(IDS_RESET_OK).as_bytes());
    write_str(CRLF);
    syscall::sys_exit(0);
}

/// `netcfg /resetdns`
fn cmd_resetdns() -> ! {
    let mut cfg = iface_config();
    cfg.dns = [0; 3];
    let _ = config::store(0, &cfg);
    write_str(tr_id!(IDS_RESETDNS_OK).as_bytes());
    write_str(CRLF);
    syscall::sys_exit(0);
}

/// `netcfg /status` — Registry config vs runtime NIC.
fn cmd_status() -> ! {
    let cfg = iface_config();
    let nic_ip = config::interface_ip(0);

    write_str(tr_id!(IDS_STATUS_HEADER).as_bytes());
    write_str(CRLF);
    write_str(b"  registry: ip=");
    write_ip(cfg.ip);
    write_str(b" mask=");
    write_ip(cfg.mask);
    write_str(b" gw=");
    write_ip(cfg.gateway);
    write_str(b" dhcp=");
    write_str(if cfg.dhcp_enabled { &b"on"[..] } else { &b"off"[..] });
    write_str(CRLF);
    write_str(b"  nic:      ip=");
    write_ip(nic_ip);
    write_str(CRLF);

    let applied = if cfg.dhcp_enabled { nic_ip != 0 } else { cfg.ip != 0 && nic_ip == cfg.ip };
    write_str(tr_id!(if applied { IDS_STATUS_APPLIED } else { IDS_STATUS_PENDING }).as_bytes());
    write_str(CRLF);
    syscall::sys_exit(0);
}

/// `netcfg /test` — validate the static config without touching the NIC.
fn cmd_test() -> ! {
    let cfg = iface_config();
    if !cfg.dhcp_enabled {
        if cfg.ip != 0 && cfg.gateway != 0 && !cfg.gateway_on_subnet() {
            write_str(tr_id!(IDS_TEST_WARN).as_bytes());
            write_str(CRLF);
            syscall::sys_exit(1);
        }
    }
    write_str(tr_id!(IDS_TEST_OK).as_bytes());
    write_str(CRLF);
    syscall::sys_exit(0);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    // netcfg is exclusively the one-shot network configuration CLI: it reads,
    // validates, writes the Registry and/or applies explicitly, then exits.
    // The resident configuration applier is the separate `NetApplier` service;
    // netcfg must never stay resident (see #365).
    let raw = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw);
    let arg_str = core::str::from_utf8(args).unwrap_or("");
    let cmd_raw = arg_str.split_ascii_whitespace().next().unwrap_or("");
    let cmd = cmd_raw
        .strip_prefix('/')
        .or_else(|| cmd_raw.strip_prefix('-'))
        .unwrap_or(cmd_raw);
    let rest = arg_str[cmd_raw.len()..].trim();

    if cmd.is_empty() {
        // Bare invocation = one-shot apply, then exit (never resident).
        cmd_apply();
    } else if cmd.eq_ignore_ascii_case("apply") {
        cmd_apply();
    } else if cmd.eq_ignore_ascii_case("setdns") {
        cmd_setdns(rest);
    } else if cmd.eq_ignore_ascii_case("setip") {
        cmd_setip(rest);
    } else if cmd.eq_ignore_ascii_case("setmask") {
        cmd_setmask(rest);
    } else if cmd.eq_ignore_ascii_case("setgateway") || cmd.eq_ignore_ascii_case("setgw") {
        cmd_setgateway(rest);
    } else if cmd.eq_ignore_ascii_case("dhcp") {
        cmd_dhcp(rest);
    } else if cmd.eq_ignore_ascii_case("reset") {
        cmd_reset();
    } else if cmd.eq_ignore_ascii_case("resetdns") {
        cmd_resetdns();
    } else if cmd.eq_ignore_ascii_case("status") {
        cmd_status();
    } else if cmd.eq_ignore_ascii_case("test") {
        cmd_test();
    } else if cmd.eq_ignore_ascii_case("help") || cmd == "?" {
        print_help();
        syscall::sys_exit(0);
    } else if cmd.eq_ignore_ascii_case("renew") {
        write_str(b"netcfg: /renew is not implemented (DHCP renewal, see #316)\r\n");
        syscall::sys_exit(2);
    } else if cmd.eq_ignore_ascii_case("nic") {
        write_str(b"netcfg: multi-NIC selection is not implemented (see #317)\r\n");
        syscall::sys_exit(2);
    } else {
        write_str(tr_id!(IDS_UNKNOWN).as_bytes());
        write_str(cmd_raw.as_bytes());
        write_str(CRLF);
        print_help();
        syscall::sys_exit(2);
    }
}
