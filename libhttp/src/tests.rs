//! Host tests for the offline HTTP core.
//!
//! Every case is deterministic and touches no network: URL parsing, header
//! validation, status/header parsing, the incremental head scanner, request
//! serialisation, limits and deadlines.

use super::*;
use alloc::vec::Vec;

// ── URL ──

#[test]
fn url_parse_http_default_port() {
    let url = Url::parse("http://example.com/").unwrap();
    assert_eq!(url.scheme, Scheme::Http);
    assert_eq!(url.host, "example.com");
    assert_eq!(url.port, 80);
    assert_eq!(url.path, "/");
    assert!(!url.is_ipv4_literal());
}

#[test]
fn url_parse_https_default_port() {
    let url = Url::parse("https://example.com").unwrap();
    assert_eq!(url.scheme, Scheme::Https);
    assert_eq!(url.port, 443);
    assert_eq!(url.path, "/");
}

#[test]
fn url_lowercases_host_and_keeps_path_query() {
    let url = Url::parse("HTTP://Example.COM/Path?x=1#frag").unwrap();
    assert_eq!(url.host, "example.com");
    assert_eq!(url.path, "/Path?x=1");
}

#[test]
fn url_strips_trailing_dot() {
    let url = Url::parse("http://example.com./").unwrap();
    assert_eq!(url.host, "example.com");
}

#[test]
fn url_query_without_path() {
    let url = Url::parse("http://example.com?q=1").unwrap();
    assert_eq!(url.path, "/?q=1");
}

#[test]
fn url_explicit_port_and_host_header() {
    let url = Url::parse("http://example.com:8080/a").unwrap();
    assert_eq!(url.port, 8080);
    assert_eq!(url.host_header(), "example.com:8080");

    let default = Url::parse("http://example.com:80/a").unwrap();
    assert_eq!(default.host_header(), "example.com");
}

#[test]
fn url_ipv4_literal() {
    let url = Url::parse("http://10.0.2.2:8080/").unwrap();
    assert!(url.is_ipv4_literal());
    assert_eq!(url.host_header(), "10.0.2.2:8080");

    let local = Url::parse("http://127.0.0.1/").unwrap();
    assert!(local.is_ipv4_literal());
}

#[test]
fn url_rejects_userinfo() {
    assert_eq!(
        Url::parse("http://user:pass@example.com/"),
        Err(HttpError::InvalidUrl)
    );
}

#[test]
fn url_rejects_ipv6_literal() {
    assert_eq!(Url::parse("http://[::1]/"), Err(HttpError::InvalidUrl));
}

#[test]
fn url_rejects_spaces_and_controls() {
    assert_eq!(
        Url::parse("http://example.com/a b"),
        Err(HttpError::InvalidUrl)
    );
    assert_eq!(
        Url::parse("http://example.com/a\r\nX: y"),
        Err(HttpError::InvalidUrl)
    );
    assert_eq!(
        Url::parse("http://example.com/\0"),
        Err(HttpError::InvalidUrl)
    );
}

#[test]
fn url_rejects_bad_ports() {
    for raw in [
        "http://example.com:0/",
        "http://example.com:65536/",
        "http://example.com:abc/",
        "http://example.com:080/",
    ] {
        assert_eq!(Url::parse(raw), Err(HttpError::InvalidUrl), "{raw}");
    }
}

#[test]
fn url_rejects_unknown_scheme_and_missing_host() {
    assert_eq!(Url::parse("ftp://example.com/"), Err(HttpError::InvalidUrl));
    assert_eq!(Url::parse("example.com/"), Err(HttpError::InvalidUrl));
    assert_eq!(Url::parse("http://"), Err(HttpError::InvalidUrl));
    assert_eq!(Url::parse("http:///path"), Err(HttpError::InvalidUrl));
}

#[test]
fn url_rejects_numeric_host_that_is_not_ipv4() {
    assert_eq!(Url::parse("http://999.1.1.1/"), Err(HttpError::InvalidUrl));
    assert_eq!(Url::parse("http://1.2.3.4.5/"), Err(HttpError::InvalidUrl));
    assert_eq!(Url::parse("http://1.2.3.256/"), Err(HttpError::InvalidUrl));
}

#[test]
fn url_rejects_bad_labels() {
    assert_eq!(
        Url::parse("http://-bad.example.com/"),
        Err(HttpError::InvalidUrl)
    );
    assert_eq!(
        Url::parse("http://bad-.example.com/"),
        Err(HttpError::InvalidUrl)
    );
    assert_eq!(Url::parse("http://a..b/"), Err(HttpError::InvalidUrl));
}

// ── Headers ──

#[test]
fn headers_lookup_is_case_insensitive() {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "text/plain").unwrap();
    assert_eq!(headers.get("content-type"), Some("text/plain"));
    assert_eq!(headers.get("CONTENT-TYPE"), Some("text/plain"));
    assert_eq!(headers.get("missing"), None);
}

