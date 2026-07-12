#![no_std]
#![no_main]

use libneodos::syscall;

// ── DHCP constants ──

#[allow(dead_code)]
const DHCP_SERVER_PORT: u16 = 67;
#[allow(dead_code)]
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
#[allow(dead_code)]
const DHCP_OPTION_DNS: u8 = 6;
const DHCP_OPTION_LEASE_TIME: u8 = 51;
const DHCP_OPTION_DHCP_MSG_TYPE: u8 = 53;
const DHCP_OPTION_SERVER_ID: u8 = 54;
const DHCP_OPTION_REQUEST_LIST: u8 = 55;
const DHCP_OPTION_END: u8 = 255;
const DHCP_BROADCAST_FLAG: u16 = 0x8000;
const MAX_RETRIES: u8 = 3;
const TIMEOUT_ITERATIONS: u32 = 200;
const LEASE_RENEW_DIVISOR: u64 = 2;
const YIELD_BATCH: u32 = 100;

const REG_NET_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";

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

enum DhcpState {
    Init,
    Selecting,
    Requesting,
    #[allow(dead_code)]
    Bound,
    #[allow(dead_code)]
    Renewing,
}

struct DhcpClient {
    xid: u32,
    socket_fd: i32,
    mac: [u8; 6],
    state: DhcpState,
    server_ip: u32,
    offered_ip: u32,
    subnet_mask: u32,
    gateway: u32,
    lease_time: u32,
    renew_interval: u64,
    ticks_in_state: u64,
    retry_count: u8,
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

fn yield_for(batches: u32) {
    for _ in 0..batches {
        for _ in 0..YIELD_BATCH {
            let _ = syscall::sys_yield();
        }
    }
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

#[allow(dead_code)]
fn write_reg_string(key_fd: u8, name: &str, val: &[u8]) {
    let _ = syscall::sys_cm_set_value(key_fd, name, syscall::REG_SZ, val);
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

struct DhcpOptions {
    msg_type: u8,
    server_id: u32,
    subnet_mask: u32,
    gateway: u32,
    lease_time: u32,
}

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
        lease_time,
    })
}

// ── DHCP state machine ──

impl DhcpClient {
    fn new(xid: u32, socket_fd: i32, mac: [u8; 6]) -> Self {
        DhcpClient {
            xid,
            socket_fd,
            mac,
            state: DhcpState::Init,
            server_ip: 0,
            offered_ip: 0,
            subnet_mask: 0x00FFFFFF,
            gateway: 0,
            lease_time: 86400,
            renew_interval: 43200,
            ticks_in_state: 0,
            retry_count: 0,
        }
    }

    fn send_dhcp(&self, msg_type: u8, request_ip: Option<u32>) {
        let ciaddr = if msg_type == DHCP_REQUEST && self.offered_ip != 0 {
            self.offered_ip
        } else {
            0
        };
        let packet = build_dhcp_packet(msg_type, self.xid, &self.mac, ciaddr, request_ip, self.server_ip);
        let len = dhcp_payload_length();
        let _ = syscall::ob_socket_send(self.socket_fd as u8, &packet[..len]);
    }

    fn run(&mut self) -> u32 {
        loop {
            match self.state {
                DhcpState::Init => {
                    write_str(b"\r\n[dhcpd] DISCOVER xid=");
                    write_hex(self.xid);
                    write_str(b"\r\n");
                    self.send_dhcp(DHCP_DISCOVER, None);
                    self.state = DhcpState::Selecting;
                    self.ticks_in_state = 0;
                    self.retry_count = 0;
                }

                DhcpState::Selecting => {
                    if let Some(ip) = self.poll_response() {
                        self.offered_ip = ip;
                        write_str(b"[dhcpd] OFFER from ");
                        write_ip(self.server_ip);
                        write_str(b": IP=");
                        write_ip(self.offered_ip);
                        write_str(b"\r\n");
                        self.state = DhcpState::Requesting;
                        self.ticks_in_state = 0;
                        self.retry_count = 0;
                        continue;
                    }
                    self.ticks_in_state += 1;
                    if self.ticks_in_state % 10 == 0 {
                        write_str(b".");
                    }
                    if self.ticks_in_state >= TIMEOUT_ITERATIONS as u64 {
                        self.retry_count += 1;
                        if self.retry_count > MAX_RETRIES {
                            write_str(b"\r\n[dhcpd] No OFFER after retries\r\n");
                            return 0;
                        }
                        write_str(b"\r\n[dhcpd] Retry DISCOVER\r\n");
                        self.send_dhcp(DHCP_DISCOVER, None);
                        self.ticks_in_state = 0;
                    } else {
                        yield_for(1);
                    }
                }

                DhcpState::Requesting => {
                    self.send_dhcp(DHCP_REQUEST, Some(self.offered_ip));
                    self.ticks_in_state = 0;
                    self.retry_count = 0;

                    loop {
                        if let Some(ip) = self.poll_response() {
                            write_str(b"[dhcpd] ACK: IP=");
                            write_ip(ip);
                            write_str(b" mask=");
                            write_ip(self.subnet_mask);
                            write_str(b" gw=");
                            write_ip(self.gateway);
                            write_str(b" lease=");
                            write_dec_u32(self.lease_time);
                            write_str(b"s\r\n");
                            return ip;
                        }
                        self.ticks_in_state += 1;
                        if self.ticks_in_state >= TIMEOUT_ITERATIONS as u64 {
                            self.retry_count += 1;
                            if self.retry_count > MAX_RETRIES {
                                write_str(b"\r\n[dhcpd] No ACK after retries\r\n");
                                return 0;
                            }
                            write_str(b"\r\n[dhcpd] Retry REQUEST\r\n");
                            self.send_dhcp(DHCP_REQUEST, Some(self.offered_ip));
                            self.ticks_in_state = 0;
                        } else {
                            yield_for(1);
                        }
                    }
                }

                DhcpState::Bound | DhcpState::Renewing => {
                    // This state is handled in the outer loop after run() returns
                    return self.offered_ip;
                }
            }
        }
    }

