#![no_std]
#![no_main]

//! Network Configuration Applier (`netapplier.nxe`).
//!
//! Resident Ring 3 service whose only job is to keep the runtime NIC coherent
//! with the interface configuration stored in the Registry:
//!
//! ```text
//! Registry (\...\Services\Network\Interfaces\0)
//!         │  read IPAddress / SubnetMask / Gateway
//!         ▼
//! Network Configuration Applier
//!         │  SetNicIp (27) / SetNicGateway (28)  ← via net.nxl
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
use libnet::{self, NetIfaceInfo};

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

const REG_NET_PATH: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Network\\Interfaces\\0";
const DEFAULT_MASK: u32 = 0x00FF_FFFF; // /24, used when SubnetMask is unset (0)
/// Spin budget between polls (~tens of ms). Polling is replaced by a
/// configuration-change notification in #364; until then the Registry is the
/// single source of truth and is re-read each iteration.
const POLL_SPIN: u32 = 5_000_000;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn read_reg_dword(key_fd: u8, name: &str) -> Option<u32> {
    let mut reg_buf = [0u8; 12];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 12 { return None; }
    let value_type = u32::from_le_bytes([reg_buf[0], reg_buf[1], reg_buf[2], reg_buf[3]]);
    if value_type != syscall::REG_DWORD { return None; }
    Some(u32::from_le_bytes([reg_buf[8], reg_buf[9], reg_buf[10], reg_buf[11]]))
}

/// Apply an explicit IP/mask/gateway to the runtime NIC (gateway 0 = unset).
fn apply_ip(ip: u32, mask: u32, gw: u32) {
    libnet::set_ip(0, ip, mask);
    if gw != 0 { libnet::set_gateway(0, gw); }
}

/// NIC link state from the kernel NicInfo (`link_up`), via net.nxl.
fn nic_link_up() -> u8 {
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
    if libnet::iface_info(0, &mut info) == 0 { info.link_up } else { 0 }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n[netapplier] Network Configuration Applier started\r\n");

    let mut last_ip = u32::MAX;
    let mut last_mask = u32::MAX;
    let mut last_gw = u32::MAX;
    let mut last_link = 0u8;

    loop {
        if let Ok(fd) = syscall::sys_cm_open_key(REG_NET_PATH) {
            let ip = read_reg_dword(fd, "IPAddress").unwrap_or(0);
            let mut mask = read_reg_dword(fd, "SubnetMask").unwrap_or(0);
            if mask == 0 { mask = DEFAULT_MASK; }
            let gw = read_reg_dword(fd, "Gateway").unwrap_or(0);
            let _ = syscall::sys_close(fd);

            let link = nic_link_up();
            let link_up_edge = link != 0 && last_link == 0;
            let changed = ip != last_ip || mask != last_mask || gw != last_gw;

            if ip != 0 && (changed || link_up_edge) {
                apply_ip(ip, mask, gw);
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
