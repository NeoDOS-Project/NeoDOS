pub mod types;
pub mod ethernet;
pub mod arp;
pub mod ipv4;
pub mod icmp;
pub mod udp;
pub mod tcp;
pub mod socket;
pub mod nic;
pub mod e1000;
pub mod dns;
mod tests;

use types::SocketType;
use socket::SOCKET_MANAGER;
use nic::NIC_REGISTRY;

static NET_INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn init_networking() {
    if NET_INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    crate::serial_println!("[NET] Initializing networking subsystem...");

    crate::object::namespace::ob_create_directory("\\Device\\Tcp").unwrap_or(());
    if let Ok(tcp_id) = crate::object::ob_create_object(
        crate::object::ObType::Device, "Tcp", 1, 0, None,
    ) {
        let _ = crate::object::namespace::ob_insert_object("\\Device\\Tcp", tcp_id);
    }

    crate::object::namespace::ob_create_directory("\\Device\\Udp").unwrap_or(());
    if let Ok(udp_id) = crate::object::ob_create_object(
        crate::object::ObType::Device, "Udp", 2, 0, None,
    ) {
        let _ = crate::object::namespace::ob_insert_object("\\Device\\Udp", udp_id);
    }

    crate::object::namespace::ob_create_directory("\\Device\\Nic").unwrap_or(());

    let _has_kernel_nic = crate::net::e1000::probe_e1000();

    let nic_count = crate::net::nic::nic_count();

    {
        let mut mgr = SOCKET_MANAGER.lock();
        let id = mgr.alloc_socket(SocketType::Tcp);
        if let Some(id) = id {
            mgr.free_socket(id);
        }
    }

    NET_INITIALIZED.store(true, core::sync::atomic::Ordering::Release);
    crate::serial_println!("[NET] Networking initialized ({} NIC(s), {} template sockets)",
        nic_count, 0);
}

pub fn net_is_initialized() -> bool {
    NET_INITIALIZED.load(core::sync::atomic::Ordering::Acquire)
}

pub fn net_tick() {
    if !net_is_initialized() { return; }
    network_poll_all();
    arp::arp_tick();
    dns::dns_tick();
}

