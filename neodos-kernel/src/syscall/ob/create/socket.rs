//! Ob create — socket objects.

use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler;
use crate::syscall::ob_err_to_syscall;

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Socket
}

/// Dispatch the `socket` object kinds.
pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Socket => {
            if !crate::net::net_is_initialized() {
                return err_to_u64(SyscallError::NoSys);
            }
            let socket_type_val = attrs & 0xFF;
            let sock_type = match socket_type_val {
                1 => crate::net::types::SocketType::Tcp,
                2 => crate::net::types::SocketType::Udp,
                3 => crate::net::types::SocketType::Raw,
                _ => return err_to_u64(SyscallError::Inval),
            };
            let port = ((attrs >> 8) & 0xFFFF) as u16;

            let socket_id = match crate::net::socket::socket_alloc(sock_type) {
                Some(id) => id,
                None => return err_to_u64(SyscallError::NoMem),
            };

            // Assign default NIC if available (no NIC_REGISTRY lock ordering concern
            // since we don't hold SOCKET_MANAGER lock here).
            crate::net::socket::socket_assign_default_nic(socket_id);

            if sock_type == crate::net::types::SocketType::Tcp {
                if let Some(tcp_id) = crate::net::tcp::tcp_alloc_connection() {
                    crate::net::socket::socket_set_tcp_conn(socket_id, tcp_id);
                    crate::net::tcp::tcp_bind(tcp_id, crate::net::types::SocketAddrV4::new(
                        crate::net::types::Ipv4Addr::unspecified(), port,
                    ));
                }
            }

            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, socket_id, None,
            ) {
                Ok(id) => id,
                Err(e) => {
                    crate::net::socket::socket_free(socket_id);
                    return err_to_u64(ob_err_to_syscall(e));
                }
            };

            // Store socket_id in entry's offset for direct retrieval by socket ops
            let entry = crate::handle::HandleEntry::ob_object(ob_id, socket_id);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    crate::net::socket::socket_free(socket_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
