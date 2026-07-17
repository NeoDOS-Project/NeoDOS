#![no_std]
#![no_main]

use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "ipconfig";
const IDS_HEADER: u32 = 1001;
const IDS_HOSTNAME: u32 = 1002;
const IDS_ETHERNET: u32 = 1003;
const IDS_DESCRIPTION: u32 = 1004;
const IDS_DRIVER: u32 = 1005;
const IDS_PCI_DEVICE: u32 = 1006;
const IDS_LINK_STATUS: u32 = 1007;
const IDS_MAC: u32 = 1008;
const IDS_IPV4: u32 = 1009;
const IDS_SUBNET_MASK: u32 = 1010;
const IDS_GATEWAY: u32 = 1011;
const IDS_DNS: u32 = 1012;
const IDS_DHCP_ENABLED: u32 = 1013;
const IDS_CONFIG_SOURCE: u32 = 1014;
const IDS_LEASE_TIME: u32 = 1015;
const IDS_UP: u32 = 1016;
const IDS_DOWN: u32 = 1017;
const IDS_DHCP: u32 = 1018;
const IDS_STATIC: u32 = 1019;
const IDS_NONE: u32 = 1020;
const IDS_YES: u32 = 1021;
const IDS_NO: u32 = 1022;
const IDS_ERR_NXL: u32 = 1023;
const IDS_NO_IFACES: u32 = 1024;

#[repr(C)]
struct NetIfaceInfo {
    nic_id: u32,
    mac: [u8; 6],
    ip: [u8; 4],
    link_up: u8,
}

const REG_NET_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";

fn write_str(s: &[u8]) { let _ = syscall::sys_write(1, s); }

fn write_label(id: u32) { write_str(tr_id!(id).as_bytes()); }

fn format_ip(ip: u32, buf: &mut [u8]) -> usize {
    let mut pos = 0;
    let o = ip.to_be_bytes();
    for (idx, &b) in o.iter().enumerate() {
        if idx > 0 { if pos < buf.len() { buf[pos] = b'.'; pos += 1; } }
        if b >= 100 { buf[pos] = b'0' + b / 100; pos += 1; }
        if b >= 10  { buf[pos] = b'0' + (b / 10) % 10; pos += 1; }
        buf[pos] = b'0' + (b % 10); pos += 1;
    }
    pos
}

fn fmt_u32(v: u32, buf: &mut [u8]) -> usize {
    if v == 0 { buf[0] = b'0'; return 1; }
    let mut tmp = [0u8; 12];
    let mut i = 12;
    let mut n = v;
    while n > 0 { i -= 1; tmp[i] = b'0' + (n % 10) as u8; n /= 10; }
    let len = 12 - i;
    buf[..len].copy_from_slice(&tmp[i..12]);
    len
}

fn write_ip_label(id: u32, ip: u32) {
    write_label(id);
    let mut b = [0u8; 16];
    let n = format_ip(ip, &mut b);
    write_str(&b[..n]);
    write_str(b"\r\n");
}

fn write_val_label(id: u32, val: u32, suffix: &[u8]) {
    write_label(id);
    let mut b = [0u8; 16];
    let n = fmt_u32(val, &mut b);
    write_str(&b[..n]);
    if suffix.len() > 0 { write_str(suffix); }
    write_str(b"\r\n");
}

fn write_mac(mac: &[u8; 6]) {
    for (i, &b) in mac.iter().enumerate() {
        let h = b"0123456789ABCDEF"[((b >> 4) & 0xF) as usize];
        let l = b"0123456789ABCDEF"[(b & 0xF) as usize];
        write_str(&[h, l]);
        if i < 5 { write_str(b":"); }
    }
    write_str(b"\r\n");
}

fn read_reg_dword(fd: u8, name: &str) -> u32 {
    let mut buf = [0u8; 16];
    let r = syscall::sys_cm_query_value(fd, name, &mut buf);
    match r {
        Ok(n) if n >= 12 => {
            let t = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            if t == syscall::REG_DWORD {
                return u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
            }
        }
        _ => {}
    }
    0
}

fn read_reg_str(fd: u8, name: &str, buf: &mut [u8]) -> usize {
    let mut reg_buf = [0u8; 64];
    match syscall::sys_cm_query_value(fd, name, &mut reg_buf) {
        Ok(n) if n >= 8 => {
            let dlen = u32::from_le_bytes([reg_buf[4], reg_buf[5], reg_buf[6], reg_buf[7]]) as usize;
            let len = dlen.min(buf.len());
            if len > 0 { buf[..len].copy_from_slice(&reg_buf[8..8+len]); }
            len
        }
        _ => 0,
    }
}

