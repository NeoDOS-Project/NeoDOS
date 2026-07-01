use crate::net::types::{Ipv4Addr, MacAddr};
use crate::net::nic::{nic_set_ip, nic_default_id, NIC_REGISTRY};
use crate::net::ethernet::{EthernetHeader, ETH_TYPE_IPV4, ETH_HDR_LEN};
use crate::net::ipv4::{Ipv4Header, build_ipv4_header, IPV4_HDR_MIN_LEN, IPV4_PROTO_UDP};
use crate::net::udp::{UdpHeader, compute_udp_checksum, UDP_HDR_LEN};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

pub const DHCP_SERVER_PORT: u16 = 67;
pub const DHCP_CLIENT_PORT: u16 = 68;
pub const DHCP_MAGIC_COOKIE: u32 = 0x63825363;

pub const DHCP_OP_REQUEST: u8 = 1;
pub const DHCP_OP_REPLY: u8 = 2;

pub const DHCP_DISCOVER: u8 = 1;
pub const DHCP_OFFER: u8 = 2;
pub const DHCP_REQUEST: u8 = 3;
pub const DHCP_DECLINE: u8 = 4;
pub const DHCP_ACK: u8 = 5;
pub const DHCP_NAK: u8 = 6;
pub const DHCP_RELEASE: u8 = 7;

pub const DHCP_OPTION_SUBNET_MASK: u8 = 1;
pub const DHCP_OPTION_ROUTER: u8 = 3;
pub const DHCP_OPTION_DNS: u8 = 6;
pub const DHCP_OPTION_LEASE_TIME: u8 = 51;
pub const DHCP_OPTION_DHCP_MSG_TYPE: u8 = 53;
pub const DHCP_OPTION_SERVER_ID: u8 = 54;
pub const DHCP_OPTION_REQUEST_LIST: u8 = 55;
pub const DHCP_OPTION_END: u8 = 255;

const DHCP_HDR_LEN: usize = 240;
const DHCP_BROADCAST_FLAG: u16 = 0x8000;

static DHCP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DhcpHeader {
    pub op: u8,
    pub htype: u8,
    pub hlen: u8,
    pub hops: u8,
    pub xid: u32,
    pub secs: u16,
    pub flags: u16,
    pub ciaddr: [u8; 4],
    pub yiaddr: [u8; 4],
    pub siaddr: [u8; 4],
    pub giaddr: [u8; 4],
    pub chaddr: [u8; 16],
    pub sname: [u8; 64],
    pub file: [u8; 128],
    pub magic: u32,
}

pub struct DhcpClient {
    pub xid: u32,
    pub state: DhcpState,
    pub server_ip: Ipv4Addr,
    pub offered_ip: Ipv4Addr,
    pub subnet_mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns_server: Ipv4Addr,
    pub lease_time: u32,
    pub ticks_since_start: u64,
    pub renew_interval: u64,
    pub last_send_tick: u64,
    pub retry_count: u8,
    pub max_retries: u8,
    pub retry_delay_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpState {
    Init,
    Selecting,
    Requesting,
    Bound,
    Renewing,
    Rebinding,
}

impl DhcpClient {
    pub fn new() -> Self {
        let xid = 0x12345678u32;
        DhcpClient {
            xid: if xid == 0 { 0x12345678 } else { xid },
            state: DhcpState::Init,
            server_ip: Ipv4Addr::unspecified(),
            offered_ip: Ipv4Addr::unspecified(),
            subnet_mask: Ipv4Addr::new([255, 255, 255, 0]),
            gateway: Ipv4Addr::new([10, 0, 1, 1]),
            dns_server: Ipv4Addr::unspecified(),
            lease_time: 86400,
            ticks_since_start: 0,
            renew_interval: 43200,
            last_send_tick: 0,
            retry_count: 0,
            max_retries: 3,
            retry_delay_ticks: 10,
        }
    }

