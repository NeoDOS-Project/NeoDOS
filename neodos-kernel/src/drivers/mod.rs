pub mod acpi;
pub mod ahci;
pub mod ata;
pub mod block;
pub mod fat32;
pub mod gpt;
pub mod iso9660;
pub mod keyboard;
pub mod pci;
pub mod rtc;
pub mod usb_hid;

use core::sync::atomic::AtomicBool;

pub struct DeviceEvent {
    pub pending: AtomicBool,
    #[allow(dead_code)]
    pub cmd: AtomicBool,
}

impl DeviceEvent {
    pub const fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
            cmd: AtomicBool::new(false),
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
