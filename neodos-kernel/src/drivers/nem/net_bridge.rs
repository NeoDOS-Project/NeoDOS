use core::sync::atomic::{AtomicU32, Ordering};
use alloc::boxed::Box;
use crate::net::nic::{NetworkInterface, nic_register, nic_unregister};
use crate::net::types::{MacAddr, Ipv4Addr};
use crate::log::LogSubsys;
static NEXT_NEM_NIC_ID: AtomicU32 = AtomicU32::new(0x8000_0000);

#[repr(C)]
pub struct NemNetworkDevice {
    device_id: u32,
    mac: MacAddr,
    ip: Ipv4Addr,
    name: [u8; 24],
    vendor_id: u16,
    device_pci_id: u16,
    description: [u8; 48],
    send_fn: unsafe extern "C" fn(u32, *const u8, u32) -> i32,
    poll_fn: unsafe extern "C" fn(u32, *mut u8, *mut u32) -> i32,
}

impl NetworkInterface for NemNetworkDevice {
    fn mac_address(&self) -> MacAddr { self.mac }
    fn name(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(24);
        core::str::from_utf8(&self.name[..len]).unwrap_or("nem_nic")
    }
    fn description(&self) -> &str {
        let len = self.description.iter().position(|&b| b == 0).unwrap_or(48);
        core::str::from_utf8(&self.description[..len]).unwrap_or("")
    }
    fn ip_address(&self) -> Ipv4Addr { self.ip }
    fn set_ip_address(&mut self, ip: Ipv4Addr) { self.ip = ip; }
    fn vendor_id(&self) -> u16 { self.vendor_id }
    fn device_id(&self) -> u16 { self.device_pci_id }

    fn send_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        let len = packet.len().min(2048) as u32;
        let rc = unsafe { (self.send_fn)(self.device_id, packet.as_ptr(), len) };
        if rc == 0 { Ok(()) } else { Err(()) }
    }

    fn poll_packet(&mut self, buf: &mut [u8]) -> Option<usize> {
        let mut len: u32 = buf.len() as u32;
        let rc = unsafe { (self.poll_fn)(self.device_id, buf.as_mut_ptr(), &mut len as *mut u32) };
        if rc == 0 && len > 0 { Some(len as usize) } else { None }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hst_register_network_device(
    name: *const u8, name_len: u32,
    mac_addr: *const u8,
    vendor_id: u32,
    device_id: u32,
    desc: *const u8,
    desc_len: u32,
    send_fn: unsafe extern "C" fn(u32, *const u8, u32) -> i32,
    poll_fn: unsafe extern "C" fn(u32, *mut u8, *mut u32) -> i32,
) -> i32 {
    let driver_id = crate::drivers::nem::driver::current_driver_id();
    if driver_id == 0 { return -1; }

    let name_slice = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    let mac_slice = unsafe { core::slice::from_raw_parts(mac_addr, 6) };
    let desc_slice = if !desc.is_null() && desc_len > 0 {
        unsafe { core::slice::from_raw_parts(desc, desc_len.min(47) as usize) }
    } else {
        &[]
    };
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&mac_slice[..6]);

    let mut name_buf = [0u8; 24];
    let len = (name_len as usize).min(23);
    name_buf[..len].copy_from_slice(&name_slice[..len]);

    let mut desc_buf = [0u8; 48];
    let dlen = desc_slice.len().min(47);
    if dlen > 0 {
        desc_buf[..dlen].copy_from_slice(&desc_slice[..dlen]);
    }

    let inner_device_id = NEXT_NEM_NIC_ID.fetch_add(1, Ordering::Relaxed);

    let nic = NemNetworkDevice {
        device_id: inner_device_id,
        mac: MacAddr(mac),
        ip: Ipv4Addr::unspecified(),
        name: name_buf,
        vendor_id: vendor_id as u16,
        device_pci_id: device_id as u16,
        description: desc_buf,
        send_fn,
        poll_fn,
    };

    match nic_register(Box::new(nic)) {
        Some(nic_id) => {
            crate::drivers::hotreload::track_resource(
                driver_id,
                crate::drivers::hotreload::ResourceType::NetworkDevice,
                nic_id,
            );
            kinfo!(LogSubsys::Net, "Registered NEM NIC {} as id {}", inner_device_id, nic_id);
            nic_id as i32
        }
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn hst_unregister_network_device(nic_id: i32) -> i32 {
    if nic_id < 0 { return -1; }
    let driver_id = crate::drivers::nem::driver::current_driver_id();
    if driver_id != 0 {
        crate::drivers::hotreload::untrack_resource(
            driver_id,
            crate::drivers::hotreload::ResourceType::NetworkDevice,
            nic_id as u32,
        );
    }
    nic_unregister(nic_id as u32);
    kinfo!(LogSubsys::Net, "Unregistered NEM NIC id={}", nic_id);
    0
}
