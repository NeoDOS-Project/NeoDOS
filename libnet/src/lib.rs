#![no_std]

use core::sync::atomic::{AtomicU64, Ordering};
use libneodos::loadlib;

const NET_NXL_PATH: &str = "C:\\System\\Libraries\\net.nxl\0";
const EXPORT_TABLE_OFFSET: u64 = 0x00;

static NET_BASE: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
pub struct NetIfaceInfo {
    pub nic_id: u32,
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub link_up: u8,
}

#[repr(C)]
pub struct NetIfaceStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u32,
    pub tx_errors: u32,
}

#[repr(C)]
pub struct NetAbiTable {
    pub version: u32,
    pub iface_count: extern "C" fn() -> u32,
    pub iface_info: unsafe extern "C" fn(u32, *mut NetIfaceInfo) -> i32,
    pub iface_stats: extern "C" fn(u32, *mut NetIfaceStats) -> i32,
    pub socket_create: extern "C" fn(u32) -> i32,
    pub socket_bind: extern "C" fn(i32, u32, u16) -> i32,
    pub socket_connect: extern "C" fn(i32, u32, u16) -> i32,
    pub socket_listen: extern "C" fn(i32) -> i32,
    pub socket_send: unsafe extern "C" fn(i32, *const u8, u32) -> i32,
    pub socket_recv: unsafe extern "C" fn(i32, *mut u8, u32) -> i32,
    pub socket_close: extern "C" fn(i32) -> i32,
    pub set_ip: extern "C" fn(u32, u32, u32) -> i32,
    pub set_gateway: extern "C" fn(u32, u32) -> i32,
    pub get_ip: extern "C" fn(u32) -> u32,
    pub get_gateway: extern "C" fn(u32) -> u32,
    pub get_mask: extern "C" fn(u32) -> u32,
    pub get_dhcp_bound: extern "C" fn() -> i32,
    pub get_dns: extern "C" fn(u32) -> u32,
    pub get_dhcp_enabled: extern "C" fn(u32) -> i32,
    pub get_lease_seconds: extern "C" fn(u32) -> u32,
    pub get_hostname: extern "C" fn(u32) -> u32,
    _reserved: [u64; 3],
}

fn get_table() -> Option<&'static NetAbiTable> {
    let base = NET_BASE.load(Ordering::Relaxed);
    if base != 0 {
        return Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const NetAbiTable) });
    }
    match loadlib(NET_NXL_PATH) {
        Ok(base) => {
            NET_BASE.store(base, Ordering::Relaxed);
            Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const NetAbiTable) })
        }
        Err(_) => None,
    }
}

pub fn is_loaded() -> bool {
    NET_BASE.load(Ordering::Relaxed) != 0
}

pub fn iface_count() -> u32 {
    match get_table() {
        Some(t) => (t.iface_count)(),
        None => 0,
    }
}

pub fn iface_info(idx: u32, info: &mut NetIfaceInfo) -> i32 {
    match get_table() {
        Some(t) => unsafe { (t.iface_info)(idx, info as *mut NetIfaceInfo) },
        None => -1,
    }
}

pub fn iface_stats(idx: u32, stats: &mut NetIfaceStats) -> i32 {
    match get_table() {
        Some(t) => (t.iface_stats)(idx, stats as *mut NetIfaceStats),
        None => -1,
    }
}

pub fn socket_create(sock_type: u32) -> i32 {
    match get_table() {
        Some(t) => (t.socket_create)(sock_type),
        None => -1,
    }
}

pub fn socket_bind(fd: i32, ip: u32, port: u16) -> i32 {
    match get_table() {
        Some(t) => (t.socket_bind)(fd, ip, port),
        None => -1,
    }
}

pub fn socket_connect(fd: i32, ip: u32, port: u16) -> i32 {
    match get_table() {
        Some(t) => (t.socket_connect)(fd, ip, port),
        None => -1,
    }
}

pub fn socket_listen(fd: i32) -> i32 {
    match get_table() {
        Some(t) => (t.socket_listen)(fd),
        None => -1,
    }
}

pub fn socket_send(fd: i32, data: &[u8]) -> i32 {
    match get_table() {
        Some(t) => unsafe { (t.socket_send)(fd, data.as_ptr(), data.len() as u32) },
        None => -1,
    }
}

