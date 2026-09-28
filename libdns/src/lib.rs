//! Shared DNS wire format and resolver core for NeoDOS.
//!
//! This crate contains the protocol logic (RFC 1035 A-record resolution) with
//! no kernel or syscall dependency, so it can be unit tested on the host. The
//! `libnet` userland library provides the transport (UDP sockets via
//! `net.nxl`) and the Registry-backed configuration on top of it.
//!
//! Only A / IPv4 resolution is implemented (plus CNAME following).
//!
//! When built for NeoDOS it is `no_std`; under `cargo test` it uses the host
//! `std` test harness.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ── Protocol constants ──

pub const DNS_PORT: u16 = 53;
pub const DNS_TYPE_A: u16 = 1;
pub const DNS_TYPE_CNAME: u16 = 5;
pub const DNS_CLASS_IN: u16 = 1;

/// CNAME chain hop limit, to protect against loops.
pub const DNS_MAX_CNAME_HOPS: usize = 10;

// ── Errors ──

/// Distinct DNS failures. Callers present these individually rather than
/// collapsing everything into a generic "DNS failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    /// The network configuration could not be read (Registry unavailable).
    NoConfig,
    /// No DNS server is configured.
    NoServer,
    /// The hostname is malformed (empty label, illegal character, too long, ...).
    InvalidHostname,
    /// No response arrived before the deadline.
    Timeout,
    /// The server could not be reached (socket setup/send failed).
    ServerUnreachable,
    /// The server answered with RCODE=3 (NXDOMAIN).
    NxDomain,
    /// The response was not a well-formed DNS reply.
    MalformedResponse,
    /// The response had the TC (truncated) bit set — TCP fallback is unsupported.
    Truncated,
    /// The server answered with a non-zero RCODE other than NXDOMAIN.
    ServerFailure,
    /// The response was valid but contained no A record for the name.
    NoARecord,
    /// The network transport (net.nxl / sockets) is unavailable.
    Network,
}

// ── Results ──

/// Result of parsing a single response (one server, one query).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsAnswer {
    pub addresses: Vec<[u8; 4]>,
    pub ttl: u32,
}

impl DnsAnswer {
    pub fn first(&self) -> Option<[u8; 4]> {
        self.addresses.first().copied()
    }
}

// ── Transport abstraction ──

/// DNS datagram transport. Abstracting this lets the resolver be unit tested
/// with synthetic responses and no network access.
pub trait DnsTransport {
    /// Send `query` to `server:53` and copy the reply into `resp`.
    /// Returns the reply length, or a [`DnsError`] on failure.
    fn exchange(
        &mut self,
        server: [u8; 4],
        query: &[u8],
        resp: &mut [u8],
    ) -> Result<usize, DnsError>;
}

// ── Wire format ──

/// Encode a domain name into DNS label format (e.g. `www.example.com` →
/// `3www7example3com0`). Empty labels (trailing dot / repeated dots) are
/// skipped.
pub fn encode_name(name: &str) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(name.len() + 2);
    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        encoded.push(label.len() as u8);
        encoded.extend_from_slice(label.as_bytes());
    }
    encoded.push(0);
    encoded
}

/// Decode a DNS name at `offset`, following compression pointers.
/// Returns the decoded name and the offset just past the name in the original
/// stream.
pub fn decode_name(data: &[u8], offset: usize) -> Result<(String, usize), DnsError> {
    let mut labels: Vec<&str> = Vec::new();
    let mut pos = offset;
    let mut jumped = false;
    let mut end = offset;

    loop {
        if pos >= data.len() {
            return Err(DnsError::MalformedResponse);
        }
        let len = data[pos] as usize;

        if len == 0 {
            pos += 1;
            if !jumped {
                end = pos;
            }
            break;
        }

        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return Err(DnsError::MalformedResponse);
            }
            let ptr = ((len & 0x3F) << 8) | data[pos + 1] as usize;
            if !jumped {
                end = pos + 2;
                jumped = true;
            }
            pos = ptr;
            continue;
        }

        if offset_plus(pos, len + 1) > data.len() {
            return Err(DnsError::MalformedResponse);
        }
        let label = core::str::from_utf8(&data[pos + 1..pos + 1 + len])
            .map_err(|_| DnsError::MalformedResponse)?;
        labels.push(label);
        pos += 1 + len;
    }

    let mut name = String::new();
    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            name.push('.');
        }
        name.push_str(label);
    }
    Ok((name, end))
}

