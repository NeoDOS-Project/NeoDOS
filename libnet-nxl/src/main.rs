#![no_std]
#![no_main]
#![allow(clippy::missing_safety_doc)]

use core::arch::asm;

// ── Ob type constants (match libneodos) ──
const OB_TYPE_SOCKET: u32 = 18;

// ── ObInfoClass constants ──
const INFO_CLASS_NIC_INFO: u32 = 20;

// ── ObSetInfoClass constants ──
const SET_SOCKET_CONNECT: u32 = 18;
const SET_SOCKET_BIND: u32 = 19;
const SET_SOCKET_LISTEN: u32 = 20;
const SET_SOCKET_SEND: u32 = 21;
const SET_SOCKET_CLOSE: u32 = 22;
const SET_NIC_IP: u32 = 27;

// ── ObAccess ──
const OB_READ: u32 = 1;

// ── Syscall wrappers ──

#[inline(always)]
unsafe fn syscall_2(sys_num: u64, a0: u64, a1: u64) -> i64 {
    let r: i64;
    asm!(
        "push rbx",
        "mov rbx, {a0}",
        "mov rcx, {a1}",
        "mov r10, {n}",
        "mov rax, r10",
        "int 0x80",
        "pop rbx",
        a0 = in(reg) a0, a1 = in(reg) a1, n = in(reg) sys_num,
        lateout("rax") r, lateout("r10") _,
        out("rcx") _, out("r8") _, out("r9") _,
    );
    r
}

#[inline(always)]
#[allow(dead_code)]
unsafe fn syscall_3(sys_num: u64, a0: u64, a1: u64, a2: u64) -> i64 {
    let r: i64;
    asm!(
        "push rbx",
        "mov rbx, {a0}",
        "mov rcx, {a1}",
        "mov rdx, {a2}",
        "mov r10, {n}",
        "mov rax, r10",
        "int 0x80",
        "pop rbx",
        a0 = in(reg) a0, a1 = in(reg) a1, a2 = in(reg) a2, n = in(reg) sys_num,
        lateout("rax") r, lateout("r10") _,
        out("rcx") _, out("rdx") _, out("r8") _, out("r9") _,
    );
    r
}

#[inline(always)]
unsafe fn syscall_4(sys_num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let r: i64;
    asm!(
        "push rbx",
        "mov rbx, {a0}",
        "mov rcx, {a1}",
        "mov rdx, {a2}",
        "mov r8,  {a3}",
        "mov r10, {n}",
        "mov rax, r10",
        "int 0x80",
        "pop rbx",
        a0 = in(reg) a0, a1 = in(reg) a1, a2 = in(reg) a2, a3 = in(reg) a3, n = in(reg) sys_num,
        lateout("rax") r, lateout("r10") _,
        out("rcx") _, out("rdx") _, out("r8") _, out("r9") _,
    );
    r
}

unsafe fn ob_open(path: &str, access: u32) -> i64 {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return -1; }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    syscall_2(40, buf.as_ptr() as u64, access as u64)
}

unsafe fn ob_create(path: &str, obj_type: u32, attrs: u64) -> i64 {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return -1; }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    syscall_4(41, buf.as_ptr() as u64, obj_type as u64, 0u64, attrs)
}

unsafe fn ob_close(fd: u8) -> i64 {
    syscall_2(23, fd as u64, 0)
}

unsafe fn ob_set_info(fd: u8, class: u32, buf: *const u8, len: usize) -> i64 {
    syscall_4(43, fd as u64, class as u64, buf as u64, len as u64)
}

unsafe fn ob_query_info(fd: u8, class: u32, buf: *mut u8, len: usize) -> i64 {
    syscall_4(42, fd as u64, class as u64, buf as u64, len as u64)
}

// ── Data types ──

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

// ── NXL API functions ──

