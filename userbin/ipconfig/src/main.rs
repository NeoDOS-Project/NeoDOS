#![no_std]
#![no_main]

use libneodos::syscall;

// NetAbiTable struct (must match libnet-nxl exports)
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

const REG_NET_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn u32_to_str(mut v: u32, buf: &mut [u8]) -> usize {
    let mut i = if buf.len() > 0 { buf.len() - 1 } else { return 0; };
    loop {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 || i == 0 { break; }
        i -= 1;
    }
    let start = if v == 0 && i < buf.len() - 1 { i } else { if buf.len() > 0 { 0 } else { return 0; } };
    let n = buf.len() - start;
    if start > 0 && n < buf.len() {
        for j in 0..n { buf[j] = buf[start + j]; }
    }
    n
}

fn format_ip(ip: u32, buf: &mut [u8]) -> usize {
    let octets = ip.to_be_bytes();
    let mut pos = 0;
    for (i, &o) in octets.iter().enumerate() {
        if i > 0 { if pos < buf.len() { buf[pos] = b'.'; pos += 1; } }
        let mut d = [0u8; 3];
        let n = u32_to_str(o as u32, &mut d);
        for j in 0..n {
            if pos < buf.len() { buf[pos] = d[j]; pos += 1; }
        }
    }
    pos
}

fn read_reg_dword(key_fd: u8, name: &str) -> Option<u32> {
    let mut reg_buf = [0u8; 12];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 12 { return None; }
    let value_type = u32::from_le_bytes([reg_buf[0], reg_buf[1], reg_buf[2], reg_buf[3]]);
    if value_type != syscall::REG_DWORD { return None; }
    Some(u32::from_le_bytes([reg_buf[8], reg_buf[9], reg_buf[10], reg_buf[11]]))
}

fn read_reg_str(key_fd: u8, name: &str, buf: &mut [u8]) -> Option<usize> {
    let mut reg_buf = [0u8; 64];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 8 { return None; }
    let data_len = u32::from_le_bytes([reg_buf[4], reg_buf[5], reg_buf[6], reg_buf[7]]) as usize;
    let copy_len = data_len.min(buf.len());
    if copy_len > 0 {
        buf[..copy_len].copy_from_slice(&reg_buf[8..8 + copy_len]);
    }
    Some(copy_len)
}

fn write_ip_label(label: &[u8], ip: u32) {
    write_str(label);
    if ip == 0 {
        write_str(b"0.0.0.0");
    } else {
        let mut ipb = [0u8; 16];
        let ipl = format_ip(ip, &mut ipb);
        write_str(&ipb[..ipl]);
    }
    write_str(b"\r\n");
}

fn print_iface(idx: u32, net: &NetAbiTable, reg_key_fd: Option<u8>) {
    let mut info = NetIfaceInfo { nic_id: 0, mac: [0; 6], ip: [0; 4], link_up: 0 };
    let r = unsafe { (net.iface_info)(idx, &mut info as *mut NetIfaceInfo) };
    if r < 0 { return; }

    write_str(b" Ethernet adapter ");
    let mut ib = [0u8; 4];
    let il = u32_to_str(info.nic_id, &mut ib);
    write_str(&ib[..il]);
    write_str(b":\r\n");

    write_str(b"   Description:  ");
    write_str(b"Intel PRO/1000 (e1000)\r\n");

    write_str(b"   MAC:          ");
    for (i, &b) in info.mac.iter().enumerate() {
        let hex = [b"0123456789ABCDEF"[((b >> 4) & 0xF) as usize],
                   b"0123456789ABCDEF"[(b & 0xF) as usize]];
        write_str(&[hex[0], hex[1]]);
        if i < 5 { write_str(b":"); }
    }
    write_str(b"\r\n");

    let ip_u32 = u32::from_be_bytes(info.ip);

    let mask_val = (net.get_mask)(idx);
    let gw = (net.get_gateway)(idx);

    write_ip_label(b"   IPv4:         ", ip_u32);
    write_ip_label(b"   Subnet mask:  ", mask_val);
    write_ip_label(b"   Gateway:      ", gw);

    // Try to read DNS from registry
    if let Some(fd) = reg_key_fd {
        let mut dns_buf = [0u8; 16];
        if let Some(len) = read_reg_str(fd, "DnsServer", &mut dns_buf) {
            if len > 0 {
                // Truncate at null terminator
                let end = dns_buf[..len].iter().position(|&c| c == 0).unwrap_or(len);
                if end > 0 {
                    write_str(b"   DNS:          ");
                    write_str(&dns_buf[..end]);
                    write_str(b"\r\n");
                }
            }
        }
    }

    // Origin: DHCP or Static
    let dhcp_bound = (net.get_dhcp_bound)();
    write_str(b"   Origin:       ");
    if dhcp_bound != 0 {
        write_str(b"DHCP");
    } else if ip_u32 != 0 {
        write_str(b"Static");
    } else {
        write_str(b"None");
    }
    write_str(b"\r\n");

    write_str(b"   Status:       ");
    write_str(if info.link_up != 0 { b"Up" } else { b"Down" });
    write_str(b"\r\n");

    // Try to read lease info from registry
    if let Some(fd) = reg_key_fd {
        if dhcp_bound != 0 {
            if let Some(lease) = read_reg_dword(fd, "LeaseTime") {
                write_str(b"   Lease time:   ");
                let mut lb = [0u8; 10];
                let ll = u32_to_str(lease, &mut lb);
                write_str(&lb[..ll]);
                write_str(b" s\r\n");
            }
        }
    }

    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");

    let base = match syscall::sys_loadlib("C:\\System\\Libraries\\net.nxl\0") {
        Ok(b) => b,
        Err(_) => {
            write_str(b"IPCONFIG: net.nxl not found\r\n");
            loop { syscall::sys_yield(); }
        }
    };
    let net: &NetAbiTable = unsafe { &*(base as *const NetAbiTable) };

    // Open registry for DNS / lease info
    let reg_fd = syscall::sys_cm_open_key(REG_NET_PATH).ok();

    let count = (net.iface_count)();
    if count == 0 {
        write_str(b"No network interfaces found.\r\n");
        loop { syscall::sys_yield(); }
    }

    for i in 0..count {
        print_iface(i, net, reg_fd);
    }

    if let Some(fd) = reg_fd {
        let _ = syscall::sys_close(fd);
    }

    loop { syscall::sys_yield(); }
}