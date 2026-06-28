use core::fmt;

pub const MAC_ADDR_LEN: usize = 6;
pub const IPV4_ADDR_LEN: usize = 4;
pub const MAX_NICS: usize = 4;
pub const MAX_SOCKETS: usize = 64;
pub const MAX_TCP_CONNECTIONS: usize = 32;
pub const TCP_SEND_BUF_SIZE: usize = 16384;
pub const TCP_RECV_BUF_SIZE: usize = 16384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddr(pub [u8; MAC_ADDR_LEN]);

impl MacAddr {
    pub const fn new(bytes: [u8; MAC_ADDR_LEN]) -> Self { MacAddr(bytes) }
    pub const fn broadcast() -> Self { MacAddr([0xFF; MAC_ADDR_LEN]) }
    pub const fn zero() -> Self { MacAddr([0; MAC_ADDR_LEN]) }

    pub fn is_broadcast(&self) -> bool { self.0 == [0xFF; MAC_ADDR_LEN] }
    pub fn is_multicast(&self) -> bool { self.0[0] & 1 != 0 }
    pub fn is_zero(&self) -> bool { self.0 == [0; MAC_ADDR_LEN] }

    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut m = [0u8; MAC_ADDR_LEN];
        let len = bytes.len().min(MAC_ADDR_LEN);
        m[..len].copy_from_slice(&bytes[..len]);
        MacAddr(m)
    }

    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2],
            self.0[3], self.0[4], self.0[5])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ipv4Addr(pub [u8; IPV4_ADDR_LEN]);

impl Ipv4Addr {
    pub const fn new(bytes: [u8; IPV4_ADDR_LEN]) -> Self { Ipv4Addr(bytes) }
    pub const fn unspecified() -> Self { Ipv4Addr([0; 4]) }
    pub const fn localhost() -> Self { Ipv4Addr([127, 0, 0, 1]) }
    pub const fn broadcast() -> Self { Ipv4Addr([255, 255, 255, 255]) }

    pub fn from_u32(v: u32) -> Self {
        Ipv4Addr([(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8])
    }

    pub fn to_u32(self) -> u32 {
        (self.0[0] as u32) << 24 | (self.0[1] as u32) << 16
            | (self.0[2] as u32) << 8 | self.0[3] as u32
    }

    pub fn is_unspecified(&self) -> bool { *self == Ipv4Addr::unspecified() }
    pub fn is_broadcast(&self) -> bool { *self == Ipv4Addr::broadcast() }
    pub fn is_loopback(&self) -> bool { self.0[0] == 127 }
    pub fn is_link_local(&self) -> bool { self.0[0] == 169 && self.0[1] == 254 }
    pub fn is_multicast(&self) -> bool { self.0[0] >= 224 && self.0[0] <= 239 }

    pub fn network_prefix(&self, prefix_len: u8) -> Ipv4Addr {
        if prefix_len >= 32 { return *self; }
        let mask = if prefix_len == 0 { 0u32 } else { !0u32 << (32 - prefix_len) };
        Ipv4Addr::from_u32(self.to_u32() & mask)
    }
}

impl fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketAddrV4 {
    pub ip: Ipv4Addr,
    pub port: u16,
}

impl SocketAddrV4 {
    pub const fn new(ip: Ipv4Addr, port: u16) -> Self { SocketAddrV4 { ip, port } }
}

impl fmt::Display for SocketAddrV4 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.ip, self.port)
    }
}

pub enum IpProtocol {
    Icmp = 1,
    Tcp = 6,
    Udp = 17,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

impl TcpState {
    pub fn is_connected(&self) -> bool {
        matches!(self, TcpState::Established)
    }

    pub fn to_u8(self) -> u8 {
        match self {
            TcpState::Closed => 0,
            TcpState::Listen => 1,
            TcpState::SynSent => 2,
            TcpState::SynReceived => 3,
            TcpState::Established => 4,
            TcpState::FinWait1 => 5,
            TcpState::FinWait2 => 6,
            TcpState::CloseWait => 7,
            TcpState::Closing => 8,
            TcpState::LastAck => 9,
            TcpState::TimeWait => 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    Unused = 0,
    Tcp = 1,
    Udp = 2,
    Raw = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketDirection {
    None,
    Connecting,
    Connected,
    Listening,
    Closed,
}
