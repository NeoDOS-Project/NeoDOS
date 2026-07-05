#![no_std]

use core::sync::atomic::{AtomicU64, Ordering};
use libneodos::loadlib;

const NET_NXL_PATH: &str = "C:\\System\\Libraries\\net.nxl\0";
const EXPORT_TABLE_OFFSET: u64 = 0x00;

static NET_BASE: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
pub struct NetIfaceInfo {
    pub nic_id: u32,
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub link_up: u8,
}

#[repr(C)]
pub struct NetIfaceStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u32,
    pub tx_errors: u32,
}

#[repr(C)]
pub struct NetAbiTable {
    pub version: u32,
    pub iface_count: extern "C" fn() -> u32,
    pub iface_info: unsafe extern "C" fn(u32, *mut NetIfaceInfo) -> i32,
    pub iface_stats: extern "C" fn(u32, *mut NetIfaceStats) -> i32,
    pub socket_create: extern "C" fn(u32) -> i32,
    pub socket_bind: extern "C" fn(i32, u32, u16) -> i32,
    pub socket_connect: extern "C" fn(i32, u32, u16) -> i32,
    pub socket_listen: extern "C" fn(i32) -> i32,
    pub socket_send: unsafe extern "C" fn(i32, *const u8, u32) -> i32,
    pub socket_recv: unsafe extern "C" fn(i32, *mut u8, u32) -> i32,
    pub socket_close: extern "C" fn(i32) -> i32,
    pub set_ip: extern "C" fn(u32, u32, u32) -> i32,
    pub set_gateway: extern "C" fn(u32, u32) -> i32,
    pub get_ip: extern "C" fn(u32) -> u32,
    pub get_gateway: extern "C" fn(u32) -> u32,
    pub get_mask: extern "C" fn(u32) -> u32,
    pub get_dhcp_bound: extern "C" fn() -> i32,
    _reserved: [u64; 7],
}

fn get_table() -> Option<&'static NetAbiTable> {
    let base = NET_BASE.load(Ordering::Relaxed);
    if base != 0 {
        return Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const NetAbiTable) });
    }
    match loadlib(NET_NXL_PATH) {
        Ok(base) => {
            NET_BASE.store(base, Ordering::Relaxed);
            Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const NetAbiTable) })
        }
        Err(_) => None,
    }
}

pub fn is_loaded() -> bool {
    NET_BASE.load(Ordering::Relaxed) != 0
}

pub fn iface_count() -> u32 {
    match get_table() {
        Some(t) => (t.iface_count)(),
        None => 0,
    }
}

pub fn iface_info(idx: u32, info: &mut NetIfaceInfo) -> i32 {
    match get_table() {
        Some(t) => unsafe { (t.iface_info)(idx, info as *mut NetIfaceInfo) },
        None => -1,
    }
}

pub fn iface_stats(idx: u32, stats: &mut NetIfaceStats) -> i32 {
    match get_table() {
        Some(t) => (t.iface_stats)(idx, stats as *mut NetIfaceStats),
        None => -1,
    }
}

pub fn socket_create(sock_type: u32) -> i32 {
    match get_table() {
        Some(t) => (t.socket_create)(sock_type),
        None => -1,
    }
}

pub fn socket_bind(fd: i32, ip: u32, port: u16) -> i32 {
    match get_table() {
        Some(t) => (t.socket_bind)(fd, ip, port),
        None => -1,
    }
}

pub fn socket_connect(fd: i32, ip: u32, port: u16) -> i32 {
    match get_table() {
        Some(t) => (t.socket_connect)(fd, ip, port),
        None => -1,
    }
}

pub fn socket_listen(fd: i32) -> i32 {
    match get_table() {
        Some(t) => (t.socket_listen)(fd),
        None => -1,
    }
}

pub fn socket_send(fd: i32, data: &[u8]) -> i32 {
    match get_table() {
        Some(t) => unsafe { (t.socket_send)(fd, data.as_ptr(), data.len() as u32) },
        None => -1,
    }
}

pub fn socket_recv(fd: i32, buf: &mut [u8]) -> i32 {
    match get_table() {
        Some(t) => unsafe { (t.socket_recv)(fd, buf.as_mut_ptr(), buf.len() as u32) },
        None => -1,
    }
}

pub fn socket_close(fd: i32) -> i32 {
    match get_table() {
        Some(t) => (t.socket_close)(fd),
        None => -1,
    }
}

pub fn set_ip(iface: u32, ip: u32, mask: u32) -> i32 {
    match get_table() {
        Some(t) => (t.set_ip)(iface, ip, mask),
        None => -1,
    }
}

pub fn set_gateway(iface: u32, gw: u32) -> i32 {
    match get_table() {
        Some(t) => (t.set_gateway)(iface, gw),
        None => -1,
    }
}

pub fn get_ip(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_ip)(iface),
        None => 0,
    }
}

pub fn get_gateway(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_gateway)(iface),
        None => 0,
    }
}

pub fn get_mask(iface: u32) -> u32 {
    match get_table() {
        Some(t) => (t.get_mask)(iface),
        None => 0,
    }
}

pub fn get_dhcp_bound() -> i32 {
    match get_table() {
        Some(t) => (t.get_dhcp_bound)(),
        None => -1,
    }
}
