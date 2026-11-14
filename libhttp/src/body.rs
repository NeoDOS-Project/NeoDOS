//! Response body framing and streaming.
//!
//! Framing is decided from the response head (RFC 7230 §3.3.3):
//! `HEAD`/1xx/204/304 have no body, `Transfer-Encoding: chunked` wins over
//! `Content-Length`, an explicit length is read exactly, and anything else is
//! delimited by connection close.
//!
//! The body is streamed through [`BodySink`], so a caller can write to a file
//! instead of buffering it (the process heap is 2 MiB). `BodyReader` owns a
//! small read buffer and accepts the bytes left over from the head read, which
//! is what makes the parser independent of read boundaries.

use alloc::vec::Vec;

use crate::error::HttpError;
use crate::headers::is_valid_name;
use crate::limits::Limits;
use crate::parse::ResponseHead;
use crate::transport::{Deadline, Read};
use crate::Method;

/// How the response body is delimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFraming {
    /// No body at all (HEAD, 1xx, 204, 304).
    Empty,
    /// Exactly `n` bytes.
    Length(u64),
    /// `Transfer-Encoding: chunked`.
    Chunked,
    /// The body ends when the peer closes the connection.
    CloseDelimited,
}

/// Where body bytes are written as they arrive.
pub trait BodySink {
    /// Consume one chunk of body bytes.
    fn write(&mut self, chunk: &[u8]) -> Result<(), HttpError>;
}

/// Sink that discards the body.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl BodySink for NullSink {
    fn write(&mut self, _chunk: &[u8]) -> Result<(), HttpError> {
        Ok(())
    }
}

/// Sink that collects the body in memory. Only for small, bounded bodies.
#[derive(Debug, Default, Clone)]
pub struct VecSink {
    /// Bytes received so far.
    pub buf: Vec<u8>,
}

impl VecSink {
    /// Empty sink.
    pub fn new() -> Self {
        VecSink { buf: Vec::new() }
    }

    /// Consume the sink and return the collected body.
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}

impl BodySink for VecSink {
    fn write(&mut self, chunk: &[u8]) -> Result<(), HttpError> {
        self.buf.extend_from_slice(chunk);
        Ok(())
    }
}

/// Decide the body framing from the request method and response head.
pub fn body_framing(method: Method, head: &ResponseHead) -> Result<BodyFraming, HttpError> {
    if method == Method::Head || head.status / 100 == 1 || head.status == 204 || head.status == 304
    {
        return Ok(BodyFraming::Empty);
    }

    check_content_encoding(head)?;

    // `Transfer-Encoding` overrides `Content-Length`.
    let mut codings = 0usize;
    let mut last = "";
    for raw in head.headers.get_all("transfer-encoding") {
        for part in raw.split(',') {
            let token = part.trim_matches([' ', '\t']);
            if token.is_empty() {
                return Err(HttpError::MalformedHeader);
            }
            codings += 1;
            last = token;
        }
    }
    if codings > 0 {
        // Only a single `chunked` coding is decodable; `gzip, chunked` and
        // friends would require an inflate we do not have.
        if codings != 1 || !last.eq_ignore_ascii_case("chunked") {
            return Err(HttpError::UnsupportedTransfer);
        }
        return Ok(BodyFraming::Chunked);
    }

    match parse_content_length(head)? {
        Some(n) => Ok(BodyFraming::Length(n)),
        None => Ok(BodyFraming::CloseDelimited),
    }
}

/// Reject any `Content-Encoding` we cannot decode (`identity` is allowed).
fn check_content_encoding(head: &ResponseHead) -> Result<(), HttpError> {
    for raw in head.headers.get_all("content-encoding") {
        for part in raw.split(',') {
            let token = part.trim_matches([' ', '\t']);
            if !token.is_empty() && !token.eq_ignore_ascii_case("identity") {
                return Err(HttpError::UnsupportedEncoding);
            }
        }
    }
    Ok(())
}

