pub mod types;
pub mod ethernet;
pub mod arp;
pub mod ipv4;
pub mod icmp;
pub mod udp;
pub mod tcp;
pub mod socket;
pub mod nic;
pub mod counters;
pub mod loopback;
mod tests;

use crate::log::LogSubsys;
use types::SocketType;
use socket::SOCKET_MANAGER;
use nic::NIC_REGISTRY;

static NET_INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static TICK_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn init_networking() {
    if NET_INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    kinfo!(LogSubsys::Net, "Initializing networking subsystem...");

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

    // Loopback (#484): virtual interface outside NicRegistry.
    loopback::init_loopback();

    // NICs are registered by NEM drivers (e.g. e1000.nem) via hst_register_network_device
    let nic_count = crate::net::nic::nic_count();

    {
        let mut mgr = SOCKET_MANAGER.lock();
        let id = mgr.alloc_socket(SocketType::Tcp);
        if let Some(id) = id {
            mgr.free_socket(id);
        }
    }

    NET_INITIALIZED.store(true, core::sync::atomic::Ordering::Release);
    kinfo!(LogSubsys::Net, "Networking initialized ({} NIC(s), {} template sockets)",
        nic_count, 0);
}

pub fn net_is_initialized() -> bool {
    NET_INITIALIZED.load(core::sync::atomic::Ordering::Acquire)
}

/// Wake any waiter (e.g. the Ring 3 network applier) blocked on
/// `EVENT_NET_CONFIG_CHANGED`. Called by the config-apply path after a writer
/// sets a NIC IP/gateway, so consumers need not poll (#364).
pub fn notify_config_changed() {
    crate::kwait::kwait_wake(&crate::kwait::WaitReason::Event {
        event_type: crate::eventbus::EVENT_NET_CONFIG_CHANGED,
    });
}

/// extern "C" wrapper to get a stable function address.
/// The raw pointer stored in this static keeps the function alive
/// through LTO and provides the real runtime address.
#[no_mangle]
pub extern "C" fn netpump_entry_wrapper() {
    netpump_entry();
}

/// Store the address in a static so we can reference it from main.rs.
/// Using as u64 on a fn item produces thunks; reading from a static
/// that holds the real address avoids that issue.
#[used]
pub static NETPUMP_PTR: unsafe extern "C" fn() = netpump_entry_wrapper;

/// Kernel Ring-0 RX pump ("netpump"). This is the always-on data-plane worker
/// that drives `network_poll_all()`; it is deliberately **not** the Ring 3
/// `netd` network service (see #362/#372). It must stay in Ring 0 so RX keeps
/// flowing even when no userland thread is scheduled.
pub fn netpump_entry() -> ! {
    crate::serial_println!("[NET] netpump running");
    let mut iters: u64 = 0;
    let mut last_cpu: u32 = u32::MAX;
    loop {
        net_tick();
        crate::scheduler::yield_current_thread();
        iters += 1;

        let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        if cpu != last_cpu {
            if last_cpu != u32::MAX {
                crate::serial_println!("[NET] netpump migrated cpu={} -> {}", last_cpu, cpu);
            }
            last_cpu = cpu;
        }

        // Low-frequency snapshot: netd CPU + protocol counters.  Kept off the
        // hot path so it never floods serial during normal operation.
        if iters <= 5 || iters % 1_000_000 == 0 {
            use core::sync::atomic::Ordering::Relaxed;
            crate::serial_println!(
                "[NET] netpump cpu={} rx={} tx={} arp_rx={} arp_tx={} icmp_rx={} icmp_tx={}",
                cpu,
                crate::net::counters::COUNTERS.rx_packets.load(Relaxed),
                crate::net::counters::COUNTERS.tx_packets.load(Relaxed),
                crate::net::counters::COUNTERS.arp_requests_rx.load(Relaxed),
                crate::net::counters::COUNTERS.arp_replies_tx.load(Relaxed),
                crate::net::counters::COUNTERS.icmp_requests_rx.load(Relaxed),
                crate::net::counters::COUNTERS.icmp_replies_tx.load(Relaxed),
            );
        }
        for _ in 0..64 {
            core::hint::spin_loop();
        }
    }
}

pub fn spawn_net_kthread(entry: u64) -> Option<u32> {
    crate::scheduler::spawn_net_kthread(entry)
}

