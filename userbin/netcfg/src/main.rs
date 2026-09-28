#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{i18n, mem, syscall, tr_id};

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

const REG_NET_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";
const CRLF: &[u8] = b"\r\n";
const DEFAULT_MASK: u32 = 0x00FF_FFFF; // /24, used when SubnetMask is unset (0)

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn read_reg_dword(key_fd: u8, name: &str) -> Option<u32> {
    let mut reg_buf = [0u8; 12];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 12 { return None; }
    let value_type = u32::from_le_bytes([reg_buf[0], reg_buf[1], reg_buf[2], reg_buf[3]]);
    if value_type != syscall::REG_DWORD { return None; }
    Some(u32::from_le_bytes([reg_buf[8], reg_buf[9], reg_buf[10], reg_buf[11]]))
}

fn write_reg_dword(key_fd: u8, name: &str, val: u32) {
    let _ = syscall::sys_cm_set_value(key_fd, name, syscall::REG_DWORD, &val.to_le_bytes());
}

/// Parse a dotted-decimal IPv4 address into a big-endian `u32`.
fn parse_ip(s: &str) -> Option<u32> {
    let mut ip: u32 = 0;
    let mut count = 0usize;
    for part in s.split('.') {
        if count == 4 { return None; }
        let octet: u32 = part.parse().ok()?;
        if octet > 255 { return None; }
        ip = (ip << 8) | octet;
        count += 1;
    }
    if count == 4 { Some(ip) } else { None }
}

fn format_ip(ip: u32, buf: &mut [u8]) -> usize {
    let octets = ip.to_be_bytes();
    let mut pos = 0;
    for (i, &octet) in octets.iter().enumerate() {
        if i > 0 { if pos < buf.len() { buf[pos] = b'.'; pos += 1; } }
        let mut d = [0u8; 3];
        let mut n = 0;
        let mut v = octet as usize;
        loop {
            if n < 3 { d[n] = b'0' + (v % 10) as u8; n += 1; }
            v /= 10;
            if v == 0 { break; }
        }
        for j in (0..n).rev() {
            if pos < buf.len() { buf[pos] = d[j]; pos += 1; }
        }
    }
    pos
}

fn write_ip(ip: u32) {
    let mut b = [0u8; 16];
    let n = format_ip(ip, &mut b);
    write_str(&b[..n]);
}

fn print_help() {
    write_str(b"\r\nnetcfg - network configurator\r\n\r\n");
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

fn iface_fd() -> u8 {
    match syscall::sys_cm_open_key(REG_NET_PATH) {
        Ok(fd) => fd,
        Err(_) => {
            write_str(tr_id!(IDS_ERR_KEY).as_bytes());
            write_str(CRLF);
            syscall::sys_exit(1);
        }
    }
}

fn load_net() -> Option<&'static NetAbiTable> {
    match syscall::sys_loadlib("C:\\System\\Libraries\\net.nxl\0") {
        Ok(base) => Some(unsafe { &*(base as *const NetAbiTable) }),
        Err(_) => None,
    }
}

/// Apply an explicit IP/mask/gateway to the runtime NIC (gateway 0 = unset).
fn apply_ip(ip: u32, mask: u32, gw: u32) {
    if let Some(net) = load_net() {
        (net.set_ip)(0, ip, mask);
        if gw != 0 { (net.set_gateway)(0, gw); }
    }
}

/// Re-apply the current Registry static values to the NIC (no-op under DHCP).
fn apply_current(fd: u8) {
    if read_reg_dword(fd, "DHCPEnabled").unwrap_or(1) != 0 { return; }
    let ip = read_reg_dword(fd, "IPAddress").unwrap_or(0);
    let mut mask = read_reg_dword(fd, "SubnetMask").unwrap_or(0);
    if mask == 0 { mask = DEFAULT_MASK; }
    let gw = read_reg_dword(fd, "Gateway").unwrap_or(0);
    if ip != 0 { apply_ip(ip, mask, gw); }
}

