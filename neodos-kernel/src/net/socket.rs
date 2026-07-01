use super::types::{Ipv4Addr, SocketAddrV4, SocketType, SocketDirection, MAX_SOCKETS};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::net::ethernet::{ETH_TYPE_IPV4, build_ethernet_frame};
use crate::net::ipv4::{IPV4_HDR_MIN_LEN, IPV4_PROTO_UDP, build_ipv4_header, Ipv4Header};
use crate::net::nic::{nic_default_id, nic_send_packet, NIC_REGISTRY};
use crate::net::arp::arp_resolve;

pub struct Socket {
    pub id: u32,
    pub socket_type: SocketType,
    pub direction: SocketDirection,
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub tcp_conn_id: Option<u32>,
    pub recv_buf: Vec<u8>,
    pub send_buf: Vec<u8>,
    pub nic_id: Option<u32>,
}

pub struct SocketManager {
    pub sockets: Vec<Option<Socket>>,
    next_id: u32,
}

impl SocketManager {
    pub const fn new() -> Self {
        SocketManager {
            sockets: Vec::new(),
            next_id: 1,
        }
    }

    pub fn alloc_socket(&mut self, socket_type: SocketType) -> Option<u32> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        for slot in self.sockets.iter_mut() {
            if slot.is_none() {
                *slot = Some(Socket {
                    id, socket_type,
                    direction: SocketDirection::None,
                    local: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                    remote: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                    tcp_conn_id: None,
                    recv_buf: Vec::new(),
                    send_buf: Vec::new(),
                    nic_id: None,
                });
                return Some(id);
            }
        }
        if self.sockets.len() < MAX_SOCKETS {
            self.sockets.push(Some(Socket {
                id, socket_type,
                direction: SocketDirection::None,
                local: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                remote: SocketAddrV4::new(Ipv4Addr::unspecified(), 0),
                tcp_conn_id: None,
                recv_buf: Vec::new(),
                send_buf: Vec::new(),
                nic_id: None,
            }));
            Some(id)
        } else {
            None
        }
    }

    pub fn free_socket(&mut self, id: u32) {
        if let Some(idx) = self.sockets.iter().position(|s| {
            s.as_ref().is_some_and(|s| s.id == id)
        }) {
            if let Some(ref socket) = self.sockets[idx] {
                if let Some(tcp_id) = socket.tcp_conn_id {
                    crate::net::tcp::tcp_free_connection(tcp_id);
                }
            }
            self.sockets[idx] = None;
        }
    }

    pub fn get_socket(&self, id: u32) -> Option<&Socket> {
        self.sockets.iter().flatten().find(|s| s.id == id)
    }

    pub fn get_socket_mut(&mut self, id: u32) -> Option<&mut Socket> {
        self.sockets.iter_mut().flatten().find(|s| s.id == id)
    }

    pub fn socket_count(&self) -> usize {
        self.sockets.iter().flatten().count()
    }

    pub fn wake_socket_readers(&mut self, socket_id: u32) {
        let magic = 0x0009_1000 | (socket_id & 0xFFF);
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut scheduler = s.lock();
            scheduler.wake_blocked_on_magic(magic);
        });
    }

    pub fn wake_socket_connect_waiters(&mut self, socket_id: u32) {
        let magic = 0x0009_2000 | (socket_id & 0xFFF);
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut scheduler = s.lock();
            scheduler.wake_blocked_on_magic(magic);
        });
    }

    pub fn wake_socket_accept_waiters(&mut self, socket_id: u32) {
        let magic = 0x0009_3000 | (socket_id & 0xFFF);
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut scheduler = s.lock();
            scheduler.wake_blocked_on_magic(magic);
        });
    }
}

lazy_static! {
    pub static ref SOCKET_MANAGER: Mutex<SocketManager> = Mutex::new(SocketManager::new());
}

