#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{i18n, mem, syscall, tr_id};

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

const APP_NAME: &str = "netcfg";
const IDS_ERR_KEY: u32 = 1001;
const IDS_STATIC: u32 = 1002;
const IDS_DHCP_WAIT: u32 = 1003;
const IDS_DHCP_TIMEOUT: u32 = 1004;
const IDS_OK: u32 = 1005;

const REG_NET_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn read_reg_dword(key_fd: u8, name: &str) -> Option<u32> {
    let mut reg_buf = [0u8; 12];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 12 { return None; }
    let value_type = u32::from_le_bytes([reg_buf[0], reg_buf[1], reg_buf[2], reg_buf[3]]);
    if value_type != syscall::REG_DWORD { return None; }
    Some(u32::from_le_bytes([reg_buf[8], reg_buf[9], reg_buf[10], reg_buf[11]]))
}

fn write_reg_dword(key_fd: u8, name: &str, val: u32) {
    let _ = syscall::sys_cm_set_value(key_fd, name, syscall::REG_DWORD, &val.to_le_bytes());
}

fn format_ip(ip: u32, buf: &mut [u8]) -> usize {
    let octets = ip.to_be_bytes();
    let mut pos = 0;
    for (i, &octet) in octets.iter().enumerate() {
        if i > 0 { if pos < buf.len() { buf[pos] = b'.'; pos += 1; } }
        let mut d = [0u8; 3];
        let mut n = 0;
        let mut v = octet as usize;
        loop {
            if n < 3 { d[n] = b'0' + (v % 10) as u8; n += 1; }
            v /= 10;
            if v == 0 { break; }
        }
        for j in (0..n).rev() {
            if pos < buf.len() { buf[pos] = d[j]; pos += 1; }
        }
    }
    pos
}

#[repr(C)]
struct NetAbiTable {
    version: u32,
    iface_count: extern "C" fn() -> u32,
    iface_info: unsafe extern "C" fn(u32, *mut NetIfaceInfo) -> i32,
    iface_stats: extern "C" fn(u32, *mut NetIfaceStats) -> i32,
    socket_create: extern "C" fn(u32) -> i32,
    socket_bind: extern "C" fn(i32, u32, u16) -> i32,
    socket_connect: extern "C" fn(i32, u32, u16) -> i32,
    socket_listen: extern "C" fn(i32) -> i32,
    socket_send: unsafe extern "C" fn(i32, *const u8, u32) -> i32,
    socket_recv: unsafe extern "C" fn(i32, *mut u8, u32) -> i32,
    socket_close: extern "C" fn(i32) -> i32,
    set_ip: extern "C" fn(u32, u32, u32) -> i32,
    set_gateway: extern "C" fn(u32, u32) -> i32,
    get_ip: extern "C" fn(u32) -> u32,
    get_gateway: extern "C" fn(u32) -> u32,
    get_mask: extern "C" fn(u32) -> u32,
    get_dhcp_bound: extern "C" fn() -> i32,
    _reserved: [u64; 7],
}

#[repr(C)]
struct NetIfaceInfo {
    nic_id: u32,
    mac: [u8; 6],
    ip: [u8; 4],
    link_up: u8,
    vendor_id: u16,
    device_id: u16,
    name: [u8; 16],
    description: [u8; 48],
}

#[repr(C)]
struct NetIfaceStats {
    rx_packets: u64,
    tx_packets: u64,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_errors: u32,
    tx_errors: u32,
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let reg_fd = match syscall::sys_cm_open_key(REG_NET_PATH) {
        Ok(fd) => fd,
        Err(_) => {
            write_str(tr_id!(IDS_ERR_KEY).as_bytes());
            write_str(b"\r\n");
            loop { syscall::sys_yield(); }
        }
    };

    let dhcp_enabled = read_reg_dword(reg_fd, "DHCPEnabled").unwrap_or(1) != 0;

    if !dhcp_enabled {
        let ip = read_reg_dword(reg_fd, "IP").unwrap_or(0);
        let mask = read_reg_dword(reg_fd, "Mask").unwrap_or(0);
        let gw = read_reg_dword(reg_fd, "Gateway").unwrap_or(0);

        let base = match syscall::sys_loadlib("C:\\System\\Libraries\\net.nxl\0") {
            Ok(b) => b,
            Err(_) => {
                write_str(tr_id!(IDS_ERR_KEY).as_bytes());
                write_str(b"\r\n");
                loop { syscall::sys_yield(); }
            }
        };
        let net: &NetAbiTable = unsafe { &*(base as *const NetAbiTable) };

        if ip != 0 {
            (net.set_ip)(0, ip, mask);
            if gw != 0 {
                (net.set_gateway)(0, gw);
            }
        }

        write_str(tr_id!(IDS_STATIC).as_bytes());
        let mut buf = [0u8; 16];
        let len = format_ip(ip, &mut buf);
        write_str(&buf[..len]);
        write_str(b"\r\n");
        let _ = syscall::sys_close(reg_fd);
        loop { syscall::sys_yield(); }
    }

    // DHCP mode — wait for it
    write_str(tr_id!(IDS_DHCP_WAIT).as_bytes());
    write_str(b"\r\n");

    let base = match syscall::sys_loadlib("C:\\System\\Libraries\\net.nxl\0") {
        Ok(b) => b,
        Err(_) => {
            write_str(tr_id!(IDS_ERR_KEY).as_bytes());
            write_str(b"\r\n");
            loop { syscall::sys_yield(); }
        }
    };
    let net: &NetAbiTable = unsafe { &*(base as *const NetAbiTable) };

    for _ in 0..100 {
        let ip = (net.get_ip)(0);
        if ip != 0 {
            write_reg_dword(reg_fd, "IP", ip);
            let mut ip_buf = [0u8; 16];
            let ip_len = format_ip(ip, &mut ip_buf);
            write_str(tr_id!(IDS_OK).as_bytes());
            write_str(b" ");
            write_str(&ip_buf[..ip_len]);
            write_str(b"\r\n");
            let _ = syscall::sys_close(reg_fd);
            loop { syscall::sys_yield(); }
        }
        for _ in 0..1000000 { core::hint::spin_loop(); }
    }

    write_str(tr_id!(IDS_DHCP_TIMEOUT).as_bytes());
    write_str(b"\r\n");
    let _ = syscall::sys_close(reg_fd);
    loop { syscall::sys_yield(); }
}