/// `netcfg [apply]` — apply the Registry config to the NIC.
fn cmd_apply() -> ! {
    let fd = iface_fd();
    if read_reg_dword(fd, "DHCPEnabled").unwrap_or(1) != 0 {
        write_str(tr_id!(IDS_DHCP_ON).as_bytes());
        write_str(CRLF);
    } else {
        let ip = read_reg_dword(fd, "IPAddress").unwrap_or(0);
        let mut mask = read_reg_dword(fd, "SubnetMask").unwrap_or(0);
        if mask == 0 { mask = DEFAULT_MASK; }
        let gw = read_reg_dword(fd, "Gateway").unwrap_or(0);
        if ip != 0 { apply_ip(ip, mask, gw); }
        write_str(tr_id!(IDS_STATIC).as_bytes());
        write_ip(ip);
        write_str(CRLF);
    }
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}

/// `netcfg /setdns <server> [server2] [server3]`
fn cmd_setdns(rest: &str) -> ! {
    let names = ["DnsServer", "DnsServer2", "DnsServer3"];
    let mut servers = [0u32; 3];
    let mut count = 0usize;

    for tok in rest.split_ascii_whitespace() {
        if count >= 3 { break; }
        match parse_ip(tok) {
            Some(ip) => { servers[count] = ip; count += 1; }
            None => invalid_addr(tok),
        }
    }
    if count == 0 {
        write_str(tr_id!(IDS_SET_DNS_USAGE).as_bytes());
        write_str(CRLF);
        syscall::sys_exit(2);
    }

    let fd = iface_fd();
    for (i, name) in names.iter().enumerate() {
        write_reg_dword(fd, name, if i < count { servers[i] } else { 0 });
    }
    let flush_ok = syscall::sys_cm_flush_key(fd).is_ok();
    let _ = syscall::sys_close(fd);

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
        Some(t) => match parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let mask = match it.next() {
        Some(t) => match parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let gw = match it.next() {
        Some(t) => match parse_ip(t) { Some(v) => Some(v), None => invalid_addr(t) },
        None => None,
    };

    let fd = iface_fd();
    write_reg_dword(fd, "IPAddress", ip);
    write_reg_dword(fd, "SubnetMask", mask);
    if let Some(g) = gw { write_reg_dword(fd, "Gateway", g); }
    write_reg_dword(fd, "DHCPEnabled", 0);
    write_reg_dword(fd, "DHCPBound", 0);
    let _ = syscall::sys_cm_flush_key(fd);
    apply_ip(ip, mask, gw.unwrap_or(0));

    write_str(tr_id!(IDS_SETIP_OK).as_bytes());
    write_ip(ip);
    write_str(b"/");
    write_ip(mask);
    if let Some(g) = gw { write_str(b" gw "); write_ip(g); }
    write_str(CRLF);
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}

/// `netcfg /setmask <mask>`
fn cmd_setmask(rest: &str) -> ! {
    let mask = match rest.split_ascii_whitespace().next() {
        Some(t) => match parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let fd = iface_fd();
    write_reg_dword(fd, "SubnetMask", mask);
    let _ = syscall::sys_cm_flush_key(fd);
    apply_current(fd);
    write_str(tr_id!(IDS_SETMASK_OK).as_bytes());
    write_ip(mask);
    write_str(CRLF);
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}

/// `netcfg /setgateway <gateway>`
fn cmd_setgateway(rest: &str) -> ! {
    let gw = match rest.split_ascii_whitespace().next() {
        Some(t) => match parse_ip(t) { Some(v) => v, None => invalid_addr(t) },
        None => usage(),
    };
    let fd = iface_fd();
    write_reg_dword(fd, "Gateway", gw);
    let _ = syscall::sys_cm_flush_key(fd);
    apply_current(fd);
    write_str(tr_id!(IDS_SETGW_OK).as_bytes());
    write_ip(gw);
    write_str(CRLF);
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}

/// `netcfg /dhcp on|off`
fn cmd_dhcp(rest: &str) -> ! {
    let arg = rest.split_ascii_whitespace().next().unwrap_or("");
    let fd = iface_fd();
    if arg.eq_ignore_ascii_case("on") {
        write_reg_dword(fd, "DHCPEnabled", 1);
        write_reg_dword(fd, "DHCPBound", 0);
        let _ = syscall::sys_cm_flush_key(fd);
        write_str(tr_id!(IDS_DHCP_ON).as_bytes());
        write_str(CRLF);
    } else if arg.eq_ignore_ascii_case("off") {
        write_reg_dword(fd, "DHCPEnabled", 0);
        write_reg_dword(fd, "DHCPBound", 0);
        let _ = syscall::sys_cm_flush_key(fd);
        apply_current(fd);
        write_str(tr_id!(IDS_DHCP_OFF).as_bytes());
        write_str(CRLF);
    } else {
        let _ = syscall::sys_close(fd);
        usage();
    }
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}

/// `netcfg /reset` — clear IP/mask/gateway/DNS and enable DHCP.
fn cmd_reset() -> ! {
    let fd = iface_fd();
    for name in ["IPAddress", "SubnetMask", "Gateway", "DnsServer", "DnsServer2", "DnsServer3"] {
        write_reg_dword(fd, name, 0);
    }
    write_reg_dword(fd, "DHCPEnabled", 1);
    write_reg_dword(fd, "DHCPBound", 0);
    let _ = syscall::sys_cm_flush_key(fd);
    write_str(tr_id!(IDS_RESET_OK).as_bytes());
    write_str(CRLF);
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}

/// `netcfg /resetdns`
fn cmd_resetdns() -> ! {
    let fd = iface_fd();
    for name in ["DnsServer", "DnsServer2", "DnsServer3"] {
        write_reg_dword(fd, name, 0);
    }
    let _ = syscall::sys_cm_flush_key(fd);
    write_str(tr_id!(IDS_RESETDNS_OK).as_bytes());
    write_str(CRLF);
    let _ = syscall::sys_close(fd);
    syscall::sys_exit(0);
}

/// Read the runtime NIC IPv4 address from the kernel NicInfo (same source as
/// `ipconfig`). `net.nxl`'s `get_ip`/`get_mask`/`get_gateway` are unreliable.
fn query_nic_ip() -> u32 {
    let fd = match syscall::sys_ob_open("\\Global\\Info\\Network", 1) {
        Ok(fd) => fd,
        Err(_) => return 0,
    };
    let mut buf = [0u8; 256];
    let r = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::NicInfo, &mut buf);
    let _ = syscall::sys_close(fd);
    match r {
        Ok(n) if n as usize >= 84 => {
            u32::from_be_bytes([buf[10], buf[11], buf[12], buf[13]])
        }
        _ => 0,
    }
}

/// NIC link state from the kernel NicInfo (`link_up` at offset 14).
fn query_nic_link() -> u8 {
    let fd = match syscall::sys_ob_open("\\Global\\Info\\Network", 1) {
        Ok(fd) => fd,
        Err(_) => return 0,
    };
    let mut buf = [0u8; 256];
    let r = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::NicInfo, &mut buf);
    let _ = syscall::sys_close(fd);
    match r {
        Ok(n) if n as usize >= 84 => buf[14],
        _ => 0,
    }
}