    pub fn tick(&mut self) {
        self.ticks_since_start += 1;
        match self.state {
            DhcpState::Init => {
                self.send_discover();
                self.state = DhcpState::Selecting;
                self.last_send_tick = self.ticks_since_start;
                self.retry_count = 0;
            }
            DhcpState::Selecting => {
                let elapsed = self.ticks_since_start.wrapping_sub(self.last_send_tick);
                if elapsed >= self.retry_delay_ticks {
                    self.retry_count += 1;
                    if self.retry_count > self.max_retries {
                        crate::serial_println!("[DHCP] No offer received, giving up");
                        DHCP_IN_PROGRESS.store(false, Ordering::Relaxed);
                        return;
                    }
                    self.send_discover();
                    self.last_send_tick = self.ticks_since_start;
                }
            }
            DhcpState::Requesting => {
                let elapsed = self.ticks_since_start.wrapping_sub(self.last_send_tick);
                if elapsed >= self.retry_delay_ticks {
                    self.retry_count += 1;
                    if self.retry_count > self.max_retries {
                        crate::serial_println!("[DHCP] No ACK received, restarting");
                        self.state = DhcpState::Init;
                        return;
                    }
                    self.send_request();
                    self.last_send_tick = self.ticks_since_start;
                }
            }
            DhcpState::Bound => {
                if self.ticks_since_start >= self.renew_interval {
                    self.state = DhcpState::Renewing;
                    self.send_request();
                    self.last_send_tick = self.ticks_since_start;
                }
            }
            DhcpState::Renewing => {
                let elapsed = self.ticks_since_start.wrapping_sub(self.last_send_tick);
                if elapsed >= self.retry_delay_ticks {
                    self.retry_count += 1;
                    if self.retry_count > self.max_retries {
                        crate::serial_println!("[DHCP] Renewal failed, rebinding");
                        self.state = DhcpState::Rebinding;
                        self.send_discover();
                        self.last_send_tick = self.ticks_since_start;
                        return;
                    }
                    self.send_request();
                    self.last_send_tick = self.ticks_since_start;
                }
            }
            DhcpState::Rebinding => {
                let elapsed = self.ticks_since_start.wrapping_sub(self.last_send_tick);
                if elapsed >= self.retry_delay_ticks {
                    self.retry_count += 1;
                    if self.retry_count > self.max_retries + 2 {
                        crate::serial_println!("[DHCP] Rebinding failed, restarting");
                        self.state = DhcpState::Init;
                        return;
                    }
                    self.send_discover();
                    self.last_send_tick = self.ticks_since_start;
                }
            }
        }
    }

    fn build_dhcp_packet(&self, msg_type: u8, request_ip: Option<Ipv4Addr>) -> Vec<u8> {
        let nic_id = match nic_default_id() {
            Some(id) => id,
            None => return Vec::new(),
        };
        let mut registry = NIC_REGISTRY.lock();
        let nic = match registry.get_mut(nic_id) {
            Some(n) => n,
            None => return Vec::new(),
        };
        let client_mac = nic.mac_address();
        let current_ip = nic.ip_address();
        drop(registry);

        let mut options = Vec::new();
        options.push(DHCP_OPTION_DHCP_MSG_TYPE);
        options.push(1);
        options.push(msg_type);

        if msg_type == DHCP_DISCOVER || msg_type == DHCP_REQUEST {
            options.push(DHCP_OPTION_REQUEST_LIST);
            options.push(4);
            options.push(1);
            options.push(3);
            options.push(6);
            options.push(51);
        }

        options.push(DHCP_OPTION_END);

        while options.len() < 64 {
            options.push(0);
        }

        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(client_mac.as_bytes());

        let ciaddr = if msg_type == DHCP_REQUEST && !current_ip.is_unspecified() {
            current_ip.0
        } else {
            [0u8; 4]
        };

        let yiaddr = match request_ip {
            Some(ip) => ip.0,
            None => [0u8; 4],
        };

        let dhcp_hdr = DhcpHeader {
            op: DHCP_OP_REQUEST,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid: self.xid.to_be(),
            secs: 0,
            flags: DHCP_BROADCAST_FLAG.to_be(),
            ciaddr,
            yiaddr,
            siaddr: [0u8; 4],
            giaddr: [0u8; 4],
            chaddr,
            sname: [0u8; 64],
            file: [0u8; 128],
            magic: DHCP_MAGIC_COOKIE.to_be(),
        };

        let payload_len = core::mem::size_of::<DhcpHeader>() + options.len();
        let total_len = ETH_HDR_LEN + IPV4_HDR_MIN_LEN + UDP_HDR_LEN + payload_len;
        let mut packet = Vec::with_capacity(total_len);

        let eth = EthernetHeader::new(
            MacAddr::broadcast(),
            client_mac,
            ETH_TYPE_IPV4,
        );
        let eth_bytes = unsafe {
            core::slice::from_raw_parts(
                &eth as *const EthernetHeader as *const u8,
                ETH_HDR_LEN,
            )
        };
        packet.extend_from_slice(eth_bytes);

        let src_ip = if !current_ip.is_unspecified() { current_ip } else { Ipv4Addr::unspecified() };
        let dst_ip = Ipv4Addr::broadcast();
        let ip_hdr = build_ipv4_header(src_ip, dst_ip, IPV4_PROTO_UDP, UDP_HDR_LEN + payload_len, 0);
        let ip_bytes = unsafe {
            core::slice::from_raw_parts(
                &ip_hdr as *const Ipv4Header as *const u8,
                IPV4_HDR_MIN_LEN,
            )
        };
        packet.extend_from_slice(ip_bytes);

        let udp_hdr = UdpHeader::new(DHCP_CLIENT_PORT, DHCP_SERVER_PORT, payload_len);
        let udp_bytes = unsafe {
            core::slice::from_raw_parts(
                &udp_hdr as *const UdpHeader as *const u8,
                UDP_HDR_LEN,
            )
        };
        packet.extend_from_slice(udp_bytes);

        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(
                &dhcp_hdr as *const DhcpHeader as *const u8,
                core::mem::size_of::<DhcpHeader>(),
            )
        };
        packet.extend_from_slice(hdr_bytes);
        packet.extend_from_slice(&options);

