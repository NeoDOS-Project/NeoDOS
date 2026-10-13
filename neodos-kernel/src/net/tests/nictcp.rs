//! Real-NIC-path TCP tests.
//!
//! These drive segments through `network_poll_all()` with a *scripted* NIC, so
//! the full Ethernet/IPv4/ARP/TCP dispatch runs with the same locking as a
//! physical NIC. Regression coverage for the RX re-entrancy deadlock: the old
//! `network_poll_all` held `NIC_REGISTRY` across dispatch while the TCP path
//! re-locked it (`nic_next_hop` / `nic_default_id`), spinning forever on the
//! first SYN/SYN-ACK received over a real NIC.

#![allow(unused_imports)]
use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use super::super::nic::{nic_register, nic_unregister, NetworkInterface, NIC_REGISTRY};
use super::super::socket::{
    socket_bind, socket_close, socket_connect, socket_listen, socket_set_tcp_conn, SOCKET_MANAGER,
};
use super::super::tcp::{
    tcp_alloc_connection, tcp_free_connection, tcp_get_state, tcp_recv, tcp_send, tcp_tick,
    TcpHeader, TCP_FLAG_ACK, TCP_FLAG_FIN, TCP_FLAG_PSH, TCP_FLAG_SYN,
};
use super::super::types::{Ipv4Addr, MacAddr, SocketAddrV4, SocketDirection, SocketType, TcpState};

/// A NIC that never touches hardware: `poll_packet` returns queued frames and
/// `send_packet` records them. Lets a test act as the external peer.
struct ScriptedNic {
    mac: MacAddr,
    ip: Ipv4Addr,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
    rx: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl NetworkInterface for ScriptedNic {
    fn mac_address(&self) -> MacAddr {
        self.mac
    }
    fn name(&self) -> &str {
        "scripted-nic"
    }
    fn send_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        self.sent.lock().push(packet.to_vec());
        Ok(())
    }
    fn poll_packet(&mut self, buf: &mut [u8]) -> Option<usize> {
        let mut q = self.rx.lock();
        if q.is_empty() {
            return None;
        }
        let pkt = q.remove(0);
        let len = pkt.len().min(buf.len());
        buf[..len].copy_from_slice(&pkt[..len]);
        Some(len)
    }
    fn set_ip_address(&mut self, ip: Ipv4Addr) {
        self.ip = ip;
    }
    fn ip_address(&self) -> Ipv4Addr {
        self.ip
    }
}

/// Build a complete Ethernet/IPv4/TCP frame (checksums via the kernel builders).
fn tcp_frame(
    src_mac: MacAddr,
    dst_mac: MacAddr,
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
) -> Vec<u8> {
    let segment = super::super::tcp::build_tcp_segment(
        src_ip, dst_ip, src_port, dst_port, seq, ack, flags, window, payload,
    );
    let ip_hdr = super::super::ipv4::build_ipv4_header(
        Ipv4Addr(src_ip),
        Ipv4Addr(dst_ip),
        super::super::ipv4::IPV4_PROTO_TCP,
        segment.len(),
        0,
    );
    let ip_bytes = unsafe {
        core::slice::from_raw_parts(
            &ip_hdr as *const super::super::ipv4::Ipv4Header as *const u8,
            super::super::ipv4::IPV4_HDR_MIN_LEN,
        )
    };
    let mut ip_pkt = Vec::with_capacity(super::super::ipv4::IPV4_HDR_MIN_LEN + segment.len());
    ip_pkt.extend_from_slice(ip_bytes);
    ip_pkt.extend_from_slice(&segment);
    super::super::ethernet::build_ethernet_frame(
        dst_mac,
        src_mac,
        super::super::ethernet::ETH_TYPE_IPV4,
        &ip_pkt,
    )
}

/// Extract (ethertype-excluded) the TCP header pointer from a captured frame.
unsafe fn tcp_of(frame: &[u8]) -> &TcpHeader {
    &*(frame
        .as_ptr()
        .add(super::super::ethernet::ETH_HDR_LEN + super::super::ipv4::IPV4_HDR_MIN_LEN)
        as *const TcpHeader)
}

