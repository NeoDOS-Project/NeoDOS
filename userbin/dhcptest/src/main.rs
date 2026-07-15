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
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
    }
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

// ── DHCP constants ──

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_MAGIC_COOKIE: u32 = 0x63825363;
const DHCP_OP_REQUEST: u8 = 1;
const DHCP_OP_REPLY: u8 = 2;
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_ACK: u8 = 5;
const DHCP_NAK: u8 = 6;
const DHCP_OPTION_SUBNET_MASK: u8 = 1;
const DHCP_OPTION_ROUTER: u8 = 3;
const DHCP_OPTION_DNS: u8 = 6;
const DHCP_OPTION_LEASE_TIME: u8 = 51;
const DHCP_OPTION_DHCP_MSG_TYPE: u8 = 53;
const DHCP_OPTION_SERVER_ID: u8 = 54;
const DHCP_OPTION_REQUEST_LIST: u8 = 55;
const DHCP_OPTION_END: u8 = 255;
const DHCP_BROADCAST_FLAG: u16 = 0x8000;
const MAX_RETRIES: u8 = 3;
const TIMEOUT_ITERATIONS: u32 = 400;
const YIELD_BATCH: u32 = 100;

const REG_NET_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";
const APIPA_FIRST: u32 = 0xA9FE0000; // 169.254.0.0
const APIPA_LAST: u32 = 0xA9FEFFFF;  // 169.254.255.255

#[repr(C, packed)]
struct DhcpHeader {
    op: u8,
    htype: u8,
    hlen: u8,
    hops: u8,
    xid: u32,
    secs: u16,
    flags: u16,
    ciaddr: [u8; 4],
    yiaddr: [u8; 4],
    siaddr: [u8; 4],
    giaddr: [u8; 4],
    chaddr: [u8; 16],
    sname: [u8; 64],
    file: [u8; 128],
    magic: u32,
}

const DHCP_HDR_LEN: usize = 240;

struct DhcpOptions {
    msg_type: u8,
    server_id: u32,
    subnet_mask: u32,
    gateway: u32,
    dns: u32,
    lease_time: u32,
}

// ── Helpers ──

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_hex(v: u32) {
    let mut buf = [0u8; 10];
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..8 {
        let nibble = ((v >> (28 - i * 4)) & 0xF) as u8;
        buf[2 + i] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
    }
    write_str(&buf);
}

fn write_dec_u32(mut v: u32) {
    let mut buf = [0u8; 10];
    let mut i = 9;
    if v == 0 {
        buf[i] = b'0';
        write_str(&buf[i..=i]);
        return;
    }
    while v > 0 && i > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..=9]);
}

