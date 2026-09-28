//! Userland DNS resolver.
//!
//! The wire format and resolver core live in the shared `libdns` crate. This
//! module adds what is specific to the userland environment:
//!
//! * the UDP socket transport (`net.nxl`, via `NetTransport`),
//! * DNS server configuration read from the Registry (`DnsServer`, `DnsServer2`,
//!   `DnsServer3`),
//! * a small bounded answer cache.
//!
//! `nslookup` and `ping` both resolve names through this API; no tool implements
//! its own DNS client. See `docs/networking/userland.md`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use libdns::{is_localhost, is_unspecified, validate_hostname};

pub use libdns::{
    build_query, decode_name, encode_name, parse_dotted_ip, parse_response, query_server,
    query_server_with_retries, resolve_with_servers, DnsAnswer, DnsError, DnsTransport,
    DNS_CLASS_IN, DNS_MAX_CNAME_HOPS, DNS_PORT, DNS_TYPE_A, DNS_TYPE_CNAME,
};

/// Maximum number of configured DNS servers consulted (server 1..N).
pub const DNS_MAX_SERVERS: usize = 3;
/// Bounded resolver cache entries.
pub const DNS_CACHE_SIZE: usize = 16;
/// Extra retries per server (total attempts = DNS_MAX_RETRIES + 1).
pub const DNS_MAX_RETRIES: usize = 1;
/// How many `socket_recv` + `sys_yield` polls before a server times out.
pub const DNS_RECV_POLLS: usize = 200;
/// RDTSC budget for waiting for a DNS reply (roughly 0.7–2 s depending on TSC).
pub const DNS_RECV_TICKS: u64 = 2_000_000_000;
/// Safety cap on the receive wait loop iterations.
pub const DNS_RECV_MAX_SPINS: u32 = 2_000_000;
/// How many times to retry the UDP send while waiting for ARP resolution.
pub const DNS_SEND_ATTEMPTS: usize = 50;

const REG_NET_PATH: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";

/// Registry value names holding DNS servers, in preference order.
const DNS_VALUE_NAMES: [&str; DNS_MAX_SERVERS] = ["DnsServer", "DnsServer2", "DnsServer3"];

// ── Result ──

/// A successful resolution.
#[derive(Debug, Clone)]
pub struct DnsResult {
    /// The DNS server that produced the answer. `[0, 0, 0, 0]` means the answer
    /// was synthesized locally (e.g. `localhost`) and no server was queried.
    pub server: [u8; 4],
    /// All A records for the resolved name, in response order.
    pub addresses: Vec<[u8; 4]>,
    /// Smallest TTL among the returned A records (seconds; 0 if unknown).
    pub ttl: u32,
}

impl DnsResult {
    pub fn first(&self) -> Option<[u8; 4]> {
        self.addresses.first().copied()
    }
}

// ── Transport ──

/// UDP transport backed by the kernel sockets exposed through `net.nxl`.
pub struct NetTransport;

impl DnsTransport for NetTransport {
    fn exchange(
        &mut self,
        server: [u8; 4],
        query: &[u8],
        resp: &mut [u8],
    ) -> Result<usize, DnsError> {
        let table = crate::get_table().ok_or(DnsError::Network)?;

        // SocketType::Udp is encoded as 2 in the Ob attrs (see net.nxl).
        let fd = (table.socket_create)(2);
        if fd < 0 {
            return Err(DnsError::Network);
        }

        if (table.socket_bind)(fd, 0, 0) < 0 {
            let _ = (table.socket_close)(fd);
            return Err(DnsError::ServerUnreachable);
        }

        let server_ip = u32::from_be_bytes(server);
        if (table.socket_connect)(fd, server_ip, DNS_PORT) < 0 {
            let _ = (table.socket_close)(fd);
            return Err(DnsError::ServerUnreachable);
        }

        // `arp_resolve` is fire-and-forget: on a cache miss it transmits an ARP
        // request and returns None immediately. The reply is processed later by
        // netd. Yield and retry the send so the ARP cache can be populated
        // before we give up (otherwise the first datagram to a new peer always
        // fails).
        let mut sent = -1i32;
        for _ in 0..DNS_SEND_ATTEMPTS {
            sent = unsafe { (table.socket_send)(fd, query.as_ptr(), query.len() as u32) };
            if sent >= 0 {
                break;
            }
            libneodos::syscall::sys_yield();
            for _ in 0..2000 {
                core::hint::spin_loop();
            }
        }
        if sent < 0 {
            let _ = (table.socket_close)(fd);
            return Err(DnsError::ServerUnreachable);
        }

        // netd drives packet polling; we must wait for the reply here. A bare
        // `sys_yield` loop returns too early (yield does not sleep), so pace the
        // wait with a real time budget using RDTSC, mirroring the kernel's
        // ICMP/ARP timeouts.
        let start = unsafe { core::arch::x86_64::_rdtsc() };
        let mut received = 0usize;
        let mut spins: u32 = 0;
        loop {
            let n = unsafe { (table.socket_recv)(fd, resp.as_mut_ptr(), resp.len() as u32) };
            if n > 0 {
                received = n as usize;
                break;
            }
            spins += 1;
            if spins > DNS_RECV_MAX_SPINS {
                break;
            }
            if unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(start) > DNS_RECV_TICKS {
                break;
            }
            libneodos::syscall::sys_yield();
        }

        let _ = (table.socket_close)(fd);

        if received == 0 {
            Err(DnsError::Timeout)
        } else {
            Ok(received)
        }
    }
}

// ── Configuration ──

/// Read the configured DNS servers from the Registry, in preference order.
///
/// Never returns `0.0.0.0`: unspecified entries are treated as "unset" and
/// skipped, so a query is never sent to the unspecified address.
pub fn configured_servers() -> Vec<[u8; 4]> {
    read_servers().unwrap_or_default()
}

