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

// No `mut`: each DeviceEvent owns an AtomicBool (NEODOS-07 / #637).
pub static DEVICE_EVENTS: [DeviceEvent; MAX_DEVICES] = [
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
/// NEODOS-07 (#637): registry soundness + no `static mut` regression tests.
pub fn register_registry_tests() {
    use crate::{test_case, test_eq, test_true};
    test_case!("neodos07_driver_registry_and_current_id", {
        use crate::drivers::nem::driver::{set_current_driver, clear_current_driver, current_driver_id};
        use crate::drivers::nem::loader::runtime::{register_inline, call_init, call_event_by_id};
        // Driver-id context is atomic (was `static mut`).
        test_eq!(current_driver_id(), 0);
        unsafe { set_current_driver(42); }
        test_eq!(current_driver_id(), 42);
        unsafe { clear_current_driver(); }
        test_eq!(current_driver_id(), 0);
        // A registry entry with no callbacks: calls are no-ops, never panic, and
        // the registry is lock-protected (was `static mut`).
        register_inline(0x0000_FEED, "test07", None, None, None);
        test_true!(call_init(0x0000_FEED).is_ok());
        test_eq!(call_event_by_id(0x0000_FEED, 1, 0, 0).unwrap(), 0);
        test_true!(call_event_by_id(0xDEAD_BEEF, 1, 0, 0).is_err());
    });
    test_case!("neodos07_mount_manager_consistent", {
        use crate::fs::vfs::mount::MountManager;
        // A fresh mount manager is empty and drive-letter lookup is exact, so the
        // volume registry never reports a mount that is not present.
        let mgr = MountManager::new();
        test_eq!(mgr.count(), 0);
        test_true!(mgr.find_by_letter('C').is_none());
    });
}

pub fn signal_device_event(device_id: u32) {
    if (device_id as usize) < MAX_DEVICES {
        DEVICE_EVENTS[device_id as usize]
            .pending
            .store(true, core::sync::atomic::Ordering::SeqCst);
    }
}