#[inline]
fn offset_plus(a: usize, b: usize) -> usize {
    a.checked_add(b).unwrap_or(usize::MAX)
}

/// Build a DNS A-record query (12-byte header + one question).
pub fn build_query(name: &str, id: u16) -> Vec<u8> {
    let encoded = encode_name(name);
    let mut pkt = Vec::with_capacity(12 + encoded.len() + 4);

    pkt.extend_from_slice(&id.to_be_bytes()); // ID
    pkt.extend_from_slice(&0x0100u16.to_be_bytes()); // Flags: RD
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    pkt.extend_from_slice(&encoded);
    pkt.extend_from_slice(&DNS_TYPE_A.to_be_bytes()); // QTYPE = A
    pkt.extend_from_slice(&DNS_CLASS_IN.to_be_bytes()); // QCLASS = IN

    pkt
}

/// Parse a DNS response and extract the A records for the queried name,
/// following a CNAME chain when necessary. Returns all matching A records.
pub fn parse_response(data: &[u8], expected_id: u16) -> Result<DnsAnswer, DnsError> {
    if data.len() < 12 {
        return Err(DnsError::MalformedResponse);
    }

    let id = u16::from_be_bytes([data[0], data[1]]);
    if id != expected_id {
        return Err(DnsError::MalformedResponse);
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 {
        // Not a response.
        return Err(DnsError::MalformedResponse);
    }
    if flags & 0x0200 != 0 {
        return Err(DnsError::Truncated);
    }

    let rcode = (flags & 0x000F) as u8;
    match rcode {
        0 => {}
        3 => return Err(DnsError::NxDomain),
        _ => return Err(DnsError::ServerFailure),
    }

    let qdcount = u16::from_be_bytes([data[4], data[5]]) as usize;
    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;

    let mut offset = 12usize;

    // Question section: remember the first QNAME so we can follow CNAMEs.
    let mut qname = String::new();
    for q in 0..qdcount {
        let (name, next) = decode_name(data, offset)?;
        if q == 0 {
            qname = name;
        }
        offset = offset_plus(next, 4); // QTYPE + QCLASS
        if offset > data.len() {
            return Err(DnsError::MalformedResponse);
        }
    }

    // Answer section: collect A records (owner -> address) and CNAMEs.
    let mut a_records: Vec<(String, [u8; 4], u32)> = Vec::new();
    let mut cnames: Vec<(String, String)> = Vec::new();

    for _ in 0..ancount {
        let (owner, next) = decode_name(data, offset)?;
        offset = next;

        if offset_plus(offset, 10) > data.len() {
            return Err(DnsError::MalformedResponse);
        }
        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let ttl = u32::from_be_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
        offset += 10;

        if offset_plus(offset, rdlength) > data.len() {
            return Err(DnsError::MalformedResponse);
        }

        match rtype {
            DNS_TYPE_A if rdlength == 4 => {
                a_records.push((
                    owner,
                    [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]],
                    ttl,
                ));
            }
            DNS_TYPE_CNAME => {
                let (target, _) = decode_name(data, offset)?;
                cnames.push((owner, target));
            }
            _ => {}
        }

        offset += rdlength;
    }

    // Pick the A records whose owner is the queried name; otherwise follow the
    // CNAME chain from the queried name.
    let mut current = qname;
    let mut seen: Vec<String> = Vec::new();
    let mut addresses: Vec<[u8; 4]> = Vec::new();
    let mut min_ttl = u32::MAX;
    let mut saw_a = false;

    for _ in 0..DNS_MAX_CNAME_HOPS {
        for (owner, addr, ttl) in &a_records {
            if *owner == current {
                addresses.push(*addr);
                saw_a = true;
                if *ttl < min_ttl {
                    min_ttl = *ttl;
                }
            }
        }

        if saw_a {
            break;
        }

        let next = cnames
            .iter()
            .find(|(owner, _)| *owner == current)
            .map(|(_, target)| target.clone());

        match next {
            Some(target) if !seen.contains(&target) => {
                seen.push(current);
                current = target;
            }
            _ => break,
        }
    }

    if addresses.is_empty() {
        return Err(DnsError::NoARecord);
    }

    Ok(DnsAnswer {
        addresses,
        ttl: if min_ttl == u32::MAX { 0 } else { min_ttl },
    })
}