pub fn socket_alloc(socket_type: SocketType) -> Option<u32> {
    SOCKET_MANAGER.lock().alloc_socket(socket_type)
}

pub fn socket_free(id: u32) {
    SOCKET_MANAGER.lock().free_socket(id);
}

pub fn socket_bind(id: u32, local: SocketAddrV4) -> bool {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = match mgr.get_socket_mut(id) {
        Some(s) => s,
        None => return false,
    };
    socket.local = local;
    if socket.socket_type == SocketType::Tcp {
        if let Some(tcp_id) = socket.tcp_conn_id {
            crate::net::tcp::tcp_bind(tcp_id, local);
        }
    }
    true
}

pub fn socket_listen(id: u32) -> bool {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = match mgr.get_socket_mut(id) {
        Some(s) => s,
        None => return false,
    };
    socket.direction = SocketDirection::Listening;
    if socket.socket_type == SocketType::Tcp {
        if let Some(tcp_id) = socket.tcp_conn_id {
            crate::net::tcp::tcp_listen(tcp_id);
        }
    }
    true
}

pub fn socket_connect(id: u32, remote: SocketAddrV4) -> bool {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = match mgr.get_socket_mut(id) {
        Some(s) => s,
        None => return false,
    };
    socket.remote = remote;
    socket.direction = SocketDirection::Connecting;
    if socket.socket_type == SocketType::Tcp {
        if let Some(tcp_id) = socket.tcp_conn_id {
            crate::net::tcp::tcp_connect(tcp_id, remote);
        }
    }
    true
}

pub fn socket_send(id: u32, data: &[u8]) -> Result<usize, ()> {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = match mgr.get_socket_mut(id) {
        Some(s) => s,
        None => return Err(()),
    };
    if socket.direction != SocketDirection::Connected {
        return Err(());
    }
    if socket.socket_type == SocketType::Tcp {
        if let Some(tcp_id) = socket.tcp_conn_id {
            return crate::net::tcp::tcp_send(tcp_id, data);
        }
    }
    socket.send_buf.extend_from_slice(data);
    Ok(data.len())
}

pub fn socket_recv(id: u32, buf: &mut [u8]) -> Result<usize, ()> {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = match mgr.get_socket_mut(id) {
        Some(s) => s,
        None => return Err(()),
    };
    if socket.direction != SocketDirection::Connected {
        return Err(());
    }
    if socket.socket_type == SocketType::Tcp {
        if let Some(tcp_id) = socket.tcp_conn_id {
            return crate::net::tcp::tcp_recv(tcp_id, buf);
        }
    }
    let available = socket.recv_buf.len().min(buf.len());
    if available == 0 {
        return Err(());
    }
    buf[..available].copy_from_slice(&socket.recv_buf[..available]);
    socket.recv_buf.drain(..available);
    Ok(available)
}

pub fn socket_close(id: u32) {
    let mut mgr = SOCKET_MANAGER.lock();
    if let Some(socket) = mgr.get_socket_mut(id) {
        socket.direction = SocketDirection::Closed;
        if socket.socket_type == SocketType::Tcp {
            if let Some(tcp_id) = socket.tcp_conn_id {
                crate::net::tcp::tcp_close(tcp_id);
            }
        }
    }
}

pub fn socket_next_accept_id(_id: u32) -> Option<u32> {
    None
}

pub fn socket_set_tcp_conn(id: u32, tcp_id: u32) {
    if let Some(socket) = SOCKET_MANAGER.lock().get_socket_mut(id) {
        socket.tcp_conn_id = Some(tcp_id);
    }
}

pub fn socket_set_connected(id: u32) {
    if let Some(socket) = SOCKET_MANAGER.lock().get_socket_mut(id) {
        socket.direction = SocketDirection::Connected;
    }
}

pub fn socket_set_local(id: u32, local: SocketAddrV4) {
    if let Some(socket) = SOCKET_MANAGER.lock().get_socket_mut(id) {
        socket.local = local;
    }
}