/// Daemon mode (no arguments, i.e. the `Netcfg` service): keep the runtime NIC
/// in sync with the interface Registry values. The Registry is the single
/// source of truth; `dhcpd` publishes leases there and `netcfg` applies them.
/// Re-applies on config changes and on link up. Never returns.
fn run_daemon() -> ! {
    let mut last_ip = u32::MAX;
    let mut last_mask = u32::MAX;
    let mut last_gw = u32::MAX;
    let mut last_link = 0u8;
    loop {
        if let Ok(fd) = syscall::sys_cm_open_key(REG_NET_PATH) {
            let ip = read_reg_dword(fd, "IPAddress").unwrap_or(0);
            let mut mask = read_reg_dword(fd, "SubnetMask").unwrap_or(0);
            if mask == 0 { mask = DEFAULT_MASK; }
            let gw = read_reg_dword(fd, "Gateway").unwrap_or(0);
            let _ = syscall::sys_close(fd);

            let link = query_nic_link();
            let link_up_edge = link != 0 && last_link == 0;
            let changed = ip != last_ip || mask != last_mask || gw != last_gw;

            if ip != 0 && (changed || link_up_edge) {
                apply_ip(ip, mask, gw);
                last_ip = ip;
                last_mask = mask;
                last_gw = gw;
            }
            last_link = link;
        }
        // ~ tens of ms between polls; yield so the rest of the system runs.
        for _ in 0..5_000_000 { core::hint::spin_loop(); }
        syscall::sys_yield();
    }
}

