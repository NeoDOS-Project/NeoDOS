//! DNS wire format: encode/decode names, build queries, parse responses.

use crate::*;
use alloc::string::String;
use alloc::vec::Vec;

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
