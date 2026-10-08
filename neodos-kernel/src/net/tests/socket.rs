#![allow(unused_imports)]
use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::format;
use alloc::vec;
use super::super::types::{TcpState, MacAddr, Ipv4Addr, SocketType, SocketDirection, SocketAddrV4};
use super::super::arp::ArpCache;
use super::super::socket::{
    SocketManager, SOCKET_MANAGER, socket_bind, socket_connect,
    socket_listen, socket_set_tcp_conn,
};
use super::super::tcp::{
    tcp_alloc_connection, tcp_bind, tcp_listen, tcp_connect, tcp_close,
    tcp_get_state, tcp_free_connection, tcp_send, tcp_recv, tcp_tick,
};
use super::super::nic::NicRegistry;
use super::super::ipv4::{compute_ip_checksum, build_ipv4_header, Ipv4Header};
use super::super::icmp::IcmpHeader;
use super::super::udp::UdpHeader;

use super::{CaptureNic, LinkNic, capture_nic};
pub fn register() {
    test_case!("socket_auto_port_assign", {
        // Test 1: SocketManager's ephemeral port allocator (direct unit test)
        let mut mgr = SocketManager::new();
        let port1 = mgr.allocate_ephemeral_port();
        test_true!(port1 >= 49152);
        let port2 = mgr.allocate_ephemeral_port();
        test_true!(port2 >= 49152);
        // Sequential allocator gives unique values
        test_true!(port2 != port1 || port1 == 65535);

        // Test 2: socket_bind with port 0 → auto-assign ephemeral
        let id = {
            let mut mgr = SOCKET_MANAGER.lock();
            mgr.alloc_socket(SocketType::Udp).unwrap()
        };
        test_true!(socket_bind(id, SocketAddrV4::new(Ipv4Addr::unspecified(), 0)));
        let port = {
            let mgr = SOCKET_MANAGER.lock();
            mgr.get_socket(id).unwrap().local.port
        };
        test_true!(port >= 49152);
        test_true!(port != 0);

        // Test 3: Explicit port is preserved (not overwritten)
        let id2 = {
            let mut mgr = SOCKET_MANAGER.lock();
            mgr.alloc_socket(SocketType::Udp).unwrap()
        };
        test_true!(socket_bind(id2, SocketAddrV4::new(Ipv4Addr::new([10, 0, 2, 15]), 8080)));
        {
            let mgr = SOCKET_MANAGER.lock();
            let s = mgr.get_socket(id2).unwrap();
            test_eq!(s.local.port, 8080);
            test_eq!(s.local.ip, Ipv4Addr::new([10, 0, 2, 15]));
        }

        // Cleanup
        SOCKET_MANAGER.lock().free_socket(id);
        SOCKET_MANAGER.lock().free_socket(id2);
    });
}
