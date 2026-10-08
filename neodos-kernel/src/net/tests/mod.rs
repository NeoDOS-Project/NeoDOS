use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::format;
use alloc::vec;
use super::types::{TcpState, MacAddr, Ipv4Addr, SocketType, SocketDirection, SocketAddrV4};
use super::arp::ArpCache;
use super::socket::{
    SocketManager, SOCKET_MANAGER, socket_bind, socket_connect,
    socket_listen, socket_set_tcp_conn,
};
use super::tcp::{
    tcp_alloc_connection, tcp_bind, tcp_listen, tcp_connect, tcp_close,
    tcp_get_state, tcp_free_connection, tcp_send, tcp_recv, tcp_tick,
};
use super::nic::NicRegistry;
use super::ipv4::{compute_ip_checksum, build_ipv4_header, Ipv4Header};
use super::icmp::IcmpHeader;
use super::udp::UdpHeader;

/// Minimal test NIC used by the IPv4 gateway/next-hop tests. It only records
/// transmitted frames (optionally) and never receives.
struct CaptureNic {
    mac: MacAddr,
    ip: Ipv4Addr,
    sent: alloc::sync::Arc<spin::Mutex<alloc::vec::Vec<alloc::vec::Vec<u8>>>>,
}

impl super::nic::NetworkInterface for CaptureNic {
    fn mac_address(&self) -> MacAddr { self.mac }
    fn name(&self) -> &str { "capture-nic" }
    fn send_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        self.sent.lock().push(packet.to_vec());
        Ok(())
    }
    fn poll_packet(&mut self, _buf: &mut [u8]) -> Option<usize> { None }
    fn set_ip_address(&mut self, ip: Ipv4Addr) { self.ip = ip; }
    fn ip_address(&self) -> Ipv4Addr { self.ip }
}

/// Test NIC with a controllable link state, used by the #339 regression tests.
struct LinkNic {
    mac: MacAddr,
    ip: Ipv4Addr,
    up: alloc::sync::Arc<core::sync::atomic::AtomicBool>,
}

impl super::nic::NetworkInterface for LinkNic {
    fn mac_address(&self) -> MacAddr { self.mac }
    fn name(&self) -> &str { "link-nic" }
    fn send_packet(&mut self, _packet: &[u8]) -> Result<(), ()> { Ok(()) }
    fn poll_packet(&mut self, _buf: &mut [u8]) -> Option<usize> { None }
    fn set_ip_address(&mut self, ip: Ipv4Addr) { self.ip = ip; }
    fn ip_address(&self) -> Ipv4Addr { self.ip }
    fn is_link_up(&self) -> bool {
        self.up.load(core::sync::atomic::Ordering::Acquire)
    }
}

fn capture_nic(mac_last: u8) -> CaptureNic {
    CaptureNic {
        mac: MacAddr::new([0x02, 0, 0, 0, 0, mac_last]),
        ip: Ipv4Addr::unspecified(),
        sent: alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new())),
    }
}


mod net;
mod dns;
mod socket;

pub fn register_net_tests() {
















    // #339: a freshly registered NIC must not be advertised as link-up until
    // its driver reports a real link, and the registry must follow the driver
    // once polled (netd's `network_poll_all` path).









    // ── DNS tests ──











    // ── Loopback (#484) tests ──






    net::register();
    dns::register();
    socket::register();
}
