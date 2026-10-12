//! Shared UDP request/reply exchange over `net.nxl` sockets (#593).
//!
//! `libnet::dns` and `ntpd` need the same sequence: create a UDP socket, bind,
//! connect, retry the send while the peer's ARP entry resolves, then wait for
//! the reply within an RDTSC time budget. Defining it once keeps the retry
//! count and the receive-wait logic identical between the two callers.

use libneodos::syscall;

/// `SocketType::Udp` as encoded in the Ob attrs (see `net.nxl`).
pub const SOCKET_TYPE_UDP: u32 = 2;
/// Retries for the send while the peer's ARP entry resolves.
pub const SEND_ATTEMPTS: usize = 50;
/// Spin iterations between send retries.
pub const SEND_RETRY_SPINS: u32 = 2000;
/// Safety cap on receive-loop iterations (independent of the TSC budget).
pub const RECV_MAX_SPINS: u32 = 2_000_000;

/// Failure of a UDP exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpError {
    /// `socket_create` failed.
    Create,
    /// `socket_bind` failed.
    Bind,
    /// `socket_connect` failed.
    Connect,
    /// The datagram could not be sent (peer unreachable / ARP unresolved).
    Send,
    /// No reply within the RDTSC budget.
    Timeout,
}

#[inline]
fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Create → bind → connect a UDP socket to `remote_ip:remote_port`, send
/// `request` (retrying while ARP resolves), receive one reply into `resp`
/// within `recv_ticks` (RDTSC), and close the socket.
///
/// `remote_ip` is the big-endian IPv4 form used by the sockets API. Returns the
/// reply length on success.
pub fn exchange(
    remote_ip: u32,
    remote_port: u16,
    request: &[u8],
    resp: &mut [u8],
    recv_ticks: u64,
) -> Result<usize, UdpError> {
    let fd = crate::socket_create(SOCKET_TYPE_UDP);
    if fd < 0 {
        return Err(UdpError::Create);
    }
    if crate::socket_bind(fd, 0, 0) < 0 {
        let _ = crate::socket_close(fd);
        return Err(UdpError::Bind);
    }
    if crate::socket_connect(fd, remote_ip, remote_port) < 0 {
        let _ = crate::socket_close(fd);
        return Err(UdpError::Connect);
    }

    // `arp_resolve` is fire-and-forget: on a cache miss it transmits an ARP
    // request and returns None immediately, and the reply is processed later by
    // netd. Yield and retry the send so the ARP cache can be populated before
    // we give up (otherwise the first datagram to a new peer always fails).
    let mut sent = -1i32;
    for _ in 0..SEND_ATTEMPTS {
        sent = crate::socket_send(fd, request);
        if sent >= 0 {
            break;
        }
        syscall::sys_yield();
        for _ in 0..SEND_RETRY_SPINS {
            core::hint::spin_loop();
        }
    }
    if sent < 0 {
        let _ = crate::socket_close(fd);
        return Err(UdpError::Send);
    }

    // netd drives packet polling; wait for the reply here within a real time
    // budget (a bare `sys_yield` loop returns too early because yield does not
    // sleep).
    let start = rdtsc();
    let mut received = 0usize;
    let mut spins = 0u32;
    loop {
        let n = crate::socket_recv(fd, resp);
        if n > 0 {
            received = n as usize;
            break;
        }
        spins += 1;
        if spins > RECV_MAX_SPINS || rdtsc().wrapping_sub(start) > recv_ticks {
            break;
        }
        syscall::sys_yield();
    }

    let _ = crate::socket_close(fd);

    if received == 0 {
        Err(UdpError::Timeout)
    } else {
        Ok(received)
    }
}