fn write_ip(ip: u32) {
    let octets = ip.to_be_bytes();
    for (i, &o) in octets.iter().enumerate() {
        if i > 0 { write_str(b"."); }
        let mut buf = [0u8; 3];
        let mut n = 0;
        let mut v = o as u32;
        loop {
            buf[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 { break; }
        }
        for j in (0..n).rev() {
            let _ = write_str(&buf[j..=j]);
        }
    }
}

fn write_ip_line(label: &[u8], ip: u32) {
    write_str(label);
    if ip == 0 {
        write_str(b"0.0.0.0");
    } else {
        write_ip(ip);
    }
    write_str(b"\r\n");
}

fn yield_for(batches: u32) {
    for _ in 0..batches {
        for _ in 0..YIELD_BATCH {
            let _ = syscall::sys_yield();
        }
    }
}

fn is_apipa(ip: u32) -> bool {
    ip >= APIPA_FIRST && ip <= APIPA_LAST
}

// ── Registry helpers ──

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

// ── DHCP packet construction ──

fn build_dhcp_packet(
    msg_type: u8,
    xid: u32,
    mac: &[u8; 6],
    ciaddr: u32,
    request_ip: Option<u32>,
    server_ip: u32,
) -> [u8; 548] {
    let mut buf = [0u8; 548];
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(mac);

    let yiaddr = match request_ip {
        Some(ip) => ip.to_be_bytes(),
        None => [0u8; 4],
    };

    let hdr = DhcpHeader {
        op: DHCP_OP_REQUEST,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: xid.to_be(),
        secs: 0,
        flags: DHCP_BROADCAST_FLAG.to_be(),
        ciaddr: ciaddr.to_be_bytes(),
        yiaddr,
        siaddr: [0u8; 4],
        giaddr: [0u8; 4],
        chaddr,
        sname: [0u8; 64],
        file: [0u8; 128],
        magic: DHCP_MAGIC_COOKIE.to_be(),
    };

    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(
            &hdr as *const DhcpHeader as *const u8,
            DHCP_HDR_LEN,
        )
    };
    buf[..DHCP_HDR_LEN].copy_from_slice(hdr_bytes);

    let mut opt = DHCP_HDR_LEN;

    // Message type option
    buf[opt] = DHCP_OPTION_DHCP_MSG_TYPE; opt += 1;
    buf[opt] = 1; opt += 1;
    buf[opt] = msg_type; opt += 1;

    // Parameter request list
    buf[opt] = DHCP_OPTION_REQUEST_LIST; opt += 1;
    buf[opt] = 4; opt += 1;
    buf[opt] = 1; opt += 1;   // subnet mask
    buf[opt] = 3; opt += 1;   // router
    buf[opt] = 6; opt += 1;   // dns
    buf[opt] = 51; opt += 1;  // lease time

    // Server identifier (for REQUEST)
    if server_ip != 0 {
        let sip = server_ip.to_be_bytes();
        buf[opt] = DHCP_OPTION_SERVER_ID; opt += 1;
        buf[opt] = 4; opt += 1;
        buf[opt..opt + 4].copy_from_slice(&sip);
        opt += 4;
    }

    // End marker
    buf[opt] = DHCP_OPTION_END; opt += 1;

    // Pad to minimum option length
    while opt < DHCP_HDR_LEN + 64 {
        buf[opt] = 0;
        opt += 1;
    }

    buf
}

fn dhcp_payload_length() -> usize {
    DHCP_HDR_LEN + 64
}

// ── DHCP option parsing ──

fn parse_dhcp_options(data: &[u8]) -> Option<DhcpOptions> {
    if data.len() < DHCP_HDR_LEN + 4 { return None; }

    let hdr: &DhcpHeader = unsafe { &*(data.as_ptr() as *const DhcpHeader) };
    let magic = u32::from_be(hdr.magic);
    if magic != DHCP_MAGIC_COOKIE { return None; }

    let options = &data[DHCP_HDR_LEN..];
    let mut msg_type = 0;
    let mut server_id = 0u32;
    let mut subnet_mask = 0x00FFFFFFu32;
    let mut gateway = 0u32;
    let mut dns = 0u32;
    let mut lease_time = 86400u32;

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
                    server_id = u32::from_be_bytes([
                        options[i + 2], options[i + 3], options[i + 4], options[i + 5],
                    ]);
                }
                i += 6;
            }
            DHCP_OPTION_SUBNET_MASK => {
                if i + 5 < options.len() {
                    subnet_mask = u32::from_be_bytes([
                        options[i + 2], options[i + 3], options[i + 4], options[i + 5],
                    ]);
                }
                i += 6;
            }
            DHCP_OPTION_ROUTER => {
                if i + 5 < options.len() {
                    gateway = u32::from_be_bytes([
                        options[i + 2], options[i + 3], options[i + 4], options[i + 5],
                    ]);
                }
                i += options[i + 1] as usize + 2;
            }
            DHCP_OPTION_DNS => {
                if i + 5 < options.len() {
                    dns = u32::from_be_bytes([
                        options[i + 2], options[i + 3], options[i + 4], options[i + 5],
                    ]);
                }
                i += options[i + 1] as usize + 2;
            }
            DHCP_OPTION_LEASE_TIME => {
                if i + 5 < options.len() {
                    lease_time = u32::from_be_bytes([
                        options[i + 2], options[i + 3], options[i + 4], options[i + 5],
                    ]);
                }
                i += 6;
            }
            _ => {
                let opt_len = options.get(i + 1).copied().unwrap_or(0) as usize;
                i += opt_len + 2;
            }
        }
    }

    if msg_type == 0 { return None; }

    Some(DhcpOptions {
        msg_type,
        server_id,
        subnet_mask,
        gateway,
        dns,
        lease_time,
    })
}

