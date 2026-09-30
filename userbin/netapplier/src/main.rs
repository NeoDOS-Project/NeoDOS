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

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n[netapplier] Network Configuration Applier started\r\n");

    let mut last_ip = u32::MAX;
    let mut last_mask = u32::MAX;
    let mut last_gw = u32::MAX;
    let mut last_link = 0u8;

    loop {
        if let Some(cfg) = config::load(0) {
            let ip = cfg.ip;
            let mask = cfg.effective_mask();
            let gw = cfg.gateway;

            let link = config::link_up(0);
            let link_up_edge = link != 0 && last_link == 0;
            let changed = ip != last_ip || mask != last_mask || gw != last_gw;

            if ip != 0 && (changed || link_up_edge) {
                config::apply(0, &cfg);
                last_ip = ip;
                last_mask = mask;
                last_gw = gw;
                write_str(b"[netapplier] applied Registry config to NIC\r\n");
            }
            last_link = link;
        }
        // Yield so the rest of the system runs between polls.
        for _ in 0..POLL_SPIN { core::hint::spin_loop(); }
        syscall::sys_yield();
    }
}