        let udp_offset = ETH_HDR_LEN + IPV4_HDR_MIN_LEN;
        let cs = compute_udp_checksum(
            &udp_hdr, src_ip.0, dst_ip.0,
            &packet[udp_offset + UDP_HDR_LEN..],
        );
        if cs != 0 {
            packet[udp_offset + 6] = (cs >> 8) as u8;
            packet[udp_offset + 7] = (cs & 0xFF) as u8;
        }

        packet
    }

    fn send_discover(&self) {
        crate::serial_println!("[DHCP] Sending DISCOVER (xid=0x{:08x})", self.xid);
        let packet = self.build_dhcp_packet(DHCP_DISCOVER, None);
        if packet.is_empty() { return; }
        if let Some(nic_id) = nic_default_id() {
            let _ = crate::net::nic::nic_send_packet(nic_id, &packet);
        }
    }

    fn send_request(&self) {
        let request_ip = if self.offered_ip.is_unspecified() { None } else { Some(self.offered_ip) };
        crate::serial_println!("[DHCP] Sending REQUEST (xid=0x{:08x})", self.xid);
        let packet = self.build_dhcp_packet(DHCP_REQUEST, request_ip);
        if packet.is_empty() { return; }
        if let Some(nic_id) = nic_default_id() {
            let _ = crate::net::nic::nic_send_packet(nic_id, &packet);
        }
    }