/// Parse `Content-Length`, rejecting conflicting or non-numeric values.
fn parse_content_length(head: &ResponseHead) -> Result<Option<u64>, HttpError> {
    let mut value: Option<u64> = None;
    for raw in head.headers.get_all("content-length") {
        for part in raw.split(',') {
            let digits = part.trim_matches([' ', '\t']);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(HttpError::MalformedHeader);
            }
            let parsed = digits
                .bytes()
                .try_fold(0u64, |acc, b| {
                    acc.checked_mul(10)?.checked_add((b - b'0') as u64)
                })
                .ok_or(HttpError::MalformedHeader)?;
            match value {
                Some(existing) if existing != parsed => return Err(HttpError::MalformedHeader),
                _ => value = Some(parsed),
            }
        }
    }
    Ok(value)
}

/// Streaming reader for the response body.
///
/// Drop it or call [`BodyReader::into_inner`] afterwards; the connection is
/// owned by the transport and closed by the caller.
pub struct BodyReader<R: Read> {
    inner: R,
    buf: Vec<u8>,
    start: usize,
    eof: bool,
    dl: Deadline,
}

impl<R: Read> BodyReader<R> {
    /// Reader with an empty buffer.
    pub fn new(inner: R, dl: Deadline) -> Self {
        BodyReader {
            inner,
            buf: Vec::new(),
            start: 0,
            eof: false,
            dl,
        }
    }

    /// Reader seeded with bytes already received after the head.
    pub fn with_buffered(inner: R, buffered: &[u8], dl: Deadline) -> Self {
        BodyReader {
            inner,
            buf: buffered.to_vec(),
            start: 0,
            eof: false,
            dl,
        }
    }

    /// Recover the transport.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Stream the body into `sink`, returning the number of bytes delivered.
    pub fn read_body(
        &mut self,
        framing: BodyFraming,
        limits: &Limits,
        sink: &mut dyn BodySink,
    ) -> Result<u64, HttpError> {
        match framing {
            BodyFraming::Empty => Ok(0),
            BodyFraming::Length(n) => {
                if limits.has_body_limit() && n > limits.max_body_bytes {
                    return Err(HttpError::BodyTooLarge);
                }
                self.read_exact_to_sink(n, sink)?;
                Ok(n)
            }
            BodyFraming::Chunked => self.read_chunked(limits, sink),
            BodyFraming::CloseDelimited => self.read_to_eof(limits, sink),
        }
    }

    fn available(&self) -> &[u8] {
        &self.buf[self.start..]
    }

    fn consume(&mut self, n: usize) {
        self.start += n;
        if self.start == self.buf.len() {
            self.buf.clear();
            self.start = 0;
        } else if self.start > 4096 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
    }

    /// Read more bytes. `Ok(false)` means the peer closed the connection.
    fn fill(&mut self) -> Result<bool, HttpError> {
        if self.eof {
            return Ok(false);
        }
        let mut tmp = [0u8; 1024];
        let n = self.inner.read(&mut tmp, self.dl)?;
        if n == 0 {
            self.eof = true;
            return Ok(false);
        }
        self.buf.extend_from_slice(&tmp[..n]);
        Ok(true)
    }

    /// Read a CRLF-terminated line, returning it without the terminator.
    fn read_line(&mut self, max: usize, too_long: HttpError) -> Result<Vec<u8>, HttpError> {
        loop {
            if let Some(pos) = find_crlf(self.available()) {
                let line = self.available()[..pos].to_vec();
                self.consume(pos + 2);
                return Ok(line);
            }
            if self.available().len() > max {
                return Err(too_long);
            }
            if !self.fill()? {
                return Err(HttpError::IncompleteBody);
            }
        }
    }

    fn read_exact_bytes(&mut self, out: &mut [u8]) -> Result<(), HttpError> {
        let mut done = 0usize;
        while done < out.len() {
            if self.available().is_empty() && !self.fill()? {
                return Err(HttpError::IncompleteBody);
            }
            let take = {
                let avail = self.available();
                (out.len() - done).min(avail.len())
            };
            {
                let avail = self.available();
                out[done..done + take].copy_from_slice(&avail[..take]);
            }
            self.consume(take);
            done += take;
        }
        Ok(())
    }

