use super::types::{Ipv4Addr, MacAddr, SocketAddrV4, SocketType, SocketDirection, MAX_SOCKETS};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::net::ethernet::{ETH_TYPE_IPV4, build_ethernet_frame};
use crate::net::ipv4::{IPV4_HDR_MIN_LEN, IPV4_PROTO_UDP, build_ipv4_header, Ipv4Header};
use crate::net::nic::{nic_default_id, nic_get_ip, nic_next_hop, nic_send_packet, NIC_REGISTRY};
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
    next_ephemeral_port: u16,
}

impl SocketManager {
    pub const fn new() -> Self {
        SocketManager {
            sockets: Vec::new(),
            next_id: 1,
            next_ephemeral_port: 49152,
        }
    }

    /// Allocate an ephemeral port in the IANA dynamic range 49152–65535.
    pub fn allocate_ephemeral_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if self.next_ephemeral_port == 65535 {
            49152
        } else {
            self.next_ephemeral_port.wrapping_add(1)
        };
        port
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
        let magic = crate::kwait::WaitReason::SocketRead { socket_id }.encode_magic();
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut scheduler = s.lock();
            scheduler.wake_blocked_on_magic(magic);
        });
    }

    pub fn wake_socket_connect_waiters(&mut self, socket_id: u32) {
        let magic = crate::kwait::WaitReason::SocketConnect { socket_id }.encode_magic();
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut scheduler = s.lock();
            scheduler.wake_blocked_on_magic(magic);
        });
    }

    pub fn wake_socket_accept_waiters(&mut self, socket_id: u32) {
        let magic = crate::kwait::WaitReason::SocketAccept { socket_id }.encode_magic();
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
    // Fetch default NIC before locking SOCKET_MANAGER (lock order: NIC_REGISTRY
    // must not be acquired after SOCKET_MANAGER — see socket_send_udp_raw).
    let default_nic = nic_default_id();
    let mut mgr = SOCKET_MANAGER.lock();
    let needs_port = local.port == 0;
    let port = if needs_port { Some(mgr.allocate_ephemeral_port()) } else { None };
    let socket = match mgr.get_socket_mut(id) {
        Some(s) => s,
        None => return false,
    };
    let mut local = local;
    if let Some(p) = port {
        local.port = p;
    }
    socket.local = local;
    if socket.nic_id.is_none() {
        socket.nic_id = default_nic;
    }
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
    // Snapshot under lock, then act without holding SOCKET_MANAGER:
    // tcp_connect() transmits the SYN, whose dispatch path takes this lock
    // again (#486).
    let tcp_id = {
        let mut mgr = SOCKET_MANAGER.lock();
        let socket = match mgr.get_socket_mut(id) {
            Some(s) => s,
            None => return false,
        };
        socket.remote = remote;
        socket.direction = SocketDirection::Connecting;
        if socket.socket_type != SocketType::Tcp {
            return true;
        }
        match socket.tcp_conn_id {
            Some(tcp_id) => tcp_id,
            None => return true,
        }
    };
    crate::net::tcp::tcp_connect(tcp_id, remote);
    true
}

/// User-facing connect (`ObSetInfoClass::SocketConnect`).
///
/// TCP initiates the handshake (`SYN`): the socket moves to `Connecting` and
/// `tcp_handle_ack` flips it to `Connected` on completion. UDP/raw have no
/// handshake, so the peer is recorded and the socket is marked `Connected`
/// immediately. Returns false if the socket does not exist.
pub fn socket_connect_user(id: u32, remote: SocketAddrV4) -> bool {
    match socket_get_type(id) {
        Some(SocketType::Tcp) => socket_connect(id, remote),
        Some(_) => {
            let mut mgr = SOCKET_MANAGER.lock();
            match mgr.get_socket_mut(id) {
                Some(s) => {
                    s.remote = remote;
                    s.direction = SocketDirection::Connected;
                    true
                }
                None => false,
            }
        }
        None => false,
    }
}