#[no_mangle]
fn query_nic_info(buf: &mut [u8]) -> i64 {
    unsafe {
        let fd = match ob_open("\\Global\\Info\\Network\0", OB_READ) {
            r if r >= 0 => r as u8,
            _ => return -1,
        };
        let r = ob_query_info(fd, INFO_CLASS_NIC_INFO, buf.as_mut_ptr(), buf.len());
        let _ = ob_close(fd);
        r
    }
}

pub extern "C" fn net_iface_count() -> u32 {
    let mut buf = [0u8; 32];
    let r = query_nic_info(&mut buf);
    if r < 0 { return 0; }
    (r as usize / 12) as u32
}

#[no_mangle]
pub unsafe extern "C" fn net_iface_info(idx: u32, info: *mut NetIfaceInfo) -> i32 {
    if info.is_null() { return -1; }
    let mut buf = [0u8; 256];
    let r = query_nic_info(&mut buf);
    if r < 0 { return -1; }
    let offset = (idx as usize) * 15;
    if offset + 15 > r as usize { return -1; }
    let raw = &buf[offset..offset + 15];
    let nic_id = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&raw[4..10]);
    let mut ip = [0u8; 4];
    ip.copy_from_slice(&raw[10..14]);
    let link_up = raw[14];
    core::ptr::write(info, NetIfaceInfo { nic_id, mac, ip, link_up });
    0
}

#[no_mangle]
pub extern "C" fn net_iface_stats(_idx: u32, _stats: *mut NetIfaceStats) -> i32 {
    -1
}

#[no_mangle]
pub extern "C" fn net_socket_create(sock_type: u32) -> i32 {
    let mut id_buf = [0u8; 16];
    // TODO(net): Use Ob API for getpid (RAX=3 removed, use ob_open + ob_query_info(ProcessId))
    let id = 0u64;
    let path_len = {
        let s = b"\\NetSock-";
        id_buf[..s.len()].copy_from_slice(s);
        let mut tmp = id;
        let mut i = 15;
        while tmp > 0 {
            let d = (tmp % 16) as u8;
            id_buf[i] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
            tmp /= 16;
            i -= 1;
        }
        s.len() + (15 - i)
    };
    let path = unsafe { core::str::from_utf8_unchecked(&id_buf[..path_len]) };
    let attrs = (sock_type & 0xFF) as u64;
    let r = unsafe { ob_create(path, OB_TYPE_SOCKET, attrs) };
    if r < 0 { r as i32 } else { r as i32 }
}

#[no_mangle]
pub extern "C" fn net_socket_bind(fd: i32, ip: u32, port: u16) -> i32 {
    let mut buf = [0u8; 6];
    buf[..4].copy_from_slice(&ip.to_be_bytes());
    buf[4..6].copy_from_slice(&port.to_be_bytes());
    let r = unsafe { ob_set_info(fd as u8, SET_SOCKET_BIND, buf.as_ptr(), 6) };
    if r < 0 { r as i32 } else { 0 }
}

#[no_mangle]
pub extern "C" fn net_socket_connect(fd: i32, ip: u32, port: u16) -> i32 {
    let mut buf = [0u8; 6];
    buf[..4].copy_from_slice(&ip.to_be_bytes());
    buf[4..6].copy_from_slice(&port.to_be_bytes());
    let r = unsafe { ob_set_info(fd as u8, SET_SOCKET_CONNECT, buf.as_ptr(), 6) };
    if r < 0 { r as i32 } else { 0 }
}

