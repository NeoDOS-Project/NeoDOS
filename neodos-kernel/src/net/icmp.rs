use alloc::vec::Vec;
use core::sync::atomic::{Ordering, AtomicU64};
use crate::net::ipv4::{Ipv4Header, IPV4_HDR_MIN_LEN};
use crate::net::udp::UdpHeader;

pub const ICMP_TYPE_ECHO_REPLY: u8 = 0;
pub const ICMP_TYPE_ECHO_REQUEST: u8 = 8;
pub const ICMP_TYPE_DEST_UNREACH: u8 = 3;
pub const ICMP_TYPE_TIME_EXCEEDED: u8 = 11;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct IcmpHeader {
    pub icmp_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub rest_of_header: u32,
}

impl IcmpHeader {
    pub fn echo_request(id: u16, seq: u16) -> Self {
        IcmpHeader {
            icmp_type: ICMP_TYPE_ECHO_REQUEST,
            code: 0,
            checksum: 0,
            rest_of_header: u32::from_be((id as u32) << 16 | seq as u32),
        }
    }

    pub fn echo_reply(id: u16, seq: u16) -> Self {
        IcmpHeader {
            icmp_type: ICMP_TYPE_ECHO_REPLY,
            code: 0,
            checksum: 0,
            rest_of_header: u32::from_be((id as u32) << 16 | seq as u32),
        }
    }

    pub fn echo_identifier(&self) -> u16 {
        (u32::from_be(self.rest_of_header) >> 16) as u16
    }

    pub fn echo_sequence(&self) -> u16 {
        (u32::from_be(self.rest_of_header) & 0xFFFF) as u16
    }

    pub fn new(icmp_type: u8, code: u8) -> Self {
        IcmpHeader {
            icmp_type,
            code,
            checksum: 0,
            rest_of_header: 0,
        }
    }

    pub fn is_echo_request(&self) -> bool {
        self.icmp_type == ICMP_TYPE_ECHO_REQUEST && self.code == 0
    }

    pub fn is_echo_reply(&self) -> bool {
        self.icmp_type == ICMP_TYPE_ECHO_REPLY && self.code == 0
    }
}

