#![no_std]
#![no_main]

//! `httptest` — HTTP/1.1 acceptance runner for NeoDOS.
//!
//! Runs a fixed matrix against a fixture server (see `tools/http-e2e`) and
//! prints one line per case plus a final summary the host-side E2E driver can
//! grep from the serial log:
//!
//! ```text
//! httptest http://10.0.2.2:18080
//!
//! PASS get_hello 200 16
//! ...
//! HTTP-E2E: PASS 11/11
//! ```
//!
//! Exit codes: `0` all cases passed, `1` at least one failed, `2` usage error.
//! The body is streamed into a counting sink (no filesystem writes), so the
//! 1 MiB case measures the network stack, not the FS.

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};

use libhttp::{execute, BodySink, Deadline, HttpError, Limits, Request, ResponseHead, Scheme, Url};
use libneodos::{args, mem, syscall};
use libnet::http_transport::TcpTransport;

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() {
            core::ptr::null_mut()
        } else {
            ptr
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

const USAGE: &[u8] = b"usage: httptest BASE_URL   (e.g. httptest http://10.0.2.2:18080)\r\n";

const HELLO_BODY: &[u8] = b"hello-from-host\n";
const CHUNKED_BODY: &[u8] = b"chunk-one-chunk-two-chunk-three\n";
const CLOSE_BODY: &[u8] = b"close-delimited-body\n";
const POST_BODY: &[u8] = b"a=1";
const POST_ECHO: &[u8] = b"POST 3 a=1";
const BIG_BYTES: u64 = 65536;
const HUGE_BYTES: u64 = 1048576;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = args::read_args();
    let code = run(args::trim_ascii(&raw));
    syscall::sys_exit(code)
}

fn run(raw: &[u8]) -> u32 {
    let Some(base_bytes) = first_arg(raw) else {
        let _ = syscall::sys_write(2, USAGE);
        return 2;
    };
    let Ok(base) = core::str::from_utf8(base_bytes) else {
        fail("invalid base URL");
        return 2;
    };
    let base = base.trim_end_matches('/');
    let parsed = match Url::parse(base) {
        Ok(url) => url,
        Err(err) => {
            fail(err.as_str());
            return 2;
        }
    };
    if parsed.scheme != Scheme::Http {
        fail("HTTPS is not supported yet (libtls pending)");
        return 2;
    }

    // Diagnostic only: the matrix does not depend on the wall clock, but the
    // E2E log records whether `ntpd` has made it trustworthy (M7).
    match libclock::system::trusted_now_unix() {
        Ok(secs) => {
            out_str(1, b"CLOCK: trusted ");
            out_u64(1, secs as u64);
            out_str(1, b"\r\n");
        }
        Err(err) => {
            out_str(1, b"CLOCK: untrusted (");
            out_str(1, err.as_str().as_bytes());
            out_str(1, b")\r\n");
        }
    }

    // Diagnostic + smoke: the kernel CSPRNG must be reachable and produce
    // varying bytes (M8). The HTTP matrix itself does not consume randomness.
    out_str(1, b"RNG: ");
    let caps = libneodos::syscall::ob_random_caps();
    let mut probe = [0u8; 32];
    let filled = libneodos::syscall::ob_random(&mut probe);
    match (caps, filled) {
        (Ok(caps), Ok(32)) => {
            out_str(
                1,
                if caps.hardware_backed() {
                    b"hardware"
                } else {
                    b"pool-only"
                },
            );
            out_str(1, b" rdrand=");
            out_dec(1, caps.hw_rdrand as u32);
            out_str(1, b" rdseed=");
            out_dec(1, caps.hw_rdseed as u32);
            let varied = probe.iter().any(|b| *b != probe[0]);
            out_str(1, if varied { b" varied" } else { b" STATIC" });
            out_str(1, b"\r\n");
        }
        _ => out_str(1, b"unavailable\r\n"),
    }

    let mut t = Tests {
        base: alloc::string::String::from(base),
        host: parsed.host.clone(),
        total: 0,
        failed: 0,
    };

    t.get("get_hello", "/hello", 200, Some(HELLO_BODY), None);
    t.head("head_hello", "/hello", 200);
    t.get("get_chunked", "/chunked", 200, Some(CHUNKED_BODY), None);
    t.post("post_echo", "/post", 200, POST_ECHO);
    t.get("status_404", "/status/404", 404, None, None);
    t.get("get_close", "/close", 200, Some(CLOSE_BODY), None);
    t.get("get_big", "/big", 200, None, Some(BIG_BYTES));
    t.get("get_huge", "/huge", 200, None, Some(HUGE_BYTES));
    t.expect_timeout("timeout_slow", "/slow");
    t.expect_connect("connect_refused", 1);
    t.expect_dns("dns_failure");

    out_str(1, b"HTTP-E2E: ");
    if t.failed == 0 {
        out_str(1, b"PASS ");
    } else {
        out_str(1, b"FAIL ");
    }
    out_dec(1, (t.total - t.failed) as u32);
    out_str(1, b"/");
    out_dec(1, t.total as u32);
    out_str(1, b"\r\n");

    if t.failed == 0 { 0 } else { 1 }
}