// ── Main entry ──

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");
    write_str(b"[DHCPTEST] NeoDOS DHCP Integration Test v0.1\r\n");
    write_str(b"[DHCPTEST] ====================================\r\n");

    // ── Open registry ──
    let reg_key = syscall::sys_cm_open_key(REG_NET_PATH);
    let key_fd;
    match reg_key {
        Ok(fd) => {
            key_fd = fd;
            write_str(b"[DHCPTEST] Registry opened\r\n");
        }
        Err(_) => {
            write_str(b"[DHCPTEST] ERROR: registry key not found\r\n");
            write_str(b"DHCPTEST_FAILED\r\n");
            loop { syscall::sys_yield(); }
        }
    }

    // ── Load net.nxl ──
    write_str(b"[DHCPTEST] Loading net.nxl...\r\n");
    libnet::iface_count(); // trigger load
    write_str(b"[DHCPTEST] net.nxl loaded\r\n");

    // ── Create socket ──
    let sock_path = "\\Socket\\DHCPTest\\0";
    let sock_fd = match syscall::ob_socket_create(sock_path, 2, 0) {
        Ok(fd) => fd,
        Err(e) => {
            write_str(b"[DHCPTEST] ERROR: socket create err=");
            write_hex(e as u32);
            write_str(b"\r\n");
            write_str(b"DHCPTEST_FAILED\r\n");
            loop { syscall::sys_yield(); }
        }
    };
    write_str(b"[DHCPTEST] Socket created\r\n");

    // ── Bind to 0.0.0.0:68 ──
    if let Err(e) = syscall::ob_socket_bind(sock_fd, [0u8; 4], DHCP_CLIENT_PORT) {
        write_str(b"[DHCPTEST] ERROR: socket bind err=");
        write_hex(e as u32);
        write_str(b"\r\n");
        write_str(b"DHCPTEST_FAILED\r\n");
        loop { syscall::sys_yield(); }
    }
    write_str(b"[DHCPTEST] Socket bound to port 68\r\n");

    // ── Connect to broadcast 255.255.255.255:67 ──
    if let Err(e) = syscall::ob_socket_connect(sock_fd, [255u8; 4], DHCP_SERVER_PORT) {
        write_str(b"[DHCPTEST] ERROR: socket connect err=");
        write_hex(e as u32);
        write_str(b"\r\n");
        write_str(b"DHCPTEST_FAILED\r\n");
        loop { syscall::sys_yield(); }
    }
    write_str(b"[DHCPTEST] Connected to DHCP server\r\n");

    // ── Get MAC address ──
    let mut iface = libnet::NetIfaceInfo {
        nic_id: 0,
        mac: [0u8; 6],
        ip: [0u8; 4],
        link_up: 0,
    };
    let mac = if libnet::iface_info(0, &mut iface) == 0 {
        iface.mac
    } else {
        write_str(b"[DHCPTEST] WARN: no NIC info, using fake MAC\r\n");
        [0x02, 0x00, 0x00, 0x00, 0x00, 0x01]
    };
    write_str(b"[DHCPTEST] MAC: ");
    for (i, &b) in mac.iter().enumerate() {
        let hex = [b"0123456789ABCDEF"[((b >> 4) & 0xF) as usize],
                   b"0123456789ABCDEF"[(b & 0xF) as usize]];
        write_str(&[hex[0], hex[1]]);
        if i < 5 { write_str(b":"); }
    }
    write_str(b"\r\n");

    // Link status
    write_str(b"[DHCPTEST] Link: ");
    write_str(if iface.link_up != 0 { b"Up" } else { b"Down" });
    write_str(b"\r\n");

    // ── DHCP DORA ──
    let xid = 0x12345678;
    let mut xid_val: u32 = 0;
    let mut offered_ip: u32 = 0;
    let mut server_ip: u32 = 0;
    let mut subnet_mask: u32 = 0x00FFFFFF;
    let mut gateway: u32 = 0;
    let mut dns: u32 = 0;
    let mut lease_time: u32 = 86400;
    let mut dora_ok = false;

    // Phase 1: DISCOVER
    write_str(b"[DHCPTEST] DISCOVER xid=");
    write_hex(xid);
    write_str(b"\r\n");

    let packet = build_dhcp_packet(DHCP_DISCOVER, xid, &mac, 0, None, 0);
    let len = dhcp_payload_length();
    let _ = syscall::ob_socket_send(sock_fd, &packet[..len]);

    // Wait for OFFER
    let mut retries = 0;
    offered_ip = 0;
    'offer: loop {
        for _ in 0..TIMEOUT_ITERATIONS {
            let mut buf = [0u8; 1024];
            if let Ok(n) = syscall::ob_socket_recv(sock_fd, &mut buf) {
                if n >= DHCP_HDR_LEN + 4 {
                    let data = &buf[..n];
                    let hdr: &DhcpHeader = unsafe { &*(data.as_ptr() as *const DhcpHeader) };
                    let pkt_xid = u32::from_be(hdr.xid);
                    if pkt_xid != xid { continue; }
                    if hdr.op != DHCP_OP_REPLY { continue; }

                    if let Some(opts) = parse_dhcp_options(data) {
                        if opts.msg_type == DHCP_OFFER {
                            offered_ip = u32::from_be_bytes(hdr.yiaddr);
                            server_ip = opts.server_id;
                            subnet_mask = opts.subnet_mask;
                            if opts.gateway != 0 { gateway = opts.gateway; }
                            if opts.dns != 0 { dns = opts.dns; }
                            lease_time = opts.lease_time;
                            write_str(b"[DHCPTEST] OFFER from ");
                            write_ip(server_ip);
                            write_str(b" IP=");
                            write_ip(offered_ip);
                            write_str(b"\r\n");
                            break 'offer;
                        }
                    }
                }
            }
            yield_for(1);
        }
        retries += 1;
        if retries > MAX_RETRIES {
            write_str(b"[DHCPTEST] No OFFER after retries\r\n");
            break 'offer;
        }
        write_str(b"[DHCPTEST] Retry DISCOVER\r\n");
        let _ = syscall::ob_socket_send(sock_fd, &packet[..len]);
    }

    // Phase 2: REQUEST
    if offered_ip != 0 {
        let req_packet = build_dhcp_packet(DHCP_REQUEST, xid, &mac, 0, Some(offered_ip), server_ip);
        retries = 0;
        'ack: loop {
            let _ = syscall::ob_socket_send(sock_fd, &req_packet[..len]);
            for _ in 0..TIMEOUT_ITERATIONS / 2 {
                let mut buf = [0u8; 1024];
                if let Ok(n) = syscall::ob_socket_recv(sock_fd, &mut buf) {
                    if n >= DHCP_HDR_LEN + 4 {
                        let data = &buf[..n];
                        let hdr: &DhcpHeader = unsafe { &*(data.as_ptr() as *const DhcpHeader) };
                        let pkt_xid = u32::from_be(hdr.xid);
                        if pkt_xid != xid { continue; }
                        if hdr.op != DHCP_OP_REPLY { continue; }

                        if let Some(opts) = parse_dhcp_options(data) {
                            if opts.msg_type == DHCP_ACK {
                                let yiaddr = u32::from_be_bytes(hdr.yiaddr);
                                offered_ip = if yiaddr != 0 { yiaddr } else { offered_ip };
                                server_ip = opts.server_id;
                                if opts.subnet_mask != 0 { subnet_mask = opts.subnet_mask; }
                                if opts.gateway != 0 { gateway = opts.gateway; }
                                if opts.dns != 0 { dns = opts.dns; }
                                lease_time = opts.lease_time;
                                write_str(b"[DHCPTEST] ACK: IP=");
                                write_ip(offered_ip);
                                write_str(b" mask=");
                                write_ip(subnet_mask);
                                write_str(b" gw=");
                                write_ip(gateway);
                                write_str(b" lease=");
                                write_dec_u32(lease_time);
                                write_str(b"s\r\n");
                                dora_ok = true;
                                break 'ack;
                            }
                            if opts.msg_type == DHCP_NAK {
                                write_str(b"[DHCPTEST] NAK received\r\n");
                                offered_ip = 0;
                                break 'ack;
                            }
                        }
                    }
                }
                yield_for(1);
            }
            if retries >= MAX_RETRIES {
                write_str(b"[DHCPTEST] No ACK after retries\r\n");
                offered_ip = 0;
                break 'ack;
            }
            retries += 1;
            write_str(b"[DHCPTEST] Retry REQUEST\r\n");
        }
    }

    // ── Validation (before applying config, to avoid process exit issues) ──
    write_str(b"\r\n[DHCPTEST] === Validation ===\r\n");

    let mut all_ok = true;

    if dora_ok {
        // Check 1: IP is not 0.0.0.0
        if offered_ip != 0 {
            write_str(b"[DHCPTEST] [PASS] IP obtained: ");
            write_ip(offered_ip);
            write_str(b"\r\n");
        } else {
            write_str(b"[DHCPTEST] [FAIL] No IP address assigned\r\n");
            all_ok = false;
        }

        // Check 2: IP is not APIPA (169.254.x.x)
        if !is_apipa(offered_ip) {
            write_str(b"[DHCPTEST] [PASS] IP is not APIPA\r\n");
        } else {
            write_str(b"[DHCPTEST] [FAIL] IP is APIPA (link-local) ");
            write_ip(offered_ip);
            write_str(b"\r\n");
            all_ok = false;
        }

        // Check 3: Subnet mask exists
        if subnet_mask != 0 && subnet_mask != 0xFFFFFFFF {
            write_str(b"[DHCPTEST] [PASS] Subnet mask: ");
            write_ip(subnet_mask);
            write_str(b"\r\n");
        } else {
            write_str(b"[DHCPTEST] [FAIL] Invalid subnet mask\r\n");
            all_ok = false;
        }

        // Check 4: Gateway exists
        if gateway != 0 {
            write_str(b"[DHCPTEST] [PASS] Gateway: ");
            write_ip(gateway);
            write_str(b"\r\n");
        } else {
            write_str(b"[DHCPTEST] [WARN] No gateway assigned\r\n");
        }

        // Check 5: DNS exists
        if dns != 0 {
            write_str(b"[DHCPTEST] [PASS] DNS: ");
            write_ip(dns);
            write_str(b"\r\n");
        } else {
            write_str(b"[DHCPTEST] [WARN] No DNS assigned\r\n");
        }

        // Check 6: Lease time
        if lease_time > 0 {
            write_str(b"[DHCPTEST] [PASS] Lease time: ");
            write_dec_u32(lease_time);
            write_str(b" s\r\n");
        } else {
            write_str(b"[DHCPTEST] [WARN] Lease time is 0\r\n");
        }
    } else {
        write_str(b"[DHCPTEST] [FAIL] DHCP DORA did not complete\r\n");
        all_ok = false;
    }

    // ── Full display (like ipconfig) ──
    write_str(b"\r\n[DHCPTEST] === Network Configuration ===\r\n");
    write_str(b" Interface 0:\r\n");
    write_str(b"   MAC:          ");
    for (i, &b) in mac.iter().enumerate() {
        let hex = [b"0123456789ABCDEF"[((b >> 4) & 0xF) as usize],
                   b"0123456789ABCDEF"[(b & 0xF) as usize]];
        write_str(&[hex[0], hex[1]]);
        if i < 5 { write_str(b":"); }
    }
    write_str(b"\r\n");
    write_ip_line(b"   IPv4:         ", offered_ip);
    write_ip_line(b"   Subnet mask:  ", subnet_mask);
    write_ip_line(b"   Gateway:      ", gateway);
    if dns != 0 {
        write_ip_line(b"   DNS:          ", dns);
    }
    write_str(b"   Origin:       ");
    write_str(if dora_ok { b"DHCP" } else { b"APIPA (DHCP failed)" });
    write_str(b"\r\n");
    write_str(b"   Status:       ");
    write_str(if iface.link_up != 0 { b"Up" } else { b"Down" });
    write_str(b"\r\n");

    // ── Summary ──
    write_str(b"\r\n[DHCPTEST] === Summary ===\r\n");
    if all_ok {
        write_str(b"[DHCPTEST] All checks passed\r\n");
        write_str(b"DHCPTEST_PASSED\r\n");
    } else {
        write_str(b"[DHCPTEST] Some checks failed\r\n");
        write_str(b"DHCPTEST_FAILED\r\n");
    }
    // ── Apply IP configuration FIRST (needed for ping to work) ──
    if dora_ok {
        let _ = libnet::set_ip(0, offered_ip, subnet_mask);
        write_reg_dword(key_fd, "IPAddress", offered_ip);
        write_reg_dword(key_fd, "SubnetMask", subnet_mask);
        write_reg_dword(key_fd, "Gateway", gateway);
        write_reg_dword(key_fd, "LeaseTime", lease_time);
        write_reg_dword(key_fd, "DHCPBound", 1);
        if dns != 0 {
            write_reg_dword(key_fd, "DnsServer", dns);
        }
        write_str(b"[DHCPTEST] Configuration applied\r\n");

        // ── Ping test (ping gateway to verify connectivity) ──
        if gateway != 0 {
            write_str(b"\r\n[DHCPTEST] === Ping Test ===\r\n");
            write_str(b"[DHCPTEST] Pinging gateway ");
            write_ip(gateway);
            write_str(b"...\r\n");

            for _ in 0..3 {
                let rtt = syscall::sys_icmp_ping(gateway);
                if rtt > 0 {
                    write_str(b"[DHCPTEST] [PASS] Gateway reachable, RTT=");
                    let mut rtt_dec = [0u8; 10];
                    let mut rtt_i = 9;
                    let mut rtt_v = rtt;
                    loop {
                        rtt_dec[rtt_i] = b'0' + (rtt_v % 10) as u8;
                        rtt_v /= 10;
                        if rtt_v == 0 || rtt_i == 0 { break; }
                        rtt_i -= 1;
                    }
                    write_str(&rtt_dec[rtt_i..=9]);
                    write_str(b" us\r\n");
                    break;
                }
                for _ in 0..10000 { syscall::sys_yield(); }
            }

            // Also ping DNS server if different from gateway
            if dns != 0 && dns != gateway {
                write_str(b"[DHCPTEST] Pinging DNS ");
                write_ip(dns);
                write_str(b"...\r\n");
                let rtt2 = syscall::sys_icmp_ping(dns);
                if rtt2 > 0 {
                    write_str(b"[DHCPTEST] [PASS] DNS reachable\r\n");
                } else {
                    write_str(b"[DHCPTEST] [WARN] DNS not reachable\r\n");
                }
            }
        }
    } else {
        let apipa = 0xA9FE0101;
        let _ = libnet::set_ip(0, apipa, 0x0000FFFF);
        write_reg_dword(key_fd, "IPAddress", apipa);
        write_reg_dword(key_fd, "DHCPBound", 0);
    }

    write_str(b"DHCPTEST_COMPLETE\r\n");

    // Close
    let _ = syscall::sys_close(key_fd);

    loop { syscall::sys_yield(); }
}