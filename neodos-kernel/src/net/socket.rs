use super::types::{Ipv4Addr, SocketAddrV4, SocketType, SocketDirection, TcpState, MAX_SOCKETS};
use super::nic::{NicRegistry, NIC_REGISTRY};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

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
    sockets: Vec<Option<Socket>>,
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
            s.as_ref().map_or(false, |s| s.id == id)
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
    for i in 0..available {
        buf[i] = socket.recv_buf[i];
    }
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

pub fn socket_next_accept_id(id: u32) -> Option<u32> {
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