    pub fn handle_offer(&mut self, packet: &[u8]) -> bool {
        let udp_offset = ETH_HDR_LEN + IPV4_HDR_MIN_LEN;
        if packet.len() < udp_offset + UDP_HDR_LEN + DHCP_HDR_LEN { return false; }

        let dhcp = unsafe {
            &*(packet.as_ptr().add(udp_offset + UDP_HDR_LEN) as *const DhcpHeader)
        };

        if u32::from_be(dhcp.magic) != DHCP_MAGIC_COOKIE { return false; }
        if u32::from_be(dhcp.xid) != self.xid { return false; }

        let options_start = udp_offset + UDP_HDR_LEN + core::mem::size_of::<DhcpHeader>();
        if options_start >= packet.len() { return false; }

        let options = &packet[options_start..];
        let mut msg_type = 0u8;
        let mut server_id = Ipv4Addr::unspecified();
        let mut subnet = Ipv4Addr::new([255, 255, 255, 0]);
        let mut gateway = Ipv4Addr::unspecified();
        let mut dns = Ipv4Addr::unspecified();
        let mut lease = 86400u32;

        let mut i = 0;
        while i < options.len() {
            match options[i] {
                DHCP_OPTION_END => break,
                DHCP_OPTION_DHCP_MSG_TYPE => {
                    if i + 2 < options.len() { msg_type = options[i + 2]; }
                    i += 3;
                }
                DHCP_OPTION_SERVER_ID => {
                    if i + 5 < options.len() {
                        server_id = Ipv4Addr::new([options[i + 2], options[i + 3], options[i + 4], options[i + 5]]);
                    }
                    i += 6;
                }
                DHCP_OPTION_SUBNET_MASK => {
                    if i + 5 < options.len() {
                        subnet = Ipv4Addr::new([options[i + 2], options[i + 3], options[i + 4], options[i + 5]]);
                    }
                    i += 6;
                }
                DHCP_OPTION_ROUTER => {
                    if i + 5 < options.len() {
                        gateway = Ipv4Addr::new([options[i + 2], options[i + 3], options[i + 4], options[i + 5]]);
                    }
                    i += options[i + 1] as usize + 2;
                }
                DHCP_OPTION_DNS => {
                    if i + 5 < options.len() {
                        dns = Ipv4Addr::new([options[i + 2], options[i + 3], options[i + 4], options[i + 5]]);
                    }
                    i += options[i + 1] as usize + 2;
                }
                DHCP_OPTION_LEASE_TIME => {
                    if i + 5 < options.len() {
                        lease = u32::from_be_bytes([
                            options[i + 2], options[i + 3],
                            options[i + 4], options[i + 5],
                        ]);
                    }
                    i += 6;
                }
                _ => {
                    let opt_len = options[i + 1] as usize;
                    i += opt_len + 2;
                }
            }
        }

        match self.state {
            DhcpState::Selecting => {
                if msg_type == DHCP_OFFER {
                    self.offered_ip = Ipv4Addr(dhcp.yiaddr);
                    self.server_ip = server_id;
                    self.subnet_mask = subnet;
                    if !gateway.is_unspecified() { self.gateway = gateway; }
                    if !dns.is_unspecified() { self.dns_server = dns; }
                    self.lease_time = lease;
                    self.renew_interval = (lease as u64 / 2).max(60);
                    crate::serial_println!("[DHCP] OFFER from {}: IP={} lease={}s",
                        server_id, self.offered_ip, lease);
                    self.state = DhcpState::Requesting;
                    self.retry_count = 0;
                    self.send_request();
                    self.last_send_tick = self.ticks_since_start;
                    return true;
                }
            }
            DhcpState::Requesting | DhcpState::Renewing => {
                if msg_type == DHCP_ACK {
                    let assigned_ip = Ipv4Addr(dhcp.yiaddr);
                    if !assigned_ip.is_unspecified() {
                        self.offered_ip = assigned_ip;
                    }
                    crate::serial_println!("[DHCP] ACK: IP={} mask={} gateway={} dns={} lease={}s",
                        self.offered_ip, self.subnet_mask, self.gateway, self.dns_server, self.lease_time);
                    nic_set_ip(0, self.offered_ip);
                    self.state = DhcpState::Bound;
                    self.retry_count = 0;
                    DHCP_IN_PROGRESS.store(false, Ordering::Relaxed);
                    return true;
                }
                if msg_type == DHCP_NAK {
                    crate::serial_println!("[DHCP] NAK received, restarting");
                    self.state = DhcpState::Init;
                    return false;
                }
            }
            _ => {}
        }
        false
    }
}

static mut DHCP_CLIENT: Option<DhcpClient> = None;

pub fn dhcp_start() {
    if DHCP_IN_PROGRESS.load(Ordering::Relaxed) {
        return;
    }
    DHCP_IN_PROGRESS.store(true, Ordering::Relaxed);
    unsafe {
        DHCP_CLIENT = Some(DhcpClient::new());
    }
    crate::serial_println!("[DHCP] Client started");
}

pub fn dhcp_tick() {
    if !DHCP_IN_PROGRESS.load(Ordering::Relaxed) { return; }
    unsafe {
        if let Some(ref mut client) = DHCP_CLIENT {
            client.tick();
            if client.state == DhcpState::Init {
                DHCP_IN_PROGRESS.store(false, Ordering::Relaxed);
            }
        }
    }
}

pub fn dhcp_handle_offer(packet: &[u8]) -> bool {
    if !DHCP_IN_PROGRESS.load(Ordering::Relaxed) { return false; }
    unsafe {
        if let Some(ref mut client) = DHCP_CLIENT {
            client.handle_offer(packet)
        } else {
            false
        }
    }
}

pub fn dhcp_is_bound() -> bool {
    unsafe {
        DHCP_CLIENT.as_ref().map_or(false, |c| c.state == DhcpState::Bound)
    }
}

pub fn dhcp_client_state() -> Option<DhcpState> {
    unsafe { DHCP_CLIENT.as_ref().map(|c| c.state) }
}

// ── Tests ──

use crate::test_case;
use crate::test_eq;
use crate::test_true;

