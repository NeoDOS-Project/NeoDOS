use super::types::{Ipv4Addr, SocketAddrV4, TcpState};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

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

pub fn compute_tcp_checksum(header: &TcpHeader, src_ip: [u8; 4], dst_ip: [u8; 4], payload: &[u8]) -> u16 {
    let mut sum = 0u32;

    sum = sum.wrapping_add(((src_ip[0] as u32) << 8 | src_ip[1] as u32));
    sum = sum.wrapping_add(((src_ip[2] as u32) << 8 | src_ip[3] as u32));
    sum = sum.wrapping_add(((dst_ip[0] as u32) << 8 | dst_ip[1] as u32));
    sum = sum.wrapping_add(((dst_ip[2] as u32) << 8 | dst_ip[3] as u32));
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
            s.as_ref().map_or(false, |c| c.id == id)
        }) {
            self.connections[idx] = None;
        }
    }

    pub fn allocate_ephemeral_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if self.next_ephemeral_port >= 65535 {
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
    for i in 0..available {
        buf[i] = conn.recv_buf.pop_front().unwrap_or(0);
    }
    Ok(available)
}

pub fn tcp_get_state(id: u32) -> Option<TcpState> {
    TCP.lock().get_connection(id).map(|c| c.state)
}