/// First whitespace-separated token of the process arguments.
fn first_arg(raw: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    while i < raw.len() && (raw[i] == b' ' || raw[i] == b'\t') {
        i += 1;
    }
    let start = i;
    while i < raw.len() && raw[i] != b' ' && raw[i] != b'\t' {
        i += 1;
    }
    if i == start {
        None
    } else {
        Some(&raw[start..i])
    }
}

struct Tests {
    base: alloc::string::String,
    host: alloc::string::String,
    total: usize,
    failed: usize,
}

#[derive(Clone, Copy)]
enum WantErr {
    Timeout,
    Connect,
    Dns,
}

/// Sink that counts the body and, when an exact body is expected, keeps enough
/// bytes to compare it.
struct CheckSink {
    expect: Option<&'static [u8]>,
    got: alloc::vec::Vec<u8>,
    count: u64,
    overflow: bool,
}

impl CheckSink {
    fn new(expect: Option<&'static [u8]>) -> Self {
        CheckSink {
            expect,
            got: alloc::vec::Vec::new(),
            count: 0,
            overflow: false,
        }
    }

    fn body_matches(&self) -> bool {
        match self.expect {
            Some(exp) => !self.overflow && self.got == exp,
            None => true,
        }
    }
}

impl BodySink for CheckSink {
    fn write(&mut self, chunk: &[u8]) -> Result<(), HttpError> {
        self.count += chunk.len() as u64;
        if let Some(exp) = self.expect {
            if !self.overflow && self.got.len() + chunk.len() <= exp.len() + 1 {
                self.got.extend_from_slice(chunk);
            } else {
                self.overflow = true;
            }
        }
        Ok(())
    }
}

fn run_request(
    request: Request<'static>,
    limits: &Limits,
    sink: &mut CheckSink,
) -> Result<ResponseHead, HttpError> {
    let now = syscall::ob_uptime_ms().unwrap_or(0);
    let deadline = Deadline::after(now, limits.total_timeout_ms);
    let mut transport = TcpTransport::new(*limits);
    execute(&mut transport, &request, limits, deadline, sink)
}

impl Tests {
    fn get(
        &mut self,
        name: &'static str,
        path: &str,
        expect_status: u16,
        expect_body: Option<&'static [u8]>,
        expect_len: Option<u64>,
    ) {
        let Some(url) = self.url(path) else {
            self.report(name, false, b"invalid URL");
            return;
        };
        let limits = Limits::default();
        let mut sink = CheckSink::new(expect_body);
        match run_request(Request::get(url), &limits, &mut sink) {
            Ok(head) => {
                let len_ok = match expect_len {
                    Some(n) => sink.count == n,
                    None => true,
                };
                let ok = head.status == expect_status && sink.body_matches() && len_ok;
                self.report_status(name, ok, head.status, sink.count);
            }
            Err(err) => self.report(name, false, err.as_str().as_bytes()),
        }
    }

    fn head(&mut self, name: &'static str, path: &str, expect_status: u16) {
        let Some(url) = self.url(path) else {
            self.report(name, false, b"invalid URL");
            return;
        };
        let limits = Limits::default();
        let mut sink = CheckSink::new(None);
        match run_request(Request::head(url), &limits, &mut sink) {
            Ok(head) => {
                let ok = head.status == expect_status && sink.count == 0;
                self.report_status(name, ok, head.status, sink.count);
            }
            Err(err) => self.report(name, false, err.as_str().as_bytes()),
        }
    }

