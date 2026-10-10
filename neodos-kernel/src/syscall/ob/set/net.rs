//! Ob set — sockets, NIC IP/gateway and hostname.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use crate::log::LogSubsys;

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::SocketConnect as u32
        || info_class == ObSetInfoClass::SocketBind as u32
        || info_class == ObSetInfoClass::SocketListen as u32
        || info_class == ObSetInfoClass::SocketSend as u32
        || info_class == ObSetInfoClass::SocketClose as u32
        || info_class == ObSetInfoClass::SetNicIp as u32
        || info_class == ObSetInfoClass::SetNicGateway as u32
        || info_class == ObSetInfoClass::SocketBindNic as u32
        || info_class == ObSetInfoClass::SetHostname as u32
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
        _ if info_class == ObSetInfoClass::SocketConnect as u32 => {
            if buf_size < 6 { return err_to_u64(SyscallError::Inval); }
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let ip_bytes = unsafe { core::ptr::read_volatile(buf_ptr as *const [u8; 4]) };
            let port = unsafe { core::ptr::read_volatile((buf_ptr + 4) as *const u16) };
            let remote = crate::net::types::SocketAddrV4::new(
                crate::net::types::Ipv4Addr(ip_bytes),
                u16::from_be(port),
            );
            // TCP initiates the handshake (SYN); UDP/raw record the peer.
            // Previously this only flipped a flag, so a userland TCP socket
            // never sent a SYN and every subsequent send failed.
            if crate::net::socket::socket_connect_user(socket_id, remote) {
                kdebug!(LogSubsys::Object, "Connect sid={}", socket_id);
                0
            } else {
                err_to_u64(SyscallError::BadF)
            }
        }
        _ if info_class == ObSetInfoClass::SocketBind as u32 => {
            if buf_size < 6 { return err_to_u64(SyscallError::Inval); }
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let ip_bytes = unsafe { core::ptr::read_volatile(buf_ptr as *const [u8; 4]) };
            let port = unsafe { core::ptr::read_volatile((buf_ptr + 4) as *const u16) };
            let local = crate::net::types::SocketAddrV4::new(
                crate::net::types::Ipv4Addr(ip_bytes),
                u16::from_be(port),
            );
            if crate::net::socket::socket_bind(socket_id, local) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SocketBindNic as u32 => {
            if buf_size < 4 { return err_to_u64(SyscallError::Inval); }
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let nic_id = u32::from_le_bytes(unsafe {
                [*(buf_ptr as *const u8), *((buf_ptr + 1) as *const u8),
                 *((buf_ptr + 2) as *const u8), *((buf_ptr + 3) as *const u8)]
            });
            if crate::net::socket::socket_set_nic(socket_id, nic_id) { 0 }
            else { err_to_u64(SyscallError::NoEnt) }
        }
        _ if info_class == ObSetInfoClass::SocketListen as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            if crate::net::socket::socket_listen(socket_id) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SocketSend as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mut temp = alloc::vec![0u8; buf_size];
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr as *const u8, temp.as_mut_ptr(), buf_size);
            }
            match crate::net::socket::socket_send(socket_id, &temp) {
                Ok(n) => n as u64,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObSetInfoClass::SocketClose as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            crate::net::socket::socket_close(socket_id);
            crate::net::socket::socket_free(socket_id);
            let _ = crate::object::namespace::ob_remove_object(obj.name_str());
            0
        }
        // ── RegistryCreateKey (23): create a subkey (name in buf) ──
        _ if info_class == ObSetInfoClass::SetNicIp as u32 => {
            if buf_size < 8 { return err_to_u64(SyscallError::Inval); }
            let iface_idx = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            let ip_bytes = unsafe { core::ptr::read_volatile((buf_ptr + 4) as *const [u8; 4]) };
            let ip = crate::net::types::Ipv4Addr(ip_bytes);
            kdebug!(LogSubsys::Object, "SetNicIp: iface={} ip={}", iface_idx, ip);
            crate::net::nic::nic_set_ip(iface_idx, ip);
            if buf_size >= 12 {
                let mask_bytes = unsafe { core::ptr::read_volatile((buf_ptr + 8) as *const [u8; 4]) };
                let mask = crate::net::types::Ipv4Addr(mask_bytes);
                kdebug!(LogSubsys::Object, "SetNicMask: iface={} mask={}", iface_idx, mask);
                crate::net::nic::nic_set_mask(iface_idx, mask);
            }
            0
        }
        // ── SetNicGateway (28): set NIC default gateway (0.0.0.0 = unset) ──
        _ if info_class == ObSetInfoClass::SetNicGateway as u32 => {
            if buf_size < 8 { return err_to_u64(SyscallError::Inval); }
            let iface_idx = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            let gw_bytes = unsafe { core::ptr::read_volatile((buf_ptr + 4) as *const [u8; 4]) };
            let gw = crate::net::types::Ipv4Addr(gw_bytes);
            kdebug!(LogSubsys::Object, "SetNicGateway: iface={} gw={}", iface_idx, gw);
            crate::net::nic::nic_set_gateway(iface_idx, gw);
            0
        }
        _ if info_class == ObSetInfoClass::SetHostname as u32 => {
            if !crate::syscall::is_current_admin() {
                return err_to_u64(SyscallError::Perm);
            }
            if buf_size == 0 || buf_size > 64 {
                return err_to_u64(SyscallError::Inval);
            }
            let hostname_bytes = {
                let mut tmp = [0u8; 64];
                let copy_len = buf_size.min(63);
                unsafe {
                    core::ptr::copy_nonoverlapping(buf_ptr as *const u8, tmp.as_mut_ptr(), copy_len);
                }
                tmp[copy_len] = 0;
                let s = match core::str::from_utf8(&tmp[..copy_len]) {
                    Ok(s) => s.trim_end_matches('\0'),
                    Err(_) => return err_to_u64(SyscallError::Inval),
                };
                if s.is_empty() {
                    return err_to_u64(SyscallError::Inval);
                }
                let mut v = alloc::vec![0u8; s.len() + 1];
                v[..s.len()].copy_from_slice(s.as_bytes());
                v
            };
            let root_native = crate::cm::encode_cell(0, 0);
            let ctrl_native = match crate::cm::cm_open_key(root_native, "CurrentControlSet\\Control") {
                Ok(nid) => nid,
                Err(_) => return err_to_u64(SyscallError::NoEnt),
            };
            let key_native = match crate::cm::cm_open_key(ctrl_native, "ComputerName") {
                Ok(nid) => nid,
                Err(_) => match crate::cm::cm_create_key(ctrl_native, "ComputerName") {
                    Ok(nid) => nid,
                    Err(_) => return err_to_u64(SyscallError::Io),
                },
            };
            match crate::cm::cm_set_value(key_native, "ComputerName", 1, &hostname_bytes) {
                Ok(()) => {
                    let _ = crate::cm::cm_flush_key(key_native);
                    0
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
