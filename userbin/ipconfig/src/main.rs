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

fn print_iface(idx: u32, net: &NetAbiTable) {
    let mut info = NetIfaceInfo { nic_id: 0, mac: [0; 6], ip: [0; 4], link_up: 0 };
    let r = unsafe { (net.iface_info)(idx, &mut info as *mut NetIfaceInfo) };
    if r < 0 { return; }

    write_str(b" Ethernet adapter ");
    let mut ib = [0u8; 4];
    let il = u32_to_str(info.nic_id, &mut ib);
    write_str(&ib[..il]);
    write_str(b":\r\n");

    write_str(b"   MAC: ");
    for (i, &b) in info.mac.iter().enumerate() {
        let hex = [b"0123456789ABCDEF"[((b >> 4) & 0xF) as usize],
                   b"0123456789ABCDEF"[(b & 0xF) as usize]];
        write_str(&[hex[0], hex[1]]);
        if i < 5 { write_str(b":"); }
    }
    write_str(b"\r\n");

    let ip_u32 = u32::from_be_bytes(info.ip);
    write_str(b"   IP:  ");
    let mut ipb = [0u8; 16];
    let ipl = format_ip(ip_u32, &mut ipb);
    if ipl > 0 { write_str(&ipb[..ipl]); } else { write_str(b"0.0.0.0"); }
    write_str(b"\r\n");

    write_str(b"   Status: ");
    write_str(if info.link_up != 0 { b"Up" } else { b"Down" });
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

    let count = (net.iface_count)();
    if count == 0 {
        write_str(b"No network interfaces found.\r\n");
        loop { syscall::sys_yield(); }
    }

    for i in 0..count {
        print_iface(i, net);
    }

    write_str(b"\r\n");
    loop { syscall::sys_yield(); }
}
