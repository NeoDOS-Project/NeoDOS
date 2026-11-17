//! NeoDOS HTTP/1.1 client core.
//!
//! Dependency-free, `no_std` + `alloc`. This crate owns the protocol: URL
//! parsing, request building, response-head parsing, limits and errors. It does
//! not open sockets and it does not know about the CLI.
//!
//! ```text
//! neocurl  ->  libhttp  ->  Transport (impl. by libnet; TLS later by libtls)
//! ```
//!
//! Milestones 1 and 4 cover the offline half (no socket implementation):
//! building a well-formed request head, parsing a response head from arbitrary
//! read boundaries, and streaming the body (`Content-Length`, `chunked`,
//! connection close) into a caller-supplied sink.
//!
//! When built for NeoDOS it is `no_std`; under `cargo test` it uses the host
//! `std` test harness, like `libdns` and `libntp`.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod body;
mod client;
mod error;
mod headers;
mod limits;
mod parse;
mod transport;
mod url;

#[cfg(test)]
mod tests;

pub use body::{body_framing, BodyFraming, BodyReader, BodySink, NullSink, VecSink};
pub use client::{execute, MAX_INFORMATIONAL_RESPONSES};
pub use error::HttpError;
pub use headers::{is_valid_name, is_valid_value, HeaderMap};
pub use limits::Limits;
pub use parse::{parse_header_line, parse_status_line, HeadScanner, HttpVersion, ResponseHead};
pub use transport::{Close, Deadline, Read, Transport, Write};
pub use url::{Scheme, Url, HTTPS_PORT, HTTP_PORT};

use alloc::vec::Vec;

/// `User-Agent` sent unless the caller sets one.
pub const USER_AGENT: &str = "NeoDOS-neocurl/0.1";

/// Request method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Retrieve a representation.
    Get,
    /// Retrieve only the head.
    Head,
    /// Submit a body.
    Post,
}

impl Method {
    /// Wire representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
        }
    }
}

/// Where a request body comes from.
///
/// Milestone 5 adds `Stdin` and `File`; both know their length without loading
/// the body into memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodySource<'a> {
    /// In-memory body.
    Bytes(&'a [u8]),
}

impl BodySource<'_> {
    /// Body length in bytes (used for `Content-Length`).
    pub fn len(&self) -> usize {
        match self {
            BodySource::Bytes(bytes) => bytes.len(),
        }
    }

    /// True for a zero-length body.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A request to one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request<'a> {
    /// Method.
    pub method: Method,
    /// Target URL.
    pub url: Url,
    /// Extra headers, validated on insert.
    pub headers: HeaderMap,
    /// Optional body (POST only).
    pub body: Option<BodySource<'a>>,
}

impl<'a> Request<'a> {
    /// `GET url`.
    pub fn get(url: Url) -> Self {
        Request::new(Method::Get, url)
    }

    /// `HEAD url`.
    pub fn head(url: Url) -> Self {
        Request::new(Method::Head, url)
    }

    /// `POST url` with an in-memory body.
    pub fn post(url: Url, body: &'a [u8]) -> Self {
        Request {
            method: Method::Post,
            url,
            headers: HeaderMap::new(),
            body: Some(BodySource::Bytes(body)),
        }
    }

    /// Empty request with the given method and URL.
    pub fn new(method: Method, url: Url) -> Self {
        Request {
            method,
            url,
            headers: HeaderMap::new(),
            body: None,
        }
    }

    /// Add a header. Framing headers are owned by the library and are rejected
    /// here so they cannot disagree with the request that is actually sent.
    pub fn header(mut self, name: &str, value: &str) -> Result<Self, HttpError> {
        if is_reserved_header(name) {
            return Err(HttpError::MalformedHeader);
        }
        self.headers.insert(name, value)?;
        Ok(self)
    }

    /// Serialise the request head (request line + headers + final CRLF).
    pub fn build_head(&self) -> Result<Vec<u8>, HttpError> {
        let mut out = Vec::with_capacity(256);
        self.write_head(&mut out)?;
        Ok(out)
    }

    /// Serialise into `out`.
    pub fn write_head(&self, out: &mut Vec<u8>) -> Result<(), HttpError> {
        if self.method != Method::Post && self.body.is_some() {
            return Err(HttpError::InvalidRequest);
        }

        out.extend_from_slice(self.method.as_str().as_bytes());
        out.push(b' ');
        out.extend_from_slice(self.url.path.as_bytes());
        out.extend_from_slice(b" HTTP/1.1\r\n");

        out.extend_from_slice(b"Host: ");
        out.extend_from_slice(self.url.host_header().as_bytes());
        out.extend_from_slice(b"\r\n");

        out.extend_from_slice(b"Connection: close\r\n");
        out.extend_from_slice(b"User-Agent: ");
        out.extend_from_slice(USER_AGENT.as_bytes());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(b"Accept-Encoding: identity\r\n");

        if self.method == Method::Post {
            let len = self.body.map(|b| b.len()).unwrap_or(0);
            out.extend_from_slice(b"Content-Length: ");
            push_usize(out, len);
            out.extend_from_slice(b"\r\n");
        }

        for (name, value) in self.headers.iter() {
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(b"\r\n");
        }

        out.extend_from_slice(b"\r\n");
        Ok(())
    }
}

/// Headers the library computes itself.
fn is_reserved_header(name: &str) -> bool {
    [
        "host",
        "connection",
        "content-length",
        "transfer-encoding",
        "accept-encoding",
    ]
    .iter()
    .any(|reserved| name.eq_ignore_ascii_case(reserved))
}

fn push_usize(out: &mut Vec<u8>, mut value: usize) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if value == 0 {
        out.push(b'0');
        return;
    }
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out.extend_from_slice(&buf[i..]);
}