    fn read_exact_to_sink(
        &mut self,
        mut remaining: u64,
        sink: &mut dyn BodySink,
    ) -> Result<(), HttpError> {
        while remaining > 0 {
            if self.available().is_empty() && !self.fill()? {
                return Err(HttpError::IncompleteBody);
            }
            let take = {
                let avail = self.available();
                (avail.len() as u64).min(remaining) as usize
            };
            {
                let avail = self.available();
                sink.write(&avail[..take])?;
            }
            self.consume(take);
            remaining -= take as u64;
        }
        Ok(())
    }

    fn read_to_eof(&mut self, limits: &Limits, sink: &mut dyn BodySink) -> Result<u64, HttpError> {
        let mut total = 0u64;
        loop {
            if self.available().is_empty() && !self.fill()? {
                return Ok(total);
            }
            let take = self.available().len();
            let new_total = total + take as u64;
            if limits.has_body_limit() && new_total > limits.max_body_bytes {
                return Err(HttpError::BodyTooLarge);
            }
            {
                let avail = self.available();
                sink.write(&avail[..take])?;
            }
            self.consume(take);
            total = new_total;
        }
    }

    fn read_chunked(&mut self, limits: &Limits, sink: &mut dyn BodySink) -> Result<u64, HttpError> {
        let mut total = 0u64;
        loop {
            let line = self.read_line(MAX_CHUNK_LINE, HttpError::MalformedChunk)?;
            let size = parse_chunk_size(&line)?;

            if size == 0 {
                self.read_trailers(limits)?;
                return Ok(total);
            }

            let new_total = total.checked_add(size).ok_or(HttpError::BodyTooLarge)?;
            if limits.has_body_limit() && new_total > limits.max_body_bytes {
                return Err(HttpError::BodyTooLarge);
            }

            self.read_exact_to_sink(size, sink)?;
            let mut crlf = [0u8; 2];
            self.read_exact_bytes(&mut crlf)?;
            if &crlf != b"\r\n" {
                return Err(HttpError::MalformedChunk);
            }
            total = new_total;
        }
    }

    fn read_trailers(&mut self, limits: &Limits) -> Result<(), HttpError> {
        let mut count = 0usize;
        loop {
            let line = self.read_line(limits.max_header_bytes, HttpError::TooManyHeaders)?;
            if line.is_empty() {
                return Ok(());
            }
            count += 1;
            if count > limits.max_headers {
                return Err(HttpError::TooManyHeaders);
            }
            validate_trailer(&line)?;
        }
    }
}

/// Longest accepted chunk-size line (size + extensions).
const MAX_CHUNK_LINE: usize = 4096;

/// Parse a chunk-size line: hex size, optional `;extensions`.
pub(crate) fn parse_chunk_size(line: &[u8]) -> Result<u64, HttpError> {
    let digits = match line.iter().position(|&b| b == b';') {
        Some(i) => &line[..i],
        None => line,
    };
    if digits.is_empty() || digits.len() > 16 {
        return Err(HttpError::MalformedChunk);
    }
    let mut value = 0u64;
    for &b in digits {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(HttpError::MalformedChunk),
        };
        value = value
            .checked_mul(16)
            .and_then(|v| v.checked_add(digit as u64))
            .ok_or(HttpError::MalformedChunk)?;
    }
    Ok(value)
}

/// Trailers are ignored, but each line must still be a well-formed header.
fn validate_trailer(line: &[u8]) -> Result<(), HttpError> {
    let colon = line
        .iter()
        .position(|&b| b == b':')
        .ok_or(HttpError::MalformedChunk)?;
    let name = core::str::from_utf8(&line[..colon]).map_err(|_| HttpError::MalformedChunk)?;
    if !is_valid_name(name) {
        return Err(HttpError::MalformedChunk);
    }
    let value = trim_ows(&line[colon + 1..]);
    if !crate::headers::is_valid_value(core::str::from_utf8(value).unwrap_or("")) {
        return Err(HttpError::MalformedChunk);
    }
    Ok(())
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

fn find_crlf(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|w| w == b"\r\n")
}
