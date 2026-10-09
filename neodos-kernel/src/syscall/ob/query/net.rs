//! Ob query — sockets, TCP status and NIC info.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::SocketInfo as u32
        || info_class == ObInfoClass::SocketAddr as u32
        || info_class == ObInfoClass::TcpStatus as u32
        || info_class == ObInfoClass::SocketRecv as u32
        || info_class == ObInfoClass::NicInfo as u32
        || info_class == ObInfoClass::NetStats as u32
        || info_class == ObInfoClass::Hostname as u32
}

/// Dispatch the `net` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObInfoClass::SocketInfo as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mgr = crate::net::socket::SOCKET_MANAGER.lock();
            let socket = match mgr.get_socket(socket_id) {
                Some(s) => s,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            #[repr(C)]
            struct NetSocketInfo {
                socket_type: u32,
                direction: u32,
                local_ip: [u8; 4],
                local_port: u16,
                remote_ip: [u8; 4],
                remote_port: u16,
            }
            let info = NetSocketInfo {
                socket_type: socket.socket_type as u32,
                direction: socket.direction as u32,
                local_ip: socket.local.ip.0,
                local_port: socket.local.port.to_be(),
                remote_ip: socket.remote.ip.0,
                remote_port: socket.remote.port.to_be(),
            };
            drop(mgr);
            let sz = core::mem::size_of::<NetSocketInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const NetSocketInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::SocketAddr as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mgr = crate::net::socket::SOCKET_MANAGER.lock();
            let socket = match mgr.get_socket(socket_id) {
                Some(s) => s,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            #[repr(C)]
            struct NetSocketAddr {
                local_ip: [u8; 4],
                local_port: u16,
                remote_ip: [u8; 4],
                remote_port: u16,
            }
            let addr = NetSocketAddr {
                local_ip: socket.local.ip.0,
                local_port: socket.local.port.to_be(),
                remote_ip: socket.remote.ip.0,
                remote_port: socket.remote.port.to_be(),
            };
            drop(mgr);
            let sz = core::mem::size_of::<NetSocketAddr>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &addr as *const NetSocketAddr as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::TcpStatus as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mgr = crate::net::socket::SOCKET_MANAGER.lock();
            let socket = match mgr.get_socket(socket_id) {
                Some(s) => s,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let tcp_state = if socket.socket_type == crate::net::types::SocketType::Tcp {
                if let Some(tcp_id) = socket.tcp_conn_id {
                    crate::net::tcp::tcp_get_state(tcp_id).map(|s| s as u32).unwrap_or(0)
                } else { 0 }
            } else { 0 };
            if buf_size < 4 { return err_to_u64(SyscallError::Inval); }
            unsafe { core::ptr::write_volatile(buf_ptr as *mut u32, tcp_state); }
            4u64
        }
        // ── SocketRecv (23): read data from socket receive buffer ──
        _ if info_class == ObInfoClass::SocketRecv as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            if buf_size == 0 || buf_ptr == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let user_buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_size) };
            match crate::net::socket::socket_recv(socket_id, user_buf) {
                Ok(n) => n as u64,
                Err(_) => err_to_u64(SyscallError::Again),
            }
        }
        _ if info_class == ObInfoClass::NicInfo as u32 => {
            #[repr(C)]
            struct NicInfoRaw {
                nic_id: u32,
                mac: [u8; 6],
                ip: [u8; 4],
                link_up: u8,
                vendor_id: u16,
                device_id: u16,
                name: [u8; 16],
                description: [u8; 48],
            }
            let entry_size = core::mem::size_of::<NicInfoRaw>();
            let max_entries = buf_size / entry_size;
            if max_entries == 0 { return 0u64; }
            let count = crate::net::nic::nic_count().min(max_entries);
            for i in 0..count {
                let nic_id = i as u32;
                let mac = crate::net::nic::NIC_REGISTRY.lock().get(nic_id).map(|n| n.mac_address().0).unwrap_or([0; 6]);
                let ip = crate::net::nic::nic_get_ip(nic_id).unwrap_or(crate::net::types::Ipv4Addr::unspecified());
                let vendor_id = crate::net::nic::nic_get_vendor_id(nic_id).unwrap_or(0);
                let device_id = crate::net::nic::nic_get_device_id(nic_id).unwrap_or(0);
                let name = crate::net::nic::nic_get_name(nic_id).unwrap_or([0u8; 16]);
                let description = crate::net::nic::nic_get_description(nic_id).unwrap_or([0u8; 48]);
                let link_up = crate::net::nic::nic_is_link_up(nic_id);
                let raw = NicInfoRaw {
                    nic_id,
                    mac,
                    ip: ip.0,
                    link_up: link_up as u8,
                    vendor_id,
                    device_id,
                    name,
                    description,
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const NicInfoRaw as *const u8,
                        (buf_ptr as *mut u8).add(i * entry_size),
                        entry_size,
                    );
                }
            }
            // Loopback (#484): appended after the physical NICs when the
            // caller buffer has room. The sentinel nic_id keeps it read-only.
            let mut total = count;
            if total < max_entries {
                let (lb_id, lb_mac, lb_ip, lb_link, lb_name, lb_desc) =
                    crate::net::loopback::nic_info_entry();
                let raw = NicInfoRaw {
                    nic_id: lb_id,
                    mac: lb_mac,
                    ip: lb_ip,
                    link_up: lb_link,
                    vendor_id: 0,
                    device_id: 0,
                    name: lb_name,
                    description: lb_desc,
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const NicInfoRaw as *const u8,
                        (buf_ptr as *mut u8).add(total * entry_size),
                        entry_size,
                    );
                }
                total += 1;
            }
            (total * entry_size) as u64
        }
        _ if info_class == ObInfoClass::NetStats as u32 => {
            // Layout mirrors userland `NetIfaceStats` (libnet/libnet-nxl):
            // rx_packets u64, tx_packets u64, rx_bytes u64, tx_bytes u64,
            // rx_errors u32, tx_errors u32 (40 bytes, no padding).
            #[repr(C)]
            struct NetStatsRaw {
                rx_packets: u64,
                tx_packets: u64,
                rx_bytes: u64,
                tx_bytes: u64,
                rx_errors: u32,
                tx_errors: u32,
            }
            let entry_size = core::mem::size_of::<NetStatsRaw>();
            let max_entries = buf_size / entry_size;
            if max_entries == 0 { return 0u64; }
            // Same order as NicInfo: physical NIC slots, then loopback.
            let phys = crate::net::nic::nic_count().min(max_entries);
            let mut total = 0usize;
            for i in 0..phys {
                let (rxp, txp, rxb, txb, rxe, txe) =
                    crate::net::counters::snapshot(i);
                let raw = NetStatsRaw {
                    rx_packets: rxp, tx_packets: txp,
                    rx_bytes: rxb, tx_bytes: txb,
                    rx_errors: rxe.min(u32::MAX as u64) as u32,
                    tx_errors: txe.min(u32::MAX as u64) as u32,
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const NetStatsRaw as *const u8,
                        (buf_ptr as *mut u8).add(total * entry_size),
                        entry_size,
                    );
                }
                total += 1;
            }
            if total < max_entries {
                let (rxp, txp, rxb, txb, rxe, txe) = crate::net::counters::snapshot(
                    crate::net::counters::LOOPBACK_SLOT,
                );
                let raw = NetStatsRaw {
                    rx_packets: rxp, tx_packets: txp,
                    rx_bytes: rxb, tx_bytes: txb,
                    rx_errors: rxe.min(u32::MAX as u64) as u32,
                    tx_errors: txe.min(u32::MAX as u64) as u32,
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const NetStatsRaw as *const u8,
                        (buf_ptr as *mut u8).add(total * entry_size),
                        entry_size,
                    );
                }
                total += 1;
            }
            (total * entry_size) as u64
        }
        // ── RegistryKey (21): query key metadata (subkey count, value count) ──
        _ if info_class == ObInfoClass::Hostname as u32 => {
            let root_native = crate::cm::encode_cell(0, 0);
            let key_native = match crate::cm::cm_open_key(root_native, "CurrentControlSet\\Control\\ComputerName") {
                Ok(nid) => nid,
                Err(_) => {
                    let default = b"NeoDOS-PC";
                    if buf_size > 0 {
                        let len = default.len().min(buf_size - 1);
                        unsafe {
                            core::ptr::copy_nonoverlapping(default.as_ptr(), buf_ptr as *mut u8, len);
                            core::ptr::write((buf_ptr + len as u64) as *mut u8, 0u8);
                        }
                        return (len + 1) as u64;
                    }
                    return 0;
                }
            };
            match crate::cm::cm_query_value(key_native, "ComputerName") {
                Ok(vc) => {
                    let data = &vc.data;
                    let len = data.len().min(buf_size);
                    let copy_len = if len > 0 && data[len - 1] == 0 { len - 1 } else { len };
                    if buf_size > 0 {
                        unsafe {
                            core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr as *mut u8, copy_len.min(buf_size - 1));
                            core::ptr::write((buf_ptr + copy_len.min(buf_size - 1) as u64) as *mut u8, 0u8);
                        }
                        (copy_len.min(buf_size - 1) + 1) as u64
                    } else {
                        0
                    }
                }
                Err(_) => {
                    let default = b"NeoDOS-PC";
                    if buf_size > 0 {
                        let len = default.len().min(buf_size - 1);
                        unsafe {
                            core::ptr::copy_nonoverlapping(default.as_ptr(), buf_ptr as *mut u8, len);
                            core::ptr::write((buf_ptr + len as u64) as *mut u8, 0u8);
                        }
                        (len + 1) as u64
                    } else {
                        0
                    }
                }
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
