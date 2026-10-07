//! Loopback interface (127.0.0.0/8) + local routing (#484).
//!
//! Deliberately kept **outside** `NicRegistry`: registering loopback as a NIC
//! would alter `default_nic_id()` (last registered wins) and consume one of
//! the 4 slots. Instead, frames addressed to 127/8 are queued here and drained
//! through the single dispatch path [`crate::net::net_handle_incoming_packet`].
//!
//! Locking: the queue lock is only held to push/pop one frame. [`loopback_pump`]
//! must be called **without** holding `NIC_REGISTRY` — replies (ARP/ICMP)
//! re-enter through [`LoopbackInterface::send_packet`]. Callers on the send
//! path must also not hold `SOCKET_MANAGER` (same contract as the NIC path:
//! the incoming dispatch locks `SOCKET_MANAGER` via `udp_dispatch`).

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use super::types::{Ipv4Addr, MacAddr};

/// Max queued loopback frames. Sends fail with `Err(())` past this point
/// instead of growing memory unbounded.
pub const LOOPBACK_QUEUE_MAX: usize = 64;

/// Sentinel NIC id used when dispatching loopback frames. It never indexes
/// `NicRegistry`; handlers must not use it to look up a NIC.
pub const LOOPBACK_NIC_ID: u32 = u32::MAX;

lazy_static! {
    static ref LOOPBACK_QUEUE: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());
}

/// Virtual interface used as the `NetworkInterface` for dispatching loopback
/// frames. Replies generated while handling a frame (ICMP echo replies) are
/// re-queued through `send_packet`, so [`loopback_pump`] keeps draining until
/// the queue is empty (bounded by `LOOPBACK_QUEUE_MAX` iterations).
pub struct LoopbackInterface;

impl super::nic::NetworkInterface for LoopbackInterface {
    fn mac_address(&self) -> MacAddr { MacAddr::loopback() }
    fn name(&self) -> &str { "loopback" }
    fn send_packet(&mut self, packet: &[u8]) -> Result<(), ()> {
        loopback_send(packet)
    }
    fn poll_packet(&mut self, buf: &mut [u8]) -> Option<usize> {
        let pkt = LOOPBACK_QUEUE.lock().pop_front()?;
        let len = pkt.len().min(buf.len());
        buf[..len].copy_from_slice(&pkt[..len]);
        Some(len)
    }
    fn set_ip_address(&mut self, _ip: Ipv4Addr) {}
    fn ip_address(&self) -> Ipv4Addr { Ipv4Addr::localhost() }
}

/// Enqueue a complete Ethernet frame for local delivery.
pub fn loopback_send(frame: &[u8]) -> Result<(), ()> {
    let mut q = LOOPBACK_QUEUE.lock();
    if q.len() >= LOOPBACK_QUEUE_MAX {
        return Err(());
    }
    q.push_back(frame.to_vec());
    Ok(())
}

/// Frames currently waiting in the loopback queue (tests/diagnostics).
pub fn loopback_pending() -> usize {
    LOOPBACK_QUEUE.lock().len()
}

/// Drain the queue through `net_handle_incoming_packet`.
///
/// Must be called without holding `NIC_REGISTRY` or `SOCKET_MANAGER`.
/// One call delivers both requests and the replies they generate, because
/// replies are re-queued and the loop continues until the queue is empty.
pub fn loopback_pump() {
    for _ in 0..LOOPBACK_QUEUE_MAX {
        let frame = match LOOPBACK_QUEUE.lock().pop_front() {
            Some(f) => f,
            None => break,
        };
        let mut lo = LoopbackInterface;
        super::net_handle_incoming_packet(LOOPBACK_NIC_ID, &mut lo, &frame);
    }
}

/// NicInfo payload for the loopback interface (#484): `(nic_id, mac, ip,
/// link_up, name, description)`.
///
/// `nic_id` is the `LOOPBACK_NIC_ID` sentinel (`u32::MAX`), which can never
/// address a real `NicRegistry` slot: the mutating paths (`nic_set_ip`, ...)
/// reject it by range check, so exposing loopback in `NicInfo` enumeration is
/// read-only by construction.
pub fn nic_info_entry() -> (u32, [u8; 6], [u8; 4], u8, [u8; 16], [u8; 48]) {
    let mut name = [0u8; 16];
    name[..8].copy_from_slice(b"loopback");
    let mut desc = [0u8; 48];
    desc[..18].copy_from_slice(b"Loopback Interface");
    (
        LOOPBACK_NIC_ID,
        MacAddr::loopback().0,
        Ipv4Addr::localhost().0,
        1,
        name,
        desc,
    )
}

/// Expose loopback in the Ob namespace (`\Device\Loopback`). No new syscalls,
/// no ABI change: sockets keep using `ObType::Socket`.
pub fn init_loopback() {
    crate::object::namespace::ob_create_directory("\\Device\\Loopback").unwrap_or(());
    if let Ok(id) = crate::object::ob_create_object(
        crate::object::ObType::Device, "Loopback", 3, 0, None,
    ) {
        let _ = crate::object::namespace::ob_insert_object("\\Device\\Loopback", id);
    }
}