pub fn socket_send(id: u32, data: &[u8]) -> Result<usize, ()> {
    let (local, remote, bound_nic) = {
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
            return Err(());
        }
        (socket.local, socket.remote, socket.nic_id)
    };
    // Drop SOCKET_MANAGER lock before transmitting to avoid lock inversion
    // with NIC_REGISTRY (incoming path locks NIC_REGISTRY then SOCKET_MANAGER).
    socket_send_udp_raw_on(local, remote, data, bound_nic)
}

pub fn socket_recv(id: u32, buf: &mut [u8]) -> Result<usize, ()> {
    // Network polling is driven by netd (net_tick), not by individual
    // socket operations.  Calling network_poll_all() here would
    // re-enter the NIC driver (e1000 NEM) while holding SOCKET_MANAGER,
    // creating a lock ordering hazard and potential GPF when the
    // hardware descriptor rings are not in a poll-safe state.
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
    // Snapshot under lock, then act unlocked: tcp_close() may transmit a
    // FIN, whose send path must never run under SOCKET_MANAGER (#486).
    // The direction is only flipped when the socket still exists below.
    let tcp_id = {
        let mgr = SOCKET_MANAGER.lock();
        match mgr.get_socket(id) {
            Some(s) if s.socket_type == SocketType::Tcp => s.tcp_conn_id,
            Some(_) => None,
            None => return,
        }
    };
    if let Some(tcp_id) = tcp_id {
        crate::net::tcp::tcp_close(tcp_id);
    }
    let mut mgr = SOCKET_MANAGER.lock();
    if let Some(socket) = mgr.get_socket_mut(id) {
        socket.direction = SocketDirection::Closed;
    }
}

pub fn socket_next_accept_id(_id: u32) -> Option<u32> {
    None
}

pub fn socket_get_type(id: u32) -> Option<SocketType> {
    SOCKET_MANAGER.lock().get_socket(id).map(|s| s.socket_type)
}

pub fn socket_get_direction(id: u32) -> Option<SocketDirection> {
    SOCKET_MANAGER.lock().get_socket(id).map(|s| s.direction)
}

/// Pin a socket to a NIC for send-interface selection (#536 follow-up).
/// Returns false when the socket does not exist or the NIC is not registered.
pub fn socket_set_nic(id: u32, nic_id: u32) -> bool {
    // Validate against registered NICs (loopback is not a registry NIC).
    {
        let mut reg = crate::net::nic::NIC_REGISTRY.lock();
        if reg.get(nic_id).is_none() {
            return false;
        }
    }
    if let Some(s) = SOCKET_MANAGER.lock().get_socket_mut(id) {
        s.nic_id = Some(nic_id);
        true
    } else {
        false
    }
}

pub fn socket_set_tcp_conn(id: u32, tcp_id: u32) {
    if let Some(socket) = SOCKET_MANAGER.lock().get_socket_mut(id) {
        socket.tcp_conn_id = Some(tcp_id);
    }
}

/// Assign the default NIC to a socket if none is set.
/// Caller must not hold SOCKET_MANAGER lock when calling this.
pub fn socket_assign_default_nic(id: u32) {
    if let Some(nic_id) = nic_default_id() {
        if let Some(s) = SOCKET_MANAGER.lock().get_socket_mut(id) {
            if s.nic_id.is_none() {
                s.nic_id = Some(nic_id);
            }
        }
    }
}

pub fn socket_set_local(id: u32, local: SocketAddrV4) {
    if let Some(socket) = SOCKET_MANAGER.lock().get_socket_mut(id) {
        socket.local = local;
    }
}

/// Send a UDP datagram using pre-extracted address info.
/// Caller must NOT hold SOCKET_MANAGER lock (lock order: SOCKET_MANAGER → NIC_REGISTRY
/// conflicts with incoming path NIC_REGISTRY → SOCKET_MANAGER).
pub fn socket_send_udp_raw(local: SocketAddrV4, remote: SocketAddrV4, data: &[u8]) -> Result<usize, ()> {
    socket_send_udp_raw_on(local, remote, data, None)
}

