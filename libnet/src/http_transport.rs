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

impl Read for TcpConn {
    fn read(&mut self, buf: &mut [u8], dl: Deadline) -> Result<usize, HttpError> {
        let now = now_ms()?;
        let idle = Deadline::after(now, self.limits.read_timeout_ms);

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
