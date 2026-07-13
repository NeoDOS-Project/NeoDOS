#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{mem, syscall};

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

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");

    let reg_key = syscall::sys_cm_open_key(REG_NET_PATH);
    let key_fd;
    match reg_key {
        Ok(fd) => { key_fd = fd; }
        Err(_) => {
            write_str(b"netcfg: registry key not found\r\n");
            loop { syscall::sys_yield(); }
        }
    }

    let dhcp_enabled = read_reg_dword(key_fd, "DHCPEnabled").unwrap_or(1);

    if dhcp_enabled == 0 {
        // Static IP mode: read and set from registry
        let ip = read_reg_dword(key_fd, "IPAddress").unwrap_or(0);
        if ip != 0 {
            let mask = read_reg_dword(key_fd, "SubnetMask").unwrap_or(0x00FFFFFF);
            let _ = libnet::set_ip(0, ip, mask);
            let mut buf = [0u8; 16];
            let len = format_ip(ip, &mut buf);
            write_str(b"static: IP ");
            write_str(&buf[..len]);
            write_str(b"\r\n");
        }
    } else {
        // DHCP mode: wait for dhcpd to assign an IP
        write_str(b"DHCP: waiting for dhcpd...\r\n");
        let mut ip = 0u32;
        for i in 0..2000 {
            if i % 100 == 0 { write_str(b"."); }
            let _ = syscall::sys_sleep_ex();
            ip = libnet::get_ip(0);
            if ip != 0 { break; }
        }
        write_str(b"\r\n");
        if ip != 0 {
            let mut buf = [0u8; 16];
            let len = format_ip(ip, &mut buf);
            write_str(b"DHCP: IP ");
            write_str(&buf[..len]);
            write_str(b"\r\n");
            write_reg_dword(key_fd, "IPAddress", ip);
        } else {
            write_str(b"DHCP: timeout waiting for dhcpd\r\n");
        }
    }

    write_str(b"netcfg: OK\r\n");
    loop {
        for _ in 0..10000 { syscall::sys_yield(); }
    }
}
