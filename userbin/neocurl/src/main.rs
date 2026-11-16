#![no_std]
#![no_main]

//! `neocurl` — HTTP/1.1 client for NeoDOS.
//!
//! The CLI only parses flags, chooses an output sink and reports errors: all
//! protocol work lives in `libhttp`, and all networking in `libnet`.
//!
//! ```text
//! neocurl URL                    GET; body to stdout
//! neocurl -I URL                 HEAD; response headers to stdout
//! neocurl -i URL                 include response headers before the body
//! neocurl -d DATA URL            POST (application/x-www-form-urlencoded)
//! neocurl -d @- URL              POST with the body read from stdin
//! neocurl -o FILE URL            stream the body to a file
//! neocurl --max-time MS URL      total deadline in milliseconds
//! neocurl --max-bytes N URL      body limit (0 = unlimited)
//! neocurl -s URL                 silent: no status summary on stderr
//! ```
//!
//! Exit codes: `0` success, `1` transfer/parse error, `2` usage error.

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};

use libhttp::{execute, BodySink, Deadline, HttpError, Limits, ResponseHead, Scheme, Url};
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

const USAGE: &[u8] = b"usage: neocurl [-I] [-i] [-s] [-d DATA | -d @-] [-o FILE] [--max-time MS] [--max-bytes N] URL\r\n";
const MAX_STDIN_BODY: usize = 256 * 1024;
const WRITE_CHUNK: usize = 4096;

#[derive(Default)]
struct Options<'a> {
    url: Option<&'a [u8]>,
    head: bool,
    include: bool,
    silent: bool,
    data: Option<&'a [u8]>,
    data_stdin: bool,
    out: Option<&'a [u8]>,
    max_time_ms: Option<u32>,
    max_bytes: Option<u64>,
}

enum ParseError {
    Help,
    Message(&'static str),
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = args::read_args();
    let code = match run(args::trim_ascii(&raw)) {
        Ok(()) => 0,
        Err(code) => code,
    };
    syscall::sys_exit(code)
}

fn run(raw: &[u8]) -> Result<(), u32> {
    let opts = match parse_args(raw) {
        Ok(opts) => opts,
        Err(ParseError::Help) => {
            let _ = syscall::sys_write(1, USAGE);
            return Ok(());
        }
        Err(ParseError::Message(msg)) => {
            err(b"neocurl: ");
            err(msg.as_bytes());
            err(b"\r\n");
            let _ = syscall::sys_write(2, USAGE);
            return Err(2);
        }
    };

    let Some(url_bytes) = opts.url else {
        let _ = syscall::sys_write(2, USAGE);
        return Err(2);
    };
    let url_str = match core::str::from_utf8(url_bytes) {
        Ok(s) => s,
        Err(_) => {
            fail("invalid URL");
            return Err(1);
        }
    };
    let url = match Url::parse(url_str) {
        Ok(url) => url,
        Err(err) => {
            fail(err.as_str());
            return Err(1);
        }
    };
    if url.scheme != Scheme::Http {
        fail("HTTPS is not supported yet (libtls pending)");
        return Err(1);
    }

    let mut limits = Limits::default();
    if let Some(ms) = opts.max_time_ms {
        limits.total_timeout_ms = ms;
    }
    if let Some(bytes) = opts.max_bytes {
        limits.max_body_bytes = bytes;
    }

    // Request body (POST) either from the CLI or from stdin.
    let mut stdin_body: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    if opts.data_stdin {
        if let Err(msg) = read_stdin(&mut stdin_body) {
            fail(msg);
            return Err(1);
        }
    }

    let request = if opts.head {
        if opts.data.is_some() || opts.data_stdin {
            let _ = syscall::sys_write(2, USAGE);
            return Err(2);
        }
        libhttp::Request::head(url)
    } else if opts.data_stdin {
        libhttp::Request::post(url, &stdin_body)
    } else if let Some(data) = opts.data {
        libhttp::Request::post(url, data)
    } else {
        libhttp::Request::get(url)
    };

    // Output sink: stdout or a freshly created file.
    let mut out_fd = 1u8;
    if let Some(path) = opts.out {
        let Ok(path) = core::str::from_utf8(path) else {
            fail("invalid output path");
            return Err(1);
        };
        match syscall::ob_file_create(path) {
            Ok(fd) => out_fd = fd,
            Err(_) => {
                fail("cannot create output file");
                return Err(1);
            }
        }
    }

    let now = syscall::ob_uptime_ms().unwrap_or(0);
    let deadline = Deadline::after(now, limits.total_timeout_ms);

    let mut transport = TcpTransport::new(limits);
    let mut sink = WriteSink {
        fd: out_fd,
        print_headers: opts.include || opts.head,
        count: 0,
    };

    match execute(&mut transport, &request, &limits, deadline, &mut sink) {
        Ok(head) => {
            if !opts.silent {
                err(b"neocurl: ");
                write_dec(2, head.status as u32);
                err(b" ");
                write_u64(2, sink.count);
                err(b" bytes\r\n");
            }
            if out_fd != 1 {
                let _ = syscall::sys_close(out_fd);
            }
            Ok(())
        }
        Err(err) => {
            if out_fd != 1 {
                let _ = syscall::sys_close(out_fd);
            }
            fail(err.as_str());
            Err(1)
        }
    }
}

/// Output sink that streams the body and optionally prints the head first.
struct WriteSink {
    fd: u8,
    print_headers: bool,
    count: u64,
}

impl BodySink for WriteSink {
    fn start(&mut self, head: &ResponseHead) {
        if self.print_headers {
            print_head(self.fd, head);
        }
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), HttpError> {
        let mut off = 0usize;
        while off < chunk.len() {
            let end = (off + WRITE_CHUNK).min(chunk.len());
            match syscall::sys_write(self.fd, &chunk[off..end]) {
                Ok(n) if n > 0 => off += n,
                _ => return Err(HttpError::Io),
            }
        }
        self.count += chunk.len() as u64;
        Ok(())
    }
}

fn parse_args(raw: &[u8]) -> Result<Options<'_>, ParseError> {
    let mut tokens: [&[u8]; 32] = [&[]; 32];
    let mut count = 0usize;
    for token in raw.split(|b| *b == b' ' || *b == b'\t') {
        if token.is_empty() {
            continue;
        }
        if count == tokens.len() {
            return Err(ParseError::Message("too many arguments"));
        }
        tokens[count] = token;
        count += 1;
    }