pub fn net_tick() {
    if !net_is_initialized() { return; }
    ktrace!(LogSubsys::Net, "tick");
    network_poll_all();
    arp::arp_tick();
    tcp::tcp_tick();
    // Wake finite-timeout socket waiters (serviced off the RX pump, not the
    // scheduler timer). Routed via net::tcp to keep boot_benchmark out of the
    // net dispatcher's dependency set.
    crate::kwait::deadline_tick(crate::net::tcp::tcp_now_tsc());

    let t = TICK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    // Heartbeat every ~100 ticks: proves netd is actually scheduled and running.
    // Without this, netd could be spawned but never picked by the scheduler.
    if t == 1 {
        kinfo!(LogSubsys::Net, "[NET] netpump alive — first tick (network_poll_all + arp_tick + tcp_tick)");
    } else if t % 1000 == 0 {
        crate::net::counters::dump_counters();
    }
}

/// Send a frame on behalf of the RX path **without holding `NIC_REGISTRY`**.
///
/// The RX dispatcher must not hold `NIC_REGISTRY` when a protocol handler emits
/// a reply: the TCP path resolves the next hop and the source MAC through the
/// registry (`nic_next_hop`, `nic_default_id`) and re-locks it. `spin::Mutex`
/// is non-reentrant, so nesting deadlocked the netpump thread on the first TCP
/// segment received over a real NIC (#TCP_E2E). Loopback frames re-enter through
/// the loopback queue.
pub fn net_send_frame(nic_id: u32, frame: &[u8]) -> Result<(), ()> {
    if nic_id == loopback::LOOPBACK_NIC_ID {
        loopback::loopback_send(frame)
    } else {
        crate::net::nic::nic_send_packet(nic_id, frame)
    }
}

/// MAC of the interface that received a frame. Loopback uses its synthetic MAC.
/// Takes `NIC_REGISTRY` only for the lookup, so it is safe from the RX path.
pub fn net_nic_mac(nic_id: u32) -> Option<crate::net::types::MacAddr> {
    if nic_id == loopback::LOOPBACK_NIC_ID {
        return Some(crate::net::types::MacAddr::loopback());
    }
    let mut reg = NIC_REGISTRY.lock();
    reg.get(nic_id).map(|n| n.mac_address())
}

/// IPv4 address of the interface that received a frame. Loopback is 127.0.0.1.
pub fn net_nic_ip(nic_id: u32) -> crate::net::types::Ipv4Addr {
    if nic_id == loopback::LOOPBACK_NIC_ID {
        return crate::net::types::Ipv4Addr::localhost();
    }
    crate::net::nic::nic_get_ip(nic_id).unwrap_or(crate::net::types::Ipv4Addr::unspecified())
}

