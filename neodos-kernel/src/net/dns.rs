use super::types::Ipv4Addr;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

// ── DNS constants ──

pub const DNS_PORT: u16 = 53;
pub const DNS_MAX_CACHE: usize = 64;
pub const DNS_TICK_INTERVAL: u64 = 10;
pub const DNS_DEFAULT_TTL_SECS: u64 = 300;

// Record types
pub const DNS_TYPE_A: u16 = 1;
pub const DNS_TYPE_AAAA: u16 = 28;
pub const DNS_TYPE_CNAME: u16 = 5;
pub const DNS_TYPE_MX: u16 = 15;

// DNS header flags
pub const DNS_FLAG_QR_QUERY: u16 = 0x0000;
pub const DNS_FLAG_QR_RESPONSE: u16 = 0x8000;
pub const DNS_FLAG_RD: u16 = 0x0100;
pub const DNS_FLAG_RA: u16 = 0x0080;

pub const REG_NET_IFACE_PATH: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";
pub const DNS_SERVER_VALUE: &str = "DnsServer";

// ── Cache types ──

#[derive(Debug, Clone)]
struct DnsCacheEntry {
    name: String,
    ip: Ipv4Addr,
    expiry_tick: u64,
}

pub struct DnsCache {
    entries: Vec<DnsCacheEntry>,
    tick_count: u64,
}

