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
        conn.send_seq = 1000;
        conn.recv_seq = 0;
        conn.send_ack = 0;
        true
    } else {
        false
    }
}

pub fn tcp_close(id: u32) {
    let mut tcp = TCP.lock();
    if let Some(conn) = tcp.get_connection_mut(id) {
        if conn.state == TcpState::Established {
            conn.state = TcpState::FinWait1;
        } else if conn.state == TcpState::Closed || conn.state == TcpState::Listen {
            conn.state = TcpState::Closed;
        } else if conn.state == TcpState::CloseWait {
            conn.state = TcpState::LastAck;
        }
    }
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
pub fn tcp_send_syn_ack(socket_id: usize, src_port: u16, dst_port: u16, their_seq: u32, src_ip: [u8; 4], dst_ip: [u8; 4]) {
    let my_seq = 2000u32;
    let mut mgr = crate::net::socket::SOCKET_MANAGER.lock();
    if let Some(ref mut sock) = mgr.sockets.get_mut(socket_id).and_then(|s| s.as_mut()) {
        sock.local.port = src_port;
        sock.remote.port = dst_port;
        sock.direction = crate::net::types::SocketDirection::Connected;
    }
    drop(mgr);

    let dst_mac = match crate::net::arp::arp_resolve(crate::net::types::Ipv4Addr(dst_ip)) {
        Some(m) => m.0,
        None => return,
    };
    send_tcp_segment(dst_mac, src_ip, dst_ip, src_port, dst_port,
        my_seq, their_seq.wrapping_add(1), TCP_FLAG_SYN | TCP_FLAG_ACK, 65535, &[]);
}

/// Handle incoming ACK (connect completes).
pub fn tcp_handle_ack(socket_id: usize, _their_seq: u32, their_ack: u32) {
    let _ = their_ack;
    let mut mgr = crate::net::socket::SOCKET_MANAGER.lock();
    if let Some(ref mut sock) = mgr.sockets.get_mut(socket_id).and_then(|s| s.as_mut()) {
        sock.direction = crate::net::types::SocketDirection::Connected;
    }
    drop(mgr);
}

/// Get the TCP connection state for a socket (by TCP connection id).
pub fn tcp_get_connection(id: u32) -> Option<TcpConnection> {
    let tcp = TCP.lock();
    tcp.get_connection(id).cloned()
}
