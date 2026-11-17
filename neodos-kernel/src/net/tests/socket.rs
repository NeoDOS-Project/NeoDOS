#![allow(unused_imports)]
use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::format;
use alloc::vec;
use super::super::types::{TcpState, MacAddr, Ipv4Addr, SocketType, SocketDirection, SocketAddrV4};
use super::super::arp::ArpCache;
use super::super::socket::{
    SocketManager, SOCKET_MANAGER, SendError, socket_bind, socket_close, socket_connect,
    socket_free, socket_listen, socket_poll_writable, socket_recv, socket_send,
    socket_set_tcp_conn, socket_write_wait_reason,
};
use super::super::tcp::{
    tcp_alloc_connection, tcp_bind, tcp_listen, tcp_connect, tcp_close,
    tcp_get_state, tcp_free_connection, tcp_send, tcp_send_space, tcp_recv, tcp_tick,
    TCP, TCP_SEND_BUF,
};
use super::super::nic::NicRegistry;
use super::super::ipv4::{compute_ip_checksum, build_ipv4_header, Ipv4Header};
use super::super::icmp::IcmpHeader;
use super::super::udp::UdpHeader;

use super::{CaptureNic, LinkNic, capture_nic};
pub fn register() {
    test_case!("socket_auto_port_assign", {
        // Test 1: SocketManager's ephemeral port allocator (direct unit test)
        let mut mgr = SocketManager::new();
        let port1 = mgr.allocate_ephemeral_port();
        test_true!(port1 >= 49152);
        let port2 = mgr.allocate_ephemeral_port();
        test_true!(port2 >= 49152);
        // Sequential allocator gives unique values
        test_true!(port2 != port1 || port1 == 65535);

        // Test 2: socket_bind with port 0 → auto-assign ephemeral
        let id = {
            let mut mgr = SOCKET_MANAGER.lock();
            mgr.alloc_socket(SocketType::Udp).unwrap()
        };
        test_true!(socket_bind(id, SocketAddrV4::new(Ipv4Addr::unspecified(), 0)));
        let port = {
            let mgr = SOCKET_MANAGER.lock();
            mgr.get_socket(id).unwrap().local.port
        };
        test_true!(port >= 49152);
        test_true!(port != 0);

        // Test 3: Explicit port is preserved (not overwritten)
        let id2 = {
            let mut mgr = SOCKET_MANAGER.lock();
            mgr.alloc_socket(SocketType::Udp).unwrap()
        };
        test_true!(socket_bind(id2, SocketAddrV4::new(Ipv4Addr::new([10, 0, 2, 15]), 8080)));
        {
            let mgr = SOCKET_MANAGER.lock();
            let s = mgr.get_socket(id2).unwrap();
            test_eq!(s.local.port, 8080);
            test_eq!(s.local.ip, Ipv4Addr::new([10, 0, 2, 15]));
        }

        // Cleanup
        SOCKET_MANAGER.lock().free_socket(id);
        SOCKET_MANAGER.lock().free_socket(id2);
    });

    test_case!("net_socket_poll_readiness", {
        // sys_poll readiness helpers (#307).
        let sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Udp).unwrap();
        test_true!(!super::super::socket::socket_poll_readable(sock));
        test_true!(!super::super::socket::socket_poll_writable(sock));
        {
            let mut mgr = SOCKET_MANAGER.lock();
            let s = mgr.get_socket_mut(sock).unwrap();
            s.recv_buf.extend_from_slice(b"x");
            s.direction = SocketDirection::Connected;
        }
        test_true!(super::super::socket::socket_poll_readable(sock));
        test_true!(super::super::socket::socket_poll_writable(sock));
        test_true!(!super::super::socket::socket_poll_hup(sock));
        SOCKET_MANAGER.lock().get_socket_mut(sock).unwrap().direction = SocketDirection::Closed;
        test_true!(super::super::socket::socket_poll_hup(sock));
        SOCKET_MANAGER.lock().free_socket(sock);
    });

    // ── Write wait + EOF (#712 / D5) ──

    /// Socket + TCP connection in `Established`, ready to send/recv.
    fn tcp_socket_established() -> (u32, u32) {
        let sid = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let tcp_id = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(sid, tcp_id);
        let local = SocketAddrV4::new(Ipv4Addr::localhost(), 40000);
        let remote = SocketAddrV4::new(Ipv4Addr::localhost(), 80);
        {
            let mut mgr = SOCKET_MANAGER.lock();
            let s = mgr.get_socket_mut(sid).unwrap();
            s.direction = SocketDirection::Connected;
            s.local = local;
            s.remote = remote;
        }
        {
            let mut tcp = TCP.lock();
            let c = tcp.get_connection_mut(tcp_id).unwrap();
            c.state = TcpState::Established;
            c.local = local;
            c.remote = remote;
        }
        (sid, tcp_id)
    }

    /// Pretend every unacknowledged byte was ACKed.
    fn drain_send_buffer(tcp_id: u32) {
        let mut tcp = TCP.lock();
        let c = tcp.get_connection_mut(tcp_id).unwrap();
        c.send_buf.clear();
        c.send_base = c.send_seq;
    }

    test_case!("tcp_send_space_tracks_buffer", {
        let id = tcp_alloc_connection().unwrap();
        {
            let mut tcp = TCP.lock();
            tcp.get_connection_mut(id).unwrap().state = TcpState::Established;
        }
        test_eq!(tcp_send_space(id), TCP_SEND_BUF);
        test_eq!(tcp_send(id, &vec![0u8; 100]).unwrap(), 100);
        test_eq!(tcp_send_space(id), TCP_SEND_BUF - 100);
        tcp_free_connection(id);
    });

    test_case!("socket_send_full_buffer_is_again", {
        let (sid, tcp_id) = tcp_socket_established();
        test_eq!(socket_send(sid, &vec![0u8; TCP_SEND_BUF]), Ok(TCP_SEND_BUF));
        test_eq!(socket_send(sid, b"x"), Err(SendError::Full));
        drain_send_buffer(tcp_id);
        test_eq!(socket_send(sid, b"x"), Ok(1));
        SOCKET_MANAGER.lock().free_socket(sid);
        tcp_free_connection(tcp_id);
    });

    test_case!("socket_recv_reports_eof", {
        let (sid, tcp_id) = tcp_socket_established();
        let mut buf = [0u8; 8];
        // Nothing arrived yet and the connection is open: retryable.
        test_true!(socket_recv(sid, &mut buf).is_err());
        // Peer FIN consumed: EOF.
        {
            let mut tcp = TCP.lock();
            tcp.get_connection_mut(tcp_id).unwrap().state = TcpState::CloseWait;
        }
        test_eq!(socket_recv(sid, &mut buf), Ok(0));
        // Application closed and the connection object is reaped: still EOF.
        socket_close(sid);
        tcp_free_connection(tcp_id);
        test_eq!(socket_recv(sid, &mut buf), Ok(0));
        SOCKET_MANAGER.lock().free_socket(sid);
    });

    test_case!("socket_write_wait_reason_tracks_space", {
        let (sid, tcp_id) = tcp_socket_established();
        test_eq!(socket_write_wait_reason(sid), Ok(None));
        test_eq!(socket_send(sid, &vec![0u8; TCP_SEND_BUF]), Ok(TCP_SEND_BUF));
        test_eq!(
            socket_write_wait_reason(sid),
            Ok(Some(crate::kwait::WaitReason::SocketWrite { socket_id: sid }))
        );
        drain_send_buffer(tcp_id);
        test_eq!(socket_write_wait_reason(sid), Ok(None));
        SOCKET_MANAGER.lock().free_socket(sid);
        tcp_free_connection(tcp_id);
    });

    test_case!("socket_poll_writable_requires_space", {
        let (sid, tcp_id) = tcp_socket_established();
        test_true!(socket_poll_writable(sid));
        test_eq!(socket_send(sid, &vec![0u8; TCP_SEND_BUF]), Ok(TCP_SEND_BUF));
        test_true!(!socket_poll_writable(sid));
        drain_send_buffer(tcp_id);
        test_true!(socket_poll_writable(sid));
        SOCKET_MANAGER.lock().free_socket(sid);
        tcp_free_connection(tcp_id);
    });

    test_case!("socket_write_wait_reason_magic_roundtrip", {
        let reason = crate::kwait::WaitReason::SocketWrite { socket_id: 42 };
        test_eq!(
            crate::kwait::WaitReason::decode_magic(reason.encode_magic()),
            Some(reason)
        );
    });
}