#[test]
fn headers_preserve_duplicates_and_order() {
    let mut headers = HeaderMap::new();
    headers.insert("Set-Cookie", "a=1").unwrap();
    headers.insert("X-Other", "y").unwrap();
    headers.insert("set-cookie", "b=2").unwrap();

    let all: Vec<&str> = headers.get_all("SET-COOKIE").collect();
    assert_eq!(all, alloc::vec!["a=1", "b=2"]);

    let order: Vec<&str> = headers.iter().map(|(n, _)| n).collect();
    assert_eq!(order, alloc::vec!["Set-Cookie", "X-Other", "set-cookie"]);
}

#[test]
fn headers_reject_injection() {
    let mut headers = HeaderMap::new();
    assert_eq!(
        headers.insert("X-Test", "a\r\nInjected: 1"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(
        headers.insert("X-Test", "a\0b"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(
        headers.insert("Bad Name", "x"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(
        headers.insert("X-Test:", "x"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(headers.insert("", "x"), Err(HttpError::MalformedHeader));
    assert!(headers.is_empty());
}

#[test]
fn headers_trim_optional_whitespace() {
    let mut headers = HeaderMap::new();
    headers.insert("X-Test", "  value \t").unwrap();
    assert_eq!(headers.get("x-test"), Some("value"));
}

#[test]
fn headers_contains_token() {
    let mut headers = HeaderMap::new();
    headers.insert("Connection", "keep-alive, Upgrade").unwrap();
    assert!(headers.contains_token("connection", "upgrade"));
    assert!(!headers.contains_token("connection", "close"));
}

// ── Status line ──

#[test]
fn status_parse_ok() {
    let (version, code, reason) = parse_status_line(b"HTTP/1.1 200 OK\r\n").unwrap();
    assert_eq!(version, HttpVersion::H11);
    assert_eq!(code, 200);
    assert_eq!(reason, "OK");
}

#[test]
fn status_parse_http10_and_no_reason() {
    let (version, code, reason) = parse_status_line(b"HTTP/1.0 404\r\n").unwrap();
    assert_eq!(version, HttpVersion::H10);
    assert_eq!(code, 404);
    assert_eq!(reason, "");

    let (_, code, reason) = parse_status_line(b"HTTP/1.1 204 \r\n").unwrap();
    assert_eq!(code, 204);
    assert_eq!(reason, "");
}

#[test]
fn status_parse_rejects_garbage() {
    let cases: &[&[u8]] = &[
        b"HTTP/2.0 200 OK\r\n",
        b"HTTP/1.1 20 OK\r\n",
        b"HTTP/1.1 999 X\r\n",
        b"HTTP/1.1 abc X\r\n",
        b"HTTP/1.1 200OK\r\n",
        b"HTTP/1.1 200 OK\n",
        b"garbage\r\n",
        b"\r\n",
    ];
    for raw in cases {
        assert_eq!(
            parse_status_line(raw),
            Err(HttpError::MalformedStatus),
            "{raw:?}"
        );
    }
}

// ── Header line ──

#[test]
fn header_line_trims_ows() {
    let (name, value) = parse_header_line(b"Content-Length:  42 \t\r\n").unwrap();
    assert_eq!(name, "Content-Length");
    assert_eq!(value, "42");

    let (_, empty) = parse_header_line(b"X-Empty:\r\n").unwrap();
    assert_eq!(empty, "");
}

#[test]
fn header_line_rejects_obs_fold_and_bad_names() {
    assert_eq!(
        parse_header_line(b" folded\r\n"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(
        parse_header_line(b"NoColon\r\n"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(
        parse_header_line(b"Bad Name: x\r\n"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(
        parse_header_line(b"X: a\rb\r\n"),
        Err(HttpError::MalformedHeader)
    );
    assert_eq!(parse_header_line(b"\r\n"), Err(HttpError::MalformedHeader));
}

// ── Head scanner ──

const SMALL_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nServer: neo\r\nContent-Length: 4\r\n\r\nBODY";

#[test]
fn scanner_single_read_returns_head_and_consumed() {
    let limits = Limits::default();
    let mut scanner = HeadScanner::new(&limits);
    let (head, consumed) = scanner.scan(SMALL_HEAD).unwrap().unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.version, HttpVersion::H11);
    assert_eq!(head.reason, "OK");
    assert_eq!(head.headers.get("server"), Some("neo"));
    assert_eq!(&SMALL_HEAD[consumed..], b"BODY");
}

#[test]
fn scanner_handles_byte_at_a_time_reads() {
    let limits = Limits::default();
    let mut scanner = HeadScanner::new(&limits);
    let mut buf = Vec::new();
    let mut found = None;

    for (i, byte) in SMALL_HEAD.iter().enumerate() {
        buf.push(*byte);
        if let Some((head, consumed)) = scanner.scan(&buf).unwrap() {
            found = Some((head, consumed, i + 1));
            break;
        }
    }

    let (head, consumed, fed) = found.expect("head must complete");
    assert_eq!(head.status, 200);
    // `consumed` is the whole head; only the bytes fed so far are buffered.
    assert_eq!(consumed, fed);
    assert_eq!(consumed, SMALL_HEAD.len() - 4);
}

#[test]
fn scanner_waits_without_terminator() {
    let limits = Limits::default();
    let mut scanner = HeadScanner::new(&limits);
    assert_eq!(
        scanner.scan(b"HTTP/1.1 200 OK\r\nServer: neo\r\n").unwrap(),
        None
    );
}

#[test]
fn scanner_enforces_byte_limit() {
    let limits = Limits {
        max_header_bytes: 16,
        ..Limits::default()
    };
    let mut scanner = HeadScanner::new(&limits);
    assert_eq!(
        scanner.scan(b"HTTP/1.1 200 OK\r\nX-Long: 0123456789"),
        Err(HttpError::TooManyHeaders)
    );
}

#[test]
fn scanner_enforces_header_count() {
    let limits = Limits {
        max_headers: 2,
        ..Limits::default()
    };
    let mut scanner = HeadScanner::new(&limits);
    let raw = b"HTTP/1.1 200 OK\r\nA: 1\r\nB: 2\r\nC: 3\r\n\r\n";
    assert_eq!(scanner.scan(raw), Err(HttpError::TooManyHeaders));
}

#[test]
fn scanner_reports_malformed_status() {
    let limits = Limits::default();
    let mut scanner = HeadScanner::new(&limits);
    assert_eq!(
        scanner.scan(b"NOT-HTTP\r\n\r\n"),
        Err(HttpError::MalformedStatus)
    );
}

// ── Request ──

#[test]
fn request_get_head_is_well_formed() {
    let url = Url::parse("http://example.com/p?q=1").unwrap();
    let head = Request::get(url).build_head().unwrap();
    assert_eq!(
        head,
        b"GET /p?q=1 HTTP/1.1\r\n\
Host: example.com\r\n\
Connection: close\r\n\
User-Agent: NeoDOS-neocurl/0.1\r\n\
Accept-Encoding: identity\r\n\
\r\n"
    );
}

#[test]
fn request_host_includes_non_default_port() {
    let url = Url::parse("http://example.com:8080/").unwrap();
    let head = Request::get(url).build_head().unwrap();
    let text = core::str::from_utf8(&head).unwrap();
    assert!(text.contains("\r\nHost: example.com:8080\r\n"));
}

#[test]
fn request_post_sets_content_length() {
    let url = Url::parse("http://example.com/form").unwrap();
    let head = Request::post(url, b"a=1&b=2").build_head().unwrap();
    let text = core::str::from_utf8(&head).unwrap();
    assert!(text.starts_with("POST /form HTTP/1.1\r\n"));
    assert!(text.contains("\r\nContent-Length: 7\r\n"));
    assert!(text.ends_with("\r\n\r\n"));
}

#[test]
fn request_post_empty_body_sends_zero_length() {
    let url = Url::parse("http://example.com/form").unwrap();
    let head = Request::post(url, b"").build_head().unwrap();
    let text = core::str::from_utf8(&head).unwrap();
    assert!(text.contains("\r\nContent-Length: 0\r\n"));
}

#[test]
fn request_head_has_no_content_length() {
    let url = Url::parse("http://example.com/").unwrap();
    let head = Request::head(url).build_head().unwrap();
    let text = core::str::from_utf8(&head).unwrap();
    assert!(!text.to_ascii_lowercase().contains("content-length"));
}

#[test]
fn request_rejects_body_on_get_and_head() {
    let url = Url::parse("http://example.com/").unwrap();
    let mut request = Request::get(url);
    request.body = Some(BodySource::Bytes(b"x"));
    assert_eq!(request.build_head(), Err(HttpError::InvalidRequest));
}

#[test]
fn request_rejects_reserved_headers() {
    let url = Url::parse("http://example.com/").unwrap();
    for name in [
        "Host",
        "connection",
        "Content-Length",
        "Transfer-Encoding",
        "Accept-Encoding",
    ] {
        let result = Request::get(url.clone()).header(name, "x");
        assert_eq!(result, Err(HttpError::MalformedHeader), "{name}");
    }
}

#[test]
fn request_appends_custom_headers_in_order() {
    let url = Url::parse("http://example.com/").unwrap();
    let request = Request::get(url)
        .header("Content-Type", "text/plain")
        .unwrap()
        .header("X-Trace", "abc")
        .unwrap();
    let head = request.build_head().unwrap();
    let text = core::str::from_utf8(&head).unwrap();
    let ct = text.find("\r\nContent-Type: text/plain\r\n").unwrap();
    let trace = text.find("\r\nX-Trace: abc\r\n").unwrap();
    assert!(ct < trace);
}

#[test]
fn request_header_cannot_inject_crlf() {
    let url = Url::parse("http://example.com/").unwrap();
    assert_eq!(
        Request::get(url).header("X-Test", "a\r\nHost: evil"),
        Err(HttpError::MalformedHeader)
    );
}

// ── Limits and deadlines ──

#[test]
fn limits_defaults_are_valid() {
    let limits = Limits::default();
    assert!(limits.validate().is_ok());
    assert!(limits.has_body_limit());
}

#[test]
fn limits_reject_zero_values() {
    assert_eq!(
        Limits {
            max_headers: 0,
            ..Limits::default()
        }
        .validate(),
        Err(HttpError::InvalidRequest)
    );
    assert_eq!(
        Limits {
            read_timeout_ms: 0,
            ..Limits::default()
        }
        .validate(),
        Err(HttpError::InvalidRequest)
    );
    let unlimited = Limits {
        max_body_bytes: 0,
        ..Limits::default()
    };
    assert!(unlimited.validate().is_ok());
    assert!(!unlimited.has_body_limit());
}

#[test]
fn deadline_arithmetic() {
    let dl = Deadline::after(1_000, 500);
    assert_eq!(dl.at_ms, 1_500);
    assert!(!dl.expired_at(1_499));
    assert!(dl.expired_at(1_500));
    assert!(dl.expired_at(2_000));
    assert_eq!(dl.remaining_ms(1_000), 500);
    assert_eq!(dl.remaining_ms(1_500), 0);

    // Saturating, never wrapping.
    let far = Deadline::after(u64::MAX - 1, 1_000);
    assert_eq!(far.at_ms, u64::MAX);
}

#[test]
fn error_messages_are_stable() {
    assert_eq!(HttpError::Timeout.to_string(), "timed out");
    assert_eq!(HttpError::MalformedStatus.as_str(), "malformed status line");
}

// ── Body framing and streaming ──

use alloc::collections::VecDeque;
use alloc::string::String;

/// Scripted transport: returns the queued chunks in order, then EOF.
struct Scripted {
    chunks: VecDeque<Vec<u8>>,
    fail: Option<HttpError>,
}

impl Scripted {
    fn new(chunks: &[&[u8]]) -> Self {
        Scripted {
            chunks: chunks.iter().map(|chunk| chunk.to_vec()).collect(),
            fail: None,
        }
    }

    fn one_byte_reads(payload: &[u8]) -> Self {
        Scripted {
            chunks: payload.iter().map(|byte| alloc::vec![*byte]).collect(),
            fail: None,
        }
    }

    fn failing(err: HttpError) -> Self {
        Scripted {
            chunks: VecDeque::new(),
            fail: Some(err),
        }
    }
}

impl Read for Scripted {
    fn read(&mut self, buf: &mut [u8], _dl: Deadline) -> Result<usize, HttpError> {
        if let Some(err) = self.fail {
            return Err(err);
        }
        match self.chunks.pop_front() {
            Some(chunk) => {
                assert!(chunk.len() <= buf.len());
                buf[..chunk.len()].copy_from_slice(&chunk);
                Ok(chunk.len())
            }
            None => Ok(0),
        }
    }
}

struct FailingSink;

impl BodySink for FailingSink {
    fn write(&mut self, _chunk: &[u8]) -> Result<(), HttpError> {
        Err(HttpError::Io)
    }
}

fn response_head(status: u16, headers: &[(&str, &str)]) -> ResponseHead {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.insert(name, value).unwrap();
    }
    ResponseHead {
        version: HttpVersion::H11,
        status,
        reason: String::new(),
        headers: map,
    }
}

fn deadline() -> Deadline {
    Deadline::after(0, 10_000)
}

#[test]
fn framing_head_and_bodiless_statuses() {
    let with_length = [("Content-Length", "5")];
    let head = response_head(200, &with_length);
    assert_eq!(body_framing(Method::Head, &head), Ok(BodyFraming::Empty));

    for status in [100u16, 101, 204, 304] {
        let head = response_head(status, &with_length);
        assert_eq!(
            body_framing(Method::Get, &head),
            Ok(BodyFraming::Empty),
            "{status}"
        );
    }
}

#[test]
fn framing_content_length() {
    let head = response_head(200, &[("Content-Length", "42")]);
    assert_eq!(
        body_framing(Method::Get, &head),
        Ok(BodyFraming::Length(42))
    );
}

#[test]
fn framing_duplicate_identical_content_length() {
    let head = response_head(200, &[("Content-Length", "5"), ("Content-Length", "5")]);
    assert_eq!(body_framing(Method::Get, &head), Ok(BodyFraming::Length(5)));

    let list = response_head(200, &[("Content-Length", "5, 5")]);
    assert_eq!(body_framing(Method::Get, &list), Ok(BodyFraming::Length(5)));
}

#[test]
fn framing_conflicting_content_length() {
    let head = response_head(200, &[("Content-Length", "5"), ("Content-Length", "6")]);
    assert_eq!(
        body_framing(Method::Get, &head),
        Err(HttpError::MalformedHeader)
    );

    let bad = response_head(200, &[("Content-Length", "5x")]);
    assert_eq!(
        body_framing(Method::Get, &bad),
        Err(HttpError::MalformedHeader)
    );
}

#[test]
fn framing_chunked_beats_content_length() {
    let head = response_head(
        200,
        &[("Transfer-Encoding", "chunked"), ("Content-Length", "5")],
    );
    assert_eq!(body_framing(Method::Get, &head), Ok(BodyFraming::Chunked));
}

#[test]
fn framing_unknown_transfer_coding() {
    for value in ["gzip", "gzip, chunked", "chunked, gzip"] {
        let head = response_head(200, &[("Transfer-Encoding", value)]);
        assert_eq!(
            body_framing(Method::Get, &head),
            Err(HttpError::UnsupportedTransfer),
            "{value}"
        );
    }
}

#[test]
fn framing_content_encoding_policy() {
    let gzip = response_head(
        200,
        &[("Content-Encoding", "gzip"), ("Content-Length", "5")],
    );
    assert_eq!(
        body_framing(Method::Get, &gzip),
        Err(HttpError::UnsupportedEncoding)
    );

    let identity = response_head(
        200,
        &[("Content-Encoding", "identity"), ("Content-Length", "5")],
    );
    assert_eq!(
        body_framing(Method::Get, &identity),
        Ok(BodyFraming::Length(5))
    );
}

#[test]
fn framing_without_length_is_close_delimited() {
    let head = response_head(200, &[("Server", "neo")]);
    assert_eq!(
        body_framing(Method::Get, &head),
        Ok(BodyFraming::CloseDelimited)
    );
}

#[test]
fn body_content_length_reads_exactly() {
    let mut reader = BodyReader::new(Scripted::new(&[b"hello", b"world"]), deadline());
    let mut sink = VecSink::new();
    let n = reader
        .read_body(BodyFraming::Length(8), &Limits::default(), &mut sink)
        .unwrap();
    assert_eq!(n, 8);
    assert_eq!(sink.buf, b"hellowor");
}

#[test]
fn body_content_length_short_is_incomplete() {
    let mut reader = BodyReader::new(Scripted::new(&[b"hello"]), deadline());
    let mut sink = VecSink::new();
    assert_eq!(
        reader.read_body(BodyFraming::Length(8), &Limits::default(), &mut sink),
        Err(HttpError::IncompleteBody)
    );
}

#[test]
fn body_content_length_limit_is_checked_before_reading() {
    let mut reader = BodyReader::new(Scripted::new(&[b"0123456789"]), deadline());
    let mut sink = VecSink::new();
    let limits = Limits {
        max_body_bytes: 4,
        ..Limits::default()
    };
    assert_eq!(
        reader.read_body(BodyFraming::Length(10), &limits, &mut sink),
        Err(HttpError::BodyTooLarge)
    );
    assert!(sink.buf.is_empty());
}

#[test]
fn body_chunked_basic() {
    let payload = b"5\r\nhello\r\n0\r\n\r\n";
    let mut reader = BodyReader::new(Scripted::new(&[payload]), deadline());
    let mut sink = VecSink::new();
    let n = reader
        .read_body(BodyFraming::Chunked, &Limits::default(), &mut sink)
        .unwrap();
    assert_eq!(n, 5);
    assert_eq!(sink.buf, b"hello");
}

#[test]
fn body_chunked_across_one_byte_reads() {
    let payload = b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
    let mut reader = BodyReader::new(Scripted::one_byte_reads(payload), deadline());
    let mut sink = VecSink::new();
    let n = reader
        .read_body(BodyFraming::Chunked, &Limits::default(), &mut sink)
        .unwrap();
    assert_eq!(n, 5);
    assert_eq!(sink.buf, b"abcde");
}

#[test]
fn body_chunked_ignores_extensions_and_trailers() {
    let payload = b"5;name=value\r\nhello\r\n0\r\nX-Sum: 1\r\nX-Other:\r\n\r\n";
    let mut reader = BodyReader::new(Scripted::new(&[payload]), deadline());
    let mut sink = VecSink::new();
    let n = reader
        .read_body(BodyFraming::Chunked, &Limits::default(), &mut sink)
        .unwrap();
    assert_eq!(n, 5);
    assert_eq!(sink.buf, b"hello");
}

#[test]
fn body_chunked_rejects_bad_size_line() {
    for payload in [&b"zz\r\n"[..], b"\r\n", b"00000000000000001\r\n"] {
        let mut reader = BodyReader::new(Scripted::new(&[payload]), deadline());
        let mut sink = NullSink;
        assert_eq!(
            reader.read_body(BodyFraming::Chunked, &Limits::default(), &mut sink),
            Err(HttpError::MalformedChunk),
            "{payload:?}"
        );
    }
}

#[test]
fn body_chunked_rejects_missing_data_crlf() {
    let payload = b"3\r\nabcXX";
    let mut reader = BodyReader::new(Scripted::new(&[payload]), deadline());
    let mut sink = VecSink::new();
    assert_eq!(
        reader.read_body(BodyFraming::Chunked, &Limits::default(), &mut sink),
        Err(HttpError::MalformedChunk)
    );
}

#[test]
fn body_chunked_incomplete_is_detected() {
    let payload = b"5\r\nhel";
    let mut reader = BodyReader::new(Scripted::new(&[payload]), deadline());
    let mut sink = VecSink::new();
    assert_eq!(
        reader.read_body(BodyFraming::Chunked, &Limits::default(), &mut sink),
        Err(HttpError::IncompleteBody)
    );
}

#[test]
fn body_chunked_enforces_total_limit() {
    let payload = b"4\r\nabcd\r\n4\r\nefgh\r\n0\r\n\r\n";
    let mut reader = BodyReader::new(Scripted::new(&[payload]), deadline());
    let mut sink = VecSink::new();
    let limits = Limits {
        max_body_bytes: 5,
        ..Limits::default()
    };
    assert_eq!(
        reader.read_body(BodyFraming::Chunked, &limits, &mut sink),
        Err(HttpError::BodyTooLarge)
    );
    assert_eq!(sink.buf, b"abcd");
}

#[test]
fn body_chunked_trailer_limit() {
    let payload = b"0\r\nA: 1\r\nB: 2\r\n\r\n";
    let mut reader = BodyReader::new(Scripted::new(&[payload]), deadline());
    let mut sink = NullSink;
    let limits = Limits {
        max_headers: 1,
        ..Limits::default()
    };
    assert_eq!(
        reader.read_body(BodyFraming::Chunked, &limits, &mut sink),
        Err(HttpError::TooManyHeaders)
    );
}

#[test]
fn body_close_delimited_reads_to_eof() {
    let mut reader = BodyReader::new(Scripted::new(&[b"abc", b"def"]), deadline());
    let mut sink = VecSink::new();
    let n = reader
        .read_body(BodyFraming::CloseDelimited, &Limits::default(), &mut sink)
        .unwrap();
    assert_eq!(n, 6);
    assert_eq!(sink.buf, b"abcdef");
}

#[test]
fn body_close_delimited_enforces_limit() {
    let mut reader = BodyReader::new(Scripted::new(&[b"abcdef"]), deadline());
    let mut sink = VecSink::new();
    let limits = Limits {
        max_body_bytes: 4,
        ..Limits::default()
    };
    assert_eq!(
        reader.read_body(BodyFraming::CloseDelimited, &limits, &mut sink),
        Err(HttpError::BodyTooLarge)
    );
}

#[test]
fn body_uses_leftover_bytes_from_the_head_read() {
    let mut reader = BodyReader::with_buffered(Scripted::new(&[b"TAIL"]), b"BODY", deadline());
    let mut sink = VecSink::new();
    let n = reader
        .read_body(BodyFraming::Length(8), &Limits::default(), &mut sink)
        .unwrap();
    assert_eq!(n, 8);
    assert_eq!(sink.buf, b"BODYTAIL");
}

#[test]
fn body_empty_writes_nothing() {
    let mut reader = BodyReader::new(Scripted::new(&[b"ignored"]), deadline());
    let mut sink = VecSink::new();
    assert_eq!(
        reader.read_body(BodyFraming::Empty, &Limits::default(), &mut sink),
        Ok(0)
    );
    assert!(sink.buf.is_empty());
}

#[test]
fn body_sink_error_propagates() {
    let mut reader = BodyReader::new(Scripted::new(&[b"x"]), deadline());
    assert_eq!(
        reader.read_body(BodyFraming::Length(1), &Limits::default(), &mut FailingSink),
        Err(HttpError::Io)
    );
}

#[test]
fn body_transport_error_propagates() {
    let mut reader = BodyReader::new(Scripted::failing(HttpError::Timeout), deadline());
    let mut sink = NullSink;
    assert_eq!(
        reader.read_body(BodyFraming::Length(1), &Limits::default(), &mut sink),
        Err(HttpError::Timeout)
    );
}

#[test]
fn chunk_size_parser_accepts_hex_and_extensions() {
    assert_eq!(crate::body::parse_chunk_size(b"a"), Ok(10));
    assert_eq!(crate::body::parse_chunk_size(b"A"), Ok(10));
    assert_eq!(crate::body::parse_chunk_size(b"1f"), Ok(31));
    assert_eq!(crate::body::parse_chunk_size(b"5;name=value"), Ok(5));
    assert_eq!(
        crate::body::parse_chunk_size(b""),
        Err(HttpError::MalformedChunk)
    );
    assert_eq!(
        crate::body::parse_chunk_size(b"0x10"),
        Err(HttpError::MalformedChunk)
    );
    assert_eq!(
        crate::body::parse_chunk_size(b"00000000000000000"),
        Err(HttpError::MalformedChunk)
    );
}

// ── Request/response flow (`execute`) ──

use alloc::rc::Rc;
use core::cell::RefCell;

#[derive(Default)]
struct Recorded {
    written: Vec<u8>,
    closed: bool,
}

struct FakeConn {
    reads: VecDeque<Vec<u8>>,
    recorded: Rc<RefCell<Recorded>>,
    read_error: Option<HttpError>,
}

impl Read for FakeConn {
    fn read(&mut self, buf: &mut [u8], _dl: Deadline) -> Result<usize, HttpError> {
        if let Some(err) = self.read_error {
            return Err(err);
        }
        match self.reads.pop_front() {
            None => Ok(0),
            Some(chunk) => {
                let n = chunk.len().min(buf.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                if n < chunk.len() {
                    self.reads.push_front(chunk[n..].to_vec());
                }
                Ok(n)
            }
        }
    }
}

impl Write for FakeConn {
    fn write_all(&mut self, buf: &[u8], _dl: Deadline) -> Result<(), HttpError> {
        self.recorded.borrow_mut().written.extend_from_slice(buf);
        Ok(())
    }
}

impl Close for FakeConn {
    fn close(self) {
        self.recorded.borrow_mut().closed = true;
    }
}

struct FakeTransport {
    reads: VecDeque<Vec<u8>>,
    recorded: Rc<RefCell<Recorded>>,
    read_error: Option<HttpError>,
    connect_error: Option<HttpError>,
}

impl Transport for FakeTransport {
    type Conn = FakeConn;

    fn connect(&mut self, _host: &str, _port: u16, _dl: Deadline) -> Result<FakeConn, HttpError> {
        if let Some(err) = self.connect_error {
            return Err(err);
        }
        Ok(FakeConn {
            reads: core::mem::take(&mut self.reads),
            recorded: self.recorded.clone(),
            read_error: self.read_error,
        })
    }
}

fn fake(chunks: &[&[u8]]) -> (FakeTransport, Rc<RefCell<Recorded>>) {
    let recorded = Rc::new(RefCell::new(Recorded::default()));
    let transport = FakeTransport {
        reads: chunks.iter().map(|chunk| chunk.to_vec()).collect(),
        recorded: recorded.clone(),
        read_error: None,
        connect_error: None,
    };
    (transport, recorded)
}

fn request(path: &str) -> Request<'static> {
    Request::get(Url::parse(path).unwrap())
}

#[test]
fn execute_get_content_length_streams_body_and_closes() {
    let (mut transport, recorded) = fake(&[b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello"]);
    let mut sink = VecSink::new();

    let head = execute(
        &mut transport,
        &request("http://example.com/x"),
        &Limits::default(),
        deadline(),
        &mut sink,
    )
    .unwrap();

    assert_eq!(head.status, 200);
    assert_eq!(sink.buf, b"hello");
    let rec = recorded.borrow();
    assert!(rec.written.starts_with(b"GET /x HTTP/1.1\r\n"));
    assert!(rec.written.ends_with(b"\r\n\r\n"));
    assert!(rec.closed);
}

#[test]
fn execute_chunked_body_split_across_reads() {
    let (mut transport, recorded) = fake(&[
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nab",
        b"c\r\n0\r\n\r\n",
    ]);
    let mut sink = VecSink::new();

    execute(
        &mut transport,
        &request("http://example.com/"),
        &Limits::default(),
        deadline(),
        &mut sink,
    )
    .unwrap();

    assert_eq!(sink.buf, b"abc");
    assert!(recorded.borrow().closed);
}

#[test]
fn execute_head_has_no_body() {
    let (mut transport, recorded) = fake(&[b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello"]);
    let mut sink = VecSink::new();

    let head = execute(
        &mut transport,
        &Request::head(Url::parse("http://example.com/").unwrap()),
        &Limits::default(),
        deadline(),
        &mut sink,
    )
    .unwrap();

    assert_eq!(head.status, 200);
    assert!(sink.buf.is_empty());
    assert!(recorded.borrow().closed);
}

#[test]
fn execute_skips_informational_responses() {
    let (mut transport, _) =
        fake(&[b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"]);
    let mut sink = VecSink::new();

    let head = execute(
        &mut transport,
        &request("http://example.com/"),
        &Limits::default(),
        deadline(),
        &mut sink,
    )
    .unwrap();

    assert_eq!(head.status, 200);
    assert_eq!(sink.buf, b"ok");
}

#[test]
fn execute_bounds_informational_responses() {
    let mut payload = Vec::new();
    for _ in 0..(MAX_INFORMATIONAL_RESPONSES + 1) {
        payload.extend_from_slice(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let (mut transport, recorded) = fake(&[&payload]);
    let mut sink = NullSink;

    assert_eq!(
        execute(
            &mut transport,
            &request("http://example.com/"),
            &Limits::default(),
            deadline(),
            &mut sink,
        ),
        Err(HttpError::TooManyHeaders)
    );
    assert!(recorded.borrow().closed);
}

#[test]
fn execute_post_sends_body_after_head() {
    let (mut transport, recorded) = fake(&[b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"]);
    let mut sink = NullSink;
    let url = Url::parse("http://example.com/form").unwrap();
    let req = Request::post(url, b"a=1");

    execute(
        &mut transport,
        &req,
        &Limits::default(),
        deadline(),
        &mut sink,
    )
    .unwrap();

    let written = recorded.borrow();
    let text = core::str::from_utf8(&written.written).unwrap();
    assert!(text.starts_with("POST /form HTTP/1.1\r\n"));
    assert!(text.contains("\r\nContent-Length: 3\r\n"));
    assert!(text.ends_with("\r\n\r\na=1"));
}

#[test]
fn execute_close_delimited_body_reads_to_eof() {
    let (mut transport, _) = fake(&[b"HTTP/1.1 200 OK\r\nServer: neo\r\n\r\nabc", b"def"]);
    let mut sink = VecSink::new();

    execute(
        &mut transport,
        &request("http://example.com/"),
        &Limits::default(),
        deadline(),
        &mut sink,
    )
    .unwrap();

    assert_eq!(sink.buf, b"abcdef");
}

#[test]
fn execute_malformed_status_closes_connection() {
    let (mut transport, recorded) = fake(&[b"GARBAGE\r\n\r\n"]);
    let mut sink = NullSink;

    assert_eq!(
        execute(
            &mut transport,
            &request("http://example.com/"),
            &Limits::default(),
            deadline(),
            &mut sink,
        ),
        Err(HttpError::MalformedStatus)
    );
    assert!(recorded.borrow().closed);
}

#[test]
fn execute_eof_before_head_is_incomplete() {
    let (mut transport, recorded) = fake(&[]);
    let mut sink = NullSink;

    assert_eq!(
        execute(
            &mut transport,
            &request("http://example.com/"),
            &Limits::default(),
            deadline(),
            &mut sink,
        ),
        Err(HttpError::IncompleteBody)
    );
    assert!(recorded.borrow().closed);
}

#[test]
fn execute_body_limit_closes_connection() {
    let (mut transport, recorded) =
        fake(&[b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n0123456789"]);
    let mut sink = VecSink::new();
    let limits = Limits {
        max_body_bytes: 4,
        ..Limits::default()
    };

    assert_eq!(
        execute(
            &mut transport,
            &request("http://example.com/"),
            &limits,
            deadline(),
            &mut sink,
        ),
        Err(HttpError::BodyTooLarge)
    );
    assert!(recorded.borrow().closed);
}

#[test]
fn execute_read_timeout_closes_connection() {
    let recorded = Rc::new(RefCell::new(Recorded::default()));
    let mut transport = FakeTransport {
        reads: VecDeque::new(),
        recorded: recorded.clone(),
        read_error: Some(HttpError::Timeout),
        connect_error: None,
    };
    let mut sink = NullSink;

    assert_eq!(
        execute(
            &mut transport,
            &request("http://example.com/"),
            &Limits::default(),
            deadline(),
            &mut sink,
        ),
        Err(HttpError::Timeout)
    );
    assert!(recorded.borrow().closed);
}

#[test]
fn execute_connect_error_is_returned() {
    let recorded = Rc::new(RefCell::new(Recorded::default()));
    let mut transport = FakeTransport {
        reads: VecDeque::new(),
        recorded: recorded.clone(),
        read_error: None,
        connect_error: Some(HttpError::Connect),
    };
    let mut sink = NullSink;

    assert_eq!(
        execute(
            &mut transport,
            &request("http://example.com/"),
            &Limits::default(),
            deadline(),
            &mut sink,
        ),
        Err(HttpError::Connect)
    );
    assert!(!recorded.borrow().closed);
}

struct HeadSink {
    status: Option<u16>,
    body: Vec<u8>,
}

impl BodySink for HeadSink {
    fn start(&mut self, head: &ResponseHead) {
        self.status = Some(head.status);
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), HttpError> {
        assert!(self.status.is_some(), "start() must run before body bytes");
        self.body.extend_from_slice(chunk);
        Ok(())
    }
}

#[test]
fn execute_notifies_sink_before_body() {
    let (mut transport, _) = fake(&[b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nhi"]);
    let mut sink = HeadSink {
        status: None,
        body: Vec::new(),
    };

    let head = execute(
        &mut transport,
        &request("http://example.com/"),
        &Limits::default(),
        deadline(),
        &mut sink,
    )
    .unwrap();

    assert_eq!(head.status, 201);
    assert_eq!(sink.status, Some(201));
    assert_eq!(sink.body, b"hi");
}