impl DnsCache {
    pub const fn new() -> Self {
        DnsCache {
            entries: Vec::new(),
            tick_count: 0,
        }
    }

    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.tick_count.is_multiple_of(DNS_TICK_INTERVAL) {
            self.evict_expired();
        }
    }

    pub fn lookup(&self, name: &str) -> Option<Ipv4Addr> {
        self.entries.iter().find(|e| e.name == name).map(|e| e.ip)
    }

    pub fn insert(&mut self, name: &str, ip: Ipv4Addr) {
        let expiry = self.tick_count.wrapping_add(DNS_DEFAULT_TTL_SECS * 100 / DNS_TICK_INTERVAL);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == name) {
            entry.ip = ip;
            entry.expiry_tick = expiry;
            return;
        }
        if self.entries.len() >= DNS_MAX_CACHE {
            self.evict_oldest();
        }
        self.entries.push(DnsCacheEntry {
            name: name.to_string(),
            ip,
            expiry_tick: expiry,
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn evict_expired(&mut self) {
        self.entries.retain(|e| {
            // Retain if NOT expired: tick_count < expiry_tick
            // With wrapping: wrapping_sub gives large value (> u64::MAX/2) when tick_count < expiry_tick
            self.tick_count.wrapping_sub(e.expiry_tick) >= u64::MAX / 2
        });
    }

    fn evict_oldest(&mut self) {
        let mut oldest = usize::MAX;
        let mut oldest_tick = u64::MAX;
        for (i, e) in self.entries.iter().enumerate() {
            if e.expiry_tick < oldest_tick {
                oldest = i;
                oldest_tick = e.expiry_tick;
            }
        }
        if oldest != usize::MAX {
            self.entries.remove(oldest);
        }
    }

    pub fn len(&self) -> usize { self.entries.len() }
}

lazy_static! {
    pub static ref DNS_CACHE: Mutex<DnsCache> = Mutex::new(DnsCache::new());
}

// ── DNS wire format helpers ──

/// Encode a domain name into DNS label format (e.g. "www.example.com" → 3www7example3com0)
pub fn encode_dns_name(name: &str) -> Vec<u8> {
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

/// Decode a DNS name from label format at the given offset.
/// Returns (decoded_name, new_offset) — the offset past the name.
pub fn decode_dns_name(data: &[u8], mut offset: usize) -> Result<(String, usize), ()> {
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

        // DNS name compression: pointer in top 2 bits (0xC0)
        if len & 0xC0 == 0xC0 {
            if offset + 1 >= data.len() { return Err(()); }
            let ptr = ((len & 0x3F) << 8) | data[offset + 1] as usize;
            if !jumped {
                end_offset = offset + 2;
                jumped = true;
            }
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

// ── DNS packet structures ──

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DnsHeader {
    pub id: u16,
    pub flags: u16,
    pub qdcount: u16,
    pub ancount: u16,
    pub nscount: u16,
    pub arcount: u16,
}

impl DnsHeader {
    pub fn new(id: u16, is_query: bool) -> Self {
        let flags = if is_query { DNS_FLAG_RD } else { DNS_FLAG_QR_RESPONSE | DNS_FLAG_RA | DNS_FLAG_RD };
        DnsHeader {
            id: id.to_be(),
            flags: flags.to_be(),
            qdcount: 0,
            ancount: 0,
            nscount: 0,
            arcount: 0,
        }
    }

    pub fn id(&self) -> u16 { u16::from_be(self.id) }
    pub fn flags(&self) -> u16 { u16::from_be(self.flags) }
    pub fn is_response(&self) -> bool { self.flags() & DNS_FLAG_QR_RESPONSE != 0 }
    pub fn rcode(&self) -> u8 { (self.flags() & 0x0F) as u8 }
    pub fn qdcount(&self) -> u16 { u16::from_be(self.qdcount) }
    pub fn ancount(&self) -> u16 { u16::from_be(self.ancount) }
    pub fn nscount(&self) -> u16 { u16::from_be(self.nscount) }
    pub fn arcount(&self) -> u16 { u16::from_be(self.arcount) }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DnsQuestion {
    pub qtype: u16,
    pub qclass: u16,
}

impl DnsQuestion {
    pub fn new(qtype: u16) -> Self {
        DnsQuestion {
            qtype: qtype.to_be(),
            qclass: (1u16).to_be(), // IN class
        }
    }
}

#[repr(C, packed)]
pub struct DnsResourceRecord {
    pub name_ptr: u16,     // compression pointer (0xC00C for name offset)
    pub rtype: u16,
    pub rclass: u16,
    pub ttl: u32,
    pub rdlength: u16,
    // rdata follows
}

impl DnsResourceRecord {
    pub fn rtype(&self) -> u16 { u16::from_be(self.rtype) }
    pub fn rclass(&self) -> u16 { u16::from_be(self.rclass) }
    pub fn ttl(&self) -> u32 { u32::from_be(self.ttl) }
    pub fn rdlength(&self) -> u16 { u16::from_be(self.rdlength) }
}

/// Record fields without the name (used after decode_dns_name consumes the name).
#[repr(C, packed)]
pub struct DnsRecordFields {
    pub rtype: u16,
    pub rclass: u16,
    pub ttl: u32,
    pub rdlength: u16,
}

impl DnsRecordFields {
    pub fn rtype(&self) -> u16 { u16::from_be(self.rtype) }
    pub fn rdlength(&self) -> u16 { u16::from_be(self.rdlength) }
    pub fn ttl(&self) -> u32 { u32::from_be(self.ttl) }
}

/// Build a complete DNS query packet.
pub fn build_dns_query(name: &str, qtype: u16, id: u16) -> Vec<u8> {
    let encoded_name = encode_dns_name(name);
    let question = DnsQuestion::new(qtype);

    let mut header = DnsHeader::new(id, true);
    header.qdcount = (1u16).to_be();
    header.ancount = 0u16.to_be();

    let mut packet = Vec::with_capacity(
        core::mem::size_of::<DnsHeader>() + encoded_name.len() + core::mem::size_of::<DnsQuestion>(),
    );

    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &header as *const DnsHeader as *const u8,
            core::mem::size_of::<DnsHeader>(),
        )
    };
    packet.extend_from_slice(hdr_bytes);
    packet.extend_from_slice(&encoded_name);

    let q_bytes = unsafe {
        core::slice::from_raw_parts(
            &question as *const DnsQuestion as *const u8,
            core::mem::size_of::<DnsQuestion>(),
        )
    };
    packet.extend_from_slice(q_bytes);

    packet
}

/// Parsed answer from a DNS response.
#[derive(Debug, Clone)]
pub enum DnsRecord {
    A { name: String, addr: Ipv4Addr, ttl: u32 },
    Aaaa { name: String, addr: [u8; 16], ttl: u32 },
    Cname { name: String, cname: String, ttl: u32 },
    Mx { name: String, preference: u16, exchange: String, ttl: u32 },
    Unknown { name: String, rtype: u16, ttl: u32 },
}

/// Result of parsing a DNS response.
#[derive(Debug)]
pub struct DnsResponse {
    pub id: u16,
    pub answers: Vec<DnsRecord>,
}

/// Parse a DNS response packet. Returns the list of records in the answer section.
pub fn parse_dns_response(data: &[u8]) -> Result<DnsResponse, ()> {
    if data.len() < core::mem::size_of::<DnsHeader>() {
        return Err(());
    }

    let header: &DnsHeader = unsafe { &*(data.as_ptr() as *const DnsHeader) };
    if !header.is_response() {
        return Err(());
    }
    if header.rcode() != 0 {
        return Err(());
    }

    let mut offset = core::mem::size_of::<DnsHeader>();

    // Skip over the question section
    let qdcount = header.qdcount();
    for _ in 0..qdcount {
        let (_, new_offset) = decode_dns_name(data, offset)?;
        offset = new_offset;
        if offset + core::mem::size_of::<DnsQuestion>() > data.len() { return Err(()); }
        offset += core::mem::size_of::<DnsQuestion>();
    }

    // Parse answer section
    let ancount = header.ancount();
    let mut answers = Vec::with_capacity(ancount as usize);

    for _ in 0..ancount {
        // Parse the name (may be compressed)
        let (name, next) = decode_dns_name(data, offset)?;
        offset = next;

        if offset + 10 > data.len() { return Err(()); } // type(2)+class(2)+ttl(4)+rdlength(2)

        let rr: &DnsRecordFields = unsafe { &*(data[offset..].as_ptr() as *const DnsRecordFields) };
        offset += 10;

        let rdlength = rr.rdlength() as usize;
        if offset + rdlength > data.len() { return Err(()); }

        let rtype = rr.rtype();
        let ttl = rr.ttl();

        let record = match rtype {
            DNS_TYPE_A => {
                if rdlength >= 4 {
                    DnsRecord::A {
                        name,
                        addr: Ipv4Addr::new([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]),
                        ttl,
                    }
                } else {
                    DnsRecord::Unknown { name, rtype, ttl }
                }
            }
            DNS_TYPE_AAAA => {
                if rdlength >= 16 {
                    let mut addr = [0u8; 16];
                    addr.copy_from_slice(&data[offset..offset + 16]);
                    DnsRecord::Aaaa { name, addr, ttl }
                } else {
                    DnsRecord::Unknown { name, rtype, ttl }
                }
            }
            DNS_TYPE_CNAME => {
                let (cname, _) = decode_dns_name(data, offset)?;
                DnsRecord::Cname { name, cname, ttl }
            }
            DNS_TYPE_MX => {
                if rdlength >= 2 {
                    let preference = u16::from_be_bytes([data[offset], data[offset + 1]]);
                    let (exchange, _) = decode_dns_name(data, offset + 2)?;
                    DnsRecord::Mx { name, preference, exchange, ttl }
                } else {
                    DnsRecord::Unknown { name, rtype, ttl }
                }
            }
            _ => DnsRecord::Unknown { name, rtype, ttl },
        };

        answers.push(record);
        offset += rdlength;
    }

    Ok(DnsResponse {
        id: header.id(),
        answers,
    })
}

/// Extract the first A record IPv4 address from a DNS response,
/// following CNAME chains if necessary.
pub fn resolve_from_response(response: &DnsResponse) -> Option<Ipv4Addr> {
    // First pass: look for an A record
    for record in &response.answers {
        if let DnsRecord::A { addr, .. } = *record {
            return Some(addr);
        }
    }
    None
}

/// Follow a CNAME chain and find the terminal A record.
pub fn resolve_cname_chain(response: &DnsResponse) -> Option<Ipv4Addr> {
    // Collect CNAME targets
    let mut seen = alloc::vec::Vec::new();
    let mut current = None;

    for record in &response.answers {
        if let DnsRecord::Cname { ref name, .. } = *record {
            current = Some(name.clone());
            break;
        }
    }

    // If no CNAME, just try direct A record
    if current.is_none() {
        return resolve_from_response(response);
    }

    // Follow the chain (limit to 10 hops to prevent loops)
    for _ in 0..10 {
        let target = match &current {
            Some(t) => t.clone(),
            None => break,
        };

        if seen.contains(&target) {
            break;
        }
        seen.push(target.clone());

        // Look for A record matching this name
        for record in &response.answers {
            match record {
                DnsRecord::A { ref name, addr, .. } if *name == target => {
                    return Some(*addr);
                }
                DnsRecord::Cname { ref name, ref cname, .. } if *name == target => {
                    current = Some(cname.clone());
                    break;
                }
                _ => {}
            }
        }
    }

    // Fallback: direct A record
    resolve_from_response(response)
}

// ── Stub resolver ──

/// Global next DNS query ID
static NEXT_DNS_ID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(1);

/// Resolve a hostname to an IPv4 address.
///
/// Checks the local cache first. On miss, queries the configured DNS server
/// via UDP and caches the result.
pub fn dns_resolve(name: &str) -> Option<Ipv4Addr> {
    // Handle localhost directly
    if name.eq_ignore_ascii_case("localhost") {
        return Some(Ipv4Addr::localhost());
    }

    // Check cache
    {
        let cache = DNS_CACHE.lock();
        if let Some(ip) = cache.lookup(name) {
            crate::serial_println!("[DNS] Cache hit: {} -> {}", name, ip);
            return Some(ip);
        }
    }

    // Read DNS server from Registry (or use default)
    let dns_server = dns_read_server_from_registry()
        .unwrap_or(Ipv4Addr::new([8, 8, 8, 8]));

    // For now, in the kernel stub resolver, we build and send the query
    // but actual network response handling requires a kernel socket binding.
    // The full resolution is done via dns_resolve_with_server.
    dns_resolve_with_server(name, dns_server)
}

/// Resolve a hostname using a specific DNS server.
pub fn dns_resolve_with_server(name: &str, server: Ipv4Addr) -> Option<Ipv4Addr> {
    // Build A record query
    let query = build_dns_query(name, DNS_TYPE_A, NEXT_DNS_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed));

    // For the kernel DNS resolver, we need to set up a proper UDP socket
    // to receive the response. This requires socket_alloc + socket_bind + socket_connect.
    let sock_id = crate::net::socket::socket_alloc(super::types::SocketType::Udp)?;

    // Bind to any available port
    let local = super::types::SocketAddrV4::new(Ipv4Addr::unspecified(), 0);
    if !crate::net::socket::socket_bind(sock_id, local) {
        crate::net::socket::socket_free(sock_id);
        return None;
    }

    // Connect to DNS server
    let remote = super::types::SocketAddrV4::new(server, DNS_PORT);
    if !crate::net::socket::socket_connect(sock_id, remote) {
        crate::net::socket::socket_free(sock_id);
        return None;
    }

    // Send DNS query
    if crate::net::socket::socket_send(sock_id, &query).is_err() {
        crate::net::socket::socket_free(sock_id);
        return None;
    }

    // Poll for response (with timeout)
    let mut buf = [0u8; 512];
    let mut response = None;

    for _ in 0..100 {
        crate::net::network_poll_all();

        let result = crate::net::socket::socket_recv(sock_id, &mut buf);
        if let Ok(len) = result {
            response = Some(buf[..len].to_vec());
            break;
        }

        // Busy-wait delay
        for _ in 0..5000 {
            core::hint::spin_loop();
        }
    }

    let result = match response {
        Some(data) => {
            match parse_dns_response(&data) {
                Ok(resp) => {
                    let ip = resolve_cname_chain(&resp);
                    if let Some(ip) = ip {
                        // Cache the result
                        DNS_CACHE.lock().insert(name, ip);
                    }
                    ip
                }
                Err(_) => None,
            }
        }
        None => None,
    };

    crate::net::socket::socket_free(sock_id);

    if result.is_some() {
        crate::serial_println!("[DNS] Resolved {} -> {}", name, result.unwrap());
    } else {
        crate::serial_println!("[DNS] Failed to resolve {}", name);
    }

    result
}

/// Read the DNS server address from the Registry.
pub fn dns_read_server_from_registry() -> Option<Ipv4Addr> {
    let key = crate::cm::api::cm_open_key(0, REG_NET_IFACE_PATH).ok()?;
    let value = crate::cm::api::cm_query_value(key, DNS_SERVER_VALUE).ok()?;

    // REG_DWORD: IP stored as u32 in little-endian
    if value.value_type == crate::cm::hive::types::REG_DWORD && value.data.len() >= 4 {
        let ip_u32 = u32::from_le_bytes([value.data[0], value.data[1], value.data[2], value.data[3]]);
        return Some(Ipv4Addr::new([
            (ip_u32 >> 24) as u8,
            (ip_u32 >> 16) as u8,
            (ip_u32 >> 8) as u8,
            ip_u32 as u8,
        ]));
    }

    // REG_BINARY or REG_NONE with 4 bytes: raw IP
    if value.data.len() == 4 {
        return Some(Ipv4Addr::new([value.data[0], value.data[1], value.data[2], value.data[3]]));
    }

    // REG_SZ: dotted-decimal string
    if value.value_type == crate::cm::hive::types::REG_SZ {
        if let Ok(s) = core::str::from_utf8(&value.data) {
            let trimmed = s.trim().trim_matches('\0').trim();
            if let Some(ip) = parse_dotted_ip(trimmed) {
                return Some(ip);
            }
        }
    }

    None
}

/// Parse a dotted-decimal IP address string (e.g. "8.8.8.8").
pub fn parse_dotted_ip(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 { return None; }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = part.parse::<u8>().ok()?;
    }
    Some(Ipv4Addr::new(octets))
}

/// Periodic maintenance: evict expired cache entries.
pub fn dns_tick() {
    DNS_CACHE.lock().tick();
}

// ── Test helpers (used by tests.rs) ──

/// Build a synthetic DNS A-record response for testing.
pub fn test_make_a_response(name: &str, ip: Ipv4Addr, id: u16, ttl: u32) -> Vec<u8> {
    let encoded = encode_dns_name(name);

    let mut header = DnsHeader::new(id, false);
    header.qdcount = (1u16).to_be();
    header.ancount = (1u16).to_be();

    let question = DnsQuestion::new(DNS_TYPE_A);

    let mut packet = Vec::new();

    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &header as *const DnsHeader as *const u8,
            core::mem::size_of::<DnsHeader>(),
        )
    };
    packet.extend_from_slice(hdr_bytes);
    packet.extend_from_slice(&encoded);
    let q_bytes = unsafe {
        core::slice::from_raw_parts(
            &question as *const DnsQuestion as *const u8,
            core::mem::size_of::<DnsQuestion>(),
        )
    };
    packet.extend_from_slice(q_bytes);

    // Write answer section — use to_be_bytes() directly (no intermediate .to_be())
    packet.extend_from_slice(&0xC00Cu16.to_be_bytes());  // name pointer to offset 12
    packet.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&ttl.to_be_bytes());
    packet.extend_from_slice(&4u16.to_be_bytes());
    packet.extend_from_slice(&ip.0);

    packet
}