// ── Hostname / address helpers ──

/// Validate a hostname (RFC 952/1035 labels). A single trailing dot (FQDN) is
/// accepted.
pub fn validate_hostname(name: &str) -> Result<(), DnsError> {
    if name.is_empty() || name.len() > 253 {
        return Err(DnsError::InvalidHostname);
    }

    let trimmed = name.strip_suffix('.').unwrap_or(name);
    if trimmed.is_empty() {
        return Err(DnsError::InvalidHostname);
    }

    for label in trimmed.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(DnsError::InvalidHostname);
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(DnsError::InvalidHostname);
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(DnsError::InvalidHostname);
        }
    }

    Ok(())
}

/// Parse a dotted-decimal IPv4 address.
pub fn parse_dotted_ip(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0usize;
    for part in s.split('.') {
        if count == 4 {
            return None;
        }
        octets[count] = part.parse::<u8>().ok()?;
        count += 1;
    }
    if count == 4 {
        Some(octets)
    } else {
        None
    }
}

/// `true` if the address is the unspecified address (`0.0.0.0`), which must
/// never be used as a DNS server.
pub fn is_unspecified(ip: [u8; 4]) -> bool {
    ip == [0, 0, 0, 0]
}

/// `true` for the `localhost` pseudo-name, resolved locally without a query.
pub fn is_localhost(name: &str) -> bool {
    name.eq_ignore_ascii_case("localhost")
}

// ── Query execution ──

/// Global next DNS query ID.
static NEXT_DNS_ID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(1);

fn next_id() -> u16 {
    let id = NEXT_DNS_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if id == 0 {
        NEXT_DNS_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
    } else {
        id
    }
}

/// Build a query, exchange it with `server` and parse the reply.
pub fn query_server<T: DnsTransport>(
    transport: &mut T,
    hostname: &str,
    server: [u8; 4],
    id: u16,
) -> Result<DnsAnswer, DnsError> {
    let query = build_query(hostname, id);
    let mut buf = [0u8; 512];
    let len = transport.exchange(server, &query, &mut buf)?;
    parse_response(&buf[..len], id)
}

/// Query one server, retrying transient failures (timeout / unreachable).
pub fn query_server_with_retries<T: DnsTransport>(
    transport: &mut T,
    hostname: &str,
    server: [u8; 4],
    retries: usize,
) -> Result<DnsAnswer, DnsError> {
    let mut last = DnsError::Timeout;
    for _ in 0..=retries {
        match query_server(transport, hostname, server, next_id()) {
            Ok(answer) => return Ok(answer),
            Err(err @ (DnsError::Timeout | DnsError::ServerUnreachable)) => last = err,
            Err(err) => return Err(err),
        }
    }
    Err(last)
}

