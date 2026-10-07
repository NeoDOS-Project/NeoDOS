use super::types::{Ipv4Addr, SocketAddrV4, TcpState};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::net::ipv4::{Ipv4Header, IPV4_HDR_MIN_LEN, IPV4_PROTO_TCP};

pub const TCP_HDR_MIN_LEN: usize = 20;
pub const TCP_DEFAULT_WINDOW: u16 = 65535;
pub const TCP_MSS: usize = 1460;
pub const TCP_SEND_BUF: usize = 16384;
pub const TCP_RECV_BUF: usize = 16384;
pub const TCP_MAX_CONNECTIONS: usize = 32;
/// Retransmission timeout for data/SYN/FIN (#486).
pub const TCP_RTO_US: u64 = 200_000;
/// Give up (Closed) after this many unanswered retransmits.
pub const TCP_MAX_RETRIES: u32 = 8;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset_reserved_flags: u16,
    pub window: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

impl TcpHeader {
    pub fn new(src_port: u16, dst_port: u16, seq: u32, ack: u32, flags: u8, window: u16) -> Self {
        let data_offset = (TCP_HDR_MIN_LEN / 4) as u16;
        TcpHeader {
            src_port: src_port.to_be(),
            dst_port: dst_port.to_be(),
            seq_num: seq.to_be(),
            ack_num: ack.to_be(),
            data_offset_reserved_flags: ((data_offset << 12) | flags as u16).to_be(),
            window: window.to_be(),
            checksum: 0,
            urgent_ptr: 0,
        }
    }

    pub fn src_port(&self) -> u16 { u16::from_be(self.src_port) }
    pub fn dst_port(&self) -> u16 { u16::from_be(self.dst_port) }
    pub fn seq(&self) -> u32 { u32::from_be(self.seq_num) }
    pub fn ack(&self) -> u32 { u32::from_be(self.ack_num) }
    pub fn data_offset(&self) -> usize {
        ((u16::from_be(self.data_offset_reserved_flags) >> 12) as usize) * 4
    }
    pub fn flags(&self) -> u8 {
        (u16::from_be(self.data_offset_reserved_flags) & 0xFF) as u8
    }
    pub fn window_size(&self) -> u16 { u16::from_be(self.window) }

    pub fn has_flag(&self, flag: u8) -> bool { self.flags() & flag != 0 }
    pub fn is_syn(&self) -> bool { self.has_flag(TCP_SYN) }
    pub fn is_ack(&self) -> bool { self.has_flag(TCP_ACK) }
    pub fn is_fin(&self) -> bool { self.has_flag(TCP_FIN) }
    pub fn is_rst(&self) -> bool { self.has_flag(TCP_RST) }
    pub fn is_psh(&self) -> bool { self.has_flag(TCP_PSH) }
}

pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_PSH: u8 = 0x08;
pub const TCP_ACK: u8 = 0x10;
pub const TCP_SYN_ACK: u8 = 0x12;

pub const TCP_FLAG_FIN: u8 = TCP_FIN;
pub const TCP_FLAG_SYN: u8 = TCP_SYN;
pub const TCP_FLAG_RST: u8 = TCP_RST;
pub const TCP_FLAG_PSH: u8 = TCP_PSH;
pub const TCP_FLAG_ACK: u8 = TCP_ACK;

