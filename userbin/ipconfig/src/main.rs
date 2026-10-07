#![no_std]
#![no_main]

//! `ipconfig` — read-only network interface state / diagnostics.
//!
//! Reads the Registry-backed configuration through the shared `libnet::config`
//! backend (#363) and the runtime NIC state through `net.nxl`. It never writes
//! configuration: that is `netcfg`/`neocfg` (and the `NetApplier` service).

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;
use libnet::config::{self, NetConfig};

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

const APP_NAME: &str = "ipconfig";
const IDS_HEADER: u32 = 1001;
const IDS_HOSTNAME: u32 = 1002;
const IDS_ETHERNET: u32 = 1003;
const IDS_DESCRIPTION: u32 = 1004;
const IDS_DRIVER: u32 = 1005;
const IDS_PCI_DEVICE: u32 = 1006;
const IDS_LINK_STATUS: u32 = 1007;
const IDS_MAC: u32 = 1008;
const IDS_IPV4: u32 = 1009;
const IDS_SUBNET_MASK: u32 = 1010;
const IDS_GATEWAY: u32 = 1011;
const IDS_DNS: u32 = 1012;
const IDS_DHCP_ENABLED: u32 = 1013;
const IDS_CONFIG_SOURCE: u32 = 1014;
const IDS_LEASE_TIME: u32 = 1015;
const IDS_UP: u32 = 1016;
const IDS_DOWN: u32 = 1017;
const IDS_DHCP: u32 = 1018;
const IDS_STATIC: u32 = 1019;
const IDS_NONE: u32 = 1020;
const IDS_YES: u32 = 1021;
const IDS_NO: u32 = 1022;
const IDS_ERR_NXL: u32 = 1023;
const IDS_NO_IFACES: u32 = 1024;
const IDS_STATS_RX: u32 = 1025;
const IDS_STATS_TX: u32 = 1026;
const IDS_STATS_RX_BYTES: u32 = 1027;
const IDS_STATS_TX_BYTES: u32 = 1028;
const IDS_LOOPBACK: u32 = 1029;
const IDS_DHCP_SERVER: u32 = 1030;
const IDS_AUTOCONFIG: u32 = 1031;
const IDS_LEASE_OBTAINED: u32 = 1032;
const IDS_LEASE_EXPIRES: u32 = 1033;
const IDS_DOMAIN: u32 = 1034;
const IDS_RENEW: u32 = 1035;
const IDS_REBIND: u32 = 1036;

/// Sentinel nic_id of the loopback `NicInfo` entry (see
/// `net::loopback::LOOPBACK_NIC_ID`). Never a real `NicRegistry` slot.
const LOOPBACK_NIC_ID: u32 = 0xFFFF_FFFF;

/// Fixed 255.0.0.0 as big-endian u32 for `config::format_ip`.
const LOOPBACK_MASK_BE: u32 = 0xFF00_0000;

fn write_str(s: &[u8]) { let _ = syscall::sys_write(1, s); }

fn write_label(id: u32) { write_str(tr_id!(id).as_bytes()); }

fn fmt_u32(v: u32, buf: &mut [u8]) -> usize {
    if v == 0 { buf[0] = b'0'; return 1; }
    let mut tmp = [0u8; 12];
    let mut i = 12;
    let mut n = v;
    while n > 0 { i -= 1; tmp[i] = b'0' + (n % 10) as u8; n /= 10; }
    let len = 12 - i;
    buf[..len].copy_from_slice(&tmp[i..12]);
    len
}

fn write_ip_label(id: u32, ip: u32) {
    write_label(id);
    let mut b = [0u8; 16];
    let n = config::format_ip(ip, &mut b);
    write_str(&b[..n]);
    write_str(b"\r\n");
}

fn write_val_label(id: u32, val: u32, suffix: &[u8]) {
    write_label(id);
    let mut b = [0u8; 16];
    let n = fmt_u32(val, &mut b);
    write_str(&b[..n]);
    if suffix.len() > 0 { write_str(suffix); }
    write_str(b"\r\n");
}

fn fmt_hex16(v: u16, buf: &mut [u8]) -> usize {
    fn hex(b: u8) -> u8 { b"0123456789ABCDEF"[b as usize] }
    buf[0] = hex((v >> 12) as u8 & 0xF);
    buf[1] = hex((v >> 8) as u8 & 0xF);
    buf[2] = hex((v >> 4) as u8 & 0xF);
    buf[3] = hex(v as u8 & 0xF);
    4
}

