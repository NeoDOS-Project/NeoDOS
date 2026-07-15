use super::types::{MacAddr, Ipv4Addr};
use spin::Mutex;
use lazy_static::lazy_static;
use alloc::vec::Vec;

pub const ARP_HW_ETHERNET: u16 = 1;
pub const ARP_PROTO_IPV4: u16 = 0x0800;
pub const ARP_OP_REQUEST: u16 = 1;
pub const ARP_OP_REPLY: u16 = 2;

pub const ARP_CACHE_SIZE: usize = 64;
pub const ARP_TIMEOUT_TICKS: u64 = 300; // 300 seconds
pub const TICKS_PER_SEC: u64 = 100;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ArpPacket {
    pub hw_type: u16,
    pub proto_type: u16,
    pub hw_len: u8,
    pub proto_len: u8,
    pub operation: u16,
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

impl ArpPacket {
    pub fn new_request(sender_mac: MacAddr, sender_ip: Ipv4Addr, target_ip: Ipv4Addr) -> Self {
        ArpPacket {
            hw_type: ARP_HW_ETHERNET.to_be(),
            proto_type: ARP_PROTO_IPV4.to_be(),
            hw_len: 6,
            proto_len: 4,
            operation: ARP_OP_REQUEST.to_be(),
            sender_mac: sender_mac.0,
            sender_ip: sender_ip.0,
            target_mac: [0; 6],
            target_ip: target_ip.0,
        }
    }

    pub fn new_reply(sender_mac: MacAddr, sender_ip: Ipv4Addr, target_mac: MacAddr, target_ip: Ipv4Addr) -> Self {
        ArpPacket {
            hw_type: ARP_HW_ETHERNET.to_be(),
            proto_type: ARP_PROTO_IPV4.to_be(),
            hw_len: 6,
            proto_len: 4,
            operation: ARP_OP_REPLY.to_be(),
            sender_mac: sender_mac.0,
            sender_ip: sender_ip.0,
            target_mac: target_mac.0,
            target_ip: target_ip.0,
        }
    }

    pub fn operation(&self) -> u16 { u16::from_be(self.operation) }
    pub fn sender_mac_addr(&self) -> MacAddr { MacAddr(self.sender_mac) }
    pub fn sender_ip_addr(&self) -> Ipv4Addr { Ipv4Addr(self.sender_ip) }
    pub fn target_ip_addr(&self) -> Ipv4Addr { Ipv4Addr(self.target_ip) }
}

#[derive(Debug, Clone)]
struct ArpEntry {
    ip: Ipv4Addr,
    mac: MacAddr,
    tick_count: u64,
    is_static: bool,
}

pub struct ArpCache {
    entries: Vec<ArpEntry>,
    tick_count: u64,
}

impl ArpCache {
    pub const fn new() -> Self {
        ArpCache {
            entries: Vec::new(),
            tick_count: 0,
        }
    }

    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.tick_count.is_multiple_of(10) {
            self.evict_expired();
        }
    }

    pub fn lookup(&self, ip: Ipv4Addr) -> Option<MacAddr> {
        self.entries.iter().find(|e| e.ip == ip).map(|e| e.mac)
    }

    pub fn insert(&mut self, ip: Ipv4Addr, mac: MacAddr) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.ip == ip) {
            entry.mac = mac;
            entry.tick_count = self.tick_count;
            return;
        }
        if self.entries.len() >= ARP_CACHE_SIZE {
            self.evict_oldest();
        }
        self.entries.push(ArpEntry {
            ip,
            mac,
            tick_count: self.tick_count,
            is_static: false,
        });
    }

    pub fn insert_static(&mut self, ip: Ipv4Addr, mac: MacAddr) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.ip == ip) {
            entry.mac = mac;
            entry.is_static = true;
            entry.tick_count = self.tick_count;
            return;
        }
        if self.entries.len() >= ARP_CACHE_SIZE {
            self.evict_oldest();
        }
        self.entries.push(ArpEntry {
            ip,
            mac,
            tick_count: self.tick_count,
            is_static: true,
        });
    }

    pub fn remove(&mut self, ip: Ipv4Addr) {
        self.entries.retain(|e| e.ip != ip);
    }

    pub fn clear(&mut self) {
        self.entries.retain(|e| e.is_static);
    }

    fn evict_expired(&mut self) {
        self.entries.retain(|e| {
            e.is_static || self.tick_count.wrapping_sub(e.tick_count) < ARP_TIMEOUT_TICKS * TICKS_PER_SEC
        });
    }

    fn evict_oldest(&mut self) {
        if self.entries.is_empty() { return; }
        let mut oldest = usize::MAX;
        let mut oldest_tick = u64::MAX;
        for (i, e) in self.entries.iter().enumerate() {
            if e.is_static { continue; }
            if e.tick_count < oldest_tick {
                oldest = i;
                oldest_tick = e.tick_count;
            }
        }
        if oldest == usize::MAX {
            // Only static entries remain — remove the first one as last resort
            self.entries.remove(0);
        } else {
            self.entries.remove(oldest);
        }
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

lazy_static! {
    pub static ref ARP_CACHE: Mutex<ArpCache> = Mutex::new(ArpCache::new());
}

pub fn arp_lookup(ip: Ipv4Addr) -> Option<MacAddr> {
    ARP_CACHE.lock().lookup(ip)
}

pub fn arp_insert(ip: Ipv4Addr, mac: MacAddr) {
    ARP_CACHE.lock().insert(ip, mac);
    crate::serial_println!("[ARP] Cache insert: {} -> {}", ip, mac);
}

pub fn arp_tick() {
    ARP_CACHE.lock().tick();
}

pub fn arp_make_packet(op: u16, sender_mac: MacAddr, sender_ip: Ipv4Addr, target_mac: MacAddr, target_ip: Ipv4Addr) -> ArpPacket {
    match op {
        ARP_OP_REQUEST => ArpPacket::new_request(sender_mac, sender_ip, target_ip),
        ARP_OP_REPLY => ArpPacket::new_reply(sender_mac, sender_ip, target_mac, target_ip),
        _ => ArpPacket::new_request(sender_mac, sender_ip, target_ip),
    }
}

pub fn arp_cache_entries() -> alloc::vec::Vec<(Ipv4Addr, MacAddr)> {
    let cache = ARP_CACHE.lock();
    cache.entries.iter().map(|e| (e.ip, e.mac)).collect()
}

/// Resolve an IP to a MAC address. First checks the cache; if not found,
/// sends an ARP request over the default NIC and returns None immediately.
/// The caller should retry later after the reply arrives.
pub fn arp_resolve(target_ip: Ipv4Addr) -> Option<MacAddr> {
    // Broadcast IP maps to broadcast MAC directly (no ARP needed)
    if target_ip.is_broadcast() {
        return Some(MacAddr::broadcast());
    }
    if let Some(mac) = arp_lookup(target_ip) {
        return Some(mac);
    }
    let nic_id = crate::net::nic::nic_default_id()?;
    let mut registry = crate::net::nic::NIC_REGISTRY.lock();
    let nic = registry.get_mut(nic_id)?;
    let src_mac = nic.mac_address();
    let src_ip = nic.ip_address();
    drop(registry);

    let arp_pkt = arp_make_packet(ARP_OP_REQUEST, src_mac, src_ip, MacAddr::zero(), target_ip);
    let arp_bytes = unsafe {
        core::slice::from_raw_parts(
            &arp_pkt as *const ArpPacket as *const u8,
            core::mem::size_of::<ArpPacket>(),
        )
    };
    let frame = crate::net::ethernet::build_ethernet_frame(
        MacAddr::broadcast(), src_mac, crate::net::ethernet::ETH_TYPE_ARP, arp_bytes,
    );
    let _ = crate::net::nic::nic_send_packet(nic_id, &frame);
    None
}
