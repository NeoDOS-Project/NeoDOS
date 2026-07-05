use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::format;
use super::types::{TcpState, MacAddr, Ipv4Addr, SocketType, SocketDirection, SocketAddrV4};
use super::arp::ArpCache;
use super::socket::{SocketManager, SOCKET_MANAGER};
use super::tcp::{tcp_alloc_connection, tcp_bind, tcp_listen, tcp_connect, tcp_close, tcp_get_state, tcp_free_connection};
use super::nic::NicRegistry;
use super::ipv4::{compute_ip_checksum, build_ipv4_header, Ipv4Header};
use super::icmp::IcmpHeader;
use super::udp::UdpHeader;

pub fn register_net_tests() {
    test_case!("net_mac_addr_basics", {
        let mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        test_eq!(format!("{}", mac), "52:54:00:12:34:56");
        test_true!(!mac.is_broadcast());
        test_true!(!mac.is_multicast());
        let bc = MacAddr::broadcast();
        test_true!(bc.is_broadcast());
    });

    test_case!("net_ipv4_addr_basics", {
        let ip = Ipv4Addr::new([10, 0, 2, 15]);
        test_eq!(format!("{}", ip), "10.0.2.15");
        test_eq!(ip.to_u32(), 0x0A00020F);
        test_eq!(Ipv4Addr::from_u32(0x0A00020F), ip);
        test_true!(ip.network_prefix(24) == Ipv4Addr::new([10, 0, 2, 0]));
    });

    test_case!("net_ipv4_checksum", {
        let ip = Ipv4Addr::new([10, 0, 2, 15]);
        let hdr = build_ipv4_header(ip, Ipv4Addr::new([10, 0, 2, 2]), 1, 0, 1);
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(
                &hdr as *const Ipv4Header as *const u8,
                core::mem::size_of::<Ipv4Header>(),
            )
        };
        let cs = compute_ip_checksum(hdr_bytes);
        test_eq!(cs, 0);
    });

    test_case!("net_arp_cache_insert_lookup", {
        let mut cache = ArpCache::new();
        let ip = Ipv4Addr::new([10, 0, 2, 2]);
        let mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        test_true!(cache.lookup(ip).is_none());
        cache.insert(ip, mac);
        test_eq!(cache.lookup(ip), Some(mac));
        test_eq!(cache.len(), 1);
    });

    test_case!("net_arp_cache_eviction", {
        let mut cache = ArpCache::new();
        for i in 0..65 {
            let ip = Ipv4Addr::new([10, 0, 2, i as u8]);
            let mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, i as u8]);
            cache.insert(ip, mac);
        }
        test_true!(cache.len() <= 64);
    });

    test_case!("net_arp_cache_static_survives_eviction", {
        let mut cache = ArpCache::new();
        cache.insert_static(Ipv4Addr::new([10, 0, 2, 1]), MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x01]));
        for i in 0..70 {
            let ip = Ipv4Addr::new([10, 0, 2, i as u8]);
            cache.insert(ip, MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, i as u8]));
        }
        test_eq!(cache.lookup(Ipv4Addr::new([10, 0, 2, 1])), Some(MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x01])));
    });

    test_case!("net_tcp_state_machine_simple", {
        test_eq!(TcpState::Closed.to_u8(), 0);
        test_eq!(TcpState::Established.to_u8(), 4);
        test_true!(TcpState::Established.is_connected());
        test_true!(!TcpState::Closed.is_connected());
    });

    test_case!("net_tcp_connection_lifecycle", {
        let id = tcp_alloc_connection().unwrap();
        test_true!(id > 0);

        let state = tcp_get_state(id).unwrap();
        test_eq!(state, TcpState::Closed);

        test_true!(tcp_bind(id, SocketAddrV4::new(Ipv4Addr::unspecified(), 8080)));
        test_true!(tcp_listen(id));

        let state = tcp_get_state(id).unwrap();
        test_eq!(state, TcpState::Listen);

        tcp_close(id);
        let state = tcp_get_state(id).unwrap();
        test_eq!(state, TcpState::Closed);
    });

    test_case!("net_tcp_connect_and_close", {
        let id = tcp_alloc_connection().unwrap();
        test_true!(tcp_connect(id, SocketAddrV4::new(Ipv4Addr::new([10, 0, 2, 2]), 80)));
        let state = tcp_get_state(id).unwrap();
        test_eq!(state, TcpState::SynSent);

        tcp_close(id);
        tcp_free_connection(id);
    });

    test_case!("net_icmp_echo_reply_build", {
        let request = IcmpHeader::echo_request(1, 1);
        let data = [0x00; 56];
        let reply = super::icmp::build_echo_reply(&request, &data);
        test_eq!(reply.len(), core::mem::size_of::<IcmpHeader>() + 56);

        let reply_hdr: &IcmpHeader = unsafe {
            &*(reply.as_ptr() as *const IcmpHeader)
        };
        test_eq!(reply_hdr.icmp_type, super::icmp::ICMP_TYPE_ECHO_REPLY);
        test_eq!(reply_hdr.echo_identifier(), 1);
        test_eq!(reply_hdr.echo_sequence(), 1);
    });

    test_case!("net_socket_manager_lifecycle", {
        let mut mgr = SocketManager::new();
        let id = mgr.alloc_socket(SocketType::Tcp).unwrap();
        test_eq!(mgr.socket_count(), 1);
        let socket = mgr.get_socket(id).unwrap();
        test_eq!(socket.socket_type, SocketType::Tcp);
        mgr.free_socket(id);
        test_eq!(mgr.socket_count(), 0);
    });

    test_case!("net_socket_bind_connect", {
        let mut mgr = SocketManager::new();
        let id = mgr.alloc_socket(SocketType::Tcp).unwrap();

        let socket = mgr.get_socket_mut(id).unwrap();
        socket.local = SocketAddrV4::new(Ipv4Addr::unspecified(), 9090);
        socket.remote = SocketAddrV4::new(Ipv4Addr::new([10, 0, 2, 2]), 80);
        socket.direction = SocketDirection::Connected;

        let socket = mgr.get_socket(id).unwrap();
        test_eq!(socket.local.port, 9090);
        test_eq!(socket.remote.port, 80);
        test_eq!(socket.direction, SocketDirection::Connected);

        mgr.free_socket(id);
    });

    test_case!("net_udp_header_checksum", {
        let hdr = UdpHeader::new(1234, 53, 0);
        test_eq!(hdr.src_port(), 1234);
        test_eq!(hdr.dst_port(), 53);
        test_eq!(hdr.len(), 8);
    });

    test_case!("net_socket_addr_fmt", {
        let addr = SocketAddrV4::new(Ipv4Addr::new([192, 168, 1, 1]), 8080);
        test_eq!(format!("{}", addr), "192.168.1.1:8080");
    });

    test_case!("net_ipv4_classification", {
        let loopback = Ipv4Addr::new([127, 0, 0, 1]);
        test_true!(loopback.is_loopback());
        let multicast = Ipv4Addr::new([224, 0, 0, 1]);
        test_true!(multicast.is_multicast());
        let link_local = Ipv4Addr::new([169, 254, 1, 1]);
        test_true!(link_local.is_link_local());
        let normal = Ipv4Addr::new([10, 0, 2, 15]);
        test_true!(!normal.is_loopback());
    });

    test_case!("net_nic_registry_empty", {
        let reg = NicRegistry::new();
        test_eq!(reg.count(), 0);
        test_true!(reg.default_nic_id().is_none());
    });

    test_case!("net_socket_recv_data", {
        let id = {
            let mut mgr = SOCKET_MANAGER.lock();
            let id = mgr.alloc_socket(SocketType::Tcp).unwrap();
            let socket = mgr.get_socket_mut(id).unwrap();
            socket.direction = SocketDirection::Connected;
            socket.remote = SocketAddrV4::new(Ipv4Addr::new([10, 0, 2, 2]), 80);
            socket.recv_buf.extend_from_slice(b"hello");
            id
        };
        let mut buf = [0u8; 64];
        let n = super::socket::socket_recv(id, &mut buf).unwrap();
        test_eq!(n, 5);
        test_eq!(&buf[..n], b"hello");
        let mgr = SOCKET_MANAGER.lock();
        let socket = mgr.get_socket(id).unwrap();
        test_true!(socket.recv_buf.is_empty());
    });

    test_case!("net_socket_recv_empty", {
        let id = {
            let mut mgr = SOCKET_MANAGER.lock();
            let id = mgr.alloc_socket(SocketType::Tcp).unwrap();
            let socket = mgr.get_socket_mut(id).unwrap();
            socket.direction = SocketDirection::Connected;
            socket.remote = SocketAddrV4::new(Ipv4Addr::new([10, 0, 2, 2]), 80);
            id
        };
        let mut buf = [0u8; 64];
        let r = super::socket::socket_recv(id, &mut buf);
        test_true!(r.is_err());
    });
}