pub fn net_handle_incoming_packet(nic_id: u32, packet: &[u8]) {
    if packet.len() < crate::net::ethernet::ETH_HDR_LEN { return; }

    let eth_hdr: &crate::net::ethernet::EthernetHeader = unsafe {
        &*(packet.as_ptr() as *const crate::net::ethernet::EthernetHeader)
    };

    crate::serial_println!("[ETH] RX {} bytes, src={} dst={} type=0x{:04x}",
        packet.len(), eth_hdr.src_mac(), eth_hdr.dst_mac(), eth_hdr.ethertype());

    if eth_hdr.is_arp() {
        if packet.len() < crate::net::ethernet::ETH_HDR_LEN + core::mem::size_of::<crate::net::arp::ArpPacket>() {
            return;
        }
        let arp_pkt: &crate::net::arp::ArpPacket = unsafe {
            &*(packet.as_ptr().add(crate::net::ethernet::ETH_HDR_LEN) as *const crate::net::arp::ArpPacket)
        };

        if arp_pkt.operation() == crate::net::arp::ARP_OP_REQUEST {
            let target_ip = arp_pkt.target_ip_addr();
            let mut registry = NIC_REGISTRY.lock();
            if let Some(nic) = registry.get_mut(nic_id) {
                if nic.ip_address() == target_ip {
                    let reply = arp::arp_make_packet(
                        crate::net::arp::ARP_OP_REPLY,
                        nic.mac_address(), target_ip,
                        arp_pkt.sender_mac_addr(), arp_pkt.sender_ip_addr(),
                    );
                    let mut reply_buf = alloc::vec::Vec::with_capacity(
                        crate::net::ethernet::ETH_HDR_LEN + core::mem::size_of::<crate::net::arp::ArpPacket>(),
                    );
                    let eth = crate::net::ethernet::EthernetHeader::new(
                        arp_pkt.sender_mac_addr(),
                        nic.mac_address(),
                        crate::net::ethernet::ETH_TYPE_ARP,
                    );
                    let eth_bytes = unsafe {
                        core::slice::from_raw_parts(
                            &eth as *const crate::net::ethernet::EthernetHeader as *const u8,
                            core::mem::size_of::<crate::net::ethernet::EthernetHeader>(),
                        )
                    };
                    reply_buf.extend_from_slice(eth_bytes);
                    let arp_bytes = unsafe {
                        core::slice::from_raw_parts(
                            &reply as *const crate::net::arp::ArpPacket as *const u8,
                            core::mem::size_of::<crate::net::arp::ArpPacket>(),
                        )
                    };
                    reply_buf.extend_from_slice(arp_bytes);
                    let _ = nic.send_packet(&reply_buf);
                }
            }
            drop(registry);
        } else if arp_pkt.operation() == crate::net::arp::ARP_OP_REPLY {
            let sender_ip = arp_pkt.sender_ip_addr();
            let sender_mac = arp_pkt.sender_mac_addr();
            crate::serial_println!("[ARP] Reply: {} -> {}", sender_ip, sender_mac);
            arp::arp_insert(sender_ip, sender_mac);
        }
    } else if eth_hdr.is_ipv4() {
        let ip_offset = crate::net::ethernet::ETH_HDR_LEN;
        if packet.len() < ip_offset + crate::net::ipv4::IPV4_HDR_MIN_LEN { return; }

        let ip_hdr: &crate::net::ipv4::Ipv4Header = unsafe {
            &*(packet.as_ptr().add(ip_offset) as *const crate::net::ipv4::Ipv4Header)
        };

        if !ip_hdr.is_valid() { return; }

        let header_len = ip_hdr.header_len();
        let payload_offset = ip_offset + header_len;
        let payload = &packet[payload_offset..];

        if ip_hdr.protocol() == crate::net::ipv4::IPV4_PROTO_UDP {
            if payload.len() < core::mem::size_of::<crate::net::udp::UdpHeader>() { return; }
            let udp_hdr: &crate::net::udp::UdpHeader = unsafe {
                &*(payload.as_ptr() as *const crate::net::udp::UdpHeader)
            };
            let udp_data = &payload[core::mem::size_of::<crate::net::udp::UdpHeader>()..];
            socket::udp_dispatch(ip_hdr.src_ip(), udp_hdr.src_port(), udp_hdr.dst_port(), udp_data);
        } else if ip_hdr.protocol() == crate::net::ipv4::IPV4_PROTO_TCP {
            if payload.len() < 20 { return; }
            socket::tcp_dispatch(ip_hdr.src_ip(), ip_hdr.dst_ip(), payload);
        } else if ip_hdr.protocol() == crate::net::ipv4::IPV4_PROTO_ICMP {
            if payload.len() < core::mem::size_of::<crate::net::icmp::IcmpHeader>() { return; }
            let icmp_hdr: &crate::net::icmp::IcmpHeader = unsafe {
                &*(payload.as_ptr() as *const crate::net::icmp::IcmpHeader)
            };
            if icmp_hdr.is_echo_reply() {
                crate::net::icmp::notify_ping_reply(
                    icmp_hdr.echo_identifier(),
                    icmp_hdr.echo_sequence(),
                );
            } else if icmp_hdr.is_echo_request() {
                let icmp_data = &payload[core::mem::size_of::<crate::net::icmp::IcmpHeader>()..];
                let reply_icmp = crate::net::icmp::build_echo_reply(icmp_hdr, icmp_data);

                let mut registry = NIC_REGISTRY.lock();
                if let Some(nic) = registry.get_mut(nic_id) {
                    let reply_ip = crate::net::ipv4::build_ipv4_header(
                        ip_hdr.dst_ip(),
                        ip_hdr.src_ip(),
                        crate::net::ipv4::IPV4_PROTO_ICMP,
                        reply_icmp.len(),
                        0,
                    );

                    let mut reply_pkt = alloc::vec::Vec::with_capacity(
                        crate::net::ethernet::ETH_HDR_LEN
                        + crate::net::ipv4::IPV4_HDR_MIN_LEN
                        + reply_icmp.len(),
                    );

                    let eth = crate::net::ethernet::EthernetHeader::new(
                        eth_hdr.src_mac(),
                        nic.mac_address(),
                        crate::net::ethernet::ETH_TYPE_IPV4,
                    );
                    let eth_bytes = unsafe {
                        core::slice::from_raw_parts(
                            &eth as *const crate::net::ethernet::EthernetHeader as *const u8,
                            core::mem::size_of::<crate::net::ethernet::EthernetHeader>(),
                        )
                    };
                    reply_pkt.extend_from_slice(eth_bytes);

                    let ip_bytes = unsafe {
                        core::slice::from_raw_parts(
                            &reply_ip as *const crate::net::ipv4::Ipv4Header as *const u8,
                            crate::net::ipv4::IPV4_HDR_MIN_LEN,
                        )
                    };
                    reply_pkt.extend_from_slice(ip_bytes);
                    reply_pkt.extend_from_slice(&reply_icmp);

                    let _ = nic.send_packet(&reply_pkt);
                }
            }
        }
    }
}

pub fn network_poll_all() {
    if !net_is_initialized() { return; }
    let mut registry = NIC_REGISTRY.lock();
    registry.for_each(|nic_id, nic| {
        let mut buf = [0u8; 2048];
        while let Some(len) = nic.poll_packet(&mut buf) {
            net_handle_incoming_packet(nic_id, &buf[..len]);
        }
    });
}

pub fn register_net_tests() {
    tests::register_net_tests();
}