/// Dispatch one received Ethernet frame.
///
/// Must be called **without** holding `NIC_REGISTRY`: protocol handlers emit
/// replies through [`net_send_frame`] and may lock the registry themselves.
/// `nic_id` is the registry id, or [`loopback::LOOPBACK_NIC_ID`].
pub fn net_handle_incoming_packet(nic_id: u32, packet: &[u8]) {
    let slot = crate::net::counters::stats_slot_for_nic(nic_id);
    if packet.len() < crate::net::ethernet::ETH_HDR_LEN {
        if let Some(slot) = slot {
            crate::net::counters::note_rx_err(slot);
        }
        return;
    }
    if let Some(slot) = slot {
        crate::net::counters::note_rx(slot, packet.len());
    }

    let eth_hdr: &crate::net::ethernet::EthernetHeader = unsafe {
        &*(packet.as_ptr() as *const crate::net::ethernet::EthernetHeader)
    };

    crate::net::counters::COUNTERS.rx_packets.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    crate::net::counters::COUNTERS.rx_bytes.fetch_add(packet.len() as u64, core::sync::atomic::Ordering::Relaxed);

    ktrace!(LogSubsys::Net, "RX {} bytes, src={} dst={} type=0x{:04x}",
        packet.len(), eth_hdr.src_mac(), eth_hdr.dst_mac(), eth_hdr.ethertype());

    // Drop frames not addressed to this interface, broadcast or multicast
    // (#310). On bridged/promiscuous segments the NIC hands over foreign
    // traffic; processing it wastes CPU and can deliver the wrong datagram.
    let our_mac = match net_nic_mac(nic_id) {
        Some(m) => m,
        None => return,
    };
    let dst_mac = eth_hdr.dst_mac();
    if dst_mac != our_mac && !dst_mac.is_broadcast() && !dst_mac.is_multicast() {
        if let Some(slot) = slot {
            crate::net::counters::note_rx_err(slot);
        }
        return;
    }

    if eth_hdr.is_arp() {
        if packet.len() < crate::net::ethernet::ETH_HDR_LEN + core::mem::size_of::<crate::net::arp::ArpPacket>() {
            return;
        }
        let arp_pkt: &crate::net::arp::ArpPacket = unsafe {
            &*(packet.as_ptr().add(crate::net::ethernet::ETH_HDR_LEN) as *const crate::net::arp::ArpPacket)
        };

        if arp_pkt.operation() == crate::net::arp::ARP_OP_REQUEST {
            crate::net::counters::COUNTERS.arp_requests_rx.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let target_ip = arp_pkt.target_ip_addr();
            let target_mac = crate::net::types::MacAddr(arp_pkt.target_mac);
            ktrace!(LogSubsys::Arp, "Request RX: op=1 sender_mac={} sender_ip={} target_mac={} target_ip={}",
                arp_pkt.sender_mac_addr(), arp_pkt.sender_ip_addr(), target_mac, target_ip);
            let our_ip = net_nic_ip(nic_id);
            let our_mac = net_nic_mac(nic_id)
                .unwrap_or_else(crate::net::types::MacAddr::zero);
            ktrace!(LogSubsys::Arp, "Request target={} our_ip={} mac={}",
                target_ip, our_ip, our_mac);
            if our_ip == target_ip {
                ktrace!(LogSubsys::Arp, "Reply TX: our_mac={} our_ip={} dst_mac={} dst_ip={}",
                    our_mac, target_ip,
                    arp_pkt.sender_mac_addr(), arp_pkt.sender_ip_addr());
                crate::net::counters::COUNTERS.arp_replies_tx.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                let reply = arp::arp_make_packet(
                    crate::net::arp::ARP_OP_REPLY,
                    our_mac, target_ip,
                    arp_pkt.sender_mac_addr(), arp_pkt.sender_ip_addr(),
                );
                let mut reply_buf = alloc::vec::Vec::with_capacity(
                    crate::net::ethernet::ETH_HDR_LEN + core::mem::size_of::<crate::net::arp::ArpPacket>(),
                );
                let eth = crate::net::ethernet::EthernetHeader::new(
                    arp_pkt.sender_mac_addr(),
                    our_mac,
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
                ktrace!(LogSubsys::Arp, "Reply pkt: {} bytes, about to send via nic.send_packet", reply_buf.len());
                let _ = net_send_frame(nic_id, &reply_buf);
            }
        } else if arp_pkt.operation() == crate::net::arp::ARP_OP_REPLY {
            let sender_ip = arp_pkt.sender_ip_addr();
            let sender_mac = arp_pkt.sender_mac_addr();
            ktrace!(LogSubsys::Arp, "Reply RX: {} -> {}", sender_ip, sender_mac);
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
        let total_len = ip_hdr.total_len();
        // Reject malformed length fields and datagrams larger than the captured
        // frame. Bounding the payload by the IP total length keeps Ethernet
        // padding out of the transport checksum.
        if header_len < crate::net::ipv4::IPV4_HDR_MIN_LEN
            || total_len < header_len
            || ip_offset + total_len > packet.len()
        {
            if let Some(slot) = slot { crate::net::counters::note_rx_err(slot); }
            return;
        }
        // Validate the IPv4 header checksum.
        if crate::net::ipv4::compute_ip_checksum(&packet[ip_offset..ip_offset + header_len]) != 0 {
            if let Some(slot) = slot { crate::net::counters::note_rx_err(slot); }
            return;
        }
        let payload = &packet[ip_offset + header_len..ip_offset + total_len];

        if ip_hdr.protocol() == crate::net::ipv4::IPV4_PROTO_UDP {
            if payload.len() < core::mem::size_of::<crate::net::udp::UdpHeader>() { return; }
            if !crate::net::udp::udp_checksum_valid(ip_hdr.src, ip_hdr.dst, payload) {
                if let Some(slot) = slot { crate::net::counters::note_rx_err(slot); }
                return;
            }
            let udp_hdr: &crate::net::udp::UdpHeader = unsafe {
                &*(payload.as_ptr() as *const crate::net::udp::UdpHeader)
            };
            let udp_data = &payload[core::mem::size_of::<crate::net::udp::UdpHeader>()..];
            socket::udp_dispatch(ip_hdr.src_ip(), udp_hdr.src_port(), udp_hdr.dst_port(), udp_data);
        } else if ip_hdr.protocol() == crate::net::ipv4::IPV4_PROTO_TCP {
            if payload.len() < 20 { return; }
            if !crate::net::tcp::tcp_checksum_valid(ip_hdr.src, ip_hdr.dst, payload) {
                if let Some(slot) = slot { crate::net::counters::note_rx_err(slot); }
                return;
            }
            socket::tcp_dispatch(ip_hdr.src_ip(), ip_hdr.dst_ip(), payload);
        } else if ip_hdr.protocol() == crate::net::ipv4::IPV4_PROTO_ICMP {
            if payload.len() < core::mem::size_of::<crate::net::icmp::IcmpHeader>() { return; }
            let icmp_hdr: &crate::net::icmp::IcmpHeader = unsafe {
                &*(payload.as_ptr() as *const crate::net::icmp::IcmpHeader)
            };
            if icmp_hdr.is_echo_reply() {
                ktrace!(LogSubsys::Icmp, "EchoReply RX: src={} dst={} id={} seq={}",
                    ip_hdr.src_ip(), ip_hdr.dst_ip(),
                    icmp_hdr.echo_identifier(), icmp_hdr.echo_sequence());
                crate::net::icmp::notify_ping_reply(
                    icmp_hdr.echo_identifier(),
                    icmp_hdr.echo_sequence(),
                );
            } else if icmp_hdr.is_echo_request() {
                crate::net::counters::COUNTERS.icmp_requests_rx.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                ktrace!(LogSubsys::Icmp, "EchoRequest RX: src={} dst={} id={} seq={}",
                    ip_hdr.src_ip(), ip_hdr.dst_ip(),
                    icmp_hdr.echo_identifier(), icmp_hdr.echo_sequence());
                let icmp_data = &payload[core::mem::size_of::<crate::net::icmp::IcmpHeader>()..];
                let reply_icmp = crate::net::icmp::build_echo_reply(icmp_hdr, icmp_data);
                crate::net::counters::COUNTERS.icmp_replies_tx.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                ktrace!(LogSubsys::Icmp, "EchoReply TX: src={} dst={} id={} seq={} reply_len={}",
                    ip_hdr.dst_ip(), ip_hdr.src_ip(),
                    icmp_hdr.echo_identifier(), icmp_hdr.echo_sequence(), reply_icmp.len());

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
                    net_nic_mac(nic_id).unwrap_or_else(crate::net::types::MacAddr::zero),
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

                let _ = net_send_frame(nic_id, &reply_pkt);
            }
        }
    }
}

pub fn network_poll_all() {
    if !net_is_initialized() { return; }
    // #376: NIC_REGISTRY is a non-IRQ-safe spin lock. If the polling thread is
    // descheduled while holding it, every other CPU that polls RX (e.g. dhcpd's
    // sys_yield -> network_poll_all) spins on it with interrupts disabled and
    // the CPU wedges. Do not allow preemption across the poll.
    crate::scheduler::preempt_disable();
    // Refresh link state from the drivers first so the NicInfo query and
    // netapplier's link-up edge detection observe real hardware state. The driver
    // poll runs without NIC_REGISTRY held (lock order: registry → driver).
    crate::net::nic::nic_poll_link_state();

    // Drain a bounded batch of RX frames while holding NIC_REGISTRY, then
    // dispatch *after releasing it*. The dispatcher must not hold the registry:
    // the TCP path re-enters it (nic_next_hop / nic_default_id / nic_send_packet)
    // and spin::Mutex is non-reentrant, so holding it deadlocked netpump on the
    // first TCP segment received over a real NIC.
    const RX_BATCH_MAX: usize = 64;
    let mut batch: alloc::vec::Vec<(u32, alloc::vec::Vec<u8>)> = alloc::vec::Vec::new();
    {
        let mut registry = NIC_REGISTRY.lock();
        registry.for_each(|nic_id, nic| {
            let mut buf = [0u8; 2048];
            while batch.len() < RX_BATCH_MAX {
                match nic.poll_packet(&mut buf) {
                    Some(len) => batch.push((nic_id, buf[..len].to_vec())),
                    None => break,
                }
            }
        });
    }
    for (nic_id, frame) in batch.iter() {
        net_handle_incoming_packet(*nic_id, frame);
    }

    // Drain loopback outside the NIC_REGISTRY lock: replies re-enter through
    // LoopbackInterface::send_packet and must not deadlock (#484).
    loopback::loopback_pump();
    crate::scheduler::preempt_enable();
}

pub fn register_net_tests() {
    tests::register_net_tests();
}
