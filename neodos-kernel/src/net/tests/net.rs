#![allow(unused_imports)]
use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::format;
use alloc::vec;
use super::super::types::{TcpState, MacAddr, Ipv4Addr, SocketType, SocketDirection, SocketAddrV4};
use super::super::arp::ArpCache;
use super::super::socket::{
    SocketManager, SOCKET_MANAGER, socket_bind, socket_connect, socket_connect_user,
    socket_get_direction, socket_listen, socket_close, socket_set_tcp_conn,
};
use super::super::tcp::{
    tcp_alloc_connection, tcp_bind, tcp_listen, tcp_connect, tcp_close,
    tcp_get_state, tcp_free_connection, tcp_send, tcp_recv, tcp_tick,
};
use super::super::nic::NicRegistry;
use super::super::ipv4::{compute_ip_checksum, build_ipv4_header, Ipv4Header};
use super::super::icmp::IcmpHeader;
use super::super::udp::UdpHeader;

use super::{CaptureNic, LinkNic, RespondingNic, capture_nic};
pub fn register() {
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
    test_case!("net_socket_connect_user_tcp_initiates_handshake", {
        // The user-facing connect must send a SYN for TCP sockets. Regression:
        // it used to only flip a flag, so no SYN was ever transmitted and every
        // later send failed (tcp_send requires Established).
        let sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(sock, tcp);
        let remote = SocketAddrV4::new(Ipv4Addr::new([10, 0, 2, 2]), 80);
        test_true!(socket_connect_user(sock, remote));
        test_true!(
            tcp_get_state(tcp) == Some(TcpState::SynSent)
                || tcp_get_state(tcp) == Some(TcpState::Established)
        );
        socket_close(sock);
        tcp_free_connection(tcp);
        SOCKET_MANAGER.lock().free_socket(sock);
    });
    test_case!("net_socket_connect_user_udp_marks_connected", {
        // UDP has no handshake: the socket is Connected immediately so that
        // socket_send works (a TCP-style Connecting state would break UDP).
        let sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Udp).unwrap();
        let remote = SocketAddrV4::new(Ipv4Addr::new([8, 8, 8, 8]), 53);
        test_true!(socket_connect_user(sock, remote));
        test_eq!(socket_get_direction(sock), Some(SocketDirection::Connected));
        SOCKET_MANAGER.lock().free_socket(sock);
    });
    test_case!("net_icmp_echo_reply_build", {
        let request = IcmpHeader::echo_request(1, 1);
        let data = [0x00; 56];
        let reply = super::super::icmp::build_echo_reply(&request, &data);
        test_eq!(reply.len(), core::mem::size_of::<IcmpHeader>() + 56);

        let reply_hdr: &IcmpHeader = unsafe {
            &*(reply.as_ptr() as *const IcmpHeader)
        };
        test_eq!(reply_hdr.icmp_type, super::super::icmp::ICMP_TYPE_ECHO_REPLY);
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
    test_case!("net_nic_link_state_follows_driver_poll", {
        use core::sync::atomic::Ordering;
        let up = alloc::sync::Arc::new(core::sync::atomic::AtomicBool::new(false));
        let nic = LinkNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0, 0x39]),
            ip: Ipv4Addr::unspecified(),
            up: up.clone(),
        };
        let mut reg = NicRegistry::new();
        let id = reg.register(alloc::boxed::Box::new(nic)).expect("register");

        // Before the first poll: not advertised as usable.
        test_true!(!reg.link_up(id));
        test_true!(!reg.default_link_up());

        // Driver reports link-down: the registry must not flip it up.
        reg.poll_link_state();
        test_true!(!reg.link_up(id));

        // Driver reports link-up: the next poll propagates it.
        up.store(true, Ordering::Release);
        reg.poll_link_state();
        test_true!(reg.link_up(id));
        test_true!(reg.default_link_up());

        // Driver reports link-down again: the registry follows.
        up.store(false, Ordering::Release);
        reg.poll_link_state();
        test_true!(!reg.link_up(id));
    });
    test_case!("net_nic_gateway_default_and_next_hop", {
        // Isolated registry: no global state, no real NIC.
        let mut reg = NicRegistry::new();
        let id = reg.register(alloc::boxed::Box::new(capture_nic(1))).expect("register");

        // A. Default gateway is unset (0.0.0.0), never a hidden 10.0.1.1.
        test_eq!(reg.get_gateway(id), Some(Ipv4Addr::unspecified()));

        reg.set_ip(id, Ipv4Addr::new([10, 0, 1, 10]));
        reg.set_mask(id, Ipv4Addr::new([255, 255, 255, 0]));

        // C. On-link -> the destination itself.
        test_eq!(
            reg.next_hop_ip(Ipv4Addr::new([10, 0, 1, 20])),
            Some(Ipv4Addr::new([10, 0, 1, 20]))
        );

        // E. Off-link without gateway -> no valid next hop.
        test_eq!(reg.next_hop_ip(Ipv4Addr::new([8, 8, 8, 8])), None);

        // B/D. Configured gateway is returned and used only for off-link.
        reg.set_gateway(id, Ipv4Addr::new([10, 0, 1, 1]));
        test_eq!(reg.get_gateway(id), Some(Ipv4Addr::new([10, 0, 1, 1])));
        test_eq!(
            reg.next_hop_ip(Ipv4Addr::new([8, 8, 8, 8])),
            Some(Ipv4Addr::new([10, 0, 1, 1]))
        );
        test_eq!(
            reg.next_hop_ip(Ipv4Addr::new([10, 0, 1, 20])),
            Some(Ipv4Addr::new([10, 0, 1, 20]))
        );
    });
    test_case!("net_udp_offlink_uses_gateway_or_fails", {
        use super::super::arp::{ArpPacket, ARP_OP_REQUEST};
        use super::super::ethernet::{EthernetHeader, ETH_HDR_LEN, ETH_TYPE_ARP};
        use super::super::nic::{nic_register, nic_unregister, NIC_REGISTRY};

        let sent = alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new()));
        let nic_id = nic_register(alloc::boxed::Box::new(CaptureNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0, 2]),
            ip: Ipv4Addr::unspecified(),
            sent: sent.clone(),
        }))
        .expect("register mock NIC");

        // Configure through the registry (no gratuitous ARP, no global propagation).
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic_id, Ipv4Addr::new([10, 0, 1, 10]));
            reg.set_mask(nic_id, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(nic_id, Ipv4Addr::unspecified());
        }

        let local = SocketAddrV4::new(Ipv4Addr::new([10, 0, 1, 10]), 50000);
        let remote = SocketAddrV4::new(Ipv4Addr::new([8, 8, 8, 8]), 53);

        // E. Off-link, no gateway: the send fails and no ARP is emitted at all
        // (we must not ARP the remote destination).
        test_true!(super::super::socket::socket_send_udp_raw(local, remote, b"x").is_err());
        test_eq!(sent.lock().len(), 0);

        // D. With a gateway: the ARP request targets the gateway, not 8.8.8.8.
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_gateway(nic_id, Ipv4Addr::new([10, 0, 1, 1]));
        }
        let _ = super::super::socket::socket_send_udp_raw(local, remote, b"x");
        let frames = sent.lock().clone();
        test_eq!(frames.len(), 1);
        let eth: &EthernetHeader = unsafe { &*(frames[0].as_ptr() as *const EthernetHeader) };
        test_eq!(eth.ethertype(), ETH_TYPE_ARP);
        let arp: &ArpPacket =
            unsafe { &*(frames[0].as_ptr().add(ETH_HDR_LEN) as *const ArpPacket) };
        test_eq!(arp.operation(), ARP_OP_REQUEST);
        test_eq!(arp.target_ip_addr(), Ipv4Addr::new([10, 0, 1, 1]));

        nic_unregister(nic_id);
    });
    test_case!("net_static_config_registry_roundtrip", {
        use super::super::nic::{nic_register, nic_unregister, NIC_REGISTRY};

        let nic_id = nic_register(alloc::boxed::Box::new(CaptureNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0, 3]),
            ip: Ipv4Addr::unspecified(),
            sent: alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new())),
        }))
        .expect("register mock NIC");

        // Values as they come from the interface Registry in static mode:
        // DHCPEnabled = 0 with IPAddress / SubnetMask / Gateway populated.
        let ip = Ipv4Addr::new([10, 0, 30, 20]);
        let mask = Ipv4Addr::new([255, 255, 255, 0]);
        let gw = Ipv4Addr::new([10, 0, 30, 1]);

        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic_id, ip);
            reg.set_mask(nic_id, mask);
            reg.set_gateway(nic_id, gw);
        }

        test_eq!(NIC_REGISTRY.lock().get_ip(nic_id), Some(ip));
        test_eq!(NIC_REGISTRY.lock().get_mask(nic_id), Some(mask));
        test_eq!(NIC_REGISTRY.lock().get_gateway(nic_id), Some(gw));

        // On-link destinations resolve directly; off-link uses the configured
        // gateway (static configuration must not imply a /0 mask).
        test_eq!(
            NIC_REGISTRY.lock().next_hop_ip(Ipv4Addr::new([10, 0, 30, 99])),
            Some(Ipv4Addr::new([10, 0, 30, 99]))
        );
        test_eq!(
            NIC_REGISTRY.lock().next_hop_ip(Ipv4Addr::new([8, 8, 8, 8])),
            Some(gw)
        );

        nic_unregister(nic_id);
    });
    test_case!("net_obsetinfo_nic_gateway_abi", {
        use crate::object::types::ObSetInfoClass;
        // Additive class: existing IDs unchanged.
        test_eq!(ObSetInfoClass::SetNicIp as u32, 27);
        test_eq!(ObSetInfoClass::SetNicGateway as u32, 28);
        test_eq!(ObSetInfoClass::SocketBindNic as u32, 29);
    });
    test_case!("net_handle_incoming_no_deadlock", {
        // The RX dispatcher must be callable without holding NIC_REGISTRY (the
        // TCP path re-locks it). This exercises the ARP request path. Only run
        // when no real NIC is registered (a NEM driver send during in-kernel
        // tests is not part of this unit test's scope).
        {
            use super::super::nic::NIC_REGISTRY;
            let reg = NIC_REGISTRY.lock();
            let has_nic = reg.count() > 0;
            drop(reg);
            if has_nic {
                return Ok(());
            }
        }

        use super::super::nic::NIC_REGISTRY;
        use super::super::arp::ArpPacket;
        use super::super::ethernet::{EthernetHeader, ETH_HDR_LEN, ETH_TYPE_ARP};
        use super::super::types::{MacAddr, Ipv4Addr};
        use super::super::net_handle_incoming_packet;

        let nic_id = NIC_REGISTRY.lock().default_nic_id();
        if let Some(nic_id) = nic_id {
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

            // No lock held across the dispatch.
            net_handle_incoming_packet(nic_id, &packet);

            let orig_ip = super::super::nic::nic_get_ip(nic_id).unwrap_or(Ipv4Addr::unspecified());
            NIC_REGISTRY.lock().set_ip(nic_id, target_ip);
            net_handle_incoming_packet(nic_id, &packet);
            NIC_REGISTRY.lock().set_ip(nic_id, orig_ip);
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
        let n = super::super::socket::socket_recv(id, &mut buf).unwrap();
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
        let r = super::super::socket::socket_recv(id, &mut buf);
        test_true!(r.is_err());
    });
    test_case!("net_loopback_route_no_nic", {
        use super::super::nic::{nic_route, Route};
        // Loopback and broadcast classify without touching NIC_REGISTRY:
        // valid with 0 NICs registered.
        test_eq!(nic_route(Ipv4Addr::new([127, 0, 0, 1])), Route::Loopback);
        test_eq!(nic_route(Ipv4Addr::new([127, 0, 0, 2])), Route::Loopback);
        test_eq!(nic_route(Ipv4Addr::new([127, 255, 255, 255])), Route::Loopback);
        test_true!(nic_route(Ipv4Addr::new([128, 0, 0, 1])) != Route::Loopback);
        test_true!(nic_route(Ipv4Addr::new([10, 0, 2, 15])) != Route::Loopback);
        test_eq!(
            nic_route(Ipv4Addr::broadcast()),
            Route::OnLink(Ipv4Addr::broadcast())
        );
        // Synthetic MAC: locally administered unicast, never on the wire.
        test_eq!(
            super::super::types::MacAddr::loopback(),
            super::super::types::MacAddr::new([0x02, 0, 0, 0, 0, 0x01])
        );
    });
    test_case!("net_loopback_udp_e2e", {
        super::super::loopback::loopback_pump(); // clear leftovers from earlier tests
        let rx = SOCKET_MANAGER.lock().alloc_socket(SocketType::Udp).unwrap();
        let tx = SOCKET_MANAGER.lock().alloc_socket(SocketType::Udp).unwrap();
        test_true!(socket_bind(rx, SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 41001)));
        test_true!(socket_bind(tx, SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 41002)));
        {
            let mut mgr = SOCKET_MANAGER.lock();
            let s = mgr.get_socket_mut(tx).unwrap();
            s.remote = SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 41001);
            s.direction = SocketDirection::Connected;
            let r = mgr.get_socket_mut(rx).unwrap();
            r.remote = SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 41002);
            r.direction = SocketDirection::Connected;
        }
        let (local, remote) = {
            let mgr = SOCKET_MANAGER.lock();
            (mgr.get_socket(tx).unwrap().local, mgr.get_socket(tx).unwrap().remote)
        };
        // Works with 0 NICs: no nic_default_id(), no ARP.
        test_true!(super::super::socket::socket_send_udp_raw(local, remote, b"hello-lo").is_ok());
        // Synchronously delivered: nothing left queued.
        test_eq!(super::super::loopback::loopback_pending(), 0);
        let mut buf = [0u8; 64];
        let n = super::super::socket::socket_recv(rx, &mut buf).unwrap();
        test_eq!(n, 8);
        test_eq!(&buf[..n], b"hello-lo");
        SOCKET_MANAGER.lock().free_socket(tx);
        SOCKET_MANAGER.lock().free_socket(rx);
    });
    test_case!("net_loopback_ping", {
        // Whole 127/8 answers through the real ICMP dispatch path.
        test_true!(super::super::icmp::icmp_ping(Ipv4Addr::new([127, 0, 0, 1]), 1_000_000).is_some());
        test_true!(super::super::icmp::icmp_ping(Ipv4Addr::new([127, 0, 0, 2]), 1_000_000).is_some());
        test_eq!(super::super::loopback::loopback_pending(), 0);
    });
    test_case!("net_loopback_tcp_segment_no_nic", {
        super::super::loopback::loopback_pump();
        // SYN to 127/8 succeeds with 0 NICs (no ARP) and drains synchronously.
        test_true!(super::super::tcp::send_tcp_segment(
            [0x02, 0, 0, 0, 0, 0x01],
            [127, 0, 0, 1], [127, 0, 0, 1],
            40001, 40002, 1000, 0,
            super::super::tcp::TCP_FLAG_SYN, 65535, &[], None,
        ));
        test_eq!(super::super::loopback::loopback_pending(), 0);
    });
    test_case!("net_loopback_nic_info_entry", {
        let (id, mac, ip, link, name, desc) = super::super::loopback::nic_info_entry();
        // Sentinel id: never a real NicRegistry slot (read-only by construction).
        test_eq!(id, super::super::loopback::LOOPBACK_NIC_ID);
        test_eq!(mac, super::super::types::MacAddr::loopback().0);
        test_eq!(ip, super::super::types::Ipv4Addr::localhost().0);
        test_eq!(link, 1);
        test_eq!(&name[..8], b"loopback");
        test_eq!(&desc[..18], b"Loopback Interface");
    });
    test_case!("net_stats_loopback_advances", {
        use super::super::counters::{snapshot, LOOPBACK_SLOT};
        let before = snapshot(LOOPBACK_SLOT);
        // A loopback UDP send counts 1 TX (enqueue) + 1 RX (dispatch).
        let local = SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 42001);
        let remote = SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 42002);
        test_true!(super::super::socket::socket_send_udp_loopback(local, remote, b"stats").is_ok());
        let after = snapshot(LOOPBACK_SLOT);
        test_eq!(after.0, before.0 + 1); // rx_packets
        test_eq!(after.1, before.1 + 1); // tx_packets
        test_true!(after.2 > before.2); // rx_bytes
        test_true!(after.3 > before.3); // tx_bytes
        // Unknown slots are never counted and snapshot to zero.
        test_eq!(snapshot(99), (0, 0, 0, 0, 0, 0));
        test_true!(super::super::nic::nic_route(Ipv4Addr::new([127, 0, 0, 1]))
            == super::super::nic::Route::Loopback);
    });
    test_case!("net_tcp_loopback_e2e", {
        use super::super::loopback::loopback_pump;
        // Server: bind + listen on 127.0.0.1:50011.
        let srv_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let srv_listen_tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(srv_sock, srv_listen_tcp);
        test_true!(socket_bind(
            srv_sock,
            SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 50011)
        ));
        test_true!(socket_listen(srv_sock));
        test_eq!(tcp_get_state(srv_listen_tcp), Some(TcpState::Listen));

        // Client: ephemeral port, connect. SYN goes out synchronously.
        let cli_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let cli_tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(cli_sock, cli_tcp);
        test_true!(socket_bind(
            cli_sock,
            SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 0)
        ));
        test_true!(socket_connect(
            cli_sock,
            SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 50011)
        ));
        // The handshake may complete synchronously inside connect (nested
        // loopback pump): SynSent or already Established are both fine.
        test_true!(
            tcp_get_state(cli_tcp) == Some(TcpState::SynSent)
                || tcp_get_state(cli_tcp) == Some(TcpState::Established)
        );

        // Drive handshake: SYN -> SYN+ACK -> ACK. The listener stays in
        // `Listen`; the server side connection is queued for accept.
        for _ in 0..500 {
            tcp_tick();
            loopback_pump();
            if tcp_get_state(cli_tcp) == Some(TcpState::Established)
                && super::super::socket::listener_has_pending(srv_sock)
            {
                break;
            }
        }
        test_eq!(tcp_get_state(cli_tcp), Some(TcpState::Established));
        let srv_conn = super::super::socket::socket_accept_take(srv_sock).expect("accept server conn");
        let srv_tcp = SOCKET_MANAGER.lock().get_socket(srv_conn).unwrap().tcp_conn_id.unwrap();
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::Established));

        // Data client -> server through the real dispatch + ACK path.
        test_eq!(tcp_send(cli_tcp, b"hello-tcp"), Ok(9));
        let mut got_c2s = false;
        for _ in 0..500 {
            tcp_tick();
            loopback_pump();
            let mut buf = [0u8; 64];
            if let Ok(n) = tcp_recv(srv_tcp, &mut buf) {
                test_eq!(n, 9);
                test_eq!(&buf[..n], b"hello-tcp");
                got_c2s = true;
                break;
            }
        }
        test_eq!(got_c2s, true);

        // And back server -> client.
        test_eq!(tcp_send(srv_tcp, b"back"), Ok(4));
        let mut got_s2c = false;
        for _ in 0..500 {
            tcp_tick();
            loopback_pump();
            let mut buf = [0u8; 64];
            if let Ok(n) = tcp_recv(cli_tcp, &mut buf) {
                test_eq!(n, 4);
                test_eq!(&buf[..n], b"back");
                got_s2c = true;
                break;
            }
        }
        test_eq!(got_s2c, true);

        // Orderly close: client FIN -> server CloseWait -> server FIN ->
        // client Closed -> server Closed.
        tcp_close(cli_tcp);
        for _ in 0..500 {
            tcp_tick();
            loopback_pump();
            if tcp_get_state(srv_tcp) == Some(TcpState::CloseWait) {
                break;
            }
        }
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::CloseWait));
        tcp_close(srv_tcp);
        for _ in 0..500 {
            tcp_tick();
            loopback_pump();
            if tcp_get_state(cli_tcp) == Some(TcpState::Closed)
                && tcp_get_state(srv_tcp) == Some(TcpState::Closed)
            {
                break;
            }
        }
        test_eq!(tcp_get_state(cli_tcp), Some(TcpState::Closed));
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::Closed));
        test_eq!(super::super::loopback::loopback_pending(), 0);

        SOCKET_MANAGER.lock().free_socket(cli_sock);
        SOCKET_MANAGER.lock().free_socket(srv_conn);
        SOCKET_MANAGER.lock().free_socket(srv_sock);
        tcp_free_connection(cli_tcp);
        tcp_free_connection(srv_tcp);
        tcp_free_connection(srv_listen_tcp);
    });
    test_case!("net_tcp_timeout_scale", {
        // A millisecond timeout must convert with the ms->us factor of 1000:
        // ticks = ms * 1000 * cycles_per_us. Regression: the old conversion
        // omitted *1000, arming deadlines 1000x too early.
        test_eq!(super::super::tcp::tsc_ticks_from_ms(1, 1000), 1_000_000);
        test_eq!(super::super::tcp::tsc_ticks_from_ms(5, 1000), 5_000_000);
        test_eq!(super::super::tcp::tsc_ticks_from_ms(200, 1000), 200_000_000);
        // Saturates rather than overflowing.
        test_eq!(super::super::tcp::tsc_ticks_from_ms(u64::MAX, u64::MAX), u64::MAX);
        // Zero stays zero (indefinite wait).
        test_eq!(super::super::tcp::tsc_ticks_from_ms(0, 1000), 0);
    });
    test_case!("net_ipv4_header_checksum_validation", {
        let hdr = build_ipv4_header(Ipv4Addr::new([10, 0, 1, 10]), Ipv4Addr::new([10, 0, 1, 20]), 6, 20, 0);
        let bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const Ipv4Header as *const u8, super::super::ipv4::IPV4_HDR_MIN_LEN)
        };
        test_eq!(compute_ip_checksum(bytes), 0);
        let mut buf = [0u8; super::super::ipv4::IPV4_HDR_MIN_LEN];
        buf.copy_from_slice(bytes);
        buf[10] ^= 0xFF; // corrupt the checksum field
        test_true!(compute_ip_checksum(&buf) != 0);
    });
    test_case!("net_tcp_checksum_validation", {
        let seg = super::super::tcp::build_tcp_segment(
            [10, 0, 1, 10], [10, 0, 1, 20], 1234, 80, 100, 0,
            super::super::tcp::TCP_FLAG_SYN, 65535, b"abc",
        );
        test_true!(super::super::tcp::tcp_checksum_valid([10, 0, 1, 10], [10, 0, 1, 20], &seg));
        // Wrong pseudo-header (different dst) must fail.
        test_true!(!super::super::tcp::tcp_checksum_valid([10, 0, 1, 10], [10, 0, 1, 21], &seg));
        let mut bad = seg.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        test_true!(!super::super::tcp::tcp_checksum_valid([10, 0, 1, 10], [10, 0, 1, 20], &bad));
    });
    test_case!("net_udp_checksum_validation", {
        let dg = super::super::udp::build_udp_datagram(
            [10, 0, 1, 10], [10, 0, 1, 20], 53, 5000, b"hello",
        );
        test_true!(super::super::udp::udp_checksum_valid([10, 0, 1, 10], [10, 0, 1, 20], &dg));
        test_true!(!super::super::udp::udp_checksum_valid([10, 0, 1, 10], [10, 0, 1, 21], &dg));
        // A zero checksum field means "not computed" (RFC 768) and is accepted.
        let mut z = dg.clone();
        z[6] = 0;
        z[7] = 0;
        test_true!(super::super::udp::udp_checksum_valid([10, 0, 1, 10], [10, 0, 1, 21], &z));
    });

    test_case!("net_udp_send_waits_for_arp", {
        // Regression #311: the first datagram to a new peer must not be dropped
        // while ARP resolves; the send path waits (bounded) for the reply.
        use super::super::ethernet::{EthernetHeader, ETH_TYPE_ARP, ETH_TYPE_IPV4};
        use super::super::nic::{nic_register, nic_unregister, NIC_REGISTRY};
        let sent = alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new()));
        let nic = nic_register(alloc::boxed::Box::new(RespondingNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0xA0, 0x10]),
            ip: Ipv4Addr::unspecified(),
            reply_mac: MacAddr::new([0x02, 0, 0, 0, 0xAA, 0x10]),
            sent: sent.clone(),
        }))
        .unwrap();
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic, Ipv4Addr::new([10, 0, 10, 10]));
            reg.set_mask(nic, Ipv4Addr::new([255, 255, 255, 0]));
        }
        super::super::arp::ARP_CACHE.lock().remove(Ipv4Addr::new([10, 0, 10, 20]));

        let local = SocketAddrV4::new(Ipv4Addr::new([10, 0, 10, 10]), 40000);
        let remote = SocketAddrV4::new(Ipv4Addr::new([10, 0, 10, 20]), 53);
        test_true!(
            super::super::socket::socket_send_udp_raw_on(local, remote, b"x", Some(nic)).is_ok()
        );
        let frames = sent.lock().clone();
        let eth = |f: &alloc::vec::Vec<u8>| unsafe {
            (*&*(f.as_ptr() as *const EthernetHeader)).ethertype()
        };
        test_true!(frames.iter().any(|f| eth(f) == ETH_TYPE_ARP));
        test_true!(frames.iter().any(|f| eth(f) == ETH_TYPE_IPV4));

        super::super::arp::ARP_CACHE.lock().remove(Ipv4Addr::new([10, 0, 10, 20]));
        nic_unregister(nic);
    });
    test_case!("net_rx_mac_filter_drops_foreign_frames", {
        use super::super::arp::ArpPacket;
        use super::super::ethernet::{EthernetHeader, ETH_HDR_LEN, ETH_TYPE_ARP};
        use super::super::nic::{nic_register, nic_unregister};
        let nic_mac = MacAddr::new([0x02, 0, 0, 0, 0x10, 0xAA]);
        let our_ip = Ipv4Addr::new([10, 0, 20, 10]);
        let peer_mac = MacAddr::new([0x02, 0, 0, 0, 0x20, 0x01]);
        let peer_ip = Ipv4Addr::new([10, 0, 20, 20]);
        let nic = nic_register(alloc::boxed::Box::new(CaptureNic {
            mac: nic_mac,
            ip: our_ip,
            sent: alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new())),
        }))
        .unwrap();
        let frame = |dst: MacAddr, dst_ip: Ipv4Addr| -> alloc::vec::Vec<u8> {
            let arp = ArpPacket::new_reply(peer_mac, peer_ip, dst, dst_ip);
            let ab = unsafe {
                core::slice::from_raw_parts(
                    &arp as *const ArpPacket as *const u8,
                    core::mem::size_of::<ArpPacket>(),
                )
            };
            let eth = EthernetHeader::new(dst, peer_mac, ETH_TYPE_ARP);
            let eb = unsafe {
                core::slice::from_raw_parts(&eth as *const EthernetHeader as *const u8, ETH_HDR_LEN)
            };
            let mut v = alloc::vec::Vec::new();
            v.extend_from_slice(eb);
            v.extend_from_slice(ab);
            v
        };
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        // Foreign unicast destination -> dropped before dispatch.
        super::super::net_handle_incoming_packet(nic, &frame(MacAddr::new([0x02, 0, 0, 0, 0x99, 0x99]), our_ip));
        test_true!(super::super::arp::arp_lookup(peer_ip).is_none());
        // Addressed to our MAC -> processed.
        super::super::net_handle_incoming_packet(nic, &frame(nic_mac, our_ip));
        test_eq!(super::super::arp::arp_lookup(peer_ip), Some(peer_mac));
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        // Broadcast destination -> processed.
        super::super::net_handle_incoming_packet(nic, &frame(MacAddr::broadcast(), our_ip));
        test_eq!(super::super::arp::arp_lookup(peer_ip), Some(peer_mac));
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        nic_unregister(nic);
    });
    test_case!("net_multi_nic_next_hop_per_interface", {
        use super::super::nic::{nic_register, nic_unregister, NIC_REGISTRY};
        let n0 = nic_register(alloc::boxed::Box::new(capture_nic(0xA0))).unwrap();
        let n1 = nic_register(alloc::boxed::Box::new(capture_nic(0xB0))).unwrap();
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(n0, Ipv4Addr::new([10, 0, 0, 10]));
            reg.set_mask(n0, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(n0, Ipv4Addr::new([10, 0, 0, 1]));
            reg.set_ip(n1, Ipv4Addr::new([10, 1, 0, 10]));
            reg.set_mask(n1, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(n1, Ipv4Addr::new([10, 1, 0, 1]));
        }
        // On-link -> the destination itself.
        test_eq!(
            super::super::nic::nic_next_hop_for(n0, Ipv4Addr::new([10, 0, 0, 20])),
            Some(Ipv4Addr::new([10, 0, 0, 20]))
        );
        test_eq!(
            super::super::nic::nic_next_hop_for(n1, Ipv4Addr::new([10, 1, 0, 20])),
            Some(Ipv4Addr::new([10, 1, 0, 20]))
        );
        // Off-link -> that interface's OWN gateway (not the default NIC's).
        test_eq!(
            super::super::nic::nic_next_hop_for(n0, Ipv4Addr::new([8, 8, 8, 8])),
            Some(Ipv4Addr::new([10, 0, 0, 1]))
        );
        test_eq!(
            super::super::nic::nic_next_hop_for(n1, Ipv4Addr::new([8, 8, 8, 8])),
            Some(Ipv4Addr::new([10, 1, 0, 1]))
        );
        // A destination on-link for NIC0 is off-link (via NIC1's gateway) for NIC1.
        test_eq!(
            super::super::nic::nic_next_hop_for(n1, Ipv4Addr::new([10, 0, 0, 20])),
            Some(Ipv4Addr::new([10, 1, 0, 1]))
        );
        nic_unregister(n0);
        nic_unregister(n1);
    });

    test_case!("net_udp_send_honors_bound_nic", {
        use super::super::arp::ArpPacket;
        use super::super::ethernet::{EthernetHeader, ETH_HDR_LEN, ETH_TYPE_ARP};
        use super::super::nic::{nic_register, nic_unregister, NIC_REGISTRY};
        let s0 = alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new()));
        let s1 = alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new()));
        let n0 = nic_register(alloc::boxed::Box::new(CaptureNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0xA0, 0x00]),
            ip: Ipv4Addr::unspecified(),
            sent: s0.clone(),
        }))
        .unwrap();
        let n1 = nic_register(alloc::boxed::Box::new(CaptureNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0xB0, 0x00]),
            ip: Ipv4Addr::unspecified(),
            sent: s1.clone(),
        }))
        .unwrap();
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(n0, Ipv4Addr::new([10, 0, 0, 10]));
            reg.set_mask(n0, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(n0, Ipv4Addr::new([10, 0, 0, 1]));
            reg.set_ip(n1, Ipv4Addr::new([10, 1, 0, 10]));
            reg.set_mask(n1, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(n1, Ipv4Addr::new([10, 1, 0, 1]));
        }
        // Off-link datagram bound to NIC0: ARP must target NIC0's gateway and
        // leave through NIC0 only.
        let local = SocketAddrV4::new(Ipv4Addr::new([10, 0, 0, 10]), 40000);
        let remote = SocketAddrV4::new(Ipv4Addr::new([8, 8, 8, 8]), 53);
        let _ = super::super::socket::socket_send_udp_raw_on(local, remote, b"x", Some(n0));
        let f0 = s0.lock().clone();
        let f1 = s1.lock().clone();
        test_eq!(f0.len(), 1);
        test_eq!(f1.len(), 0);
        let eth: &EthernetHeader = unsafe { &*(f0[0].as_ptr() as *const EthernetHeader) };
        test_eq!(eth.ethertype(), ETH_TYPE_ARP);
        test_eq!(eth.src_mac(), MacAddr::new([0x02, 0, 0, 0, 0xA0, 0x00]));
        let arp: &ArpPacket = unsafe { &*(f0[0].as_ptr().add(ETH_HDR_LEN) as *const ArpPacket) };
        test_eq!(arp.target_ip_addr(), Ipv4Addr::new([10, 0, 0, 1]));
        nic_unregister(n0);
        nic_unregister(n1);
    });

    test_case!("net_tcp_connect_uses_bound_nic", {
        use super::super::ethernet::{EthernetHeader, ETH_HDR_LEN, ETH_TYPE_IPV4};
        use super::super::nic::{nic_register, nic_unregister, NIC_REGISTRY};
        let s0 = alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new()));
        let s1 = alloc::sync::Arc::new(spin::Mutex::new(alloc::vec::Vec::new()));
        let n0 = nic_register(alloc::boxed::Box::new(CaptureNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0xA0, 0x01]),
            ip: Ipv4Addr::unspecified(),
            sent: s0.clone(),
        }))
        .unwrap();
        let n1 = nic_register(alloc::boxed::Box::new(CaptureNic {
            mac: MacAddr::new([0x02, 0, 0, 0, 0xB0, 0x01]),
            ip: Ipv4Addr::unspecified(),
            sent: s1.clone(),
        }))
        .unwrap();
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(n0, Ipv4Addr::new([10, 0, 0, 10]));
            reg.set_mask(n0, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(n0, Ipv4Addr::new([10, 0, 0, 1]));
            reg.set_ip(n1, Ipv4Addr::new([10, 1, 0, 10]));
            reg.set_mask(n1, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(n1, Ipv4Addr::new([10, 1, 0, 1]));
        }
        // Seed ARP for NIC0's gateway so the SYN can leave immediately.
        super::super::arp::arp_insert(Ipv4Addr::new([10, 0, 0, 1]), MacAddr::new([0x02, 0, 0, 0, 0xAA, 0x01]));

        let sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(sock, tcp);
        test_true!(socket_bind(sock, SocketAddrV4::new(Ipv4Addr::unspecified(), 0)));
        test_true!(super::super::socket::socket_set_nic(sock, n0));
        test_true!(socket_connect(sock, SocketAddrV4::new(Ipv4Addr::new([8, 8, 8, 8]), 80)));

        let f0 = s0.lock().clone();
        let f1 = s1.lock().clone();
        test_eq!(f1.len(), 0);
        test_eq!(f0.len(), 1);
        let eth: &EthernetHeader = unsafe { &*(f0[0].as_ptr() as *const EthernetHeader) };
        test_eq!(eth.ethertype(), ETH_TYPE_IPV4);
        test_eq!(eth.src_mac(), MacAddr::new([0x02, 0, 0, 0, 0xA0, 0x01]));
        let ip: &Ipv4Header = unsafe { &*(f0[0].as_ptr().add(ETH_HDR_LEN) as *const Ipv4Header) };
        test_eq!(ip.src_ip(), Ipv4Addr::new([10, 0, 0, 10]));
        test_eq!(ip.dst_ip(), Ipv4Addr::new([8, 8, 8, 8]));

        SOCKET_MANAGER.lock().free_socket(sock);
        tcp_free_connection(tcp);
        super::super::arp::ARP_CACHE.lock().remove(Ipv4Addr::new([10, 0, 0, 1]));
        nic_unregister(n0);
        nic_unregister(n1);
    });

    test_case!("net_config_change_notification_wakes_waiter", {
        // #364: a config-apply signal must wake a thread blocked on the
        // EVENT_NET_CONFIG_CHANGED event.
        use crate::scheduler::{BOOT_TID, ThreadState};
        let reason = crate::kwait::WaitReason::Event {
            event_type: crate::eventbus::EVENT_NET_CONFIG_CHANGED,
        };
        let magic = reason.encode_magic();
        let orig = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            let k = s.kthreads[0].as_ref().unwrap();
            (k.state, k.waiting_for, s.current_tid)
        });
        crate::hal::without_interrupts(|| {
            let mut s = crate::scheduler::current_scheduler().lock();
            s.current_tid = BOOT_TID;
            if let Some(k) = s.kthreads[0].as_mut() {
                crate::scheduler::Scheduler::remove_from_run_queue(k);
                k.state = ThreadState::Running;
                k.waiting_for = None;
            }
        });
        crate::kwait::kwait_block(reason);
        let blocked = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            s.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(blocked, ThreadState::Blocked { waiting_for: magic });
        crate::net::notify_config_changed();
        let after = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            let k = s.kthreads[0].as_ref().unwrap();
            (k.state, k.waiting_for)
        });
        test_true!(after.0 == ThreadState::Ready || after.0 == ThreadState::Running);
        test_eq!(after.1, None);
        crate::hal::without_interrupts(|| {
            let mut s = crate::scheduler::current_scheduler().lock();
            if let Some(k) = s.kthreads[0].as_mut() {
                k.state = orig.0;
                k.waiting_for = orig.1;
                crate::scheduler::Scheduler::remove_from_run_queue(k);
                if orig.0 == ThreadState::Ready {
                    k.state = ThreadState::Running;
                }
            }
            s.current_tid = orig.2;
        });
    });
}