/// UDP send on an explicit NIC (#536 follow-up). `nic` overrides the default
/// NIC for interface/MAC selection only; next-hop/ARP stay default-based
/// (full bound-NIC routing is #317).
pub fn socket_send_udp_raw_on(
    local: SocketAddrV4,
    remote: SocketAddrV4,
    data: &[u8],
    nic: Option<u32>,
) -> Result<usize, ()> {
    // Loopback (#484): 127.0.0.0/8 never touches a NIC or ARP, works with
    // 0 NICs and leaves the default route untouched.
    if remote.ip.is_loopback() {
        return socket_send_udp_loopback(local, remote, data);
    }
    let nic_id = nic.or_else(nic_default_id).ok_or(())?;

    let src_mac = {
        let mut registry = NIC_REGISTRY.lock();
        registry.get_mut(nic_id).ok_or(())?.mac_address()
    };

    let dst_ip = remote.ip;

    // A UDP socket bound to 0.0.0.0 (e.g. the DNS resolver) still needs a valid
    // source address on the wire. Fill it from the NIC, except for broadcast
    // datagrams (DHCP DISCOVER legitimately uses 0.0.0.0).
    let mut src_ip = local.ip;
    if src_ip.is_unspecified() && !dst_ip.is_broadcast() {
        if let Some(ip) = nic_get_ip(nic_id) {
            if !ip.is_unspecified() {
                src_ip = ip;
            }
        }
    }

    // Next hop (single source of truth, see `nic_next_hop`): on-link -> the
    // destination; off-link -> the configured gateway; off-link without a
    // gateway -> clean failure (never ARP the remote destination).
    let arp_target = nic_next_hop(dst_ip).ok_or(())?;

    let dst_mac = if dst_ip.is_broadcast() {
        MacAddr::broadcast()
    } else {
        arp_resolve(arp_target).ok_or(())?
    };

    let udp_data = crate::net::udp::build_udp_datagram(
        src_ip.0, dst_ip.0,
        local.port, remote.port,
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

    let frame = build_ethernet_frame(dst_mac, src_mac, ETH_TYPE_IPV4, &ip_pkt);
    nic_send_packet(nic_id, &frame)?;
    Ok(data.len())
}

/// Loopback UDP send path (#484): no NIC, no ARP, no gateway lookup.
/// Builds the full Ethernet/IPv4/UDP frame with the synthetic loopback MAC
/// and delivers it synchronously via `loopback_pump()`, so a back-to-back
/// send+recv never observes `EAGAIN` for lack of scheduling.
pub fn socket_send_udp_loopback(local: SocketAddrV4, remote: SocketAddrV4, data: &[u8]) -> Result<usize, ()> {
    let mac = MacAddr::loopback();
    // A loopback datagram must carry a loopback source (RFC 1122 §3.2.1.3).
    let mut src_ip = local.ip;
    if !src_ip.is_loopback() {
        src_ip = Ipv4Addr::localhost();
    }
    let udp_data = crate::net::udp::build_udp_datagram(
        src_ip.0, remote.ip.0,
        local.port, remote.port,
        data,
    );
    let ip_hdr = build_ipv4_header(src_ip, remote.ip, IPV4_PROTO_UDP, udp_data.len(), 0);
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            &ip_hdr as *const Ipv4Header as *const u8,
            IPV4_HDR_MIN_LEN,
        )
    };
    let mut ip_pkt = Vec::with_capacity(IPV4_HDR_MIN_LEN + udp_data.len());
    ip_pkt.extend_from_slice(ip_bytes);
    ip_pkt.extend_from_slice(&udp_data);

    let frame = build_ethernet_frame(mac, mac, ETH_TYPE_IPV4, &ip_pkt);
    crate::net::loopback::loopback_send(&frame)?;
    crate::net::loopback::loopback_pump();
    Ok(data.len())
}