#[no_mangle]
pub extern "C" fn net_socket_listen(fd: i32) -> i32 {
    let r = unsafe { ob_set_info(fd as u8, SET_SOCKET_LISTEN, core::ptr::null(), 0) };
    if r < 0 { r as i32 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn net_socket_send(fd: i32, data: *const u8, len: u32) -> i32 {
    let r = ob_set_info(fd as u8, SET_SOCKET_SEND, data, len as usize);
    if r < 0 { r as i32 } else { r as i32 }
}

#[no_mangle]
pub unsafe extern "C" fn net_socket_recv(fd: i32, buf: *mut u8, max: u32) -> i32 {
    // SocketRecv uses ObInfoClass::SocketRecv (value 23) via ob_query_info
    let r = ob_query_info(fd as u8, 23, buf, max as usize);
    if r < 0 { r as i32 } else { r as i32 }
}

#[no_mangle]
pub extern "C" fn net_socket_close(fd: i32) -> i32 {
    let r = unsafe { ob_set_info(fd as u8, SET_SOCKET_CLOSE, core::ptr::null(), 0) };
    if r < 0 { r as i32 } else { 0 }
}

#[no_mangle]
pub extern "C" fn net_set_ip(iface: u32, ip: u32, mask: u32) -> i32 {
    let mut buf = [0u8; 12];
    buf[..4].copy_from_slice(&iface.to_le_bytes());
    buf[4..8].copy_from_slice(&ip.to_be_bytes());
    buf[8..12].copy_from_slice(&mask.to_be_bytes());
    // Open \Global\Info\Network to set IP (fd 0 is stdin, not valid here)
    let fd = unsafe {
        match ob_open("\\Global\\Info\\Network\0", 3) { // OB_READ|OB_WRITE
            r if r >= 0 => r as u8,
            _ => return -1,
        }
    };
    let r = unsafe { ob_set_info(fd, SET_NIC_IP, buf.as_ptr(), 12) };
    unsafe { ob_close(fd) };
    if r < 0 { r as i32 } else { 0 }
}

#[no_mangle]
pub extern "C" fn net_set_gateway(_iface: u32, gw: u32) -> i32 {
    let key_path = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0\0";
    let fd = unsafe {
        match ob_open(key_path, 3) {
            r if r >= 0 => r as u8,
            _ => return -1,
        }
    };
    let gw_bytes = gw.to_be_bytes();
    let r = unsafe { syscall_4(53, fd as u64, 1u64, gw_bytes.as_ptr() as u64, 4u64) };
    let _ = unsafe { ob_close(fd) };
    if r < 0 { r as i32 } else { 0 }
}

#[no_mangle]
pub extern "C" fn net_get_ip(iface: u32) -> u32 {
    let mut buf = [0u8; 32];
    let r = query_nic_info(&mut buf);
    if r < 0 { return 0; }
    let offset = (iface as usize) * 15;
    if offset + 15 > r as usize { return 0; }
    u32::from_be_bytes([buf[offset + 10], buf[offset + 11], buf[offset + 12], buf[offset + 13]])
}

#[no_mangle]
pub extern "C" fn net_get_gateway(_iface: u32) -> u32 {
    0
}

#[no_mangle]
pub extern "C" fn net_get_dhcp_bound() -> i32 {
    // DHCP bound status: check if NIC 0 has a non-zero IP
    // (dhcpd.nxe sets IP via net_set_ip when bound)
    let ip = net_get_ip(0);
    if ip != 0 { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn net_get_mask(_iface: u32) -> u32 {
    0x00FFFFFF
}

// ── Export table ──

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

#[no_mangle]
#[link_section = ".export_table"]
pub static NET_EXPORT_TABLE: NetAbiTable = NetAbiTable {
    version: 1,
    iface_count: net_iface_count,
    iface_info: net_iface_info,
    iface_stats: net_iface_stats,
    socket_create: net_socket_create,
    socket_bind: net_socket_bind,
    socket_connect: net_socket_connect,
    socket_listen: net_socket_listen,
    socket_send: net_socket_send,
    socket_recv: net_socket_recv,
    socket_close: net_socket_close,
    set_ip: net_set_ip,
    set_gateway: net_set_gateway,
    get_ip: net_get_ip,
    get_gateway: net_get_gateway,
    get_mask: net_get_mask,
    get_dhcp_bound: net_get_dhcp_bound,
    _reserved: [0; 7],
};

// ── NXL boilerplate ──

#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { asm!("hlt"); } }
}

#[panic_handler]
fn nxl_panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("hlt"); } }
}