    let mut opts = Options::default();
    let mut i = 0usize;
    while i < count {
        let token = tokens[i];
        if token == b"-h" || token == b"--help" {
            return Err(ParseError::Help);
        } else if token == b"-I" || token == b"--head" {
            opts.head = true;
        } else if token == b"-i" {
            opts.include = true;
        } else if token == b"-s" || token == b"--silent" {
            opts.silent = true;
        } else if token == b"-d" {
            i += 1;
            let value = *tokens
                .get(i)
                .ok_or(ParseError::Message("-d needs a value"))?;
            if value == b"@-" {
                opts.data_stdin = true;
            } else {
                opts.data = Some(value);
            }
        } else if token == b"-o" {
            i += 1;
            let value = *tokens
                .get(i)
                .ok_or(ParseError::Message("-o needs a file"))?;
            opts.out = Some(value);
        } else if token == b"--max-time" {
            i += 1;
            let value = tokens
                .get(i)
                .ok_or(ParseError::Message("--max-time needs milliseconds"))?;
            opts.max_time_ms =
                Some(parse_u32(value).ok_or(ParseError::Message("invalid --max-time"))?);
        } else if token == b"--max-bytes" {
            i += 1;
            let value = tokens
                .get(i)
                .ok_or(ParseError::Message("--max-bytes needs a number"))?;
            opts.max_bytes =
                Some(parse_u64(value).ok_or(ParseError::Message("invalid --max-bytes"))?);
        } else if token.first() == Some(&b'-') {
            return Err(ParseError::Message("unknown option"));
        } else if opts.url.is_some() {
            return Err(ParseError::Message("only one URL is supported"));
        } else {
            opts.url = Some(token);
        }
        i += 1;
    }

    Ok(opts)
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    bytes.iter().try_fold(0u32, |acc, b| {
        acc.checked_mul(10)?.checked_add((*b - b'0') as u32)
    })
}

fn parse_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    bytes.iter().try_fold(0u64, |acc, b| {
        acc.checked_mul(10)?.checked_add((*b - b'0') as u64)
    })
}

fn read_stdin(out: &mut alloc::vec::Vec<u8>) -> Result<(), &'static str> {
    let mut buf = [0u8; 1024];
    loop {
        match syscall::sys_read(0, &mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                if out.len() + n > MAX_STDIN_BODY {
                    return Err("request body too large");
                }
                out.extend_from_slice(&buf[..n]);
            }
            Err(_) => return Err("cannot read stdin"),
        }
    }
}

fn print_head(fd: u8, head: &ResponseHead) {
    let _ = syscall::sys_write(fd, head.version.as_str().as_bytes());
    let _ = syscall::sys_write(fd, b" ");
    write_dec(fd, head.status as u32);
    if !head.reason.is_empty() {
        let _ = syscall::sys_write(fd, b" ");
        let _ = syscall::sys_write(fd, head.reason.as_bytes());
    }
    let _ = syscall::sys_write(fd, b"\r\n");
    for (name, value) in head.headers.iter() {
        let _ = syscall::sys_write(fd, name.as_bytes());
        let _ = syscall::sys_write(fd, b": ");
        let _ = syscall::sys_write(fd, value.as_bytes());
        let _ = syscall::sys_write(fd, b"\r\n");
    }
    let _ = syscall::sys_write(fd, b"\r\n");
}

fn err(bytes: &[u8]) {
    let _ = syscall::sys_write(2, bytes);
}

fn fail(message: &str) {
    err(b"neocurl: ");
    err(message.as_bytes());
    err(b"\r\n");
}

fn write_dec(fd: u8, value: u32) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    let mut value = value;
    if value == 0 {
        let _ = syscall::sys_write(fd, b"0");
        return;
    }
    while value > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    let _ = syscall::sys_write(fd, &buf[i..]);
}

fn write_u64(fd: u8, mut value: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if value == 0 {
        let _ = syscall::sys_write(fd, b"0");
        return;
    }
    while value > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    let _ = syscall::sys_write(fd, &buf[i..]);
}