fn print_iface(iface_idx: u32, info: &NetIfaceInfo, reg_fd: u8) {
    let _ = iface_idx;
    write_str(b"\r\n");
    write_label(IDS_ETHERNET);
    write_str(b" ");
    let mut ib = [0u8; 4];
    let il = fmt_u32(info.nic_id, &mut ib);
    write_str(&ib[..il]);
    write_str(b":\r\n\r\n");

    write_label(IDS_DESCRIPTION);
    write_str(b"Intel 82540EM Gigabit Ethernet\r\n");

    write_label(IDS_DRIVER);
    write_str(b"e1000.nem\r\n");

    write_label(IDS_PCI_DEVICE);
    write_str(b"8086:100E\r\n");

    write_label(IDS_LINK_STATUS);
    if info.link_up != 0 { write_label(IDS_UP); } else { write_label(IDS_DOWN); }
    write_str(b"\r\n\r\n");

    let ip_u32 = u32::from_be_bytes(info.ip);
    let mask = read_reg_dword(reg_fd, "SubnetMask");
    let gw = read_reg_dword(reg_fd, "Gateway");
    let dns = read_reg_dword(reg_fd, "DnsServer");

    write_label(IDS_MAC);
    write_mac(&info.mac);

    write_ip_label(IDS_IPV4, ip_u32);
    write_ip_label(IDS_SUBNET_MASK, if mask != 0 { mask } else { 0x00FFFFFF });
    write_ip_label(IDS_GATEWAY, gw);
    if dns != 0 {
        write_ip_label(IDS_DNS, dns);
    }
    write_str(b"\r\n");

    let dhcp_enabled = read_reg_dword(reg_fd, "DHCPEnabled") != 0;
    let dhcp_bound = read_reg_dword(reg_fd, "DHCPBound") != 0;

    write_label(IDS_DHCP_ENABLED);
    if dhcp_enabled { write_label(IDS_YES); } else { write_label(IDS_NO); }
    write_str(b"\r\n");

    write_label(IDS_CONFIG_SOURCE);
    if dhcp_bound { write_label(IDS_DHCP); }
    else if ip_u32 != 0 { write_label(IDS_STATIC); }
    else { write_label(IDS_NONE); }
    write_str(b"\r\n");

    if dhcp_bound {
        let lease = read_reg_dword(reg_fd, "LeaseTime");
        if lease > 0 {
            write_val_label(IDS_LEASE_TIME, lease, b" s");
        }
    }

    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    write_str(b"\r\n");
    write_label(IDS_HEADER);
    write_str(b"\r\n\r\n");

    write_label(IDS_HOSTNAME);
    write_str(b"NeoDOS-PC\r\n\r\n");

    let reg_fd = match syscall::sys_cm_open_key(REG_NET_PATH) {
        Ok(fd) => fd,
        Err(_) => {
            write_label(IDS_NO_IFACES);
            write_str(b"\r\n");
            loop { syscall::sys_yield(); }
        }
    };

    // Read NIC info via ObInfoClass (doesn't need NXL)
    let obj_fd = match syscall::sys_ob_open("\\Global\\Info\\Network", 1) {
        Ok(fd) => fd,
        Err(_) => {
            write_label(IDS_NO_IFACES);
            write_str(b"\r\n");
            let _ = syscall::sys_close(reg_fd);
            loop { syscall::sys_yield(); }
        }
    };

    let mut buf = [0u8; 256];
    let r = syscall::sys_ob_query_info(obj_fd, syscall::ObInfoClass::NicInfo, &mut buf);
    let _ = syscall::sys_close(obj_fd);

    if r.is_err() { write_label(IDS_NO_IFACES); write_str(b"\r\n"); let _ = syscall::sys_close(reg_fd); loop { syscall::sys_yield(); } }
    let total = r.unwrap() as usize;
    let entry_size = 15;
    let count = total / entry_size;
    if count == 0 {
        write_label(IDS_NO_IFACES);
        write_str(b"\r\n");
        let _ = syscall::sys_close(reg_fd);
        loop { syscall::sys_yield(); }
    }

    for i in 0..count {
        let off = i * entry_size;
        if off + entry_size > buf.len() { break; }
        let raw = &buf[off..off+entry_size];
        let info = NetIfaceInfo {
            nic_id: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            mac: [raw[4], raw[5], raw[6], raw[7], raw[8], raw[9]],
            ip: [raw[10], raw[11], raw[12], raw[13]],
            link_up: raw[14],
        };
        print_iface(i as u32, &info, reg_fd);
    }

    let _ = syscall::sys_close(reg_fd);
    loop { syscall::sys_yield(); }
}