/// Send data via a UDP socket. Resolves destination MAC via ARP, builds
/// Ethernet+IP+UDP+payload, and transmits.
pub fn socket_send_udp(socket: &Socket, data: &[u8]) -> Result<(), ()> {
    let src_ip = socket.local.ip;
    let dst_ip = socket.remote.ip;
    let dst_mac = arp_resolve(dst_ip).ok_or(())?;
    let udp_data = crate::net::udp::build_udp_datagram(
        src_ip.0, dst_ip.0,
        socket.local.port, socket.remote.port,
        data,
    );
    let ip_hdr = build_ipv4_header(src_ip, dst_ip, IPV4_PROTO_UDP, udp_data.len(), 0);
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            &ip_hdr as *const Ipv4Header as *const u8,
            IPV4_HDR_MIN_LEN,
        )
    };
    let mut ip_pkt = Vec::with_capacity(IPV4_HDR_MIN_LEN + udp_data.len());
    ip_pkt.extend_from_slice(ip_bytes);
    ip_pkt.extend_from_slice(&udp_data);

    let nic_id = nic_default_id().ok_or(())?;
    let mut registry = NIC_REGISTRY.lock();
    let nic = registry.get_mut(nic_id).ok_or(())?;
    let src_mac = nic.mac_address();
    drop(registry);
    let frame = build_ethernet_frame(dst_mac, src_mac, ETH_TYPE_IPV4, &ip_pkt);
    nic_send_packet(nic_id, &frame)
}

/// Dispatch a received UDP datagram to a bound socket.
pub fn udp_dispatch(_src_ip: Ipv4Addr, src_port: u16, data: &[u8]) {
    let mut mgr = SOCKET_MANAGER.lock();
    for i in 0..MAX_SOCKETS {
        if i >= mgr.sockets.len() { break; }
        let Some(ref mut socket) = mgr.sockets[i] else { continue };
        if socket.socket_type == SocketType::Udp
            && socket.direction == SocketDirection::Connected
            && (socket.remote.port == src_port
                || (socket.remote.port == 0 && socket.local.port != 0))
        {
            socket.recv_buf.extend_from_slice(data);
            break;
        }
    }
}

/// Dispatch a received TCP segment to the matching connection.
pub fn tcp_dispatch(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) {
    let parsed = crate::net::tcp::parse_tcp_segment(segment);
    let Some((src_port, dst_port, seq, ack, flags, _window, payload)) = parsed else { return };
    let mut mgr = SOCKET_MANAGER.lock();
    for i in 0..MAX_SOCKETS {
        if i >= mgr.sockets.len() { break; }
        let Some(ref mut socket) = mgr.sockets[i] else { continue };
        if socket.socket_type == SocketType::Tcp
            && socket.direction != SocketDirection::None
            && socket.remote.port == src_port
            && socket.local.port == dst_port
        {
            let tcp_id = match socket.tcp_conn_id {
                Some(id) => id,
                None => return,
            };
            let conn = match crate::net::tcp::tcp_get_connection(tcp_id) {
                Some(c) => c,
                None => return,
            };
            if flags & crate::net::tcp::TCP_FLAG_SYN != 0 && conn.state == crate::net::types::TcpState::Listen {
                crate::net::tcp::tcp_send_syn_ack(i, dst_port, src_port, seq, src_ip.0, dst_ip.0);
            } else if flags & crate::net::tcp::TCP_FLAG_ACK != 0 && conn.state == crate::net::types::TcpState::SynSent {
                crate::net::tcp::tcp_handle_ack(i, seq, ack);
            } else if (flags & crate::net::tcp::TCP_FLAG_PSH != 0 || flags & crate::net::tcp::TCP_FLAG_ACK != 0)
                && conn.state == crate::net::types::TcpState::Established
            {
                socket.recv_buf.extend_from_slice(payload);
            }
            break;
        }
    }
}