/// Dispatch a received UDP datagram to a bound socket.
pub fn udp_dispatch(_src_ip: Ipv4Addr, src_port: u16, dst_port: u16, data: &[u8]) {
    let mut mgr = SOCKET_MANAGER.lock();
    for i in 0..MAX_SOCKETS {
        if i >= mgr.sockets.len() { break; }
        let Some(ref mut socket) = mgr.sockets[i] else { continue };
        if socket.socket_type != SocketType::Udp { continue; }
        // Connected UDP socket: match the destination port (the socket's local
        // port) and, when set, the remote port. Matching only on `remote.port`
        // would let a stale/previous socket capture replies addressed to a
        // different local port.
        if socket.direction == SocketDirection::Connected
            && socket.local.port != 0
            && socket.local.port == dst_port
            && (socket.remote.port == 0 || socket.remote.port == src_port)
        {
            socket.recv_buf.extend_from_slice(data);
            break;
        }
        // Bound UDP socket (not yet connected): deliver if dst_port matches local port
        if socket.direction == SocketDirection::None
            && socket.local.port != 0
            && socket.local.port == dst_port
        {
            socket.recv_buf.extend_from_slice(data);
            break;
        }
    }
}

/// Dispatch a received TCP segment to the matching connection.
///
/// Locking: the socket table is only held to LOCATE the socket. All protocol
/// actions run through `tcp_*` helpers that take locks briefly themselves;
/// holding SOCKET_MANAGER across them deadlocked SYN handling (#486), since
/// the reply path locks it again.
pub fn tcp_dispatch(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) {
    use super::types::TcpState;
    let parsed = crate::net::tcp::parse_tcp_segment(segment);
    let Some((src_port, dst_port, seq, ack, flags, window, payload)) = parsed else { return };
    struct Hit {
        idx: usize,
        tcp_id: u32,
    }
    let hit = {
        let mgr = SOCKET_MANAGER.lock();
        let mut out = None;
        for (i, slot) in mgr.sockets.iter().enumerate() {
            let socket = match slot {
                Some(s) => s,
                None => continue,
            };
            // Listening sockets wildcard the remote side (#486).
            let remote_ok = socket.direction == SocketDirection::Listening
                || socket.remote.port == src_port;
            if socket.socket_type == SocketType::Tcp
                && socket.direction != SocketDirection::None
                && remote_ok
                && socket.local.port == dst_port
            {
                match socket.tcp_conn_id {
                    Some(tcp_id) => {
                        out = Some(Hit { idx: i, tcp_id });
                        break;
                    }
                    None => return,
                }
            }
        }
        out
    };
    let Some(hit) = hit else { return };
    let state = crate::net::tcp::tcp_get_state(hit.tcp_id);
    if flags & crate::net::tcp::TCP_FLAG_SYN != 0 && state == Some(TcpState::Listen) {
        crate::net::tcp::tcp_send_syn_ack(hit.idx, dst_port, src_port, seq, src_ip.0, dst_ip.0);
    } else {
        if flags & crate::net::tcp::TCP_FLAG_ACK != 0 {
            crate::net::tcp::tcp_handle_ack(hit.idx, seq, ack, window);
        }
        // Pure ACKs (no payload) must not trigger data ACKs: that would
        // answer every ACK with another ACK.
        if !payload.is_empty() && state == Some(TcpState::Established) {
            crate::net::tcp::tcp_handle_data(
                hit.tcp_id, seq, payload,
                src_ip, dst_ip, src_port, dst_port,
            );
        }
        if flags & crate::net::tcp::TCP_FLAG_FIN != 0 {
            crate::net::tcp::tcp_handle_fin(
                hit.tcp_id, seq,
                src_ip, dst_ip, src_port, dst_port,
            );
        }
    }
}