/// Build a synthetic DNS CNAME+A response chain for testing.
pub fn test_make_cname_a_response(alias: &str, canonical: &str, ip: Ipv4Addr, id: u16, ttl: u32) -> Vec<u8> {
    let encoded_alias = encode_dns_name(alias);
    let cname_encoded = encode_dns_name(canonical);

    let mut header = DnsHeader::new(id, false);
    header.qdcount = (1u16).to_be();
    header.ancount = (2u16).to_be();

    let question = DnsQuestion::new(DNS_TYPE_A);

    let mut packet = Vec::new();

    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &header as *const DnsHeader as *const u8,
            core::mem::size_of::<DnsHeader>(),
        )
    };
    packet.extend_from_slice(hdr_bytes);
    packet.extend_from_slice(&encoded_alias);
    let q_bytes = unsafe {
        core::slice::from_raw_parts(
            &question as *const DnsQuestion as *const u8,
            core::mem::size_of::<DnsQuestion>(),
        )
    };
    packet.extend_from_slice(q_bytes);

    // Answer 1: CNAME — use to_be_bytes() directly, no intermediate .to_be()
    let cname_rdlength = cname_encoded.len() as u16;
    packet.extend_from_slice(&0xC00Cu16.to_be_bytes());
    packet.extend_from_slice(&DNS_TYPE_CNAME.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&ttl.to_be_bytes());
    packet.extend_from_slice(&cname_rdlength.to_be_bytes());
    packet.extend_from_slice(&cname_encoded);

    // Answer 2: A record with compressed name pointing to CNAME rdata
    let cname_answer_start = core::mem::size_of::<DnsHeader>() + encoded_alias.len() + 4;
    let cname_rdata_offset = cname_answer_start + 10;
    let cname_target_ptr: u16 = 0xC000 | (cname_rdata_offset as u16);

    packet.extend_from_slice(&cname_target_ptr.to_be_bytes());
    packet.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&ttl.to_be_bytes());
    packet.extend_from_slice(&4u16.to_be_bytes());
    packet.extend_from_slice(&ip.0);

    packet
}