    fn poll_response(&mut self) -> Option<u32> {
        let mut buf = [0u8; 1024];
        let ret = syscall::ob_socket_recv(self.socket_fd as u8, &mut buf);
        let len = match ret {
            Ok(n) => n,
            Err(_) => return None,
        };
        if len >= DHCP_HDR_LEN + 4 {
            let data = &buf[..len];

            let hdr: &DhcpHeader = unsafe { &*(data.as_ptr() as *const DhcpHeader) };
            let pkt_xid = u32::from_be(hdr.xid);
            if pkt_xid != self.xid {
                return None;
            }
            if hdr.op != DHCP_OP_REPLY {
                return None;
            }

            let opts = parse_dhcp_options(data)?;

            match opts.msg_type {
                DHCP_OFFER => {
                    if self.server_ip == 0 && self.offered_ip == 0 {
                        self.server_ip = opts.server_id;
                        self.offered_ip = u32::from_be_bytes(hdr.yiaddr);
                        self.subnet_mask = opts.subnet_mask;
                        if opts.gateway != 0 { self.gateway = opts.gateway; }
                        self.lease_time = opts.lease_time;
                        self.renew_interval = (opts.lease_time as u64 / LEASE_RENEW_DIVISOR).max(60);
                        return Some(self.offered_ip);
                    }
                }
                DHCP_ACK => {
                    let yiaddr = u32::from_be_bytes(hdr.yiaddr);
                    let ip = if yiaddr != 0 { yiaddr } else { self.offered_ip };
                    self.offered_ip = ip;
                    self.server_ip = opts.server_id;
                    if opts.subnet_mask != 0 { self.subnet_mask = opts.subnet_mask; }
                    if opts.gateway != 0 { self.gateway = opts.gateway; }
                    self.lease_time = opts.lease_time;
                    self.renew_interval = (opts.lease_time as u64 / LEASE_RENEW_DIVISOR).max(60);
                    return Some(ip);
                }
                DHCP_NAK => {
                    write_str(b"\r\n[dhcpd] NAK received\r\n");
                    self.state = DhcpState::Init;
                    return Some(0xFFFFFFFF);
                }
                _ => {}
            }
        }
        None
    }

    #[allow(dead_code)]
    fn renew(&mut self) -> bool {
        write_str(b"[dhcpd] Renewing lease...\r\n");
        self.send_dhcp(DHCP_REQUEST, Some(self.offered_ip));
        for _ in 0..TIMEOUT_ITERATIONS {
            if let Some(ip) = self.poll_response() {
                if ip == 0xFFFFFFFF { return false; }
                write_str(b"[dhcpd] Renewal OK: IP=");
                write_ip(ip);
                write_str(b"\r\n");
                return true;
            }
            yield_for(1);
        }
        write_str(b"[dhcpd] Renewal failed\r\n");
        false
    }
}