pub fn socket_recv(fd: i32, buf: &mut [u8]) -> i32 {
    match get_table() {
        Some(t) => unsafe { (t.socket_recv)(fd, buf.as_mut_ptr(), buf.len() as u32) },
        None => -1,
    }
}

pub fn socket_close(fd: i32) -> i32 {
    match get_table() {
        Some(t) => (t.socket_close)(fd),
        None => -1,
    }
}

pub fn set_ip(iface: u32, ip: u32, mask: u32) -> i32 {
    match get_table() {
        Some(t) => (t.set_ip)(iface, ip, mask),
        None => -1,
    }
}

pub fn set_gateway(iface: u32, gw: u32) -> i32 {
    match get_table() {
        Some(t) => (t.set_gateway)(iface, gw),
        None => -1,
    }
}

pub fn get_ip(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_ip)(iface),
        None => 0,
    }
}

pub fn get_gateway(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_gateway)(iface),
        None => 0,
    }
}

pub fn get_mask(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_mask)(iface),
        None => 0,
    }
}

pub fn get_dhcp_bound() -> i32 {
    match get_table() {
        Some(t) => (t.get_dhcp_bound)(),
        None => -1,
    }
}

pub fn get_dns(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_dns)(iface),
        None => 0,
    }
}

pub fn get_dhcp_enabled(iface: u32) -> i32 {
    match get_table() {
        Some(t) => (t.get_dhcp_enabled)(iface),
        None => 0,
    }
}

pub fn get_lease_seconds(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_lease_seconds)(iface),
        None => 0,
    }
}

pub fn get_hostname(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_hostname)(iface),
        None => 0,
    }
}

// ── DNS resolution ──

const DNS_PORT: u16 = 53;
const DNS_TYPE_A: u16 = 1;
const DNS_TYPE_CNAME: u16 = 5;

const REG_NET_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";
const DNS_SERVER_VALUE: &str = "DnsServer";

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;

/// Encode a domain name into DNS label format.
fn dns_encode_name(name: &str) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(name.len() + 2);
    for label in name.split('.') {
        if !label.is_empty() {
            encoded.push(label.len() as u8);
            encoded.extend_from_slice(label.as_bytes());
        }
    }
    encoded.push(0);
    encoded
}

/// Decode a DNS name from label format, supporting compression pointers.
fn dns_decode_name(data: &[u8], mut offset: usize) -> Result<(String, usize), ()> {
    let mut labels = Vec::new();
    let mut jumped = false;
    let mut end_offset = offset;

    loop {
        if offset >= data.len() { return Err(()); }
        let len = data[offset] as usize;

        if len == 0 {
            offset += 1;
            if !jumped { end_offset = offset; }
            break;
        }

        if len & 0xC0 == 0xC0 {
            if offset + 1 >= data.len() { return Err(()); }
            let ptr = ((len & 0x3F) << 8) | data[offset + 1] as usize;
            if !jumped { end_offset = offset + 2; jumped = true; }
            offset = ptr;
            continue;
        }

        if offset + 1 + len > data.len() { return Err(()); }
        let label = core::str::from_utf8(&data[offset + 1..offset + 1 + len]).map_err(|_| ())?;
        labels.push(label);
        offset += 1 + len;
    }

    Ok((labels.join("."), end_offset))
}

/// Build a DNS A-record query packet.
fn dns_build_query(name: &str, id: u16) -> Vec<u8> {
    let encoded = dns_encode_name(name);
    let mut pkt = Vec::with_capacity(12 + encoded.len() + 4);

    // Header
    pkt.extend_from_slice(&id.to_be_bytes());
    pkt.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: RD
    pkt.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ancount
    pkt.extend_from_slice(&0u16.to_be_bytes()); // nscount
    pkt.extend_from_slice(&0u16.to_be_bytes()); // arcount

    // Question
    pkt.extend_from_slice(&encoded);
    pkt.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
    pkt.extend_from_slice(&1u16.to_be_bytes()); // class IN

    pkt
}