fn test_dhcp_discover_offer_sequence() -> Result<(), &'static str> {
    let mut client = DhcpClient::new();
    client.xid = 0x12345678;
    test_eq!(client.state, DhcpState::Init);

    client.tick();
    test_eq!(client.state, DhcpState::Selecting);

    let mut offer_pkt = alloc::vec![0u8; 400];
    let eth = EthernetHeader::new(
        MacAddr::broadcast(), MacAddr::new([0; 6]), ETH_TYPE_IPV4,
    );
    let eth_bytes = unsafe {
        core::slice::from_raw_parts(
            &eth as *const EthernetHeader as *const u8, ETH_HDR_LEN,
        )
    };
    offer_pkt[..ETH_HDR_LEN].copy_from_slice(eth_bytes);

    let src_ip = Ipv4Addr::new([192, 168, 1, 1]);
    let dst_ip = Ipv4Addr::broadcast();
    let ip_hdr = build_ipv4_header(src_ip, dst_ip, IPV4_PROTO_UDP, 300, 0);
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            &ip_hdr as *const Ipv4Header as *const u8, IPV4_HDR_MIN_LEN,
        )
    };
    offer_pkt[ETH_HDR_LEN..ETH_HDR_LEN + IPV4_HDR_MIN_LEN].copy_from_slice(ip_bytes);

    let udp_hdr = UdpHeader::new(DHCP_SERVER_PORT, DHCP_CLIENT_PORT, 300 - UDP_HDR_LEN);
    let udp_bytes = unsafe {
        core::slice::from_raw_parts(
            &udp_hdr as *const UdpHeader as *const u8, UDP_HDR_LEN,
        )
    };
    offer_pkt[ETH_HDR_LEN + IPV4_HDR_MIN_LEN..ETH_HDR_LEN + IPV4_HDR_MIN_LEN + UDP_HDR_LEN]
        .copy_from_slice(udp_bytes);

    let dhcp_offset = ETH_HDR_LEN + IPV4_HDR_MIN_LEN + UDP_HDR_LEN;
    let dhcp = DhcpHeader {
        op: DHCP_OP_REPLY,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: 0x12345678u32.to_be(),
        secs: 0,
        flags: 0,
        ciaddr: [0; 4],
        yiaddr: [10, 0, 2, 100],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0; 16],
        sname: [0; 64],
        file: [0; 128],
        magic: DHCP_MAGIC_COOKIE.to_be(),
    };
    let dhcp_bytes = unsafe {
        core::slice::from_raw_parts(
            &dhcp as *const DhcpHeader as *const u8,
            core::mem::size_of::<DhcpHeader>(),
        )
    };
    offer_pkt[dhcp_offset..dhcp_offset + core::mem::size_of::<DhcpHeader>()]
        .copy_from_slice(dhcp_bytes);

    let mut opt = dhcp_offset + core::mem::size_of::<DhcpHeader>();
    offer_pkt[opt] = DHCP_OPTION_DHCP_MSG_TYPE; opt += 1;
    offer_pkt[opt] = 1; opt += 1;
    offer_pkt[opt] = DHCP_OFFER; opt += 1;
    offer_pkt[opt] = DHCP_OPTION_SUBNET_MASK; opt += 1;
    offer_pkt[opt] = 4; opt += 1;
    offer_pkt[opt] = 255; opt += 1;
    offer_pkt[opt] = 255; opt += 1;
    offer_pkt[opt] = 255; opt += 1;
    offer_pkt[opt] = 0; opt += 1;
    offer_pkt[opt] = DHCP_OPTION_ROUTER; opt += 1;
    offer_pkt[opt] = 4; opt += 1;
    offer_pkt[opt] = 10; opt += 1;
    offer_pkt[opt] = 0; opt += 1;
    offer_pkt[opt] = 1; opt += 1;
    offer_pkt[opt] = 1; opt += 1;
    offer_pkt[opt] = DHCP_OPTION_SERVER_ID; opt += 1;
    offer_pkt[opt] = 4; opt += 1;
    offer_pkt[opt] = 192; opt += 1;
    offer_pkt[opt] = 168; opt += 1;
    offer_pkt[opt] = 1; opt += 1;
    offer_pkt[opt] = 1; opt += 1;
    offer_pkt[opt] = DHCP_OPTION_LEASE_TIME; opt += 1;
    offer_pkt[opt] = 4; opt += 1;
    offer_pkt[opt] = 0; opt += 1;
    offer_pkt[opt] = 0; opt += 1;
    offer_pkt[opt] = 0x00; opt += 1;
    offer_pkt[opt] = 0x12; opt += 1;
    offer_pkt[opt] = DHCP_OPTION_END; opt += 1;

    let handled = client.handle_offer(&offer_pkt);
    test_true!(handled);
    test_eq!(client.state, DhcpState::Requesting);
    test_eq!(client.offered_ip, Ipv4Addr::new([10, 0, 2, 100]));
    test_eq!(client.server_ip, Ipv4Addr::new([192, 168, 1, 1]));

    Ok(())
}

