//! Socket, hostname and ICMP syscall wrappers.

use super::{sys_ob_open, sys_ob_query_info, sys_ob_set_info, sys_ob_create, sys_close, ObInfoClass, ObSetInfoClass, ob_type};

pub fn sys_get_hostname(buf: &mut [u8]) -> Result<usize, i64> {
    let fd = sys_ob_open("\\Global\\Info\\Network", 1)?;
    let r = sys_ob_query_info(fd, ObInfoClass::Hostname, buf);
    let _ = sys_close(fd);
    r
}

pub fn sys_set_hostname(name: &str) -> Result<(), i64> {
    let fd = sys_ob_open("\\Global\\Info\\Network", 3)?;
    let r = sys_ob_set_info(fd, ObSetInfoClass::SetHostname, name.as_bytes());
    let _ = sys_close(fd);
    r
}

// ── Socket wrappers ──

/// SocketAddrV4 — ABI-compatible address for socket operations.
/// IP is network-byte-order (big-endian), port is host-byte-order.
#[repr(C)]
pub struct SocketAddrV4 {
    pub ip: [u8; 4],
    pub port: u16,
}

impl SocketAddrV4 {
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        SocketAddrV4 { ip, port }
    }
}

/// ob_socket_create: create a socket via ob_create(Socket).
/// `sock_type` = 1 (TCP), 2 (UDP), 3 (Raw). `port` = local port hint (0 = ephemeral).
pub fn ob_socket_create(path: &str, sock_type: u32, port: u16) -> Result<u8, i64> {
    let attrs = (sock_type & 0xFF) as u64 | ((port as u64) << 8);
    sys_ob_create(path, ob_type::SOCKET, None, attrs)
}

/// ob_socket_connect: connect to a remote address via ob_set_info(SocketConnect).
pub fn ob_socket_connect(fd: u8, ip: [u8; 4], port: u16) -> Result<(), i64> {
    let mut buf = [0u8; 6];
    buf[..4].copy_from_slice(&ip);
    buf[4..6].copy_from_slice(&port.to_be_bytes());
    sys_ob_set_info(fd, ObSetInfoClass::SocketConnect, &buf)
}

/// ob_socket_bind: bind to a local address via ob_set_info(SocketBind).
pub fn ob_socket_bind(fd: u8, ip: [u8; 4], port: u16) -> Result<(), i64> {
    let mut buf = [0u8; 6];
    buf[..4].copy_from_slice(&ip);
    buf[4..6].copy_from_slice(&port.to_be_bytes());
    sys_ob_set_info(fd, ObSetInfoClass::SocketBind, &buf)
}

/// ob_socket_listen: start listening via ob_set_info(SocketListen).
pub fn ob_socket_listen(fd: u8) -> Result<(), i64> {
    sys_ob_set_info(fd, ObSetInfoClass::SocketListen, &[])
}

/// ob_socket_send: send data via ob_set_info(SocketSend).
/// Returns number of bytes sent on success.
pub fn ob_socket_send(fd: u8, data: &[u8]) -> Result<usize, i64> {
    let r = unsafe { ob_syscall_4!(43, fd as u64, ObSetInfoClass::SocketSend as u32 as u64, data.as_ptr() as u64, data.len() as u64) };
    if r < 0 { Err(r) } else { Ok(r as usize) }
}

/// ob_socket_recv: receive data via ob_query_info(SocketRecv).
pub fn ob_socket_recv(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    sys_ob_query_info(fd, ObInfoClass::SocketRecv, buf)
}

/// ob_socket_close: close a socket via ob_set_info(SocketClose).
pub fn ob_socket_close(fd: u8) -> Result<(), i64> {
    sys_ob_set_info(fd, ObSetInfoClass::SocketClose, &[])
}

/// RAX 36: icmp_ping(ipv4_addr_be32) -> rtt_us
/// Sends an ICMP echo request and returns RTT in microseconds, or 0 on failure.
pub fn sys_icmp_ping(ip: u32) -> u64 {
    let r: u64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rax, 36",
            "mov rbx, {ip}",
            "int 0x80",
            "pop rbx",
            ip = in(reg) ip as u64,
            out("rax") r,
            options(nostack),
        );
    }
    r
}
