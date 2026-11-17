//! Response-head parsing: status line, headers and the end-of-head scanner.
//!
//! The scanner is pure and caller-buffered: the transport owns the read buffer
//! and passes the bytes seen so far; on success the scanner reports how many
//! bytes formed the head so the caller can keep the rest (body) untouched.
//! This is what makes the parser independent of read boundaries.

use alloc::string::{String, ToString};

use crate::error::HttpError;
use crate::headers::{is_valid_name, is_valid_value, HeaderMap};
use crate::limits::Limits;

/// Response HTTP version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVersion {
    /// `HTTP/1.0`
    H10,
    /// `HTTP/1.1`
    H11,
}

impl HttpVersion {
    /// Wire representation.
    pub fn as_str(self) -> &'static str {
        match self {
            HttpVersion::H10 => "HTTP/1.0",
            HttpVersion::H11 => "HTTP/1.1",
        }
    }
}

/// Parsed response head (everything before the body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHead {
    /// Protocol version reported by the server.
    pub version: HttpVersion,
    /// Three-digit status code (100..=599).
    pub status: u16,
    /// Reason phrase; may be empty.
    pub reason: String,
    /// Response headers in wire order.
    pub headers: HeaderMap,
}

/// Parse a status line including its CRLF.
pub fn parse_status_line(line: &[u8]) -> Result<(HttpVersion, u16, String), HttpError> {
    let line = line
        .strip_suffix(b"\r\n")
        .ok_or(HttpError::MalformedStatus)?;

    let space = line
        .iter()
        .position(|&b| b == b' ')
        .ok_or(HttpError::MalformedStatus)?;

    let version = match &line[..space] {
        b"HTTP/1.1" => HttpVersion::H11,
        b"HTTP/1.0" => HttpVersion::H10,
        _ => return Err(HttpError::MalformedStatus),
    };

    let rest = &line[space + 1..];
    if rest.len() < 3 || !rest[..3].iter().all(|b| b.is_ascii_digit()) {
        return Err(HttpError::MalformedStatus);
    }
    let status = rest[..3]
        .iter()
        .fold(0u16, |acc, b| acc * 10 + (b - b'0') as u16);
    if !(100..=599).contains(&status) {
        return Err(HttpError::MalformedStatus);
    }

    let reason = match rest.len() {
        3 => String::new(),
        _ => {
            if rest[3] != b' ' {
                return Err(HttpError::MalformedStatus);
            }
            let bytes = &rest[4..];
            if !is_valid_value(core::str::from_utf8(bytes).map_err(|_| HttpError::MalformedStatus)?)
            {
                return Err(HttpError::MalformedStatus);
            }
            core::str::from_utf8(bytes)
                .map_err(|_| HttpError::MalformedStatus)?
                .to_string()
        }
    };

    Ok((version, status, reason))
}

/// Parse a header line including its CRLF. The empty line is not a header and
/// returns [`HttpError::MalformedHeader`]; callers detect the terminator first.
pub fn parse_header_line(line: &[u8]) -> Result<(String, String), HttpError> {
    let line = line
        .strip_suffix(b"\r\n")
        .ok_or(HttpError::MalformedHeader)?;
    if line.is_empty() {
        return Err(HttpError::MalformedHeader);
    }
    // Obsolete line folding is rejected (RFC 7230 §3.2.4).
    if line[0] == b' ' || line[0] == b'\t' {
        return Err(HttpError::MalformedHeader);
    }

    let colon = line
        .iter()
        .position(|&b| b == b':')
        .ok_or(HttpError::MalformedHeader)?;
    let name = core::str::from_utf8(&line[..colon]).map_err(|_| HttpError::MalformedHeader)?;
    if !is_valid_name(name) {
        return Err(HttpError::MalformedHeader);
    }

    let value = core::str::from_utf8(trim_ows(&line[colon + 1..]))
        .map_err(|_| HttpError::MalformedHeader)?;
    if !is_valid_value(value) {
        return Err(HttpError::MalformedHeader);
    }

    Ok((name.to_string(), value.to_string()))
}

/// Incremental scanner for a response head.
///
/// The transport keeps the read buffer; `scan` returns `Some((head, consumed))`
/// once `\r\n\r\n` has been seen, and the caller drops `consumed` bytes so the
/// body (already received in the same read) stays in the buffer.
#[derive(Debug)]
pub struct HeadScanner {
    scanned: usize,
    max_bytes: usize,
    max_headers: usize,
}

impl HeadScanner {
    /// New scanner configured from [`Limits`].
    pub fn new(limits: &Limits) -> Self {
        HeadScanner {
            scanned: 0,
            max_bytes: limits.max_header_bytes,
            max_headers: limits.max_headers,
        }
    }

    /// Forget scan progress (e.g. after an error, before a retry).
    pub fn reset(&mut self) {
        self.scanned = 0;
    }

    /// Look for a complete head in `buf`.
    ///
    /// - `Ok(None)` — more bytes are needed (and the buffer is still within the
    ///   limit).
    /// - `Ok(Some((head, consumed)))` — `buf[..consumed]` is the head; any
    ///   remaining bytes belong to the body.
    /// - `Err(TooManyHeaders | MalformedStatus | MalformedHeader)` — the response
    ///   head is invalid.
    pub fn scan(&mut self, buf: &[u8]) -> Result<Option<(ResponseHead, usize)>, HttpError> {
        let end = match find_head_end(buf, &mut self.scanned) {
            Some(end) => end,
            None => {
                if buf.len() > self.max_bytes {
                    return Err(HttpError::TooManyHeaders);
                }
                return Ok(None);
            }
        };
        if end > self.max_bytes {
            return Err(HttpError::TooManyHeaders);
        }
        parse_head(&buf[..end], self.max_headers).map(|head| Some((head, end)))
    }
}

/// Parse a complete head block (status line + headers + final CRLF).
fn parse_head(block: &[u8], max_headers: usize) -> Result<ResponseHead, HttpError> {
    let mut pos = 0usize;
    let mut count = 0usize;
    let mut status: Option<(HttpVersion, u16, String)> = None;
    let mut headers = HeaderMap::new();

    loop {
        let rel = block[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(HttpError::MalformedHeader)?;
        let line_end = pos + rel + 2;
        let line = &block[pos..line_end];

        if line == b"\r\n" {
            if status.is_none() {
                return Err(HttpError::MalformedStatus);
            }
            break;
        }

        match status {
            None => status = Some(parse_status_line(line)?),
            Some(_) => {
                count += 1;
                if count > max_headers {
                    return Err(HttpError::TooManyHeaders);
                }
                let (name, value) = parse_header_line(line)?;
                headers.push_validated(name, value);
            }
        }
        pos = line_end;
    }

    let (version, code, reason) = status.ok_or(HttpError::MalformedStatus)?;
    Ok(ResponseHead {
        version,
        status: code,
        reason,
        headers,
    })
}

/// Find the end of the head (index just past `\r\n\r\n`), resuming from the
/// previous scan position so byte-at-a-time reads stay linear.
fn find_head_end(buf: &[u8], scanned: &mut usize) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    let start = scanned.saturating_sub(3);
    let mut i = start;
    while i + 4 <= buf.len() {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            let end = i + 4;
            *scanned = end;
            return Some(end);
        }
        i += 1;
    }
    *scanned = buf.len();
    None
}

fn trim_ows(bytes: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = bytes.len();
    while start < end && (bytes[start] == b' ' || bytes[start] == b'\t') {
        start += 1;
    }
    while end > start && (bytes[end - 1] == b' ' || bytes[end - 1] == b'\t') {
        end -= 1;
    }
    &bytes[start..end]
}