fn test_dhcp_ack_sequence() -> Result<(), &'static str> {
    let mut client = DhcpClient::new();
    client.xid = 0x87654321;
    client.state = DhcpState::Requesting;
    client.offered_ip = Ipv4Addr::new([10, 0, 2, 100]);

    let mut ack_pkt = alloc::vec![0u8; 400];
    let eth = EthernetHeader::new(
        MacAddr::broadcast(), MacAddr::new([0; 6]), ETH_TYPE_IPV4,
    );
    let eth_bytes = unsafe {
        core::slice::from_raw_parts(
            &eth as *const EthernetHeader as *const u8, ETH_HDR_LEN,
        )
    };
    ack_pkt[..ETH_HDR_LEN].copy_from_slice(eth_bytes);

    let ip_hdr = build_ipv4_header(
        Ipv4Addr::new([192, 168, 1, 1]), Ipv4Addr::broadcast(), IPV4_PROTO_UDP, 300, 0,
    );
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            &ip_hdr as *const Ipv4Header as *const u8, IPV4_HDR_MIN_LEN,
        )
    };
    ack_pkt[ETH_HDR_LEN..ETH_HDR_LEN + IPV4_HDR_MIN_LEN].copy_from_slice(ip_bytes);

    let udp_hdr = UdpHeader::new(DHCP_SERVER_PORT, DHCP_CLIENT_PORT, 300 - UDP_HDR_LEN);
    let udp_bytes = unsafe {
        core::slice::from_raw_parts(
            &udp_hdr as *const UdpHeader as *const u8, UDP_HDR_LEN,
        )
    };
    ack_pkt[ETH_HDR_LEN + IPV4_HDR_MIN_LEN..ETH_HDR_LEN + IPV4_HDR_MIN_LEN + UDP_HDR_LEN]
        .copy_from_slice(udp_bytes);

    let dhcp_offset = ETH_HDR_LEN + IPV4_HDR_MIN_LEN + UDP_HDR_LEN;
    let dhcp = DhcpHeader {
        op: DHCP_OP_REPLY,
        htype: 1, hlen: 6, hops: 0,
        xid: 0x87654321u32.to_be(),
        secs: 0, flags: 0,
        ciaddr: [0; 4],
        yiaddr: [10, 0, 2, 100],
        siaddr: [0; 4], giaddr: [0; 4],
        chaddr: [0; 16],
        sname: [0; 64], file: [0; 128],
        magic: DHCP_MAGIC_COOKIE.to_be(),
    };
    let dhcp_bytes = unsafe {
        core::slice::from_raw_parts(
            &dhcp as *const DhcpHeader as *const u8,
            core::mem::size_of::<DhcpHeader>(),
        )
    };
    ack_pkt[dhcp_offset..dhcp_offset + core::mem::size_of::<DhcpHeader>()]
        .copy_from_slice(dhcp_bytes);

    let mut opt = dhcp_offset + core::mem::size_of::<DhcpHeader>();
    ack_pkt[opt] = DHCP_OPTION_DHCP_MSG_TYPE; opt += 1;
    ack_pkt[opt] = 1; opt += 1;
    ack_pkt[opt] = DHCP_ACK; opt += 1;
    ack_pkt[opt] = DHCP_OPTION_SUBNET_MASK; opt += 1;
    ack_pkt[opt] = 4; opt += 1;
    ack_pkt[opt] = 255; opt += 1;
    ack_pkt[opt] = 255; opt += 1;
    ack_pkt[opt] = 255; opt += 1;
    ack_pkt[opt] = 0; opt += 1;
    ack_pkt[opt] = DHCP_OPTION_END; opt += 1;

    let handled = client.handle_offer(&ack_pkt);
    test_true!(handled);
    test_eq!(client.state, DhcpState::Bound);
    test_eq!(client.offered_ip, Ipv4Addr::new([10, 0, 2, 100]));

    Ok(())
}

pub fn register_dhcp_tests() {
    test_case!("dhcp_discover_offer_sequence", {
        test_dhcp_discover_offer_sequence().unwrap();
    });
    test_case!("dhcp_lease_renewal", {
        test_dhcp_ack_sequence().unwrap();
    });
}