/// Resolve `hostname` against a list of servers, in preference order.
///
/// `NXDOMAIN` from any server is returned immediately. Unspecified (`0.0.0.0`)
/// servers are skipped. On success returns the answer and the server that
/// produced it.
pub fn resolve_with_servers<T: DnsTransport>(
    transport: &mut T,
    hostname: &str,
    servers: &[[u8; 4]],
    retries: usize,
) -> Result<(DnsAnswer, [u8; 4]), DnsError> {
    validate_hostname(hostname)?;

    let mut last = DnsError::NoServer;
    let mut tried = false;

    for &server in servers {
        if is_unspecified(server) {
            continue;
        }
        tried = true;
        match query_server_with_retries(transport, hostname, server, retries) {
            Ok(answer) => return Ok((answer, server)),
            Err(DnsError::NxDomain) => return Err(DnsError::NxDomain),
            Err(err) => last = err,
        }
    }

    if !tried {
        return Err(DnsError::NoServer);
    }
    Err(last)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    // ── Test doubles ──

    struct MockTransport {
        response: Vec<u8>,
        last_server: Option<[u8; 4]>,
        calls: usize,
    }

    impl MockTransport {
        fn new(response: Vec<u8>) -> Self {
            MockTransport {
                response,
                last_server: None,
                calls: 0,
            }
        }
    }

    impl DnsTransport for MockTransport {
        fn exchange(
            &mut self,
            server: [u8; 4],
            _query: &[u8],
            resp: &mut [u8],
        ) -> Result<usize, DnsError> {
            self.calls += 1;
            self.last_server = Some(server);
            let len = self.response.len().min(resp.len());
            resp[..len].copy_from_slice(&self.response[..len]);
            Ok(len)
        }
    }

    struct FailingTransport {
        error: DnsError,
    }

    impl DnsTransport for FailingTransport {
        fn exchange(
            &mut self,
            _server: [u8; 4],
            _query: &[u8],
            _resp: &mut [u8],
        ) -> Result<usize, DnsError> {
            Err(self.error)
        }
    }

    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    /// Build a synthetic response: question + one A record per address.
    fn response_with_a(
        name: &str,
        addrs: &[[u8; 4]],
        id: u16,
        rcode: u8,
        truncated: bool,
    ) -> Vec<u8> {
        let mut pkt = Vec::new();
        let flags = 0x8000u16 | (if truncated { 0x0200 } else { 0 }) | (rcode as u16);
        push_u16(&mut pkt, id);
        push_u16(&mut pkt, flags);
        push_u16(&mut pkt, 1); // QDCOUNT
        push_u16(&mut pkt, addrs.len() as u16); // ANCOUNT
        push_u16(&mut pkt, 0);
        push_u16(&mut pkt, 0);

        pkt.extend_from_slice(&encode_name(name));
        push_u16(&mut pkt, DNS_TYPE_A);
        push_u16(&mut pkt, DNS_CLASS_IN);

        for addr in addrs {
            push_u16(&mut pkt, 0xC00C); // owner -> question name
            push_u16(&mut pkt, DNS_TYPE_A);
            push_u16(&mut pkt, DNS_CLASS_IN);
            push_u32(&mut pkt, 300);
            push_u16(&mut pkt, 4);
            pkt.extend_from_slice(addr);
        }
        pkt
    }

    // ── Encoding ──

    #[test]
    fn encode_name_labels() {
        assert_eq!(
            encode_name("www.example.com"),
            vec![3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]
        );
    }

    #[test]
    fn encode_name_handles_trailing_dot() {
        assert_eq!(encode_name("example.com."), encode_name("example.com"));
    }

    #[test]
    fn decode_name_roundtrip() {
        let wire = encode_name("www.example.com");
        let (name, next) = decode_name(&wire, 0).unwrap();
        assert_eq!(name, "www.example.com");
        assert_eq!(next, wire.len());
    }

    #[test]
    fn decode_name_follows_pointer() {
        let mut data = encode_name("example.com");
        data.push(0xC0);
        data.push(0x00);
        let (name, next) = decode_name(&data, 13).unwrap();
        assert_eq!(name, "example.com");
        assert_eq!(next, 15);
    }

    // ── Query construction ──

    #[test]
    fn build_query_header_and_question() {
        let q = build_query("example.com", 0x1234);
        assert!(q.len() > 12);
        assert_eq!(u16::from_be_bytes([q[0], q[1]]), 0x1234);
        assert_eq!(u16::from_be_bytes([q[2], q[3]]), 0x0100); // RD
        assert_eq!(u16::from_be_bytes([q[4], q[5]]), 1); // QDCOUNT
        assert_eq!(u16::from_be_bytes([q[6], q[7]]), 0); // ANCOUNT
        let n = q.len();
        assert_eq!(u16::from_be_bytes([q[n - 4], q[n - 3]]), DNS_TYPE_A);
        assert_eq!(u16::from_be_bytes([q[n - 2], q[n - 1]]), DNS_CLASS_IN);
        assert_eq!(u16::from_be_bytes([q[2], q[3]]) & 0x8000, 0);
    }

    // ── Response parsing ──

    #[test]
    fn parse_single_a_record() {
        let resp = response_with_a("example.com", &[[93, 184, 216, 34]], 7, 0, false);
        let answer = parse_response(&resp, 7).unwrap();
        assert_eq!(answer.addresses, vec![[93, 184, 216, 34]]);
        assert_eq!(answer.ttl, 300);
    }

    #[test]
    fn parse_multiple_a_records() {
        let addrs = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12]];
        let resp = response_with_a("multi.example.com", &addrs, 9, 0, false);
        let answer = parse_response(&resp, 9).unwrap();
        assert_eq!(answer.addresses, vec![addrs[0], addrs[1], addrs[2]]);
    }

    #[test]
    fn parse_nxdomain() {
        let resp = response_with_a("nope.example.com", &[], 3, 3, false);
        assert_eq!(parse_response(&resp, 3), Err(DnsError::NxDomain));
    }

    #[test]
    fn parse_server_failure() {
        let resp = response_with_a("fail.example.com", &[], 3, 2, false);
        assert_eq!(parse_response(&resp, 3), Err(DnsError::ServerFailure));
    }

    #[test]
    fn parse_truncated() {
        let resp = response_with_a("big.example.com", &[[1, 2, 3, 4]], 3, 0, true);
        assert_eq!(parse_response(&resp, 3), Err(DnsError::Truncated));
    }

    #[test]
    fn parse_rejects_wrong_id() {
        let resp = response_with_a("example.com", &[[1, 2, 3, 4]], 10, 0, false);
        assert_eq!(parse_response(&resp, 11), Err(DnsError::MalformedResponse));
    }

    #[test]
    fn parse_rejects_short_and_garbage() {
        assert_eq!(parse_response(&[], 1), Err(DnsError::MalformedResponse));
        assert_eq!(parse_response(&[0u8; 5], 1), Err(DnsError::MalformedResponse));
        let mut bad = vec![0, 1, 0x80, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        bad.push(10); // label length 10 with no data
        assert!(parse_response(&bad, 1).is_err());
    }

    #[test]
    fn parse_no_a_record() {
        let resp = response_with_a("empty.example.com", &[], 4, 0, false);
        assert_eq!(parse_response(&resp, 4), Err(DnsError::NoARecord));
    }

    // ── CNAME chain ──

    #[test]
    fn parse_follows_cname_chain() {
        let mut pkt = Vec::new();
        push_u16(&mut pkt, 42);
        push_u16(&mut pkt, 0x8180);
        push_u16(&mut pkt, 1);
        push_u16(&mut pkt, 2);
        push_u16(&mut pkt, 0);
        push_u16(&mut pkt, 0);

        pkt.extend_from_slice(&encode_name("alias.example.com"));
        push_u16(&mut pkt, DNS_TYPE_A);
        push_u16(&mut pkt, DNS_CLASS_IN);

        // Answer 1: alias.example.com CNAME canonical.example.com
        push_u16(&mut pkt, 0xC00C);
        push_u16(&mut pkt, DNS_TYPE_CNAME);
        push_u16(&mut pkt, DNS_CLASS_IN);
        push_u32(&mut pkt, 120);
        let cname = encode_name("canonical.example.com");
        push_u16(&mut pkt, cname.len() as u16);
        pkt.extend_from_slice(&cname);

        // Answer 2: canonical.example.com A 10.20.30.40
        pkt.extend_from_slice(&encode_name("canonical.example.com"));
        push_u16(&mut pkt, DNS_TYPE_A);
        push_u16(&mut pkt, DNS_CLASS_IN);
        push_u32(&mut pkt, 60);
        push_u16(&mut pkt, 4);
        pkt.extend_from_slice(&[10, 20, 30, 40]);

        let answer = parse_response(&pkt, 42).unwrap();
        assert_eq!(answer.addresses, vec![[10, 20, 30, 40]]);
        assert_eq!(answer.ttl, 60);
    }

    // ── Hostname validation ──

    #[test]
    fn validate_hostname_accepts_normal_names() {
        assert!(validate_hostname("example.com").is_ok());
        assert!(validate_hostname("a.b.c.d.example.com").is_ok());
        assert!(validate_hostname("host-1.example.com").is_ok());
        assert!(validate_hostname("example.com.").is_ok());
    }

    #[test]
    fn validate_hostname_rejects_bad_names() {
        assert_eq!(validate_hostname(""), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("."), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("a..b"), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("-bad.example.com"), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("bad-.example.com"), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("has space.com"), Err(DnsError::InvalidHostname));
        let long_label = "a".repeat(64);
        assert_eq!(validate_hostname(&long_label), Err(DnsError::InvalidHostname));
    }

    // ── Server selection / transport ──

    #[test]
    fn query_server_uses_given_server_and_parses() {
        let resp = response_with_a("example.com", &[[8, 8, 4, 4]], 55, 0, false);
        let mut t = MockTransport::new(resp);
        let answer = query_server(&mut t, "example.com", [10, 0, 1, 1], 55).unwrap();
        assert_eq!(answer.addresses, vec![[8, 8, 4, 4]]);
        assert_eq!(t.last_server, Some([10, 0, 1, 1]));
        assert_eq!(t.calls, 1);
    }

    #[test]
    fn resolve_with_servers_picks_working_server() {
        // Response has an arbitrary ID; retries generate IDs, so answer parsing
        // must match. Use a transport that echoes the query ID by inspecting it.
        struct EchoTransport(Vec<u8>);
        impl DnsTransport for EchoTransport {
            fn exchange(
                &mut self,
                _server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                let id = u16::from_be_bytes([query[0], query[1]]);
                let mut r = self.0.clone();
                r[0..2].copy_from_slice(&id.to_be_bytes());
                let len = r.len().min(resp.len());
                resp[..len].copy_from_slice(&r[..len]);
                Ok(len)
            }
        }

        let base = response_with_a("example.com", &[[9, 9, 9, 9]], 0, 0, false);
        let mut t = EchoTransport(base);
        let servers = [[192, 168, 1, 1], [10, 0, 0, 1]];
        let (answer, server) = resolve_with_servers(&mut t, "example.com", &servers, 0).unwrap();
        assert_eq!(answer.addresses, vec![[9, 9, 9, 9]]);
        assert_eq!(server, [192, 168, 1, 1]);
    }

    #[test]
    fn resolve_with_servers_skips_bad_server() {
        struct Selective {
            bad: [u8; 4],
            good: Vec<u8>,
        }
        impl DnsTransport for Selective {
            fn exchange(
                &mut self,
                server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                if server == self.bad {
                    return Err(DnsError::Timeout);
                }
                let id = u16::from_be_bytes([query[0], query[1]]);
                let mut r = self.good.clone();
                r[0..2].copy_from_slice(&id.to_be_bytes());
                let len = r.len().min(resp.len());
                resp[..len].copy_from_slice(&r[..len]);
                Ok(len)
            }
        }

        let mut t = Selective {
            bad: [192, 168, 1, 1],
            good: response_with_a("example.com", &[[7, 7, 7, 7]], 0, 0, false),
        };
        let servers = [[192, 168, 1, 1], [10, 0, 0, 1]];
        let (answer, server) = resolve_with_servers(&mut t, "example.com", &servers, 0).unwrap();
        assert_eq!(answer.addresses, vec![[7, 7, 7, 7]]);
        assert_eq!(server, [10, 0, 0, 1]);
    }

    #[test]
    fn resolve_with_servers_never_queries_unspecified() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        // Only 0.0.0.0 configured -> NoServer, not a query to 0.0.0.0.
        let result = resolve_with_servers(&mut t, "example.com", &[[0, 0, 0, 0]], 0);
        assert_eq!(result, Err(DnsError::NoServer));
    }

    #[test]
    fn resolve_with_servers_empty_is_no_server() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            resolve_with_servers(&mut t, "example.com", &[], 0),
            Err(DnsError::NoServer)
        );
    }

    #[test]
    fn resolve_with_servers_propagates_nxdomain() {
        struct EchoTransport(Vec<u8>);
        impl DnsTransport for EchoTransport {
            fn exchange(
                &mut self,
                _server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                let id = u16::from_be_bytes([query[0], query[1]]);
                let mut r = self.0.clone();
                r[0..2].copy_from_slice(&id.to_be_bytes());
                let len = r.len().min(resp.len());
                resp[..len].copy_from_slice(&r[..len]);
                Ok(len)
            }
        }

        let mut t = EchoTransport(response_with_a("nope.example.com", &[], 0, 3, false));
        let servers = [[8, 8, 8, 8], [8, 8, 4, 4]];
        assert_eq!(
            resolve_with_servers(&mut t, "nope.example.com", &servers, 0),
            Err(DnsError::NxDomain)
        );
    }

    #[test]
    fn resolve_with_servers_reports_timeout() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            resolve_with_servers(&mut t, "example.com", &[[8, 8, 8, 8]], 0),
            Err(DnsError::Timeout)
        );
    }

    #[test]
    fn resolve_with_servers_rejects_invalid_hostname() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            resolve_with_servers(&mut t, "bad name", &[[8, 8, 8, 8]], 0),
            Err(DnsError::InvalidHostname)
        );
    }

    #[test]
    fn transport_error_is_propagated() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            query_server(&mut t, "example.com", [8, 8, 8, 8], 1),
            Err(DnsError::Timeout)
        );
    }

    #[test]
    fn parse_dotted_ip_variants() {
        assert_eq!(parse_dotted_ip("8.8.8.8"), Some([8, 8, 8, 8]));
        assert_eq!(parse_dotted_ip("192.168.1.1"), Some([192, 168, 1, 1]));
        assert_eq!(parse_dotted_ip("invalid"), None);
        assert_eq!(parse_dotted_ip("1.2.3"), None);
        assert_eq!(parse_dotted_ip("1.2.3.4.5"), None);
        assert_eq!(parse_dotted_ip("256.1.1.1"), None);
    }

    #[test]
    fn localhost_and_unspecified_helpers() {
        assert!(is_localhost("localhost"));
        assert!(is_localhost("LOCALHOST"));
        assert!(!is_localhost("localhost.example.com"));
        assert!(is_unspecified([0, 0, 0, 0]));
        assert!(!is_unspecified([0, 0, 0, 1]));
    }

    /// Optional integration test against real public DNS servers.
    ///
    /// Ignored by default so the unit suite never depends on the network.
    /// Run with: `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires network access"]
    fn resolve_public_name_over_real_udp() {
        use std::net::UdpSocket;
        use std::time::Duration;

        struct StdTransport;

        impl DnsTransport for StdTransport {
            fn exchange(
                &mut self,
                server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                let sock = UdpSocket::bind("0.0.0.0:0").map_err(|_| DnsError::Network)?;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
                let addr = std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::new(server[0], server[1], server[2], server[3]),
                    DNS_PORT,
                );
                sock.send_to(query, addr)
                    .map_err(|_| DnsError::ServerUnreachable)?;
                let (n, _) = sock.recv_from(resp).map_err(|_| DnsError::Timeout)?;
                Ok(n)
            }
        }

        let mut transport = StdTransport;
        let servers = [[1, 1, 1, 1], [8, 8, 8, 8]];
        let (answer, server) =
            resolve_with_servers(&mut transport, "example.com", &servers, 1).unwrap();
        assert!(!answer.addresses.is_empty());
        assert!(servers.contains(&server));
    }
}
