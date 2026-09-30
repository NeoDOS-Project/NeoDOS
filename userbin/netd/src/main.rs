#![no_std]
#![no_main]

//! `netd` — Ring 3 network service (MVP).
//!
//! This binary is the **network service layer** over the kernel networking
//! stack, and is deliberately **not** the RX pump. The always-on, polled RX
//! path stays in the Ring-0 kernel worker `netpump` (`net/mod.rs`,
//! `network_poll_all()`), so RX keeps flowing even when no Ring 3 thread is
//! scheduled (see #362/#372).
//!
//! v1 scope: process/service identity (the kernel Service Manager launches it
//! from `Services\Netd`), a bounded yielding main loop, and network-state
//! monitoring (interface presence + link up/down edges). It never applies NIC
//! configuration — that is the `netapplier` service (#365).
//!
//! Deliberately **out of scope** for v1: DHCP (`dhcpd`), DNS, routing, firewall.

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{mem, syscall};
use libnet::{config, NetIfaceInfo};

struct SbrkAlloc;

unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

/// Spin budget between polls (~tens of ms), mirroring `netapplier`. The loop
/// always yields afterwards, so `netd` never holds a CPU runnable (see #355).
const POLL_SPIN: u32 = 5_000_000;
const MAX_IFACES: u32 = 4;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_dec(mut v: u32) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&buf[i..]);
}

fn write_ip(ip: u32) {
    let mut b = [0u8; 16];
    let n = config::format_ip(ip, &mut b);
    write_str(&b[..n]);
}

fn read_iface(idx: u32) -> Option<NetIfaceInfo> {
    let mut info = NetIfaceInfo {
        nic_id: 0,
        mac: [0u8; 6],
        ip: [0u8; 4],
        link_up: 0,
        vendor_id: 0,
        device_id: 0,
        name: [0u8; 16],
        description: [0u8; 48],
    };
    if libnet::iface_info(idx, &mut info) == 0 { Some(info) } else { None }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n[netd] Ring 3 network service started\r\n");
    write_str(b"[netd] interfaces=");
    write_dec(libnet::iface_count());
    write_str(b"\r\n");

    // Last observed link state per interface (255 = not sampled yet).
    let mut last_link = [u8::MAX; MAX_IFACES as usize];

    loop {
        let n = libnet::iface_count().min(MAX_IFACES);
        for i in 0..n {
            if let Some(info) = read_iface(i) {
                let link = info.link_up;
                if last_link[i as usize] != link {
                    write_str(b"[netd] nic ");
                    write_dec(i);
                    if link != 0 {
                        write_str(b" link=UP ip=");
                    } else {
                        write_str(b" link=DOWN ip=");
                    }
                    write_ip(u32::from_be_bytes(info.ip));
                    write_str(b"\r\n");
                    last_link[i as usize] = link;
                }
            }
        }

        // Yield so the rest of the system (and the Ring-0 netpump) runs.
        for _ in 0..POLL_SPIN {
            core::hint::spin_loop();
        }
        syscall::sys_yield();
    }
}
