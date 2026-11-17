//! `libhttp::Transport` over `net.nxl` TCP sockets.
//!
//! The HTTP core never touches a socket: this module implements its transport
//! seam with the userland socket API. It owns the deadline logic:
//!
//! * connect waits for `Established` (or fails) within the caller's deadline;
//! * `read`/`write_all` bound both the total deadline and their own inactivity
//!   window from [`Limits`];
//! * a full send buffer (`EAGAIN`) waits for space with `wait_type = 2`;
//! * `SocketRecv` returning `0` is EOF.
//!
//! DNS uses `libnet::dns` (A records, IPv4 literals bypass the resolver).

use core::sync::atomic::{AtomicUsize, Ordering};

use libhttp::{Close, Deadline, HttpError, Limits, Read, Transport, Write};
use libneodos::syscall;

/// `SocketType::Tcp` in the `net.nxl` ABI.
pub const SOCKET_TYPE_TCP: u32 = 1;
/// Kernel `TcpState::Established as u32`.
const TCP_ESTABLISHED: u32 = 4;
/// Kernel `TcpState::Closed as u32`.
const TCP_CLOSED: u32 = 0;

/// Factory for TCP connections.
#[derive(Debug, Clone, Copy)]
pub struct TcpTransport {
    limits: Limits,
}

impl TcpTransport {
    /// Transport that applies `limits` to each connection.
    pub fn new(limits: Limits) -> Self {
        TcpTransport { limits }
    }
}

/// One connected TCP socket.
pub struct TcpConn {
    fd: i32,
    limits: Limits,
}

impl Transport for TcpTransport {
    type Conn = TcpConn;

    fn connect(&mut self, host: &str, port: u16, dl: Deadline) -> Result<TcpConn, HttpError> {
        let ip = resolve(host)?;

        let fd = crate::socket_create(SOCKET_TYPE_TCP);
        if fd < 0 {
            return Err(HttpError::Connect);
        }
        let conn = TcpConn {
            fd,
            limits: self.limits,
        };

        if crate::socket_bind(fd, 0, 0) < 0 {
            conn.close();
            return Err(HttpError::Connect);
        }
        if crate::socket_connect(fd, u32::from_be_bytes(ip), port) < 0 {
            conn.close();
            return Err(HttpError::Connect);
        }

        loop {
            match tcp_state(fd) {
                Ok(TCP_ESTABLISHED) => return Ok(conn),
                Ok(TCP_CLOSED) => {
                    conn.close();
                    return Err(HttpError::Connect);
                }
                Ok(_) => {}
                Err(err) => {
                    conn.close();
                    return Err(err);
                }
            }

            let now = match now_ms() {
                Ok(now) => now,
                Err(err) => {
                    conn.close();
                    return Err(err);
                }
            };
            if dl.expired_at(now) {
                conn.close();
                return Err(HttpError::Timeout);
            }
            let _ = syscall::sys_ob_wait_timeout(fd as u8, dl.remaining_ms(now) as u64);
        }
    }
}

/// Fault the pages backing `buf` into the process before handing it to the
/// kernel.
///
/// Heap pages are demand-paged: a buffer that crosses into a page the process
/// has not touched yet is genuinely unmapped, and the kernel refuses to copy
/// into non-resident user memory (it returns `EFAULT` rather than faulting in
/// Ring 0). Reading the first byte of every page in user mode maps them first.
fn touch_pages(buf: &mut [u8]) {
    if buf.is_empty() {
        return;
    }
    let start = buf.as_ptr() as usize;
    let end = start + buf.len();
    let mut page = start & !0xfff;
    while page < end {
        let offset = page - start;
        // A user-mode read demand-faults the page in; volatile so it is kept.
        unsafe { core::ptr::read_volatile(&buf[offset]) };
        page += 4096;
    }
}

impl Read for TcpConn {
    fn read(&mut self, buf: &mut [u8], dl: Deadline) -> Result<usize, HttpError> {
        let now = now_ms()?;
        let idle = Deadline::after(now, self.limits.read_timeout_ms);
        touch_pages(buf);

        loop {
            let n = crate::socket_recv(self.fd, buf);
            if n >= 0 {
                return Ok(n as usize);
            }
            if n != syscall::EAGAIN as i32 {
                return Err(HttpError::Io);
            }

            let now = now_ms()?;
            if dl.expired_at(now) || idle.expired_at(now) {
                return Err(HttpError::Timeout);
            }
            let rem = dl.remaining_ms(now).min(idle.remaining_ms(now));
            if rem == 0 {
                return Err(HttpError::Timeout);
            }
            let _ = syscall::sys_ob_wait_timeout(self.fd as u8, rem as u64);
        }
    }
}

impl Write for TcpConn {
    fn write_all(&mut self, buf: &[u8], dl: Deadline) -> Result<(), HttpError> {
        let now = now_ms()?;
        let idle = Deadline::after(now, self.limits.write_timeout_ms);

        let mut off = 0usize;
        while off < buf.len() {
            let n = crate::socket_send(self.fd, &buf[off..]);
            if n > 0 {
                off += n as usize;
                continue;
            }
            if n != syscall::EAGAIN as i32 {
                return Err(HttpError::Io);
            }

            let now = now_ms()?;
            if dl.expired_at(now) || idle.expired_at(now) {
                return Err(HttpError::Timeout);
            }
            let rem = dl.remaining_ms(now).min(idle.remaining_ms(now));
            if rem == 0 {
                return Err(HttpError::Timeout);
            }
            let _ = syscall::sys_ob_wait_write_timeout(self.fd as u8, rem as u64);
        }
        Ok(())
    }
}