pub fn compute_icmp_checksum(header: &IcmpHeader, data: &[u8]) -> u16 {
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            header as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>(),
        )
    };
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < hdr_bytes.len() {
        let word = u16::from_be_bytes([hdr_bytes[i], hdr_bytes[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn compute_icmp_checksum_raw(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Build an ICMP Destination Unreachable (Type 3, Code 3 — Port Unreachable).
/// Includes the original IP header + first 8 bytes of the offending payload.
pub fn build_port_unreachable(original_ip: &Ipv4Header, original_udp: &[u8]) -> Vec<u8> {
    let mut reply = Vec::with_capacity(8 + IPV4_HDR_MIN_LEN + 8);
    let hdr = IcmpHeader::new(ICMP_TYPE_DEST_UNREACH, 3);
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &hdr as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>(),
        )
    };
    reply.extend_from_slice(hdr_bytes);
    reply.extend_from_slice(&[0u8; 4]);
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            original_ip as *const Ipv4Header as *const u8,
            IPV4_HDR_MIN_LEN,
        )
    };
    reply.extend_from_slice(ip_bytes);
    let udp_len = core::mem::size_of::<UdpHeader>().min(original_udp.len());
    reply.extend_from_slice(&original_udp[..udp_len]);
    let cs = compute_icmp_checksum_raw(&reply);
    reply[2] = (cs >> 8) as u8;
    reply[3] = (cs & 0xFF) as u8;
    reply
}

/// Send an ICMP echo request and wait for reply.
/// Returns Some(rtt_us) on success, None on timeout or ARP failure.
pub fn icmp_ping(dest_ip: crate::net::types::Ipv4Addr, timeout_us: u64) -> Option<u64> {
    use crate::net::types::Ipv4Addr;
    use crate::net::nic::{nic_send_packet, nic_default_id, nic_get_ip};
    use crate::net::ethernet::{EthernetHeader, ETH_HDR_LEN, ETH_TYPE_IPV4, ETH_TYPE_ARP};
    use crate::net::ipv4::{build_ipv4_header, IPV4_HDR_MIN_LEN, IPV4_PROTO_ICMP};
    use crate::net::arp::ArpPacket;
    use crate::net::nic::NIC_REGISTRY;
    use core::sync::atomic::AtomicU16;
    use alloc::vec::Vec;

    static PING_ID: AtomicU16 = AtomicU16::new(1);
    let id = PING_ID.fetch_add(1, Ordering::Relaxed);
    let seq = 1u16;

    let nic_id = nic_default_id()?;
    let src_ip = nic_get_ip(nic_id).unwrap_or(Ipv4Addr::unspecified());
    if src_ip == Ipv4Addr::unspecified() { return None; }

    // Get source MAC
    let (src_mac, subnet_mask, gateway) = {
        let mut registry = NIC_REGISTRY.lock();
        let nic = registry.get_mut(nic_id)?;
        (nic.mac_address(), nic.subnet_mask(), nic.gateway())
    };

    // Determine target IP for ARP resolution (gateway if off-subnet)
    let arp_target = if (dest_ip.to_u32() & subnet_mask.to_u32()) == (src_ip.to_u32() & subnet_mask.to_u32()) {
        dest_ip
    } else {
        #[cfg(debug_assertions)]
        crate::serial_println!("[ICMP] {} is off-subnet, routing via gateway {}", dest_ip, gateway);
        gateway
    };

    // Resolve destination MAC — try ARP cache first
    let dest_mac = crate::net::arp::arp_lookup(arp_target).or_else(|| {
        // Send ARP request
        let arp_req = ArpPacket::new_request(src_mac, src_ip, arp_target);
        let arp_bytes = unsafe {
            core::slice::from_raw_parts(
                &arp_req as *const ArpPacket as *const u8,
                core::mem::size_of::<ArpPacket>(),
            )
        };
        let eth = EthernetHeader::new(
            crate::net::types::MacAddr::broadcast(), src_mac, ETH_TYPE_ARP,
        );
        let eth_bytes = unsafe {
            core::slice::from_raw_parts(
                &eth as *const EthernetHeader as *const u8,
                core::mem::size_of::<EthernetHeader>(),
            )
        };
        let mut arp_pkt = Vec::with_capacity(ETH_HDR_LEN + core::mem::size_of::<ArpPacket>());
        arp_pkt.extend_from_slice(eth_bytes);
        arp_pkt.extend_from_slice(arp_bytes);
        let _ = nic_send_packet(nic_id, &arp_pkt);

        #[cfg(debug_assertions)]
        crate::serial_println!("[ARP] Request sent for {}", arp_target);

        // Poll for ARP reply with RDTSC-based timeout (500ms)
        let arp_start = crate::boot_benchmark::rdtsc();
        let arp_tsc_per_us = crate::boot_benchmark::get_tsc_khz() / 1000;
        let arp_timeout_ticks = arp_tsc_per_us * 500_000;

        loop {
            crate::net::network_poll_all();
            if let Some(mac) = crate::net::arp::arp_lookup(arp_target) {
                #[cfg(debug_assertions)]
                crate::serial_println!("[ARP] Resolved {} -> {}", arp_target, mac);
                return Some(mac);
            }
            let now = crate::boot_benchmark::rdtsc();
            if now.wrapping_sub(arp_start) > arp_timeout_ticks {
                #[cfg(debug_assertions)]
                crate::serial_println!("[ARP] Timeout resolving {}", arp_target);
                return None;
            }
            core::hint::spin_loop();
        }
    })?;

    // Build ICMP echo request
    let icmp_hdr = IcmpHeader::echo_request(id, seq);
    let payload = [0x00u8; 56];
    let icmp_checksum = compute_icmp_checksum(&icmp_hdr, &payload);
    let mut hdr_with_cs = icmp_hdr;
    hdr_with_cs.checksum = icmp_checksum.to_be();
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &hdr_with_cs as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>(),
        )
    };
    let mut icmp_pkt = Vec::with_capacity(core::mem::size_of::<IcmpHeader>() + payload.len());
    icmp_pkt.extend_from_slice(hdr_bytes);
    icmp_pkt.extend_from_slice(&payload);

    // Build IP header
    let ip_hdr = build_ipv4_header(src_ip, dest_ip, IPV4_PROTO_ICMP, icmp_pkt.len(), 0);
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            &ip_hdr as *const Ipv4Header as *const u8,
            IPV4_HDR_MIN_LEN,
        )
    };

    // Build Ethernet frame
    let eth = EthernetHeader::new(dest_mac, src_mac, ETH_TYPE_IPV4);
    let eth_bytes = unsafe {
        core::slice::from_raw_parts(
            &eth as *const EthernetHeader as *const u8,
            core::mem::size_of::<EthernetHeader>(),
        )
    };

    // Assemble and send
    let mut packet = Vec::with_capacity(ETH_HDR_LEN + IPV4_HDR_MIN_LEN + icmp_pkt.len());
    packet.extend_from_slice(eth_bytes);
    packet.extend_from_slice(ip_bytes);
    packet.extend_from_slice(&icmp_pkt);
    #[cfg(debug_assertions)]
    crate::serial_println!("[ICMP] Echo Request id={} seq={} {} -> {} (dst_mac={})",
        id, seq, src_ip, dest_ip, dest_mac);
    nic_send_packet(nic_id, &packet).ok()?;

    // Poll for ICMP echo reply
    LAST_PING_REPLY.store(0, Ordering::Release);
    let start = crate::boot_benchmark::rdtsc();
    let tsc_per_us = crate::boot_benchmark::get_tsc_khz() / 1000;
    let max_ticks = tsc_per_us * timeout_us;

    loop {
        crate::net::network_poll_all();
        if LAST_PING_REPLY.load(Ordering::Acquire) == id as u64 {
            let elapsed = crate::boot_benchmark::rdtsc().wrapping_sub(start);
            #[cfg(debug_assertions)]
            crate::serial_println!("[ICMP] Echo Reply id={} rtt={}us", id, elapsed / tsc_per_us.max(1));
            return Some(elapsed / tsc_per_us.max(1));
        }
        let now = crate::boot_benchmark::rdtsc();
        if now.wrapping_sub(start) > max_ticks {
            #[cfg(debug_assertions)]
            crate::serial_println!("[ICMP] Timeout waiting for Echo Reply id={}", id);
            return None;
        }
        core::hint::spin_loop();
    }
}

static LAST_PING_REPLY: AtomicU64 = AtomicU64::new(0);

/// Called from net_handle_incoming_packet when an ICMP echo reply is received.
pub fn notify_ping_reply(id: u16, _seq: u16) {
    LAST_PING_REPLY.store(id as u64, Ordering::Release);
}

pub fn build_echo_reply(request: &IcmpHeader, data: &[u8]) -> alloc::vec::Vec<u8> {
    let mut reply = alloc::vec::Vec::with_capacity(core::mem::size_of::<IcmpHeader>() + data.len());
    let header = IcmpHeader::echo_reply(request.echo_identifier(), request.echo_sequence());
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &header as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>(),
        )
    };
    reply.extend_from_slice(hdr_bytes);
    reply.extend_from_slice(data);
    let checksum = compute_icmp_checksum(&header, data);
    let cs_bytes = checksum.to_be_bytes();
    reply[2] = cs_bytes[0];
    reply[3] = cs_bytes[1];
    #[cfg(debug_assertions)]
    crate::serial_println!("[ICMP] build_echo_reply: id={} seq={} checksum=0x{:04X} data_len={} reply_len={}",
        request.echo_identifier(), request.echo_sequence(), checksum, data.len(), reply.len());
    reply
}
