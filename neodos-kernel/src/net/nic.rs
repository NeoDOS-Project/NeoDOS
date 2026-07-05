use super::types::{MacAddr, Ipv4Addr, MAX_NICS};
use spin::Mutex;
use lazy_static::lazy_static;
use alloc::boxed::Box;

pub trait NetworkInterface: Send + Sync {
    fn mac_address(&self) -> MacAddr;
    fn name(&self) -> &str;
    fn send_packet(&mut self, packet: &[u8]) -> Result<(), ()>;
    fn poll_packet(&mut self, buf: &mut [u8]) -> Option<usize>;
    fn set_ip_address(&mut self, ip: Ipv4Addr);
    fn ip_address(&self) -> Ipv4Addr;
    fn subnet_mask(&self) -> Ipv4Addr { Ipv4Addr::new([255, 255, 255, 0]) }
    fn gateway(&self) -> Ipv4Addr { Ipv4Addr::new([10, 0, 1, 1]) }
    fn is_link_up(&self) -> bool { true }
    fn mtu(&self) -> usize { 1500 }
}

struct NicSlot {
    interface: Option<Box<dyn NetworkInterface>>,
    ip: Ipv4Addr,
    mask: Ipv4Addr,
    mac: MacAddr,
}

pub struct NicRegistry {
    nics: [NicSlot; MAX_NICS],
    next_id: u32,
    active_count: usize,
}

impl NicRegistry {
    pub const fn new() -> Self {
        const EMPTY: NicSlot = NicSlot {
            interface: None,
            ip: Ipv4Addr::new([0; 4]),
            mask: Ipv4Addr::new([0; 4]),
            mac: MacAddr::new([0; 6]),
        };
        NicRegistry {
            nics: [EMPTY; MAX_NICS],
            next_id: 0,
            active_count: 0,
        }
    }

    pub fn register(&mut self, interface: Box<dyn NetworkInterface>) -> Option<u32> {
        for i in 0..MAX_NICS {
            if self.nics[i].interface.is_none() {
                let mac = interface.mac_address();
                self.nics[i] = NicSlot {
                    interface: Some(interface),
                    ip: Ipv4Addr::unspecified(),
                    mask: Ipv4Addr::unspecified(),
                    mac,
                };
                self.active_count += 1;
                return Some(i as u32);
            }
        }
        None
    }

    pub fn unregister(&mut self, id: u32) {
        if (id as usize) < MAX_NICS && self.nics[id as usize].interface.is_some() {
            self.nics[id as usize].interface = None;
            self.active_count -= 1;
        }
    }

    pub fn get(&mut self, id: u32) -> Option<&mut Box<dyn NetworkInterface>> {
        if (id as usize) < MAX_NICS {
            self.nics[id as usize].interface.as_mut()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Box<dyn NetworkInterface>> {
        if (id as usize) < MAX_NICS {
            self.nics[id as usize].interface.as_mut()
        } else {
            None
        }
    }

    pub fn count(&self) -> usize { self.active_count }

    pub fn for_each<F: FnMut(u32, &mut Box<dyn NetworkInterface>)>(&mut self, mut f: F) {
        for i in 0..MAX_NICS {
            if let Some(ref mut nic) = self.nics[i].interface {
                f(i as u32, nic);
            }
        }
    }

    pub fn get_ip(&self, id: u32) -> Option<Ipv4Addr> {
        if (id as usize) < MAX_NICS && self.nics[id as usize].interface.is_some() {
            return Some(self.nics[id as usize].ip);
        }
        None
    }

    pub fn set_ip(&mut self, id: u32, ip: Ipv4Addr) {
        if (id as usize) < MAX_NICS {
            if let Some(ref mut nic) = self.nics[id as usize].interface {
                nic.set_ip_address(ip);
                self.nics[id as usize].ip = ip;
            }
        }
    }

    pub fn get_mask(&self, id: u32) -> Option<Ipv4Addr> {
        if (id as usize) < MAX_NICS && self.nics[id as usize].interface.is_some() {
            return Some(self.nics[id as usize].mask);
        }
        None
    }

    pub fn set_mask(&mut self, id: u32, mask: Ipv4Addr) {
        if (id as usize) < MAX_NICS && self.nics[id as usize].interface.is_some() {
            self.nics[id as usize].mask = mask;
        }
    }

    pub fn next_hop_mac(&mut self, dest_ip: Ipv4Addr) -> Option<MacAddr> {
        let gateway = if self.active_count > 0 {
            if let Some(ref nic) = self.nics[0].interface {
                nic.gateway()
            } else { dest_ip }
        } else { dest_ip };

        let target = if (dest_ip.to_u32() & 0xFFFFFF00) == 0x0A000200 {
            dest_ip
        } else {
            gateway
        };

        crate::net::arp::arp_lookup(target)
    }

    pub fn default_nic_id(&self) -> Option<u32> {
        for i in 0..MAX_NICS {
            if self.nics[i].interface.is_some() {
                return Some(i as u32);
            }
        }
        None
    }
}

lazy_static! {
    pub static ref NIC_REGISTRY: Mutex<NicRegistry> = Mutex::new(NicRegistry::new());
}

pub fn nic_register(interface: Box<dyn NetworkInterface>) -> Option<u32> {
    NIC_REGISTRY.lock().register(interface)
}

pub fn nic_unregister(id: u32) {
    NIC_REGISTRY.lock().unregister(id);
}

pub fn nic_send_packet(nic_id: u32, packet: &[u8]) -> Result<(), ()> {
    NIC_REGISTRY.lock().get_mut(nic_id)
        .ok_or(())?
        .send_packet(packet)
}

pub fn nic_count() -> usize {
    NIC_REGISTRY.lock().count()
}

pub fn nic_get_ip(nic_id: u32) -> Option<Ipv4Addr> {
    NIC_REGISTRY.lock().get_ip(nic_id)
}

pub fn nic_set_ip(nic_id: u32, ip: Ipv4Addr) {
    NIC_REGISTRY.lock().set_ip(nic_id, ip);
}

pub fn nic_get_mask(nic_id: u32) -> Option<Ipv4Addr> {
    NIC_REGISTRY.lock().get_mask(nic_id)
}

pub fn nic_set_mask(nic_id: u32, mask: Ipv4Addr) {
    NIC_REGISTRY.lock().set_mask(nic_id, mask);
}

pub fn nic_default_id() -> Option<u32> {
    NIC_REGISTRY.lock().default_nic_id()
}