fn read_servers() -> Result<Vec<[u8; 4]>, DnsError> {
    let fd = libneodos::sys_cm_open_key(REG_NET_PATH).map_err(|_| DnsError::NoConfig)?;

    let mut servers: Vec<[u8; 4]> = Vec::new();
    for value_name in DNS_VALUE_NAMES {
        if let Some(ip) = read_server_value(fd, value_name) {
            if !is_unspecified(ip) && !servers.contains(&ip) {
                servers.push(ip);
            }
        }
    }

    let _ = libneodos::sys_close(fd);
    Ok(servers)
}

fn read_server_value(fd: u8, name: &str) -> Option<[u8; 4]> {
    let mut buf = [0u8; 64];
    let total = libneodos::sys_cm_query_value(fd, name, &mut buf).ok()?;
    if total < 8 {
        return None;
    }

    let value_type = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let data_len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    let available = total.saturating_sub(8).min(buf.len() - 8);
    let data_len = data_len.min(available);
    let data = &buf[8..8 + data_len];

    match value_type {
        // REG_DWORD: IP stored as a big-endian value in a little-endian DWORD.
        2 if data_len >= 4 => {
            let ip_u32 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            Some(ip_u32.to_be_bytes())
        }
        // REG_BINARY / REG_NONE: raw 4 bytes.
        _ if data_len >= 4 => Some([data[0], data[1], data[2], data[3]]),
        // REG_SZ: dotted-decimal string.
        1 => {
            let s = core::str::from_utf8(data).ok()?;
            parse_dotted_ip(s.trim().trim_matches('\0'))
        }
        _ => None,
    }
}

// ── Resolver ──

fn localhost_result() -> DnsResult {
    DnsResult {
        server: [0, 0, 0, 0],
        addresses: alloc::vec![[127, 0, 0, 1]],
        ttl: 0,
    }
}

/// Resolve `hostname` using the configured DNS servers, in preference order.
///
/// `localhost` resolves locally. `NXDOMAIN` from any server is returned
/// immediately. Other per-server failures fall through to the next server; if
/// all servers fail, the last error is returned.
pub fn resolve(hostname: &str) -> Result<DnsResult, DnsError> {
    validate_hostname(hostname)?;

    if is_localhost(hostname) {
        return Ok(localhost_result());
    }

    let servers = read_servers()?;
    if servers.is_empty() {
        return Err(DnsError::NoServer);
    }

    // Cache hit for any configured server?
    for &server in &servers {
        if let Some(addr) = cache_lookup(hostname, server) {
            return Ok(DnsResult {
                server,
                addresses: alloc::vec![addr],
                ttl: 0,
            });
        }
    }

    let mut transport = NetTransport;
    let (answer, server) =
        resolve_with_servers(&mut transport, hostname, &servers, DNS_MAX_RETRIES)?;
    cache_answer(hostname, server, &answer);

    Ok(DnsResult {
        server,
        addresses: answer.addresses,
        ttl: answer.ttl,
    })
}

/// Resolve `hostname` using an explicit DNS server.
///
/// The unspecified address (`0.0.0.0`) is rejected with [`DnsError::NoServer`];
/// a query is never sent to it.
pub fn resolve_with_server(hostname: &str, server: [u8; 4]) -> Result<DnsResult, DnsError> {
    validate_hostname(hostname)?;

    if is_unspecified(server) {
        return Err(DnsError::NoServer);
    }

    if is_localhost(hostname) {
        return Ok(localhost_result());
    }

    if let Some(addr) = cache_lookup(hostname, server) {
        return Ok(DnsResult {
            server,
            addresses: alloc::vec![addr],
            ttl: 0,
        });
    }

    let mut transport = NetTransport;
    let (answer, _) = resolve_with_servers(&mut transport, hostname, &[server], DNS_MAX_RETRIES)?;
    cache_answer(hostname, server, &answer);

    Ok(DnsResult {
        server,
        addresses: answer.addresses,
        ttl: answer.ttl,
    })
}

// ── Cache ──

struct CacheEntry {
    host: String,
    server: [u8; 4],
    addr: [u8; 4],
}

static mut CACHE: [Option<CacheEntry>; DNS_CACHE_SIZE] = [const { None }; DNS_CACHE_SIZE];

fn cache_lookup(host: &str, server: [u8; 4]) -> Option<[u8; 4]> {
    // SAFETY: userland tools are single-threaded and the resolver is not reentrant.
    unsafe {
        for entry in CACHE.iter() {
            if let Some(e) = entry {
                if e.host == host && e.server == server {
                    return Some(e.addr);
                }
            }
        }
    }
    None
}

fn cache_answer(host: &str, server: [u8; 4], answer: &DnsAnswer) {
    if let Some(addr) = answer.first() {
        cache_insert(host, server, addr);
    }
}

fn cache_insert(host: &str, server: [u8; 4], addr: [u8; 4]) {
    // SAFETY: see `cache_lookup`.
    unsafe {
        for entry in CACHE.iter_mut() {
            if let Some(e) = entry {
                if e.host == host && e.server == server {
                    e.addr = addr;
                    return;
                }
            }
        }
        for entry in CACHE.iter_mut() {
            if entry.is_none() {
                *entry = Some(CacheEntry {
                    host: host.to_string(),
                    server,
                    addr,
                });
                return;
            }
        }
    }
}

/// Drop all cached resolver entries.
pub fn clear_cache() {
    // SAFETY: see `cache_lookup`.
    unsafe {
        for entry in CACHE.iter_mut() {
            *entry = None;
        }
    }
}