fn write_mac(mac: &[u8; 6]) {
    for (i, &b) in mac.iter().enumerate() {
        let h = b"0123456789ABCDEF"[((b >> 4) & 0xF) as usize];
        let l = b"0123456789ABCDEF"[(b & 0xF) as usize];
        write_str(&[h, l]);
        if i < 5 { write_str(b":"); }
    }
    write_str(b"\r\n");
}

fn write_u64(mut v: u64) {
    if v == 0 { write_str(b"0"); return; }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while v > 0 { i -= 1; tmp[i] = b'0' + (v % 10) as u8; v /= 10; }
    write_str(&tmp[i..20]);
}

fn write_stat_line(id: u32, val: u64, suffix: &[u8]) {
    write_label(id);
    write_u64(val);
    if suffix.len() > 0 { write_str(suffix); }
    write_str(b"\r\n");
}

fn print_stats(idx: u32) {
    let mut st = libnet::NetIfaceStats {
        rx_packets: 0, tx_packets: 0, rx_bytes: 0, tx_bytes: 0,
        rx_errors: 0, tx_errors: 0,
    };
    if libnet::iface_stats(idx, &mut st) != 0 { return; }
    write_stat_line(IDS_STATS_RX, st.rx_packets, b" packets");
    write_stat_line(IDS_STATS_TX, st.tx_packets, b" packets");
    write_stat_line(IDS_STATS_RX_BYTES, st.rx_bytes, b" bytes");
    write_stat_line(IDS_STATS_TX_BYTES, st.tx_bytes, b" bytes");
    write_str(b"\r\n");
}

fn write_padded_str(buf: &[u8]) {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    if end > 0 { write_str(&buf[..end]); }
}

fn write_02(v: u8) {
    write_str(&[b'0' + (v / 10).min(9), b'0' + (v % 10)]);
}

fn write_04(v: u16) {
    let mut b = [b'0'; 4];
    b[0] += ((v / 1000) % 10) as u8;
    b[1] += ((v / 100) % 10) as u8;
    b[2] += ((v / 10) % 10) as u8;
    b[3] += (v % 10) as u8;
    write_str(&b);
}

/// Write Unix seconds as `DD/MM/YYYY HH:MM:SS` (UTC).
fn write_datetime(secs: u32) {
    let Some(dt) = libntp::unix_secs_to_utc(secs as i64) else { return };
    write_02(dt.day);
    write_str(b"/");
    write_02(dt.month);
    write_str(b"/");
    write_04(2000 + dt.year as u16);
    write_str(b" ");
    write_02(dt.hour);
    write_str(b":");
    write_02(dt.minute);
    write_str(b":");
    write_02(dt.second);
    write_str(b"\r\n");
}