/// `netcfg /status` — Registry config vs runtime NIC.
fn cmd_status() -> ! {
    let fd = iface_fd();
    let dhcp = read_reg_dword(fd, "DHCPEnabled").unwrap_or(1) != 0;
    let rip = read_reg_dword(fd, "IPAddress").unwrap_or(0);
    let rmask = read_reg_dword(fd, "SubnetMask").unwrap_or(0);
    let rgw = read_reg_dword(fd, "Gateway").unwrap_or(0);
    let _ = syscall::sys_close(fd);

    let nic_ip = query_nic_ip();

    write_str(tr_id!(IDS_STATUS_HEADER).as_bytes());
    write_str(CRLF);
    write_str(b"  registry: ip=");
    write_ip(rip);
    write_str(b" mask=");
    write_ip(rmask);
    write_str(b" gw=");
    write_ip(rgw);
    write_str(b" dhcp=");
    write_str(if dhcp { &b"on"[..] } else { &b"off"[..] });
    write_str(CRLF);
    write_str(b"  nic:      ip=");
    write_ip(nic_ip);
    write_str(CRLF);

    let applied = if dhcp { nic_ip != 0 } else { rip != 0 && nic_ip == rip };
    write_str(tr_id!(if applied { IDS_STATUS_APPLIED } else { IDS_STATUS_PENDING }).as_bytes());
    write_str(CRLF);
    syscall::sys_exit(0);
}

/// `netcfg /test` — validate the static config without touching the NIC.
fn cmd_test() -> ! {
    let fd = iface_fd();
    let dhcp = read_reg_dword(fd, "DHCPEnabled").unwrap_or(1) != 0;
    let _ = syscall::sys_close(fd);

    if !dhcp {
        let fd = iface_fd();
        let ip = read_reg_dword(fd, "IPAddress").unwrap_or(0);
        let mut mask = read_reg_dword(fd, "SubnetMask").unwrap_or(0);
        if mask == 0 { mask = DEFAULT_MASK; }
        let gw = read_reg_dword(fd, "Gateway").unwrap_or(0);
        let _ = syscall::sys_close(fd);

        if ip != 0 && gw != 0 && (gw & mask) != (ip & mask) {
            write_str(tr_id!(IDS_TEST_WARN).as_bytes());
            write_str(CRLF);
            syscall::sys_exit(1);
        }
    }
    write_str(tr_id!(IDS_TEST_OK).as_bytes());
    write_str(CRLF);
    syscall::sys_exit(0);
}

#[repr(C)]
struct NetAbiTable {
    version: u32,
    iface_count: extern "C" fn() -> u32,
    iface_info: unsafe extern "C" fn(u32, *mut NetIfaceInfo) -> i32,
    iface_stats: extern "C" fn(u32, *mut NetIfaceStats) -> i32,
    socket_create: extern "C" fn(u32) -> i32,
    socket_bind: extern "C" fn(i32, u32, u16) -> i32,
    socket_connect: extern "C" fn(i32, u32, u16) -> i32,
    socket_listen: extern "C" fn(i32) -> i32,
    socket_send: unsafe extern "C" fn(i32, *const u8, u32) -> i32,
    socket_recv: unsafe extern "C" fn(i32, *mut u8, u32) -> i32,
    socket_close: extern "C" fn(i32) -> i32,
    set_ip: extern "C" fn(u32, u32, u32) -> i32,
    set_gateway: extern "C" fn(u32, u32) -> i32,
    get_ip: extern "C" fn(u32) -> u32,
    get_gateway: extern "C" fn(u32) -> u32,
    get_mask: extern "C" fn(u32) -> u32,
    get_dhcp_bound: extern "C" fn() -> i32,
    _reserved: [u64; 7],
}

#[repr(C)]
struct NetIfaceInfo {
    nic_id: u32,
    mac: [u8; 6],
    ip: [u8; 4],
    link_up: u8,
    vendor_id: u16,
    device_id: u16,
    name: [u8; 16],
    description: [u8; 48],
}

#[repr(C)]
struct NetIfaceStats {
    rx_packets: u64,
    tx_packets: u64,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_errors: u32,
    tx_errors: u32,
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    // netcfg has two roles: with no arguments it runs as the resident
    // configurator daemon (the `Netcfg` service); with arguments it is a
    // one-shot configuration CLI that writes/applies and exits.
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
        run_daemon();
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
