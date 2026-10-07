#![no_std]
#![no_main]

//! Network Configuration Applier (`netapplier.nxe`).
//!
//! Resident Ring 3 service whose only job is to keep the runtime NIC coherent
//! with the interface configuration stored in the Registry:
//!
//! ```text
//! Registry (\...\Services\Network\Interfaces\0)
//!         │  libnet::config::load()
//!         ▼
//! Network Configuration Applier
//!         │  libnet::config::apply()  →  SetNicIp (27) / SetNicGateway (28)
//!         ▼
//!      NIC runtime
//! ```
//!
//! This is deliberately **not** `netcfg` (the one-shot configuration CLI) and
//! **not** `netd` (the kernel network service / RX pump). There is exactly one
//! authority of application: this service. `dhcpd` only publishes the lease to
//! the Registry; `netcfg`/`neocfg` only write configuration (or apply
//! explicitly for `netcfg /apply`). See issue #365.

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::{mem, syscall};
use libnet::config;

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

/// Spin budget between polls (~tens of ms). Polling is replaced by a
/// configuration-change notification in #364; until then the Registry is the
/// single source of truth and is re-read each iteration.
const POLL_SPIN: u32 = 5_000_000;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_dec_u32(mut v: u32) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut tmp = [0u8; 10];
    let mut i = 10;
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&tmp[i..10]);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n[netapplier] Network Configuration Applier started\r\n");

    // Per-interface last-applied state (multi-NIC). Slots follow the
    // NicInfo enumeration: physical NICs in order, loopback last.
    const SLOTS: usize = 5;
    let mut last_ip = [u32::MAX; SLOTS];
    let mut last_mask = [u32::MAX; SLOTS];
    let mut last_gw = [u32::MAX; SLOTS];
    let mut last_link = [0u8; SLOTS];

    loop {
        let count = libnet::iface_count().min(SLOTS as u32);
        let mut i = 0u32;
        while i < count {
            // Loopback has no Registry key and needs no applier.
            let mut info = libnet::NetIfaceInfo {
                nic_id: 0,
                mac: [0u8; 6],
                ip: [0u8; 4],
                link_up: 0,
                vendor_id: 0,
                device_id: 0,
                name: [0u8; 16],
                description: [0u8; 48],
            };
            if libnet::iface_info(i, &mut info) != 0 || info.nic_id == 0xFFFF_FFFF {
                i += 1;
                continue;
            }
            if let Some(cfg) = config::load(i) {
                let ip = cfg.ip;
                let mask = cfg.effective_mask();
                let gw = cfg.gateway;

                let link = config::link_up(i);
                let slot = i as usize;
                let link_up_edge = link != 0 && last_link[slot] == 0;
                let changed =
                    ip != last_ip[slot] || mask != last_mask[slot] || gw != last_gw[slot];

                if ip != 0 && (changed || link_up_edge) {
                    config::apply(i, &cfg);
                    last_ip[slot] = ip;
                    last_mask[slot] = mask;
                    last_gw[slot] = gw;
                    write_str(b"[netapplier] applied Registry config to NIC ");
                    write_dec_u32(i);
                    write_str(b"\r\n");
                }
                last_link[slot] = link;
            }
            i += 1;
        }
        // Yield so the rest of the system runs between polls.
        for _ in 0..POLL_SPIN { core::hint::spin_loop(); }
        syscall::sys_yield();
    }
}