unsafe fn ip_of(frame: &[u8]) -> &super::super::ipv4::Ipv4Header {
    &*(frame.as_ptr().add(super::super::ethernet::ETH_HDR_LEN)
        as *const super::super::ipv4::Ipv4Header)
}

pub fn register() {
    test_case!("net_tcp_nic_path_syn_synack_no_deadlock", {
        super::super::loopback::loopback_pump(); // clear leftovers

        let peer_mac = MacAddr::new([0x02, 0, 0, 0, 0x20, 0x01]);
        let our_mac = MacAddr::new([0x02, 0, 0, 0, 0x10, 0x01]);
        let our_ip = Ipv4Addr::new([10, 0, 1, 10]);
        let peer_ip = Ipv4Addr::new([10, 0, 1, 20]);
        let listen_port = 8080u16;
        let peer_port = 52000u16;
        let peer_isn = 7000u32;

        let sent = Arc::new(Mutex::new(Vec::new()));
        let rx = Arc::new(Mutex::new(Vec::new()));
        let nic_id = nic_register(alloc::boxed::Box::new(ScriptedNic {
            mac: our_mac,
            ip: our_ip,
            sent: sent.clone(),
            rx: rx.clone(),
        }))
        .expect("register scripted NIC");

        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic_id, our_ip);
            reg.set_mask(nic_id, Ipv4Addr::new([255, 255, 255, 0]));
            reg.set_gateway(nic_id, Ipv4Addr::unspecified());
        }
        super::super::arp::arp_insert(peer_ip, peer_mac);

        // Server: bind + listen.
        let srv_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let srv_listen_tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(srv_sock, srv_listen_tcp);
        test_true!(socket_bind(
            srv_sock,
            SocketAddrV4::new(Ipv4Addr::unspecified(), listen_port)
        ));
        test_true!(socket_listen(srv_sock));
        test_eq!(tcp_get_state(srv_listen_tcp), Some(TcpState::Listen));

        // Listener: the generic wait primitive parks on accept.
        test_true!(matches!(
            super::super::socket::socket_wait_reason(srv_sock),
            Ok(Some(crate::kwait::WaitReason::SocketAccept { .. }))
        ));

        // Peer SYN -> our listener. Queued on the scripted NIC and delivered by
        // network_poll_all(), exactly like a physical RX interrupt/poll.
        rx.lock().push(tcp_frame(
            peer_mac,
            our_mac,
            peer_ip.0,
            our_ip.0,
            peer_port,
            listen_port,
            peer_isn,
            0,
            TCP_FLAG_SYN,
            65535,
            &[],
        ));

        // Would spin forever before the RX lock fix.
        super::super::network_poll_all();

        let frames = sent.lock().clone();
        test_true!(!frames.is_empty());
        let f = &frames[frames.len() - 1];
        let ip = unsafe { ip_of(f) };
        let tcp = unsafe { tcp_of(f) };
        test_eq!(ip.src_ip(), our_ip);
        test_eq!(ip.dst_ip(), peer_ip);
        test_eq!(tcp.src_port(), listen_port);
        test_eq!(tcp.dst_port(), peer_port);
        test_true!(tcp.is_syn());
        test_true!(tcp.is_ack());
        test_eq!(tcp.ack(), peer_isn.wrapping_add(1));
        // The listener stays in Listen; a distinct connection is queued.
        test_eq!(tcp_get_state(srv_listen_tcp), Some(TcpState::Listen));
        let pending_sock = SOCKET_MANAGER
            .lock()
            .get_socket(srv_sock)
            .unwrap()
            .accept_backlog
            .clone();
        test_eq!(pending_sock.len(), 1);
        let pending_tcp = SOCKET_MANAGER
            .lock()
            .get_socket(pending_sock[0])
            .unwrap()
            .tcp_conn_id
            .unwrap();
        test_eq!(tcp_get_state(pending_tcp), Some(TcpState::SynReceived));

        // Final ACK -> Established (on the queued connection, not the listener).
        let srv_isn = tcp.seq();
        rx.lock().push(tcp_frame(
            peer_mac,
            our_mac,
            peer_ip.0,
            our_ip.0,
            peer_port,
            listen_port,
            peer_isn.wrapping_add(1),
            srv_isn.wrapping_add(1),
            TCP_FLAG_ACK,
            65535,
            &[],
        ));
        super::super::network_poll_all();
        let srv_conn = super::super::socket::socket_accept_take(srv_sock).expect("accept");
        let srv_tcp = SOCKET_MANAGER
            .lock()
            .get_socket(srv_conn)
            .unwrap()
            .tcp_conn_id
            .unwrap();
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::Established));

        // Connected with an empty receive buffer: park on read.
        test_true!(matches!(
            super::super::socket::socket_wait_reason(srv_conn),
            Ok(Some(crate::kwait::WaitReason::SocketRead { .. }))
        ));

        // Peer data -> delivered to the server receive buffer + ACKed.
        let data = b"hello-nic";
        rx.lock().push(tcp_frame(
            peer_mac,
            our_mac,
            peer_ip.0,
            our_ip.0,
            peer_port,
            listen_port,
            peer_isn.wrapping_add(1),
            srv_isn.wrapping_add(1),
            TCP_FLAG_PSH | TCP_FLAG_ACK,
            65535,
            data,
        ));
        super::super::network_poll_all();
        let mut buf = [0u8; 32];
        let n = tcp_recv(srv_tcp, &mut buf).unwrap();
        test_eq!(n, data.len());
        test_eq!(&buf[..n], &data[..]);

        // After draining, the socket is idle again; with data buffered it was
        // immediately ready (checked before the recv above).
        test_true!(matches!(
            super::super::socket::socket_wait_reason(srv_conn),
            Ok(Some(crate::kwait::WaitReason::SocketRead { .. }))
        ));

        // Server -> peer data.
        test_eq!(tcp_send(srv_tcp, b"world"), Ok(5));
        tcp_tick();
        let frames = sent.lock().clone();
        let data_frame = frames.iter().rev().find(|f| {
            let t = unsafe { tcp_of(f) };
            t.src_port() == listen_port && t.dst_port() == peer_port && !t.has_flag(TCP_FLAG_SYN)
        });
        test_true!(data_frame.is_some());
        // Locate and verify payload.
        let df = data_frame.unwrap();
        let ip_len = unsafe { ip_of(df) }.total_len();
        let tcp_off = unsafe { tcp_of(df) }.data_offset();
        let payload_off =
            super::super::ethernet::ETH_HDR_LEN + super::super::ipv4::IPV4_HDR_MIN_LEN + tcp_off;
        let payload = &df[payload_off..];
        test_eq!(payload, &b"world"[..]);
        let _ = ip_len;

        // Peer FIN -> server CloseWait, ACKed.
        rx.lock().push(tcp_frame(
            peer_mac,
            our_mac,
            peer_ip.0,
            our_ip.0,
            peer_port,
            listen_port,
            peer_isn.wrapping_add(1 + data.len() as u32),
            srv_isn.wrapping_add(1 + 5),
            TCP_FLAG_FIN | TCP_FLAG_ACK,
            65535,
            &[],
        ));
        super::super::network_poll_all();
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::CloseWait));

        // Server FIN -> LastAck -> Closed on peer ACK.
        super::super::tcp::tcp_close(srv_tcp);
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::LastAck));
        // The FIN consumed the next seq; peer ACKs it.
        let fin_seq = {
            let frames = sent.lock().clone();
            let fin = frames
                .iter()
                .rev()
                .find(|f| {
                    let t = unsafe { tcp_of(f) };
                    t.src_port() == listen_port && t.is_fin()
                })
                .expect("FIN sent");
            unsafe { tcp_of(fin) }.seq().wrapping_add(1)
        };
        rx.lock().push(tcp_frame(
            peer_mac,
            our_mac,
            peer_ip.0,
            our_ip.0,
            peer_port,
            listen_port,
            peer_isn.wrapping_add(1 + data.len() as u32 + 1),
            fin_seq,
            TCP_FLAG_ACK,
            65535,
            &[],
        ));
        super::super::network_poll_all();
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::Closed));

        // Cleanup global state.
        SOCKET_MANAGER.lock().free_socket(srv_conn);
        SOCKET_MANAGER.lock().free_socket(srv_sock);
        tcp_free_connection(srv_tcp);
        tcp_free_connection(srv_listen_tcp);
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        nic_unregister(nic_id);
    });

    test_case!("net_tcp_synack_retransmit_bounded", {
        super::super::loopback::loopback_pump();

        let peer_mac = MacAddr::new([0x02, 0, 0, 0, 0x20, 0x02]);
        let our_mac = MacAddr::new([0x02, 0, 0, 0, 0x10, 0x02]);
        let our_ip = Ipv4Addr::new([10, 0, 2, 10]);
        let peer_ip = Ipv4Addr::new([10, 0, 2, 20]);
        let listen_port = 9090u16;
        let peer_port = 53000u16;

        let sent = Arc::new(Mutex::new(Vec::new()));
        let rx = Arc::new(Mutex::new(Vec::new()));
        let nic_id = nic_register(alloc::boxed::Box::new(ScriptedNic {
            mac: our_mac,
            ip: our_ip,
            sent: sent.clone(),
            rx: rx.clone(),
        }))
        .expect("register scripted NIC");
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic_id, our_ip);
            reg.set_mask(nic_id, Ipv4Addr::new([255, 255, 255, 0]));
        }
        super::super::arp::arp_insert(peer_ip, peer_mac);

        let srv_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let srv_listen_tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(srv_sock, srv_listen_tcp);
        test_true!(socket_bind(
            srv_sock,
            SocketAddrV4::new(Ipv4Addr::unspecified(), listen_port)
        ));
        test_true!(socket_listen(srv_sock));

        rx.lock().push(tcp_frame(
            peer_mac,
            our_mac,
            peer_ip.0,
            our_ip.0,
            peer_port,
            listen_port,
            1234,
            0,
            TCP_FLAG_SYN,
            65535,
            &[],
        ));
        super::super::network_poll_all();
        // Listener stays listening; the pending SYNs connection is queued.
        test_eq!(tcp_get_state(srv_listen_tcp), Some(TcpState::Listen));
        let pending_sock = SOCKET_MANAGER
            .lock()
            .get_socket(srv_sock)
            .unwrap()
            .accept_backlog
            .clone()[0];
        let srv_tcp = SOCKET_MANAGER
            .lock()
            .get_socket(pending_sock)
            .unwrap()
            .tcp_conn_id
            .unwrap();
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::SynReceived));

        let synack_count = |sent: &Arc<Mutex<Vec<Vec<u8>>>>| -> usize {
            sent.lock()
                .iter()
                .filter(|f| {
                    let t = unsafe { tcp_of(f) };
                    t.src_port() == listen_port
                        && t.dst_port() == peer_port
                        && t.is_syn()
                        && t.is_ack()
                })
                .count()
        };
        test_eq!(synack_count(&sent), 1);

        // Force RTO expiry (backdate the transmit timestamp) and retransmit.
        {
            let mut tcp = super::super::tcp::TCP.lock();
            let c = tcp.get_connection_mut(srv_tcp).unwrap();
            c.last_tx_tsc = 0;
            c.retries = 0;
        }
        tcp_tick();
        test_eq!(synack_count(&sent), 2);
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::SynReceived));

        // Bounded retry: at the cap the connection is torn down (never hangs).
        {
            let mut tcp = super::super::tcp::TCP.lock();
            let c = tcp.get_connection_mut(srv_tcp).unwrap();
            c.last_tx_tsc = 0;
            c.retries = super::super::tcp::TCP_MAX_RETRIES;
        }
        tcp_tick();
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::Closed));

        SOCKET_MANAGER.lock().free_socket(srv_sock);
        tcp_free_connection(srv_tcp);
        tcp_free_connection(srv_listen_tcp);
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        nic_unregister(nic_id);
    });

    test_case!("net_tcp_nic_bad_checksum_dropped", {
        // A segment whose TCP checksum does not verify must not move state or
        // elicit a reply.
        super::super::loopback::loopback_pump();

        let peer_mac = MacAddr::new([0x02, 0, 0, 0, 0x20, 0x03]);
        let our_mac = MacAddr::new([0x02, 0, 0, 0, 0x10, 0x03]);
        let our_ip = Ipv4Addr::new([10, 0, 3, 10]);
        let peer_ip = Ipv4Addr::new([10, 0, 3, 20]);

        let sent = Arc::new(Mutex::new(Vec::new()));
        let rx = Arc::new(Mutex::new(Vec::new()));
        let nic_id = nic_register(alloc::boxed::Box::new(ScriptedNic {
            mac: our_mac,
            ip: our_ip,
            sent: sent.clone(),
            rx: rx.clone(),
        }))
        .expect("register scripted NIC");
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic_id, our_ip);
            reg.set_mask(nic_id, Ipv4Addr::new([255, 255, 255, 0]));
        }
        super::super::arp::arp_insert(peer_ip, peer_mac);

        let srv_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let srv_tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(srv_sock, srv_tcp);
        test_true!(socket_bind(
            srv_sock,
            SocketAddrV4::new(Ipv4Addr::unspecified(), 7070)
        ));
        test_true!(socket_listen(srv_sock));

        let mut frame = tcp_frame(
            peer_mac,
            our_mac,
            peer_ip.0,
            our_ip.0,
            54000,
            7070,
            42,
            0,
            TCP_FLAG_SYN,
            65535,
            &[],
        );
        // TCP checksum field: ETH(14)+IP(20)+16.
        frame[super::super::ethernet::ETH_HDR_LEN + super::super::ipv4::IPV4_HDR_MIN_LEN + 16] ^=
            0xFF;
        rx.lock().push(frame);

        super::super::network_poll_all();
        test_eq!(tcp_get_state(srv_tcp), Some(TcpState::Listen));
        test_true!(sent.lock().is_empty());

        SOCKET_MANAGER.lock().free_socket(srv_sock);
        tcp_free_connection(srv_tcp);
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        nic_unregister(nic_id);
    });

    test_case!("net_tcp_close_flushes_buffered_data", {
        // An application that writes and immediately closes must not lose the
        // buffered payload: the connection drains before it is released.
        super::super::loopback::loopback_pump();

        let peer_mac = MacAddr::new([0x02, 0, 0, 0, 0x20, 0x04]);
        let our_mac = MacAddr::new([0x02, 0, 0, 0, 0x10, 0x04]);
        let our_ip = Ipv4Addr::new([10, 0, 4, 10]);
        let peer_ip = Ipv4Addr::new([10, 0, 4, 20]);
        let local_port = 40000u16;
        let remote_port = 8080u16;

        let sent = Arc::new(Mutex::new(Vec::new()));
        let rx = Arc::new(Mutex::new(Vec::new()));
        let nic_id = nic_register(alloc::boxed::Box::new(ScriptedNic {
            mac: our_mac,
            ip: our_ip,
            sent: sent.clone(),
            rx: rx.clone(),
        }))
        .expect("register scripted NIC");
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic_id, our_ip);
            reg.set_mask(nic_id, Ipv4Addr::new([255, 255, 255, 0]));
        }
        super::super::arp::arp_insert(peer_ip, peer_mac);

        let sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let tcp_id = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(sock, tcp_id);
        {
            let mut tbl = super::super::tcp::TCP.lock();
            let c = tbl.get_connection_mut(tcp_id).unwrap();
            c.state = TcpState::Established;
            c.local = SocketAddrV4::new(our_ip, local_port);
            c.remote = SocketAddrV4::new(peer_ip, remote_port);
            c.send_base = 100;
            c.send_seq = 100;
            c.recv_seq = 500;
        }
        SOCKET_MANAGER
            .lock()
            .get_socket_mut(sock)
            .unwrap()
            .direction = SocketDirection::Connected;

        test_eq!(tcp_send(tcp_id, b"bye"), Ok(3));
        // Close + destroy the socket immediately, then drive the flush.
        socket_close(sock);
        SOCKET_MANAGER.lock().free_socket(sock);
        tcp_tick();

        let frames = sent.lock().clone();
        let mut has_payload = false;
        for f in &frames {
            let t = unsafe { tcp_of(f) };
            if t.src_port() != local_port || t.dst_port() != remote_port {
                continue;
            }
            let off = super::super::ethernet::ETH_HDR_LEN
                + super::super::ipv4::IPV4_HDR_MIN_LEN
                + t.data_offset();
            if off <= f.len() && &f[off..] == b"bye" {
                has_payload = true;
            }
        }
        test_true!(has_payload);

        tcp_free_connection(tcp_id);
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        nic_unregister(nic_id);
    });

    test_case!("net_tcp_accept_multiple_sequential", {
        // A listener must stay usable across several connections and never
        // deliver one client's data to another connection.
        super::super::loopback::loopback_pump();

        let peer_mac = MacAddr::new([0x02, 0, 0, 0, 0x20, 0x05]);
        let our_mac = MacAddr::new([0x02, 0, 0, 0, 0x10, 0x05]);
        let our_ip = Ipv4Addr::new([10, 0, 5, 10]);
        let peer_ip = Ipv4Addr::new([10, 0, 5, 20]);
        let listen_port = 6060u16;

        let sent = Arc::new(Mutex::new(Vec::new()));
        let rx = Arc::new(Mutex::new(Vec::new()));
        let nic_id = nic_register(alloc::boxed::Box::new(ScriptedNic {
            mac: our_mac,
            ip: our_ip,
            sent: sent.clone(),
            rx: rx.clone(),
        }))
        .expect("register scripted NIC");
        {
            let mut reg = NIC_REGISTRY.lock();
            reg.set_ip(nic_id, our_ip);
            reg.set_mask(nic_id, Ipv4Addr::new([255, 255, 255, 0]));
        }
        super::super::arp::arp_insert(peer_ip, peer_mac);

        let srv_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let srv_listen_tcp = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(srv_sock, srv_listen_tcp);
        test_true!(socket_bind(srv_sock, SocketAddrV4::new(Ipv4Addr::unspecified(), listen_port)));
        test_true!(socket_listen(srv_sock));

        let peers: [(u16, &[u8]); 2] = [(60001, b"AAA"), (60002, b"BBB")];
        let mut accepted: alloc::vec::Vec<(u32, u32)> = alloc::vec::Vec::new();
        for (i, (pport, payload)) in peers.iter().enumerate() {
            let peer_isn = 1000 + (i as u32) * 100;
            rx.lock().push(tcp_frame(
                peer_mac, our_mac, peer_ip.0, our_ip.0, *pport, listen_port,
                peer_isn, 0, TCP_FLAG_SYN, 65535, &[],
            ));
            super::super::network_poll_all();

            let pending = SOCKET_MANAGER.lock().get_socket(srv_sock).unwrap().accept_backlog.clone();
            test_eq!(pending.len(), 1);
            let synack_seq = {
                let frames = sent.lock().clone();
                let f = frames.iter().rev().find(|f| {
                    let t = unsafe { tcp_of(f) };
                    t.dst_port() == *pport && t.is_syn()
                }).unwrap();
                unsafe { tcp_of(f) }.seq()
            };

            rx.lock().push(tcp_frame(
                peer_mac, our_mac, peer_ip.0, our_ip.0, *pport, listen_port,
                peer_isn.wrapping_add(1), synack_seq.wrapping_add(1),
                TCP_FLAG_ACK, 65535, &[],
            ));
            super::super::network_poll_all();

            let conn = super::super::socket::socket_accept_take(srv_sock).expect("accept");
            let ctcp = SOCKET_MANAGER.lock().get_socket(conn).unwrap().tcp_conn_id.unwrap();
            test_eq!(tcp_get_state(ctcp), Some(TcpState::Established));
            // Each accepted connection has the correct peer port.
            test_eq!(
                SOCKET_MANAGER.lock().get_socket(conn).unwrap().remote.port,
                *pport
            );

            rx.lock().push(tcp_frame(
                peer_mac, our_mac, peer_ip.0, our_ip.0, *pport, listen_port,
                peer_isn.wrapping_add(1), synack_seq.wrapping_add(1),
                TCP_FLAG_PSH | TCP_FLAG_ACK, 65535, payload,
            ));
            super::super::network_poll_all();
            let mut buf = [0u8; 8];
            let n = tcp_recv(ctcp, &mut buf).unwrap();
            test_eq!(&buf[..n], *payload);

            // Response must be addressed to this peer's port only.
            test_eq!(tcp_send(ctcp, payload), Ok(payload.len()));
            tcp_tick();
            let frames = sent.lock().clone();
            let found = frames.iter().any(|f| {
                let t = unsafe { tcp_of(f) };
                if t.src_port() != listen_port || t.dst_port() != *pport || t.is_syn() {
                    return false;
                }
                let off = super::super::ethernet::ETH_HDR_LEN
                    + super::super::ipv4::IPV4_HDR_MIN_LEN
                    + t.data_offset();
                off <= f.len() && &f[off..] == *payload
            });
            test_true!(found);
            accepted.push((conn, ctcp));
        }

        // The listener survived both connections.
        test_eq!(tcp_get_state(srv_listen_tcp), Some(TcpState::Listen));
        test_eq!(
            SOCKET_MANAGER.lock().get_socket(srv_sock).unwrap().direction,
            SocketDirection::Listening
        );

        for (conn, ctcp) in accepted {
            SOCKET_MANAGER.lock().free_socket(conn);
            tcp_free_connection(ctcp);
        }
        SOCKET_MANAGER.lock().free_socket(srv_sock);
        tcp_free_connection(srv_listen_tcp);
        super::super::arp::ARP_CACHE.lock().remove(peer_ip);
        nic_unregister(nic_id);
    });

    test_case!("net_tcp_ack_wraparound", {
        // An ACK that crosses the u32 sequence wrap must still advance the
        // window. Regression: the plain `their_ack > send_base` test ignored it.
        let sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let tcp_id = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(sock, tcp_id);
        {
            let mut tbl = super::super::tcp::TCP.lock();
            let c = tbl.get_connection_mut(tcp_id).unwrap();
            c.state = TcpState::Established;
            c.local = SocketAddrV4::new(Ipv4Addr::localhost(), 40000);
            c.remote = SocketAddrV4::new(Ipv4Addr::localhost(), 50000);
            c.send_base = u32::MAX - 3;
            c.send_seq = c.send_base.wrapping_add(14);
            c.recv_seq = 500;
            c.window = 65535;
            c.send_buf.clear();
            for b in 0..14u8 {
                c.send_buf.push_back(b);
            }
        }
        SOCKET_MANAGER.lock().get_socket_mut(sock).unwrap().direction = SocketDirection::Connected;
        let idx = SOCKET_MANAGER
            .lock()
            .sockets
            .iter()
            .position(|s| s.as_ref().map(|x| x.id) == Some(sock))
            .unwrap();
        let ack = (u32::MAX - 3).wrapping_add(6); // wraps to 2
        super::super::tcp::tcp_handle_ack(idx, 500, ack, 65535);
        let c = super::super::tcp::tcp_get_connection(tcp_id).unwrap();
        test_eq!(c.send_base, ack);
        test_eq!(c.send_buf.len(), 8);

        SOCKET_MANAGER.lock().free_socket(sock);
        tcp_free_connection(tcp_id);
    });

    test_case!("net_tcp_duplicate_and_out_of_order", {
        // Duplicate and gap segments must not be delivered twice or out of
        // order; the in-order next byte is still accepted.
        let tcp_id = tcp_alloc_connection().unwrap();
        {
            let mut tbl = super::super::tcp::TCP.lock();
            let c = tbl.get_connection_mut(tcp_id).unwrap();
            c.state = TcpState::Established;
            c.local = SocketAddrV4::new(Ipv4Addr::localhost(), 40000);
            c.remote = SocketAddrV4::new(Ipv4Addr::localhost(), 50000);
            c.recv_seq = 100;
            c.send_base = 10;
            c.send_seq = 10;
        }
        let loop_ip = Ipv4Addr::localhost();
        test_true!(super::super::tcp::tcp_handle_data(
            tcp_id, 100, b"AAAA", loop_ip, loop_ip, 50000, 40000
        ));
        let mut buf = [0u8; 16];
        let n = tcp_recv(tcp_id, &mut buf).unwrap();
        test_eq!(&buf[..n], b"AAAA");
        // Duplicate (old seq) is rejected and not delivered again.
        test_true!(!super::super::tcp::tcp_handle_data(
            tcp_id, 100, b"AAAA", loop_ip, loop_ip, 50000, 40000
        ));
        test_true!(tcp_recv(tcp_id, &mut buf).is_err());
        // A gap is dropped (no out-of-order reassembly).
        test_true!(!super::super::tcp::tcp_handle_data(
            tcp_id, 200, b"XXXX", loop_ip, loop_ip, 50000, 40000
        ));
        test_true!(tcp_recv(tcp_id, &mut buf).is_err());
        // The in-order continuation is accepted.
        test_true!(super::super::tcp::tcp_handle_data(
            tcp_id, 104, b"BB", loop_ip, loop_ip, 50000, 40000
        ));
        let n = tcp_recv(tcp_id, &mut buf).unwrap();
        test_eq!(&buf[..n], b"BB");
        tcp_free_connection(tcp_id);
    });

    test_case!("net_tcp_loopback_large_transfer", {
        // A payload spanning several segments must transfer intact.
        use super::super::loopback::loopback_pump;
        let srv_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let srv_listen = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(srv_sock, srv_listen);
        socket_bind(srv_sock, SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 50121));
        socket_listen(srv_sock);

        let cli_sock = SOCKET_MANAGER.lock().alloc_socket(SocketType::Tcp).unwrap();
        let cli = tcp_alloc_connection().unwrap();
        socket_set_tcp_conn(cli_sock, cli);
        socket_bind(cli_sock, SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 0));
        socket_connect(cli_sock, SocketAddrV4::new(Ipv4Addr::new([127, 0, 0, 1]), 50121));

        let mut srv_conn = 0u32;
        for _ in 0..4000 {
            tcp_tick();
            loopback_pump();
            if super::super::socket::listener_has_pending(srv_sock) {
                srv_conn = super::super::socket::socket_accept_take(srv_sock).unwrap();
                break;
            }
        }
        test_true!(srv_conn != 0);
        let srv_tcp = SOCKET_MANAGER
            .lock()
            .get_socket(srv_conn)
            .unwrap()
            .tcp_conn_id
            .unwrap();

        let total = 5000usize;
        let mut payload = alloc::vec::Vec::with_capacity(total);
        for i in 0..total {
            payload.push((i % 251) as u8);
        }
        let mut off = 0usize;
        let mut received: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(total);
        for _ in 0..40000 {
            if off < payload.len() {
                if let Ok(n) = tcp_send(cli, &payload[off..]) {
                    off += n;
                }
            }
            tcp_tick();
            loopback_pump();
            let mut buf = [0u8; 1024];
            while let Ok(n) = tcp_recv(srv_tcp, &mut buf) {
                received.extend_from_slice(&buf[..n]);
            }
            if off >= payload.len() && received.len() >= payload.len() {
                break;
            }
        }
        test_eq!(received.len(), payload.len());
        test_true!(received == payload);

        SOCKET_MANAGER.lock().free_socket(cli_sock);
        SOCKET_MANAGER.lock().free_socket(srv_conn);
        SOCKET_MANAGER.lock().free_socket(srv_sock);
        tcp_free_connection(cli);
        tcp_free_connection(srv_tcp);
        tcp_free_connection(srv_listen);
    });
}
