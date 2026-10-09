pub mod device;
pub mod hw;
pub mod nem;
pub mod storage;
pub mod virtio;

// ── Historical path re-exports ──────────────────────────────────────
// Keep the pre-reorganization paths (`crate::drivers::<old>`) resolving so
// call-sites outside `drivers/` do not have to change.
pub use hw::{ahci as boot_ahci, ata, nvme, pci, ps2, rtc as rtc_bridge, virtio_blk};
pub use storage::{block, gpt, manager as storage_manager};
pub use nem::management::{abi, boot_loader, caps, dependency, driver_manager, hotreload, isolation, manifest};
pub use nem::runtime as driver_runtime;

use core::sync::atomic::AtomicBool;

pub struct DeviceEvent {
    pub pending: AtomicBool,
}

impl DeviceEvent {
    pub const fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
        }
    }
}

pub const MAX_DEVICES: usize = 8;

pub static mut DEVICE_EVENTS: [DeviceEvent; MAX_DEVICES] = [
    DeviceEvent::new(),
    DeviceEvent::new(),
    DeviceEvent::new(),
    DeviceEvent::new(),
    DeviceEvent::new(),
    DeviceEvent::new(),
    DeviceEvent::new(),
    DeviceEvent::new(),
];

/// Signal that a device has pending data (called from interrupt handlers or other kernel code)
pub fn signal_device_event(device_id: u32) {
    if (device_id as usize) < MAX_DEVICES {
        unsafe {
            DEVICE_EVENTS[device_id as usize].pending.store(true, core::sync::atomic::Ordering::SeqCst);
        }
    }
}