impl Close for TcpConn {
    fn close(self) {
        let _ = crate::socket_close(self.fd);
    }
}

fn now_ms() -> Result<u64, HttpError> {
    syscall::ob_uptime_ms().map_err(|_| HttpError::Io)
}

fn tcp_state(fd: i32) -> Result<u32, HttpError> {
    let mut buf = [0u8; 4];
    match syscall::sys_ob_query_info(fd as u8, syscall::ObInfoClass::TcpStatus, &mut buf) {
        Ok(4) => Ok(u32::from_ne_bytes(buf)),
        _ => Err(HttpError::Io),
    }
}

/// Resolve `host` to an IPv4 address; literals never hit DNS.
fn resolve(host: &str) -> Result<[u8; 4], HttpError> {
    if let Some(ip) = crate::dns::parse_dotted_ip(host) {
        return Ok(ip);
    }
    match crate::dns::resolve(host) {
        Ok(answer) => answer.first().ok_or(HttpError::Dns),
        Err(crate::dns::DnsError::Timeout) => Err(HttpError::Timeout),
        Err(_) => Err(HttpError::Dns),
    }
}

// ── HTTPS (M10/M11) ──

use libtls::{ClientConfig, Host, Rng, TlsError, Transport as TlsTransportTrait};

/// Kernel CSPRNG backed by `\Device\Random`; fails closed unless the pool is
/// backed by a hardware source (M8).
struct KernelRng;

impl Rng for KernelRng {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), TlsError> {
        let caps = syscall::ob_random_caps().map_err(|_| TlsError::RngUnavailable)?;
        if !caps.hardware_backed() {
            return Err(TlsError::RngUnavailable);
        }
        syscall::ob_random(buf).map_err(|_| TlsError::RngUnavailable)?;
        Ok(())
    }
}

/// `libtls::Transport` over the TCP socket wrapper, borrowing the deadline.
struct TlsTcp {
    conn: Option<TcpConn>,
    dl: Deadline,
}

impl TlsTransportTrait for TlsTcp {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, TlsError> {
        let Some(conn) = self.conn.as_mut() else {
            return Ok(0);
        };
        Read::read(conn, buf, self.dl).map_err(|_| TlsError::Io)
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let Some(conn) = self.conn.as_mut() else {
            return Err(TlsError::Io);
        };
        Write::write_all(conn, data, self.dl).map_err(|_| TlsError::Io)
    }

    fn close(&mut self) {
        if let Some(conn) = self.conn.take() {
            conn.close();
        }
    }
}

// ── TLS failure detail ──

/// `HttpError::Tls` carries no payload (the HTTP core does not depend on
/// `libtls`), so the transport records the reason here for CLIs to print.
static LAST_TLS_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_TLS_LEN: AtomicUsize = AtomicUsize::new(0);

fn record_tls_error(err: TlsError) {
    let text = err.as_str();
    LAST_TLS_LEN.store(text.len(), Ordering::Relaxed);
    LAST_TLS_PTR.store(text.as_ptr() as usize, Ordering::Relaxed);
}

/// Reason for the most recent `HttpError::Tls` in this process, or `""`.
pub fn last_tls_error() -> &'static str {
    let ptr = LAST_TLS_PTR.load(Ordering::Relaxed) as *const u8;
    let len = LAST_TLS_LEN.load(Ordering::Relaxed);
    if ptr.is_null() || len == 0 {
        return "";
    }
    // SAFETY: `record_tls_error` only stores `&'static str` literals.
    unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len)) }
}

/// HTTPS transport: TCP + TLS 1.3 with a caller-provided trust anchor list.
pub struct HttpsTransport {
    limits: Limits,
    roots: alloc::vec::Vec<alloc::vec::Vec<u8>>,
}

impl HttpsTransport {
    pub fn new(limits: Limits, roots: alloc::vec::Vec<alloc::vec::Vec<u8>>) -> Self {
        HttpsTransport { limits, roots }
    }
}

/// One TLS connection.
pub struct HttpsConn {
    stream: libtls::TlsStream<TlsTcp>,
}

impl Transport for HttpsTransport {
    type Conn = HttpsConn;

    fn connect(&mut self, host: &str, port: u16, dl: Deadline) -> Result<HttpsConn, HttpError> {
        let tcp = TcpTransport::new(self.limits).connect(host, port, dl)?;
        let clock = libclock::system::SystemClock;
        let mut rng = KernelRng;
        let (host_kind, sni) = match libdns::parse_dotted_ip(host) {
            Some(ip) => (Host::Ip(ip), None),
            None => (Host::Dns(host), Some(host)),
        };
        let cfg = ClientConfig {
            roots: &self.roots,
            clock: &clock,
            rng: &mut rng,
            server_name: sni,
            host: host_kind,
        };
        let stream = libtls::connect(TlsTcp { conn: Some(tcp), dl }, cfg).map_err(|e| {
            record_tls_error(e);
            HttpError::Tls
        })?;
        Ok(HttpsConn { stream })
    }
}

impl Read for HttpsConn {
    fn read(&mut self, buf: &mut [u8], _dl: Deadline) -> Result<usize, HttpError> {
        self.stream.read(buf).map_err(|_| HttpError::Io)
    }
}

impl Write for HttpsConn {
    fn write_all(&mut self, buf: &[u8], _dl: Deadline) -> Result<(), HttpError> {
        self.stream.write_all(buf).map_err(|_| HttpError::Io)
    }
}

impl Close for HttpsConn {
    fn close(self) {
        self.stream.close();
    }
}