pub fn compute_tcp_checksum(header: &TcpHeader, src_ip: [u8; 4], dst_ip: [u8; 4], payload: &[u8]) -> u16 {
    let mut sum = 0u32;

    sum = sum.wrapping_add((src_ip[0] as u32) << 8 | src_ip[1] as u32);
    sum = sum.wrapping_add((src_ip[2] as u32) << 8 | src_ip[3] as u32);
    sum = sum.wrapping_add((dst_ip[0] as u32) << 8 | dst_ip[1] as u32);
    sum = sum.wrapping_add((dst_ip[2] as u32) << 8 | dst_ip[3] as u32);
    sum = sum.wrapping_add(6u32);
    let tcp_len = (header.data_offset() + payload.len()) as u16;
    sum = sum.wrapping_add(tcp_len as u32);

    let hdr_len = header.data_offset();
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            header as *const TcpHeader as *const u8,
            hdr_len,
        )
    };
    let mut i = 0;
    while i + 1 < hdr_bytes.len() {
        let word = u16::from_be_bytes([hdr_bytes[i], hdr_bytes[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < hdr_bytes.len() {
        sum = sum.wrapping_add((hdr_bytes[i] as u32) << 8);
    }
    i = 0;
    while i + 1 < payload.len() {
        let word = u16::from_be_bytes([payload[i], payload[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < payload.len() {
        sum = sum.wrapping_add((payload[i] as u32) << 8);
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    let cs = !(sum as u16);
    if cs == 0 { 0xFFFF } else { cs }
}

#[derive(Clone)]
pub struct TcpConnection {
    pub id: u32,
    pub state: TcpState,
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub send_seq: u32,
    pub recv_seq: u32,
    pub send_ack: u32,
    /// Oldest unacknowledged sequence number (#486).
    pub send_base: u32,
    /// TSC of the last transmit carrying unacked data (RTO base).
    pub last_tx_tsc: u64,
    /// Consecutive unanswered retransmits (abort at TCP_MAX_RETRIES).
    pub retries: u32,
    /// App closed with unsent data: emit FIN once caught up (#486).
    pub fin_pending: bool,
    pub send_buf: VecDeque<u8>,
    pub recv_buf: VecDeque<u8>,
    pub window: u16,
    pub retransmit_count: u32,
    pub ob_id: u64,
}

pub struct TcpControlBlock {
    pub connections: Vec<Option<TcpConnection>>,
    pub next_id: u32,
    pub next_ephemeral_port: u16,
    pub next_ip_id: u16,
}

impl TcpControlBlock {
    pub const fn new() -> Self {
        TcpControlBlock {
            connections: Vec::new(),
            next_id: 1,
            next_ephemeral_port: 49152,
            next_ip_id: 1,
        }
    }

    pub fn alloc_connection(&mut self) -> Option<u32> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        for slot in self.connections.iter_mut() {
            if slot.is_none() {
                *slot = Some(TcpConnection {
                    id, state: TcpState::Closed,
                    local: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                    remote: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                    send_seq: 0, recv_seq: 0, send_ack: 0,
                    send_base: 0, last_tx_tsc: 0, retries: 0, fin_pending: false,
                    send_buf: VecDeque::new(),
                    recv_buf: VecDeque::new(),
                    window: TCP_DEFAULT_WINDOW,
                    retransmit_count: 0,
                    ob_id: 0,
                });
                return Some(id);
            }
        }
        if self.connections.len() < TCP_MAX_CONNECTIONS {
            self.connections.push(Some(TcpConnection {
                id, state: TcpState::Closed,
                local: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                remote: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                send_seq: 0, recv_seq: 0, send_ack: 0,
                send_base: 0, last_tx_tsc: 0, retries: 0, fin_pending: false,
                send_buf: VecDeque::new(),
                recv_buf: VecDeque::new(),
                window: TCP_DEFAULT_WINDOW,
                retransmit_count: 0,
                ob_id: 0,
            }));
            Some(id)
        } else {
            None
        }
    }

    pub fn get_connection(&self, id: u32) -> Option<&TcpConnection> {
        self.connections.iter().flatten().find(|c| c.id == id)
    }

    pub fn get_connection_mut(&mut self, id: u32) -> Option<&mut TcpConnection> {
        self.connections.iter_mut().flatten().find(|c| c.id == id)
    }

    pub fn free_connection(&mut self, id: u32) {
        if let Some(idx) = self.connections.iter().position(|s| {
            s.as_ref().is_some_and(|c| c.id == id)
        }) {
            self.connections[idx] = None;
        }
    }

    pub fn allocate_ephemeral_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if self.next_ephemeral_port == 65535 {
            49152
        } else {
            self.next_ephemeral_port.wrapping_add(1)
        };
        port
    }

    pub fn next_ip_id(&mut self) -> u16 {
        let id = self.next_ip_id;
        self.next_ip_id = self.next_ip_id.wrapping_add(1);
        id
    }

    pub fn find_connection_by_addr(&self, local: SocketAddrV4, remote: SocketAddrV4) -> Option<u32> {
        self.connections.iter().flatten().find(|c| {
            c.local == local && c.remote == remote && c.state != TcpState::Closed
        }).map(|c| c.id)
    }

    pub fn find_listener(&self, port: u16) -> Option<u32> {
        self.connections.iter().flatten().find(|c| {
            c.state == TcpState::Listen && c.local.port == port
        }).map(|c| c.id)
    }
}

lazy_static! {
    pub static ref TCP: Mutex<TcpControlBlock> = Mutex::new(TcpControlBlock::new());
}

/// Serialize transmitters: snapshot/send/commit must not interleave across
/// CPUs. Leaf-first lock (never held while taking others in reverse order).
static FLUSH_LOCK: Mutex<()> = Mutex::new(());

fn tsc_per_us() -> u64 {
    (crate::boot_benchmark::get_tsc_khz() / 1000).max(1)
}

/// Resolve the source IP for an outgoing segment.
fn resolve_src_ip(remote: Ipv4Addr, local: Ipv4Addr) -> Ipv4Addr {
    if !local.is_unspecified() {
        return local;
    }
    if remote.is_loopback() {
        return Ipv4Addr::localhost();
    }
    match crate::net::nic::nic_default_id() {
        Some(id) => crate::net::nic::nic_get_ip(id).unwrap_or(Ipv4Addr::unspecified()),
        None => Ipv4Addr::unspecified(),
    }
}

/// MAC for a TCP reply/send: loopback is synthetic, otherwise the ARP entry
/// for the next hop (gateway-aware, same rule as ICMP).
fn tcp_mac_for(dst: Ipv4Addr) -> Option<[u8; 6]> {
    if dst.is_loopback() {
        return Some(crate::net::types::MacAddr::loopback().0);
    }
    let target = crate::net::nic::nic_next_hop(dst)?;
    crate::net::arp::arp_resolve(target).map(|m| m.0)
}

/// Advertised window from free receive space.
fn adv_window(used: usize) -> u16 {
    TCP_RECV_BUF.saturating_sub(used).min(TCP_DEFAULT_WINDOW as usize) as u16
}

pub fn tcp_alloc_connection() -> Option<u32> {
    TCP.lock().alloc_connection()
}

pub fn tcp_free_connection(id: u32) {
    TCP.lock().free_connection(id);
}

pub fn tcp_bind(id: u32, local: SocketAddrV4) -> bool {
    let mut tcp = TCP.lock();
    if let Some(conn) = tcp.get_connection_mut(id) {
        conn.local = local;
        true
    } else {
        false
    }
}

pub fn tcp_listen(id: u32) -> bool {
    let mut tcp = TCP.lock();
    if let Some(conn) = tcp.get_connection_mut(id) {
        if conn.state == TcpState::Closed {
            conn.state = TcpState::Listen;
            return true;
        }
    }
    false
}

pub fn tcp_connect(id: u32, remote: SocketAddrV4) -> bool {
    let (local, syn_seq) = {
        let mut tcp = TCP.lock();
        let needs_port = {
            if let Some(conn) = tcp.get_connection(id) {
                conn.local.port == 0
            } else { false }
        };
        let port = if needs_port { Some(tcp.allocate_ephemeral_port()) } else { None };
        if let Some(conn) = tcp.get_connection_mut(id) {
            if conn.state != TcpState::Closed { return false; }
            conn.remote = remote;
            if let Some(p) = port {
                conn.local.port = p;
            }
            conn.state = TcpState::SynSent;
            // ISS = 1000: the SYN below consumes it, so next = 1001 and the
            // RTO clock starts now even if the first SYN has no route yet.
            conn.send_seq = 1001;
            conn.send_base = 1000;
            conn.recv_seq = 0;
            conn.send_ack = 0;
            conn.retries = 0;
            conn.fin_pending = false;
            conn.last_tx_tsc = crate::boot_benchmark::rdtsc();
            (conn.local, conn.send_base)
        } else {
            return false;
        }
    };
    // Best effort: tcp_tick retransmits the SYN on RTO (#486).
    // The SYN consumes send_base; send_seq already points past it.
    send_syn_segment(id, local, remote, syn_seq);
    true
}

/// Transmit a SYN for `id` (initial or retransmit). Returns false when there
/// is no route/MAC yet; the tick retries.
fn send_syn_segment(id: u32, local: SocketAddrV4, remote: SocketAddrV4, seq: u32) -> bool {
    let src_ip = resolve_src_ip(remote.ip, local.ip);
    if src_ip.is_unspecified() {
        return false;
    }
    let dst_mac = match tcp_mac_for(remote.ip) {
        Some(m) => m,
        None => return false,
    };
    let mut local = local;
    local.ip = src_ip;
    {
        let mut tcp = TCP.lock();
        if let Some(conn) = tcp.get_connection_mut(id) {
            conn.local.ip = src_ip;
        }
    }
    send_tcp_segment(
        dst_mac, src_ip.0, remote.ip.0,
        local.port, remote.port, seq, 0,
        TCP_FLAG_SYN, TCP_DEFAULT_WINDOW, &[],
    )
}

pub fn tcp_send(id: u32, data: &[u8]) -> Result<usize, ()> {
    let mut tcp = TCP.lock();
    let conn = tcp.get_connection_mut(id).ok_or(())?;
    if conn.state != TcpState::Established {
        return Err(());
    }
    let available = TCP_SEND_BUF.saturating_sub(conn.send_buf.len());
    let to_send = data.len().min(available);
    if to_send == 0 {
        return Err(());
    }
    conn.send_buf.extend(&data[..to_send]);
    Ok(to_send)
}

pub fn tcp_recv(id: u32, buf: &mut [u8]) -> Result<usize, ()> {
    let mut tcp = TCP.lock();
    let conn = tcp.get_connection_mut(id).ok_or(())?;
    let available = conn.recv_buf.len().min(buf.len());
    if available == 0 {
        return Err(());
    }
    for item in buf.iter_mut().take(available) {
        *item = conn.recv_buf.pop_front().unwrap_or(0);
    }
    Ok(available)
}

pub fn tcp_get_state(id: u32) -> Option<TcpState> {
    TCP.lock().get_connection(id).map(|c| c.state)
}

pub fn tcp_close(id: u32) {
    // Decide under lock, transmit without holding TCP (lock order).
    enum CloseOp {
        None,
        FinNow(SocketAddrV4, SocketAddrV4, u32, u32, TcpState),
        FinLater,
    }
    let op = {
        let mut tcp = TCP.lock();
        match tcp.get_connection_mut(id) {
            Some(conn) => match conn.state {
                TcpState::Established | TcpState::CloseWait => {
                    let next = if conn.state == TcpState::Established {
                        TcpState::FinWait1
                    } else {
                        TcpState::LastAck
                    };
                    let unsent = conn.send_buf.len().saturating_sub(
                        conn.send_seq.wrapping_sub(conn.send_base) as usize,
                    );
                    if unsent == 0 {
                        let op = CloseOp::FinNow(
                            conn.local, conn.remote,
                            conn.send_seq, conn.recv_seq, next,
                        );
                        conn.send_seq = conn.send_seq.wrapping_add(1);
                        conn.state = next;
                        op
                    } else {
                        conn.fin_pending = true;
                        CloseOp::FinLater
                    }
                }
                _ => {
                    conn.state = TcpState::Closed;
                    conn.fin_pending = false;
                    CloseOp::None
                }
            },
            None => CloseOp::None,
        }
    };
    if let CloseOp::FinNow(local, remote, seq, ack, _next) = op {
        send_fin_segment(local, remote, seq, ack);
    }
}

/// Transmit a FIN for an established/closing connection.
fn send_fin_segment(local: SocketAddrV4, remote: SocketAddrV4, seq: u32, ack: u32) -> bool {
    let src_ip = resolve_src_ip(remote.ip, local.ip);
    if src_ip.is_unspecified() {
        return false;
    }
    let dst_mac = match tcp_mac_for(remote.ip) {
        Some(m) => m,
        None => return false,
    };
    send_tcp_segment(
        dst_mac, src_ip.0, remote.ip.0,
        local.port, remote.port, seq, ack,
        TCP_FLAG_FIN | TCP_FLAG_ACK, TCP_DEFAULT_WINDOW, &[],
    )
}

/// Drive transmits + retransmits for all connections (#486).
/// Called from `net_tick`; safe to call from tests directly.
///
/// Locking: serializes transmitters on FLUSH_LOCK (leaf-first). The TCP table
/// lock is only held for snapshots/commits, never across a send, so the
/// NIC_REGISTRY-first order of the RX path can never deadlock against this.
pub fn tcp_tick() {
    let _flush = FLUSH_LOCK.lock();
    let now = crate::boot_benchmark::rdtsc();
    let per_us = tsc_per_us();
    struct Work {
        id: u32,
        state: TcpState,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        base: u32,
        next: u32,
        rack: u32,
        rused: usize,
        unsent_bytes: usize,
        unacked_span: usize,
        retries: u32,
        last_tx: u64,
        fin_pending: bool,
    }
    // Snapshot (TCP lock held briefly, never across sends).
    let mut jobs: alloc::vec::Vec<Work> = alloc::vec::Vec::new();
    let mut payloads: alloc::vec::Vec<alloc::vec::Vec<u8>> = alloc::vec::Vec::new();
    {
        let tcp = TCP.lock();
        for conn in tcp.connections.iter().flatten() {
            match conn.state {
                TcpState::Established
                | TcpState::SynSent
                | TcpState::FinWait1
                | TcpState::LastAck => {
                    let in_flight =
                        conn.send_seq.wrapping_sub(conn.send_base) as usize;
                    let total = conn.send_buf.len();
                    let unsent = total.saturating_sub(in_flight.min(total));
                    let mut chunk = alloc::vec::Vec::new();
                    if conn.state == TcpState::Established && unsent > 0 {
                        let n = unsent
                            .min(TCP_MSS)
                            .min(conn.window as usize)
                            .min(in_flight.saturating_add(unsent));
                        // Copy the first `n` unsent bytes (send_buf holds
                        // unsent + unacked; unacked prefix length = in_flight).
                        let skip = in_flight.min(total);
                        let (a, b) = conn.send_buf.as_slices();
                        let mut left = n;
                        let a_from = skip.min(a.len());
                        let a_take = (a.len() - a_from).min(left);
                        chunk.extend_from_slice(&a[a_from..a_from + a_take]);
                        left -= a_take;
                        if left > 0 {
                            let b_take = b.len().min(left);
                            chunk.extend_from_slice(&b[..b_take]);
                        }
                    }
                    jobs.push(Work {
                        id: conn.id,
                        state: conn.state,
                        local: conn.local,
                        remote: conn.remote,
                        base: conn.send_base,
                        next: conn.send_seq,
                        rack: conn.recv_seq,
                        rused: conn.recv_buf.len(),
                        unsent_bytes: unsent,
                        unacked_span: conn
                            .send_seq
                            .wrapping_sub(conn.send_base)
                            as usize,
                        retries: conn.retries,
                        last_tx: conn.last_tx_tsc,
                        fin_pending: conn.fin_pending,
                    });
                    payloads.push(chunk);
                }
                _ => {}
            }
        }
    }
    let rto_ticks = TCP_RTO_US.saturating_mul(per_us);
    for (job, chunk) in jobs.iter().zip(payloads.iter()) {
        let expired = now.wrapping_sub(job.last_tx) >= rto_ticks;
        // SYN (re)transmit while unanswered.
        if job.state == TcpState::SynSent {
            if job.unacked_span > 0 && expired {
                if job.retries >= TCP_MAX_RETRIES {
                    close_dead(job.id);
                    continue;
                }
                if send_syn_segment(job.id, job.local, job.remote, job.base) {
                    touch_tx(job.id, now, job.retries + 1);
                }
            }
            continue;
        }
        // Fresh data flush.
        if !chunk.is_empty() {
            let src_ip = resolve_src_ip(job.remote.ip, job.local.ip);
            if !src_ip.is_unspecified() {
                if let Some(dst_mac) = tcp_mac_for(job.remote.ip) {
                    let win = adv_window(job.rused);
                    if send_tcp_segment(
                        dst_mac, src_ip.0, job.remote.ip.0,
                        job.local.port, job.remote.port,
                        job.next, job.rack,
                        TCP_FLAG_PSH | TCP_FLAG_ACK, win,
                        chunk,
                    ) {
                        commit_sent(job.id, job.next, chunk.len() as u32, now);
                    }
                }
            }
        }
        // FIN once all data is out.
        if job.fin_pending && job.unsent_bytes == 0 {
            let next_state = if job.state == TcpState::Established {
                Some(TcpState::FinWait1)
            } else if job.state == TcpState::CloseWait {
                Some(TcpState::LastAck)
            } else {
                None
            };
            // CloseWait is not in the snapshot set; Established only here,
            // but keep the shape for the shared helper.
            if let Some(next_state) = next_state {
                if send_fin_segment(job.local, job.remote, job.next, job.rack) {
                    commit_fin(job.id, next_state, now);
                }
            }
        }
        // RTO retransmit of the oldest unacked span (data and/or FIN).
        if job.unacked_span > 0 && expired {
            if job.retries >= TCP_MAX_RETRIES {
                close_dead(job.id);
                continue;
            }
            retransmit_oldest(
                job.id, job.local, job.remote, job.base, job.next,
                job.rack, job.rused, job.state, job.retries, now,
            );
        }
    }
}

/// Snapshot helpers: brief TCP critical sections (FLUSH_LOCK already held).
fn commit_sent(id: u32, expect_next: u32, len: u32, now: u64) {
    let mut tcp = TCP.lock();
    if let Some(conn) = tcp.get_connection_mut(id) {
        if conn.send_seq == expect_next {
            conn.send_seq = conn.send_seq.wrapping_add(len);
            conn.last_tx_tsc = now;
            conn.retries = 0;
        }
    }
}

fn commit_fin(id: u32, next_state: TcpState, now: u64) {
    let mut tcp = TCP.lock();
    if let Some(conn) = tcp.get_connection_mut(id) {
        conn.send_seq = conn.send_seq.wrapping_add(1);
        conn.state = next_state;
        conn.fin_pending = false;
        conn.last_tx_tsc = now;
        conn.retries = 0;
    }
}

fn touch_tx(id: u32, now: u64, retries: u32) {
    let mut tcp = TCP.lock();
    if let Some(conn) = tcp.get_connection_mut(id) {
        conn.last_tx_tsc = now;
        conn.retries = retries;
    }
}

/// Give up on an unresponsive peer: connection (and socket) to Closed.
fn close_dead(id: u32) {
    {
        let mut tcp = TCP.lock();
        if let Some(conn) = tcp.get_connection_mut(id) {
            conn.state = TcpState::Closed;
        }
    }
    let mut mgr = crate::net::socket::SOCKET_MANAGER.lock();
    for slot in mgr.sockets.iter_mut().flatten() {
        if slot.tcp_conn_id == Some(id) {
            slot.direction = crate::net::types::SocketDirection::Closed;
        }
    }
}

/// Retransmit the oldest unacked span (payload tail and/or FIN).
#[allow(clippy::too_many_arguments)]
fn retransmit_oldest(
    id: u32,
    local: SocketAddrV4,
    remote: SocketAddrV4,
    base: u32,
    next: u32,
    rack: u32,
    rused: usize,
    state: TcpState,
    retries: u32,
    now: u64,
) {
    let span = next.wrapping_sub(base) as usize;
    if span == 0 {
        return;
    }
    let n = span.min(TCP_MSS);
    let payload: Vec<u8> = {
        let tcp = TCP.lock();
        match tcp.get_connection(id) {
            Some(conn) => {
                let take = n.min(conn.send_buf.len());
                let (a, b) = conn.send_buf.as_slices();
                let mut v = Vec::with_capacity(take);
                v.extend_from_slice(&a[..take.min(a.len())]);
                if v.len() < take {
                    v.extend_from_slice(&b[..take - v.len()]);
                }
                v
            }
            None => return,
        }
    };
    // A FIN was transmitted iff we left Established/CloseWait for a closing
    // state; it occupies [next - 1, next).
    let fin_here = state != TcpState::Established
        && state != TcpState::SynSent
        && base.wrapping_add(n as u32) == next;
    let mut flags = TCP_FLAG_ACK;
    if !payload.is_empty() {
        flags |= TCP_FLAG_PSH;
    }
    if fin_here {
        flags |= TCP_FLAG_FIN;
    }
    if payload.is_empty() && !fin_here {
        return;
    }
    let src_ip = resolve_src_ip(remote.ip, local.ip);
    if src_ip.is_unspecified() {
        return;
    }
    let dst_mac = match tcp_mac_for(remote.ip) {
        Some(m) => m,
        None => return,
    };
    if send_tcp_segment(
        dst_mac, src_ip.0, remote.ip.0,
        local.port, remote.port, base, rack,
        flags, adv_window(rused),
        &payload,
    ) {
        touch_tx(id, now, retries + 1);
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct TcpPacket {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8,
    pub flags: u8,
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

fn compute_tcp_checksum_raw(src: [u8; 4], dst: [u8; 4], segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    sum = sum.wrapping_add((src[0] as u32) << 8 | src[1] as u32);
    sum = sum.wrapping_add((src[2] as u32) << 8 | src[3] as u32);
    sum = sum.wrapping_add((dst[0] as u32) << 8 | dst[1] as u32);
    sum = sum.wrapping_add((dst[2] as u32) << 8 | dst[3] as u32);
    sum = sum.wrapping_add(IPV4_PROTO_TCP as u32);
    sum = sum.wrapping_add(segment.len() as u32);
    let mut i = 0;
    while i + 1 < segment.len() {
        let word = u16::from_be_bytes([segment[i], segment[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < segment.len() {
        sum = sum.wrapping_add((segment[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let cs = !(sum as u16);
    if cs == 0 { 0xFFFF } else { cs }
}

pub fn build_tcp_segment(src_ip: [u8; 4], dst_ip: [u8; 4], src_port: u16, dst_port: u16,
    seq_num: u32, ack_num: u32, flags: u8, window_size: u16, payload: &[u8]) -> Vec<u8>
{
    let hdr_size = 20usize;
    let seg = TcpPacket {
        src_port: src_port.to_be(),
        dst_port: dst_port.to_be(),
        seq_num: seq_num.to_be(),
        ack_num: ack_num.to_be(),
        data_offset: ((hdr_size / 4) as u8) << 4,
        flags,
        window_size: window_size.to_be(),
        checksum: 0,
        urgent_ptr: 0,
    };
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &seg as *const TcpPacket as *const u8,
            hdr_size,
        )
    };
    let mut segment = Vec::with_capacity(hdr_size + payload.len());
    segment.extend_from_slice(hdr_bytes);
    segment.extend_from_slice(payload);
    let cs = compute_tcp_checksum_raw(src_ip, dst_ip, &segment);
    segment[16] = (cs >> 8) as u8;
    segment[17] = (cs & 0xFF) as u8;
    segment
}

pub fn send_tcp_segment(dst_mac: [u8; 6], src_ip: [u8; 4], dst_ip: [u8; 4],
    src_port: u16, dst_port: u16, seq: u32, ack: u32, flags: u8, win: u16, payload: &[u8]) -> bool
{
    let segment = build_tcp_segment(src_ip, dst_ip, src_port, dst_port, seq, ack, flags, win, payload);
    let ip_payload_len = segment.len();
    let ip_hdr = crate::net::ipv4::build_ipv4_header(
        crate::net::types::Ipv4Addr(src_ip),
        crate::net::types::Ipv4Addr(dst_ip),
        IPV4_PROTO_TCP,
        ip_payload_len,
        0,
    );
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            &ip_hdr as *const Ipv4Header as *const u8,
            IPV4_HDR_MIN_LEN,
        )
    };
    let mut ip_pkt = Vec::with_capacity(IPV4_HDR_MIN_LEN + ip_payload_len);
    ip_pkt.extend_from_slice(ip_bytes);
    ip_pkt.extend_from_slice(&segment);

    // Loopback (#484): 127/8 never resolves ARP nor touches a NIC. The frame
    // re-enters through the single dispatch path via loopback_pump().
    if crate::net::types::Ipv4Addr(dst_ip).is_loopback() {
        let mac = crate::net::types::MacAddr::loopback();
        let frame = crate::net::ethernet::build_ethernet_frame(
            mac, mac,
            crate::net::ethernet::ETH_TYPE_IPV4, &ip_pkt,
        );
        if crate::net::loopback::loopback_send(&frame).is_err() {
            return false;
        }
        crate::net::loopback::loopback_pump();
        return true;
    }

    let nic_id = match crate::net::nic::nic_default_id() { Some(id) => id, None => return false };
    let mut registry = crate::net::nic::NIC_REGISTRY.lock();
    let nic = match registry.get_mut(nic_id) { Some(n) => n, None => return false };
    let src_mac = nic.mac_address();
    drop(registry);

    let frame = crate::net::ethernet::build_ethernet_frame(
        crate::net::types::MacAddr(dst_mac), src_mac,
        crate::net::ethernet::ETH_TYPE_IPV4, &ip_pkt,
    );
    crate::net::nic::nic_send_packet(nic_id, &frame).is_ok()
}

/// Parse a raw TCP segment (starting at the TCP header, no IP/ETH).
/// Returns (src_port, dst_port, seq, ack, flags, window, payload).
pub fn parse_tcp_segment(data: &[u8]) -> Option<(u16, u16, u32, u32, u8, u16, &[u8])> {
    if data.len() < 20 { return None; }
    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let seq = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let ack = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let data_offset = (data[12] >> 4) as usize * 4;
    let flags = data[13];
    let window = u16::from_be_bytes([data[14], data[15]]);
    if data_offset < 20 { return None; }
    Some((src_port, dst_port, seq, ack, flags, window, &data[data_offset..]))
}

/// Send SYN-ACK in response to an incoming SYN on a listening socket.
/// Caller must NOT hold SOCKET_MANAGER (briefly taken inside). Sets the
/// connection to SynReceived; the completing ACK moves it to Established.
pub fn tcp_send_syn_ack(socket_id: usize, src_port: u16, dst_port: u16, their_seq: u32, src_ip: [u8; 4], dst_ip: [u8; 4]) {
    let my_seq = 2000u32;
    let tcp_id = {
        let mut mgr = crate::net::socket::SOCKET_MANAGER.lock();
        let sock = match mgr.sockets.get_mut(socket_id).and_then(|s| s.as_mut()) {
            Some(s) => s,
            None => return,
        };
        // Only answer SYNs on listening sockets with a live connection.
        let tcp_id = match sock.tcp_conn_id {
            Some(id) => id,
            None => return,
        };
        let listen = {
            let tcp = TCP.lock();
            matches!(
                tcp.get_connection(tcp_id).map(|c| c.state),
                Some(crate::net::types::TcpState::Listen)
            )
        };
        if !listen {
            return;
        }
        sock.local.port = src_port;
        sock.remote.port = dst_port;
        sock.direction = crate::net::types::SocketDirection::Connected;
        tcp_id
    };
    {
        let mut tcp = TCP.lock();
        if let Some(conn) = tcp.get_connection_mut(tcp_id) {
            conn.state = crate::net::types::TcpState::SynReceived;
            // Params come from the received SYN: src = peer, dst = us (#486:
            // without these the server side has no route back for data/FIN).
            conn.remote.ip = crate::net::types::Ipv4Addr(src_ip);
            conn.local.ip = crate::net::types::Ipv4Addr(dst_ip);
            // Handler params are named from the SYN's perspective: src_port
            // is our (server) port, dst_port the peer's.
            conn.local.port = src_port;
            conn.remote.port = dst_port;
            conn.recv_seq = their_seq.wrapping_add(1);
            conn.send_base = my_seq;
            conn.send_seq = my_seq.wrapping_add(1);
            conn.retries = 0;
            conn.last_tx_tsc = crate::boot_benchmark::rdtsc();
        }
    }
    let dst_mac = match tcp_mac_for(crate::net::types::Ipv4Addr(dst_ip)) {
        Some(m) => m,
        None => return,
    };
    send_tcp_segment(dst_mac, src_ip, dst_ip, src_port, dst_port,
        my_seq, their_seq.wrapping_add(1), TCP_FLAG_SYN | TCP_FLAG_ACK, TCP_DEFAULT_WINDOW, &[]);
}

/// Handle an incoming ACK (#486). Caller must NOT hold SOCKET_MANAGER.
/// Advances the send window, completes handshakes, and steps FIN states.
pub fn tcp_handle_ack(socket_id: usize, their_seq: u32, their_ack: u32, window: u16) {
    use crate::net::types::TcpState;
    let (sid, tcp_id) = {
        let mgr = crate::net::socket::SOCKET_MANAGER.lock();
        match mgr.sockets.get(socket_id).and_then(|s| s.as_ref()) {
            Some(s) => (s.id, s.tcp_conn_id),
            None => return,
        }
    };
    let tcp_id = match tcp_id {
        Some(id) => id,
        None => return,
    };
    let now = crate::boot_benchmark::rdtsc();
    let mut became_established = false;
    let mut hs_ack: Option<(SocketAddrV4, SocketAddrV4, u32, u32)> = None;
    {
        let mut tcp = TCP.lock();
        let conn = match tcp.get_connection_mut(tcp_id) {
            Some(c) => c,
            None => return,
        };
        match conn.state {
            TcpState::SynSent => {
                // Our SYN consumed send_base; expect its ACK.
                if their_ack == conn.send_seq {
                    conn.send_base = their_ack;
                    conn.recv_seq = their_seq.wrapping_add(1);
                    conn.window = window;
                    conn.state = TcpState::Established;
                    became_established = true;
                    hs_ack = Some((conn.local, conn.remote, conn.send_seq, conn.recv_seq));
                }
            }
            TcpState::SynReceived => {
                if their_ack == conn.send_seq {
                    // Our SYN+ACK is acked: advance past it, like SynSent.
                    // Otherwise a phantom in-flight byte corrupts the
                    // unsent math for everything sent afterwards (#486).
                    conn.send_base = their_ack;
                    conn.state = TcpState::Established;
                    became_established = true;
                }
                conn.window = window;
            }
            TcpState::Established
            | TcpState::FinWait1
            | TcpState::FinWait2
            | TcpState::CloseWait
            | TcpState::LastAck => {
                if their_ack > conn.send_base && their_ack <= conn.send_seq {
                    let acked = their_ack.wrapping_sub(conn.send_base) as usize;
                    let drop_n = acked.min(conn.send_buf.len());
                    conn.send_buf.drain(..drop_n);
                    conn.send_base = their_ack;
                    conn.last_tx_tsc = now;
                    conn.retries = 0;
                }
                conn.window = window;
                if conn.state == TcpState::FinWait1 && their_ack == conn.send_seq {
                    conn.state = TcpState::FinWait2;
                } else if conn.state == TcpState::LastAck && their_ack == conn.send_seq {
                    conn.state = TcpState::Closed;
                }
            }
            _ => {}
        }
    }
    {
        let mut mgr = crate::net::socket::SOCKET_MANAGER.lock();
        if let Some(ref mut sock) = mgr
            .sockets
            .get_mut(socket_id)
            .and_then(|s| s.as_mut())
        {
            if became_established
                || sock.direction == crate::net::types::SocketDirection::Connecting
            {
                sock.direction = crate::net::types::SocketDirection::Connected;
            }
        }
    }
    if became_established {
        let mut mgr = crate::net::socket::SOCKET_MANAGER.lock();
        mgr.wake_socket_connect_waiters(sid);
    }
    // Completing handshake ACK (no locks held).
    if let Some((local, remote, seq, ack)) = hs_ack {
        let src_ip = resolve_src_ip(remote.ip, local.ip);
        if !src_ip.is_unspecified() {
            if let Some(dst_mac) = tcp_mac_for(remote.ip) {
                send_tcp_segment(
                    dst_mac, src_ip.0, remote.ip.0,
                    local.port, remote.port, seq, ack,
                    TCP_FLAG_ACK, TCP_DEFAULT_WINDOW, &[],
                );
            }
        }
    }
}

/// Handle incoming payload in Established state: store, ACK, wake readers.
/// Returns true when data was accepted. Caller must NOT hold SOCKET_MANAGER.
pub fn tcp_handle_data(
    tcp_id: u32,
    seq: u32,
    payload: &[u8],
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
) -> bool {
    // Store + snapshot reply coordinates (brief TCP critical section).
    // NOTE: the socket-id lookup runs AFTER the TCP guard is dropped:
    // socket_bind() takes SOCKET_MANAGER then TCP, so nesting the reverse
    // order here would deadlock. tcp_handle_data itself is only ever called
    // with no locks held (see tcp_dispatch).
    struct Reply {
        ack: u32,
        window: u16,
        accepted: bool,
    }
    let reply = {
        let mut tcp = TCP.lock();
        let conn = match tcp.get_connection_mut(tcp_id) {
            Some(c) => c,
            None => return false,
        };
        if conn.state != crate::net::types::TcpState::Established {
            return false;
        }
        let mut accepted = false;
        if seq == conn.recv_seq && !payload.is_empty() {
            let free = TCP_RECV_BUF.saturating_sub(conn.recv_buf.len());
            let take = payload.len().min(free);
            conn.recv_buf.extend(&payload[..take]);
            conn.recv_seq = conn.recv_seq.wrapping_add(take as u32);
            accepted = take > 0;
        }
        Reply {
            ack: conn.recv_seq,
            window: adv_window(conn.recv_buf.len()),
            accepted,
        }
    };
    let sid = {
        let mgr = crate::net::socket::SOCKET_MANAGER.lock();
        mgr.sockets
            .iter()
            .flatten()
            .find(|s| s.tcp_conn_id == Some(tcp_id))
            .map(|s| s.id)
            .unwrap_or(0)
    };
    let dst_mac = match tcp_mac_for(src_ip) {
        Some(m) => m,
        None => return reply.accepted,
    };
    send_tcp_segment(
        dst_mac, dst_ip.0, src_ip.0,
        dst_port, src_port, conn_seq_for(tcp_id), reply.ack,
        TCP_FLAG_ACK, reply.window, &[],
    );
    if reply.accepted && sid != 0 {
        let mut mgr = crate::net::socket::SOCKET_MANAGER.lock();
        mgr.wake_socket_readers(sid);
    }
    reply.accepted
}

/// Current send_seq for pure-ACK segments (brief read).
fn conn_seq_for(tcp_id: u32) -> u32 {
    TCP.lock()
        .get_connection(tcp_id)
        .map(|c| c.send_seq)
        .unwrap_or(0)
}

/// Handle an incoming FIN: ACK it and park the connection in CloseWait
/// (received data stays readable until the app closes). Caller must NOT
/// hold SOCKET_MANAGER.
pub fn tcp_handle_fin(tcp_id: u32, seq: u32, src_ip: Ipv4Addr, dst_ip: Ipv4Addr, src_port: u16, dst_port: u16) {
    use crate::net::types::TcpState;
    let (ack, send_seq, do_ack) = {
        let mut tcp = TCP.lock();
        let conn = match tcp.get_connection_mut(tcp_id) {
            Some(c) => c,
            None => return,
        };
        match conn.state {
            TcpState::Established | TcpState::FinWait1 | TcpState::FinWait2 => {
                // Consume the FIN byte.
                if seq == conn.recv_seq {
                    conn.recv_seq = conn.recv_seq.wrapping_add(1);
                }
                let next = if conn.state == TcpState::FinWait1
                    || conn.state == TcpState::FinWait2
                {
                    TcpState::Closed
                } else {
                    TcpState::CloseWait
                };
                conn.state = next;
                (conn.recv_seq, conn.send_seq, true)
            }
            _ => return,
        }
    };
    if do_ack {
        if let Some(dst_mac) = tcp_mac_for(src_ip) {
            send_tcp_segment(
                dst_mac, dst_ip.0, src_ip.0,
                dst_port, src_port, send_seq, ack,
                TCP_FLAG_ACK, TCP_DEFAULT_WINDOW, &[],
            );
        }
    }
}

/// Get the TCP connection state for a socket (by TCP connection id).
pub fn tcp_get_connection(id: u32) -> Option<TcpConnection> {
    let tcp = TCP.lock();
    tcp.get_connection(id).cloned()
}