fn print_iface(iface_idx: u32, info: &libnet::NetIfaceInfo, cfg: &NetConfig) {
    let _ = iface_idx;
    write_str(b"\r\n");
    write_label(IDS_ETHERNET);
    write_str(b" ");
    let mut ib = [0u8; 4];
    let il = fmt_u32(info.nic_id, &mut ib);
    write_str(&ib[..il]);
    write_str(b":\r\n\r\n");

    write_label(IDS_DESCRIPTION);
    write_padded_str(&info.description);
    write_str(b"\r\n");

    write_label(IDS_DRIVER);
    write_padded_str(&info.name);
    write_str(b"\r\n");

    write_label(IDS_PCI_DEVICE);
    let mut hb = [0u8; 10];
    let hl = fmt_hex16(info.vendor_id, &mut hb);
    write_str(&hb[..hl]);
    write_str(b":");
    let hl = fmt_hex16(info.device_id, &mut hb);
    write_str(&hb[..hl]);
    write_str(b"\r\n");

    write_label(IDS_LINK_STATUS);
    if info.link_up != 0 { write_label(IDS_UP); } else { write_label(IDS_DOWN); }
    write_str(b"\r\n\r\n");

    let ip_u32 = u32::from_be_bytes(info.ip);
    let mask = cfg.mask;
    let gw = cfg.gateway;
    let dns1 = cfg.dns[0];
    let dns2 = cfg.dns[1];
    let dns3 = cfg.dns[2];

    write_label(IDS_MAC);
    write_mac(&info.mac);

    write_ip_label(IDS_IPV4, ip_u32);
    write_ip_label(IDS_SUBNET_MASK, if mask != 0 { mask } else { config::DEFAULT_MASK });
    write_ip_label(IDS_GATEWAY, gw);
    if cfg.dhcp_server != 0 { write_ip_label(IDS_DHCP_SERVER, cfg.dhcp_server); }
    if dns1 != 0 { write_ip_label(IDS_DNS, dns1); }
    if dns2 != 0 { write_ip_label(IDS_DNS, dns2); }
    if dns3 != 0 { write_ip_label(IDS_DNS, dns3); }
    write_str(b"\r\n");

    write_label(IDS_DHCP_ENABLED);
    if cfg.dhcp_enabled { write_label(IDS_YES); } else { write_label(IDS_NO); }
    write_str(b"\r\n");

    write_label(IDS_CONFIG_SOURCE);
    if config::is_apipa(ip_u32) { write_label(IDS_AUTOCONFIG); }
    else if cfg.dhcp_bound { write_label(IDS_DHCP); }
    else if ip_u32 != 0 { write_label(IDS_STATIC); }
    else { write_label(IDS_NONE); }
    write_str(b"\r\n");

    if cfg.lease_obtained != 0 {
        write_label(IDS_LEASE_OBTAINED);
        write_datetime(cfg.lease_obtained);
        if cfg.lease_time > 0 {
            write_label(IDS_LEASE_EXPIRES);
            write_datetime(cfg.lease_obtained.saturating_add(cfg.lease_time));
        }
        if cfg.t1_renew != 0 {
            write_label(IDS_RENEW);
            write_datetime(cfg.lease_obtained.saturating_add(cfg.t1_renew));
        }
        if cfg.t2_rebind != 0 {
            write_label(IDS_REBIND);
            write_datetime(cfg.lease_obtained.saturating_add(cfg.t2_rebind));
        }
    }
    let dlen = config::domain_len(&cfg.domain);
    if dlen > 0 {
        write_label(IDS_DOMAIN);
        write_str(&cfg.domain[..dlen]);
        write_str(b"\r\n");
    }

    if cfg.dhcp_bound {
        let lease = cfg.lease_time;
        if lease > 0 {
            write_val_label(IDS_LEASE_TIME, lease, b" s");
        }
    }

    write_str(b"\r\n");
}

fn print_loopback(info: &libnet::NetIfaceInfo) {
    write_str(b"\r\n");
    write_label(IDS_LOOPBACK);
    write_str(b":\r\n\r\n");

    write_label(IDS_DESCRIPTION);
    write_padded_str(&info.description);
    write_str(b"\r\n");

    write_label(IDS_LINK_STATUS);
    write_label(IDS_UP);
    write_str(b"\r\n\r\n");

    write_label(IDS_MAC);
    write_mac(&info.mac);

    let ip_u32 = u32::from_be_bytes(info.ip);
    write_ip_label(IDS_IPV4, ip_u32);
    write_ip_label(IDS_SUBNET_MASK, LOOPBACK_MASK_BE);
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    write_str(b"\r\n");
    write_label(IDS_HEADER);
    write_str(b"\r\n\r\n");

    write_label(IDS_HOSTNAME);
    let mut hn_buf = [0u8; 64];
    match syscall::sys_get_hostname(&mut hn_buf) {
        Ok(n) if n > 0 => {
            let end = hn_buf.iter().position(|&b| b == 0).unwrap_or(n);
            write_str(&hn_buf[..end]);
        }
        _ => {
            write_str(b"NeoDOS-PC");
        }
    }
    write_str(b"\r\n\r\n");

    let cfg = match config::load(0) {
        Some(cfg) => cfg,
        None => {
            write_label(IDS_NO_IFACES);
            write_str(b"\r\n");
            syscall::sys_exit(0);
        }
    };

    let count = libnet::iface_count();
    if count == 0 {
        write_label(IDS_NO_IFACES);
        write_str(b"\r\n");
        syscall::sys_exit(0);
    }

    for i in 0..count {
        let mut info = libnet::NetIfaceInfo {
            nic_id: 0,
            mac: [0u8; 6],
            ip: [0u8; 4],
            link_up: 0,
            vendor_id: 0,
            device_id: 0,
            name: [0u8; 16],
            description: [0u8; 48],
        };
        if libnet::iface_info(i, &mut info) == 0 {
            if info.nic_id == LOOPBACK_NIC_ID {
                print_loopback(&info);
            } else {
                print_iface(i, &info, &cfg);
            }
            print_stats(i);
        }
    }

    syscall::sys_exit(0);
}