// ── Main entry ──

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n[dhcpd] NeoDOS DHCP Service v0.1\r\n");

    let reg_key = syscall::sys_cm_open_key(REG_NET_PATH);
    let key_fd;
    match reg_key {
        Ok(fd) => { key_fd = fd; }
        Err(_) => {
            write_str(b"[dhcpd] ERROR: registry key not found\r\n");
            loop { syscall::sys_yield(); }
        }
    }

    let dhcp_enabled = read_reg_dword(key_fd, "DHCPEnabled").unwrap_or(1);

    if dhcp_enabled == 0 {
        // Static IP mode
        write_str(b"[dhcpd] Static IP config\r\n");
        let ip = read_reg_dword(key_fd, "IPAddress").unwrap_or(0);
        if ip != 0 {
            let mask = read_reg_dword(key_fd, "SubnetMask").unwrap_or(0x00FFFFFF);
            let _ = libnet::set_ip(0, ip, mask);
            write_str(b"[dhcpd] Static IP=");
            write_ip(ip);
            write_str(b"\r\n");
            write_reg_dword(key_fd, "IPAddress", ip);
        }
        write_str(b"[dhcpd] OK\r\n");
        loop { syscall::sys_yield(); }
    }

    // ── DHCP mode ──
    write_str(b"[dhcpd] DHCP enabled, starting DORA...\r\n");

    let sock_path = "\\Socket\\DHCP\\0";
    let sock_fd = match syscall::ob_socket_create(sock_path, 2, 0) {
        Ok(fd) => fd,
        Err(e) => {
            write_str(b"[dhcpd] ERROR: socket create err=");
            write_hex(e as u32);
            write_str(b"\r\n");
            let apipa = 0xA9FE0101;
            let _ = libnet::set_ip(0, apipa, 0x0000FFFF);
            write_str(b"[dhcpd] APIPA 169.254.1.1 (socket create failed)\r\n");
            write_reg_dword(key_fd, "IPAddress", apipa);
            write_reg_dword(key_fd, "DHCPBound", 1);
            loop { syscall::sys_yield(); }
        }
    };

    // Bind to 0.0.0.0:68 (DHCP client port)
    if let Err(e) = syscall::ob_socket_bind(sock_fd, [0u8; 4], 68) {
        write_str(b"[dhcpd] ERROR: socket bind err=");
        write_hex(e as u32);
        write_str(b"\r\n");
        let apipa = 0xA9FE0101;
        let _ = libnet::set_ip(0, apipa, 0x0000FFFF);
        write_str(b"[dhcpd] APIPA 169.254.1.1 (bind failed)\r\n");
        write_reg_dword(key_fd, "IPAddress", apipa);
        write_reg_dword(key_fd, "DHCPBound", 1);
        loop { syscall::sys_yield(); }
    }

    // Connect to broadcast 255.255.255.255:67 (DHCP server port)
    if let Err(e) = syscall::ob_socket_connect(sock_fd, [255u8; 4], 67) {
        write_str(b"[dhcpd] ERROR: socket connect err=");
        write_hex(e as u32);
        write_str(b"\r\n");
        let apipa = 0xA9FE0101;
        let _ = libnet::set_ip(0, apipa, 0x0000FFFF);
        write_str(b"[dhcpd] APIPA 169.254.1.1 (connect failed)\r\n");
        write_reg_dword(key_fd, "IPAddress", apipa);
        write_reg_dword(key_fd, "DHCPBound", 1);
        loop { syscall::sys_yield(); }
    }

    // Get MAC address and current IP from NIC 0
    let mut iface = libnet::NetIfaceInfo {
        nic_id: 0,
        mac: [0u8; 6],
        ip: [0u8; 4],
        link_up: 0,
    };
    let mac = if libnet::iface_info(0, &mut iface) == 0 {
        let cur_ip = u32::from_be_bytes(iface.ip);
        write_str(b"[dhcpd] NIC IP=");
        write_ip(cur_ip);
        write_str(b"\r\n");
        iface.mac
    } else {
        write_str(b"[dhcpd] WARN: no NIC info, using fake MAC\r\n");
        [0x02, 0x00, 0x00, 0x00, 0x00, 0x01]
    };

    let xid = 0x12345678;
    let mut client = DhcpClient::new(xid, sock_fd as i32, mac);
    let ip = client.run();

    if ip != 0 {
        write_str(b"[dhcpd] DORA complete, IP=");
        write_ip(ip);
        write_str(b" mask=");
        write_ip(client.subnet_mask);
        write_str(b" gw=");
        write_ip(client.gateway);
        write_str(b" lease=");
        write_dec_u32(client.lease_time);
        write_str(b"s\r\n");

        // Configure NIC via libnet
        let _ = libnet::set_ip(0, ip, client.subnet_mask);

        write_reg_dword(key_fd, "IPAddress", ip);
        write_reg_dword(key_fd, "SubnetMask", client.subnet_mask);
        write_reg_dword(key_fd, "Gateway", client.gateway);
        write_reg_dword(key_fd, "LeaseTime", client.lease_time);
        write_reg_dword(key_fd, "DHCPBound", 1);

        write_str(b"[dhcpd] Network configured\r\n");
    } else {
        write_str(b"[dhcpd] DORA failed, using APIPA fallback\r\n");
        let apipa = 0xA9FE0101;
        let _ = libnet::set_ip(0, apipa, 0x0000FFFF);
        write_str(b"[dhcpd] APIPA 169.254.1.1\r\n");
        write_reg_dword(key_fd, "IPAddress", apipa);
        write_reg_dword(key_fd, "DHCPBound", 1);
    }

    // Main loop: yield forever (DHCP renew handled by OS)
    loop { syscall::sys_yield(); }
}
