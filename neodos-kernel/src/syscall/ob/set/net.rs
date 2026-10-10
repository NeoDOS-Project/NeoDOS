//! Ob set — sockets, NIC IP/gateway and hostname.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError, copy_from_user};
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
            let mut ab = [0u8; 6];
            if copy_from_user(&mut ab, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let ip_bytes = [ab[0], ab[1], ab[2], ab[3]];
            let port = u16::from_ne_bytes([ab[4], ab[5]]);
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
            let mut ab = [0u8; 6];
            if copy_from_user(&mut ab, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let ip_bytes = [ab[0], ab[1], ab[2], ab[3]];
            let port = u16::from_ne_bytes([ab[4], ab[5]]);
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
            let mut nb = [0u8; 4];
            if copy_from_user(&mut nb, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let nic_id = u32::from_le_bytes(nb);
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
            if copy_from_user(&mut temp, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
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
            let mut hdr = [0u8; 8];
            if copy_from_user(&mut hdr, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let iface_idx = u32::from_ne_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
            let ip = crate::net::types::Ipv4Addr([hdr[4], hdr[5], hdr[6], hdr[7]]);
            kdebug!(LogSubsys::Object, "SetNicIp: iface={} ip={}", iface_idx, ip);
            crate::net::nic::nic_set_ip(iface_idx, ip);
            if buf_size >= 12 {
                let mut mb = [0u8; 4];
                if copy_from_user(&mut mb, buf_ptr + 8).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                let mask = crate::net::types::Ipv4Addr(mb);
                kdebug!(LogSubsys::Object, "SetNicMask: iface={} mask={}", iface_idx, mask);
                crate::net::nic::nic_set_mask(iface_idx, mask);
            }
            0
        }
        // ── SetNicGateway (28): set NIC default gateway (0.0.0.0 = unset) ──
        _ if info_class == ObSetInfoClass::SetNicGateway as u32 => {
            if buf_size < 8 { return err_to_u64(SyscallError::Inval); }
            let mut hdr = [0u8; 8];
            if copy_from_user(&mut hdr, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let iface_idx = u32::from_ne_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
            let gw = crate::net::types::Ipv4Addr([hdr[4], hdr[5], hdr[6], hdr[7]]);
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
                if copy_from_user(&mut tmp[..copy_len], buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
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
