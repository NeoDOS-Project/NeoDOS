use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::format;
use alloc::vec;
use super::types::{TcpState, MacAddr, Ipv4Addr, SocketType, SocketDirection, SocketAddrV4};
use super::arp::ArpCache;
use super::socket::{SocketManager, SOCKET_MANAGER, socket_bind};
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

    test_case!("net_handle_incoming_no_deadlock", {
        use super::nic::NIC_REGISTRY;
        use super::arp::ArpPacket;
        use super::ethernet::{EthernetHeader, ETH_HDR_LEN, ETH_TYPE_ARP};
        use super::types::{MacAddr, Ipv4Addr};
        use super::net_handle_incoming_packet;

        let mut registry = NIC_REGISTRY.lock();
        if let Some(nic_id) = registry.default_nic_id() {
            if let Some(nic) = registry.get_mut(nic_id) {
                let target_ip = Ipv4Addr::new([192, 168, 99, 99]);

                let arp = ArpPacket::new_request(
                    MacAddr::new([0xde, 0xad, 0xbe, 0xef, 0x00, 0x01]),
                    Ipv4Addr::new([10, 99, 99, 99]),
                    target_ip,
                );
                let arp_bytes = unsafe {
                    core::slice::from_raw_parts(
                        &arp as *const ArpPacket as *const u8,
                        core::mem::size_of::<ArpPacket>(),
                    )
                };
                let eth = EthernetHeader::new(
                    MacAddr::broadcast(),
                    MacAddr::new([0xde, 0xad, 0xbe, 0xef, 0x00, 0x01]),
                    ETH_TYPE_ARP,
                );
                let eth_bytes = unsafe {
                    core::slice::from_raw_parts(
                        &eth as *const EthernetHeader as *const u8,
                        ETH_HDR_LEN,
                    )
                };
                let mut packet = alloc::vec::Vec::with_capacity(ETH_HDR_LEN + core::mem::size_of::<ArpPacket>());
                packet.extend_from_slice(eth_bytes);
                packet.extend_from_slice(arp_bytes);

                net_handle_incoming_packet(nic_id, &mut **nic, &packet);

                let orig_ip = nic.ip_address();
                nic.set_ip_address(target_ip);
                net_handle_incoming_packet(nic_id, &mut **nic, &packet);
                nic.set_ip_address(orig_ip);
            }
        }
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

    // ── DNS tests ──
    test_case!("dns_parse_a_response", {
        let ip = Ipv4Addr::new([8, 8, 8, 8]);
        let data = super::dns::test_make_a_response("google.com", ip, 42, 300);
        let response = super::dns::parse_dns_response(&data).unwrap();
        test_eq!(response.id, 42);
        test_eq!(response.answers.len(), 1);
        match &response.answers[0] {
            super::dns::DnsRecord::A { name, addr, ttl } => {
                test_eq!(name, "google.com");
                test_eq!(*addr, ip);
                test_eq!(*ttl, 300);
            }
            _ => panic!("Expected A record"),
        }
    });

    test_case!("dns_parse_cname_chain", {
        let ip = Ipv4Addr::new([142, 250, 80, 46]);
        let data = super::dns::test_make_cname_a_response("www.google.com", "forcesafesearch.google.com", ip, 7, 300);
        let response = super::dns::parse_dns_response(&data).unwrap();
        test_eq!(response.answers.len(), 2);

        match &response.answers[0] {
            super::dns::DnsRecord::Cname { name, cname, .. } => {
                test_eq!(name, "www.google.com");
                test_eq!(cname, "forcesafesearch.google.com");
            }
            _ => panic!("Expected CNAME record"),
        }

        let resolved = super::dns::resolve_cname_chain(&response);
        test_true!(resolved.is_some());
        test_eq!(resolved.unwrap(), ip);
    });

    test_case!("dns_cache_hit_ttl", {
        let mut cache = super::dns::DnsCache::new();
        let ip = Ipv4Addr::new([8, 8, 8, 8]);
        cache.insert("google.com", ip);

        let cached = cache.lookup("google.com");
        test_eq!(cached, Some(ip));

        cache.insert("example.com", Ipv4Addr::new([93, 184, 216, 34]));
        test_eq!(cache.len(), 2);
    });

    test_case!("dns_cache_expiry", {
        let mut cache = super::dns::DnsCache::new();
        cache.insert("short-ttl.example.com", Ipv4Addr::new([1, 2, 3, 4]));

        let ttl_ticks = super::dns::DNS_DEFAULT_TTL_SECS * 100 / super::dns::DNS_TICK_INTERVAL;
        for _ in 0..ttl_ticks + 20 {
            cache.tick();
        }

        let cached = cache.lookup("short-ttl.example.com");
        test_true!(cached.is_none());
    });

    test_case!("dns_resolve_localhost", {
        let ip = super::dns::dns_resolve("localhost");
        test_eq!(ip, Some(Ipv4Addr::localhost()));
    });

    test_case!("dns_encode_decode_name", {
        let encoded = super::dns::encode_dns_name("www.example.com");
        test_eq!(encoded, vec![3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]);

        let (decoded, _) = super::dns::decode_dns_name(&encoded, 0).unwrap();
        test_eq!(decoded, "www.example.com");
    });

    test_case!("dns_parse_dotted_ip", {
        let ip = super::dns::parse_dotted_ip("8.8.8.8");
        test_eq!(ip, Some(Ipv4Addr::new([8, 8, 8, 8])));

        let ip = super::dns::parse_dotted_ip("192.168.1.1");
        test_eq!(ip, Some(Ipv4Addr::new([192, 168, 1, 1])));

        let ip = super::dns::parse_dotted_ip("invalid");
        test_eq!(ip, None);
    });

    test_case!("dns_empty_response", {
        let data = super::dns::build_dns_query("nonexistent.example.com", super::dns::DNS_TYPE_A, 1);
        let result = super::dns::parse_dns_response(&data);
        test_true!(result.is_err());
    });

    test_case!("dns_build_query", {
        let query = super::dns::build_dns_query("example.com", super::dns::DNS_TYPE_A, 99);
        test_true!(query.len() > 12);

        let header: &super::dns::DnsHeader = unsafe { &*(query.as_ptr() as *const super::dns::DnsHeader) };
        test_eq!(header.id(), 99);
        test_true!(!header.is_response());
        test_eq!(header.qdcount(), 1);
    });

    test_case!("dns_cache_max_entries", {
        let mut cache = super::dns::DnsCache::new();
        for i in 0..70 {
            cache.insert(
                &alloc::format!("host{}.example.com", i),
                Ipv4Addr::new([10, 0, 0, i as u8]),
            );
        }
        test_true!(cache.len() <= super::dns::DNS_MAX_CACHE);
    });

    test_case!("dns_cache_clear", {
        let mut cache = super::dns::DnsCache::new();
        cache.insert("test.example.com", Ipv4Addr::new([1, 2, 3, 4]));
        cache.insert("test2.example.com", Ipv4Addr::new([5, 6, 7, 8]));
        test_eq!(cache.len(), 2);
        cache.clear();
        test_eq!(cache.len(), 0);
    });
}