    fn post(
        &mut self,
        name: &'static str,
        path: &str,
        expect_status: u16,
        expect_body: &'static [u8],
    ) {
        let Some(url) = self.url(path) else {
            self.report(name, false, b"invalid URL");
            return;
        };
        let limits = Limits::default();
        let mut sink = CheckSink::new(Some(expect_body));
        match run_request(Request::post(url, POST_BODY), &limits, &mut sink) {
            Ok(head) => {
                let ok = head.status == expect_status && sink.body_matches();
                self.report_status(name, ok, head.status, sink.count);
            }
            Err(err) => self.report(name, false, err.as_str().as_bytes()),
        }
    }

    fn expect_timeout(&mut self, name: &'static str, path: &str) {
        let mut limits = Limits::default();
        limits.total_timeout_ms = 500;
        limits.read_timeout_ms = 500;
        let Some(url) = self.url(path) else {
            self.report(name, false, b"invalid URL");
            return;
        };
        self.expect_error(name, url, &limits, WantErr::Timeout);
    }

    fn expect_connect(&mut self, name: &'static str, port: u16) {
        let mut limits = Limits::default();
        limits.total_timeout_ms = 2_000;
        limits.read_timeout_ms = 2_000;
        let raw = alloc::format!("http://{}:{}/", self.host, port);
        self.expect_error_raw(name, &raw, &limits, WantErr::Connect);
    }

    fn expect_dns(&mut self, name: &'static str) {
        let mut limits = Limits::default();
        limits.total_timeout_ms = 5_000;
        self.expect_error_raw(name, "http://nonexistent.invalid/", &limits, WantErr::Dns);
    }

    fn expect_error_raw(&mut self, name: &'static str, raw: &str, limits: &Limits, want: WantErr) {
        let Ok(url) = Url::parse(raw) else {
            self.report(name, false, b"invalid URL");
            return;
        };
        self.expect_error(name, url, limits, want);
    }

    fn expect_error(&mut self, name: &'static str, url: Url, limits: &Limits, want: WantErr) {
        let mut sink = CheckSink::new(None);
        let result = run_request(Request::get(url), limits, &mut sink);
        let ok = match (&result, want) {
            (Err(HttpError::Timeout), WantErr::Timeout) => true,
            (Err(HttpError::Connect), WantErr::Connect) => true,
            // A refused port may also surface as the connect deadline.
            (Err(HttpError::Timeout), WantErr::Connect) => true,
            (Err(HttpError::Dns), WantErr::Dns) => true,
            _ => false,
        };
        match result {
            Ok(head) => self.report_status(name, ok, head.status, sink.count),
            Err(err) => self.report(name, ok, err.as_str().as_bytes()),
        }
    }

    fn url(&self, path: &str) -> Option<Url> {
        let raw = alloc::format!("{}{}", self.base, path);
        Url::parse(&raw).ok()
    }

    fn report_status(&mut self, name: &'static str, ok: bool, status: u16, bytes: u64) {
        self.total += 1;
        if ok {
            out_str(1, b"PASS ");
            out_str(1, name.as_bytes());
            out_str(1, b" ");
            out_dec(1, status as u32);
            out_str(1, b" ");
            out_u64(1, bytes);
            out_str(1, b"\r\n");
        } else {
            self.failed += 1;
            out_str(1, b"FAIL ");
            out_str(1, name.as_bytes());
            out_str(1, b": status ");
            out_dec(1, status as u32);
            out_str(1, b", ");
            out_u64(1, bytes);
            out_str(1, b" bytes\r\n");
        }
    }

    fn report(&mut self, name: &'static str, ok: bool, detail: &[u8]) {
        self.total += 1;
        if ok {
            out_str(1, b"PASS ");
            out_str(1, name.as_bytes());
            out_str(1, b"\r\n");
        } else {
            self.failed += 1;
            out_str(1, b"FAIL ");
            out_str(1, name.as_bytes());
            out_str(1, b": ");
            out_str(1, detail);
            out_str(1, b"\r\n");
        }
    }
}

fn out_str(fd: u8, s: &[u8]) {
    let _ = syscall::sys_write(fd, s);
}

fn out_dec(fd: u8, mut value: u32) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    out_str(fd, &buf[i..]);
}

fn out_u64(fd: u8, mut value: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    out_str(fd, &buf[i..]);
}

fn fail(msg: &str) {
    out_str(2, b"httptest: ");
    out_str(2, msg.as_bytes());
    out_str(2, b"\r\n");
}
