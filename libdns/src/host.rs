//! Hostname and IPv4 literal helpers.

use crate::DnsError;

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

/// Parse a dotted-decimal IPv4 address into the four-octet wire form.
///
/// `libdns` is deliberately dependency-free, so this is the `[u8; 4]`
/// counterpart of the canonical `libnet_config::parse_ip` (`u32`, big-endian)
/// used by `libnet::config` and the network tools; the two representations are
/// bridged by `libnet_config::ip_to_octets` / `ip_from_octets` (#592).
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

/// Drop a single trailing dot (FQDN form) so `host.` and `host` compare equal.
/// A lone `.` is returned unchanged (it is not a valid name anyway).
fn strip_final_dot(name: &str) -> &str {
    match name.strip_suffix('.') {
        Some(rest) if !rest.is_empty() => rest,
        _ => name,
    }
}

/// `true` for the `localhost` pseudo-name, resolved locally without a query.
/// A single trailing dot (FQDN form) is tolerated (#485).
pub fn is_localhost(name: &str) -> bool {
    strip_final_dot(name).eq_ignore_ascii_case("localhost")
}

/// `true` when `name` denotes the local machine's own `hostname`.
///
/// Comparison is case-insensitive and tolerates a single trailing dot on either
/// side (FQDN form). An empty `hostname` never matches (#485).
pub fn is_local_name(name: &str, hostname: &str) -> bool {
    let host = strip_final_dot(hostname);
    !host.is_empty() && strip_final_dot(name).eq_ignore_ascii_case(host)
}
