//! Minimal absolute-URL parser for HTTP/1.1 requests.
//!
//! Supported form: `scheme://host[:port][/path][?query][#fragment]`.
//!
//! Deliberately absent: userinfo (`user:pass@host`), IPv6 literals (bracketed
//! form) and percent-decoding. Each of those is rejected or passed through
//! unchanged rather than guessed at, so a request can never be built from an
//! ambiguous authority.

use alloc::string::{String, ToString};

use crate::error::HttpError;

/// Default port for `http`.
pub const HTTP_PORT: u16 = 80;
/// Default port for `https`.
pub const HTTPS_PORT: u16 = 443;

/// URL scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Cleartext HTTP/1.1.
    Http,
    /// HTTP/1.1 over TLS (parsed here; the transport is added later).
    Https,
}

impl Scheme {
    /// Lowercase scheme name.
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }

    /// Port used when the URL omits one.
    pub fn default_port(self) -> u16 {
        match self {
            Scheme::Http => HTTP_PORT,
            Scheme::Https => HTTPS_PORT,
        }
    }
}

/// An absolute URL reduced to what an HTTP request needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    /// Scheme.
    pub scheme: Scheme,
    /// Lowercase host, without a trailing dot.
    pub host: String,
    /// Effective port (explicit or the scheme default).
    pub port: u16,
    /// Request target: path plus query, always starting with `/`.
    pub path: String,
}

impl Url {
    /// Parse an absolute URL.
    ///
    /// Rejects spaces, controls, userinfo, bracketed IPv6, empty hosts, invalid
    /// ports and numeric hosts that are not valid IPv4 addresses.
    pub fn parse(input: &str) -> Result<Self, HttpError> {
        // Anything that could split the request line or a header is refused
        // outright; there is no percent-decoding to hide behind.
        if input.bytes().any(|b| b <= 0x20 || b == 0x7f) {
            return Err(HttpError::InvalidUrl);
        }

        let (scheme, rest) = split_scheme(input)?;
        let (authority, path) = split_authority(rest)?;
        if authority.contains('@') {
            return Err(HttpError::InvalidUrl);
        }
        if authority.starts_with('[') {
            // IPv6 literals are not supported yet (see module docs).
            return Err(HttpError::InvalidUrl);
        }

        let (host_part, port) = match authority.rfind(':') {
            Some(i) => {
                let port_str = &authority[i + 1..];
                (&authority[..i], parse_port(port_str)?)
            }
            None => (authority, scheme.default_port()),
        };

        let host = normalize_host(host_part)?;
        Ok(Url {
            scheme,
            host,
            port,
            path,
        })
    }

    /// Value for the `Host` header: the port is elided when it is the default.
    pub fn host_header(&self) -> String {
        if self.port == self.scheme.default_port() {
            self.host.clone()
        } else {
            let mut s = self.host.clone();
            s.push(':');
            push_u16(&mut s, self.port);
            s
        }
    }

    /// True when the host is an IPv4 literal (so DNS must be skipped).
    pub fn is_ipv4_literal(&self) -> bool {
        parse_ipv4(&self.host).is_some()
    }
}

/// Split `scheme://rest`, accepting only `http` and `https` case-insensitively.
fn split_scheme(input: &str) -> Result<(Scheme, &str), HttpError> {
    let idx = input.find("://").ok_or(HttpError::InvalidUrl)?;
    let scheme = if input[..idx].eq_ignore_ascii_case("http") {
        Scheme::Http
    } else if input[..idx].eq_ignore_ascii_case("https") {
        Scheme::Https
    } else {
        return Err(HttpError::InvalidUrl);
    };
    Ok((scheme, &input[idx + 3..]))
}

/// Split the authority from the path/query and drop the fragment.
fn split_authority(rest: &str) -> Result<(&str, String), HttpError> {
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.is_empty() {
        return Err(HttpError::InvalidUrl);
    }

    let tail = &rest[end..];
    // A fragment is never sent to the server.
    let tail = match tail.find('#') {
        Some(i) => &tail[..i],
        None => tail,
    };

    let path = if tail.is_empty() {
        "/".to_string()
    } else if let Some(query) = tail.strip_prefix('?') {
        let mut p = String::from("/?");
        p.push_str(query);
        p
    } else {
        tail.to_string()
    };

    Ok((authority, path))
}

/// Parse a decimal port in `1..=65535`; leading zeros are rejected.
fn parse_port(s: &str) -> Result<u16, HttpError> {
    if s.is_empty() || s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(HttpError::InvalidUrl);
    }
    if s.len() > 1 && s.starts_with('0') {
        return Err(HttpError::InvalidUrl);
    }
    let value: u32 = s.bytes().fold(0, |acc, b| acc * 10 + (b - b'0') as u32);
    if value == 0 || value > u16::MAX as u32 {
        return Err(HttpError::InvalidUrl);
    }
    Ok(value as u16)
}

/// Validate and lowercase a host; numeric hosts must be valid IPv4 literals.
fn normalize_host(host: &str) -> Result<String, HttpError> {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 {
        return Err(HttpError::InvalidUrl);
    }

    let mut all_numeric = true;
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(HttpError::InvalidUrl);
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(HttpError::InvalidUrl);
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(HttpError::InvalidUrl);
        }
        if !label.bytes().all(|b| b.is_ascii_digit()) {
            all_numeric = false;
        }
    }

    let mut out = String::with_capacity(host.len());
    for b in host.bytes() {
        out.push(b.to_ascii_lowercase() as char);
    }

    if all_numeric && parse_ipv4(&out).is_none() {
        // `999.1.1.1` is neither a valid IP nor a usable hostname.
        return Err(HttpError::InvalidUrl);
    }

    Ok(out)
}

/// Strict dotted-quad parser used only to classify a host as an IPv4 literal.
fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut count = 0usize;
    for part in s.split('.') {
        if count == 4 || part.is_empty() || part.len() > 3 {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let value: u32 = part.bytes().fold(0, |acc, b| acc * 10 + (b - b'0') as u32);
        if value > 255 {
            return None;
        }
        out[count] = value as u8;
        count += 1;
    }
    if count == 4 {
        Some(out)
    } else {
        None
    }
}

fn push_u16(s: &mut String, mut value: u16) {
    let mut buf = [0u8; 5];
    let mut i = buf.len();
    if value == 0 {
        s.push('0');
        return;
    }
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for &b in &buf[i..] {
        s.push(b as char);
    }
}