/// Read the DNS server IP from Registry, or return default 8.8.8.8.
fn dns_get_server() -> [u8; 4] {
    let key = match libneodos::sys_cm_open_key(REG_NET_PATH) {
        Ok(fd) => fd,
        Err(_) => return [8, 8, 8, 8],
    };

    let mut buf = [0u8; 64];
    let res = libneodos::sys_cm_query_value(key, DNS_SERVER_VALUE, &mut buf);
    let _ = libneodos::sys_close(key);

    match res {
        Ok(total) if total >= 8 => {
            // buf[0..4] = type (u32 LE), buf[4..8] = data_len (u32 LE), buf[8..] = data
            let data_len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
            let value_type = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            let data_start = 8;
            let available = total - data_start;
            let data_end = data_start + data_len.min(available).min(buf.len() - data_start);

            if value_type == 2 && data_len >= 4 {
                // REG_DWORD
                let ip_u32 = u32::from_le_bytes([buf[data_start], buf[data_start + 1], buf[data_start + 2], buf[data_start + 3]]);
                return [(ip_u32 >> 24) as u8, (ip_u32 >> 16) as u8, (ip_u32 >> 8) as u8, ip_u32 as u8];
            }

            if data_len >= 4 && data_end >= data_start + 4 {
                return [buf[data_start], buf[data_start + 1], buf[data_start + 2], buf[data_start + 3]];
            }

            if value_type == 1 && data_end > data_start {
                // REG_SZ: parse dotted decimal
                let s = core::str::from_utf8(&buf[data_start..data_end]).unwrap_or("");
                let trimmed = s.trim().trim_matches('\0');
                if let Some(ip) = parse_dotted_ip(trimmed) {
                    return ip;
                }
            }
            [8, 8, 8, 8]
        }
        _ => [8, 8, 8, 8],
    }
}

fn parse_dotted_ip(s: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 { return None; }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = part.parse::<u8>().ok()?;
    }
    Some(octets)
}

/// Parse a DNS response and extract the first A record IP address,
/// following CNAME chains if necessary.
fn parse_a_record_response(data: &[u8]) -> Option<[u8; 4]> {
    if data.len() < 12 { return None; }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 { return None; } // not a response
    if flags & 0x0F != 0 { return None; } // rcode != 0

    let ancount = u16::from_be_bytes([data[6], data[7]]);
    if ancount == 0 { return None; }

    let mut offset = 12usize;

    // Skip question section
    let qdcount = u16::from_be_bytes([data[4], data[5]]);
    for _ in 0..qdcount {
        let (_, new_off) = dns_decode_name(data, offset).ok()?;
        offset = new_off + 4; // qtype(2) + qclass(2)
    }

    // Parse answer section
    for _ in 0..ancount {
        let (_, new_off) = dns_decode_name(data, offset).ok()?;
        offset = new_off;

        if offset + 10 > data.len() { return None; }
        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let _rclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        offset += 8; // skip type(2) + class(2) + ttl(4)
        let rdlength = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;

        if offset + rdlength > data.len() { return None; }

        if rtype == DNS_TYPE_A && rdlength >= 4 {
            return Some([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
        }

        // For CNAME, skip rdata but continue looking for A record
        offset += rdlength;
    }

    None
}

/// Resolve a hostname to an IPv4 address.
///
/// Queries the configured DNS server (from Registry or 8.8.8.8) via UDP.
/// Returns `Some([a, b, c, d])` on success, `None` on failure.
pub fn dns_resolve(hostname: &str) -> Option<[u8; 4]> {
    let table = get_table()?;

    let dns_server = dns_get_server();
    let dns_ip_u32 = u32::from_be_bytes(dns_server);

    // Create UDP socket
    let fd = (table.socket_create)(2); // 2 = UDP
    if fd < 0 { return None; }

    // Bind to any available port
    let bind_result = (table.socket_bind)(fd, 0, 0);
    if bind_result < 0 {
        let _ = (table.socket_close)(fd);
        return None;
    }

    // Connect to DNS server:53
    let connect_result = (table.socket_connect)(fd, dns_ip_u32, DNS_PORT);
    if connect_result < 0 {
        let _ = (table.socket_close)(fd);
        return None;
    }

    let query_id: u16 = 1;
    let query = dns_build_query(hostname, query_id);

    // Send query
    let send_result = unsafe { (table.socket_send)(fd, query.as_ptr(), query.len() as u32) };
    if send_result < 0 {
        let _ = (table.socket_close)(fd);
        return None;
    }

    // Receive response with polling
    let mut buf = [0u8; 512];
    let mut result = None;

    for _ in 0..50 {
        let recv_result = unsafe { (table.socket_recv)(fd, buf.as_mut_ptr(), buf.len() as u32) };
        if recv_result > 0 {
            let len = recv_result as usize;
            result = parse_a_record_response(&buf[..len]);
            break;
        }
    }

    let _ = (table.socket_close)(fd);
    result
}
