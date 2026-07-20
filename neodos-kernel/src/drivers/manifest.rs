use super::device::{BusType, DeviceClass};
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDeviceId {
    pub vendor: u16,
    pub device: u16,
}

#[derive(Debug, Clone)]
pub struct DriverDescriptor {
    pub name: &'static str,
    pub version_major: u16,
    pub version_minor: u16,
    pub version_patch: u16,
    pub device_class: DeviceClass,
    pub bus_type: BusType,
    pub pci_devices: &'static [PciDeviceId],
    pub priority: u8,
}

impl DriverDescriptor {
    pub const fn new_network(name: &'static str, pci_ids: &'static [PciDeviceId]) -> Self {
        DriverDescriptor {
            name,
            version_major: 1, version_minor: 0, version_patch: 0,
            device_class: DeviceClass::Network,
            bus_type: BusType::Pci,
            pci_devices: pci_ids,
            priority: 10,
        }
    }

    pub const fn new_storage(name: &'static str, pci_ids: &'static [PciDeviceId]) -> Self {
        DriverDescriptor {
            name,
            version_major: 1, version_minor: 0, version_patch: 0,
            device_class: DeviceClass::Storage,
            bus_type: BusType::Pci,
            pci_devices: pci_ids,
            priority: 10,
        }
    }

    pub const fn new_bridge(name: &'static str) -> Self {
        DriverDescriptor {
            name,
            version_major: 1, version_minor: 0, version_patch: 0,
            device_class: DeviceClass::Bridge,
            bus_type: BusType::Platform,
            pci_devices: &[],
            priority: 10,
        }
    }

    pub const fn new_input(name: &'static str) -> Self {
        DriverDescriptor {
            name,
            version_major: 1, version_minor: 0, version_patch: 0,
            device_class: DeviceClass::Input,
            bus_type: BusType::Platform,
            pci_devices: &[],
            priority: 10,
        }
    }

    pub const fn new_system(name: &'static str) -> Self {
        DriverDescriptor {
            name,
            version_major: 1, version_minor: 0, version_patch: 0,
            device_class: DeviceClass::System,
            bus_type: BusType::Platform,
            pci_devices: &[],
            priority: 10,
        }
    }

    pub fn matches_pci(&self, vendor: u16, device: u16) -> bool {
        if self.bus_type != BusType::Pci {
            return false;
        }
        self.pci_devices.iter().any(|id| id.vendor == vendor && id.device == device)
    }
}

const INTEL: u16 = 0x8086;
const VIRTIO: u16 = 0x1AF4;

pub static DRIVER_MANIFEST: &[DriverDescriptor] = &[
    DriverDescriptor::new_network("E1000", &[
        PciDeviceId { vendor: INTEL, device: 0x100E },
        PciDeviceId { vendor: INTEL, device: 0x1004 },
        PciDeviceId { vendor: INTEL, device: 0x100F },
        PciDeviceId { vendor: INTEL, device: 0x10D3 },
    ]),
    DriverDescriptor::new_network("VIRTIO-BLK", &[
        PciDeviceId { vendor: VIRTIO, device: 0x1001 },
        PciDeviceId { vendor: VIRTIO, device: 0x1042 },
    ]),
    DriverDescriptor::new_bridge("ACPI"),
    DriverDescriptor::new_input("PS2KBD"),
    DriverDescriptor::new_input("PS2MOUSE"),
    DriverDescriptor::new_input("SERIAL"),
    DriverDescriptor::new_system("RTC"),
];

pub fn find_descriptor(name: &str) -> Option<&'static DriverDescriptor> {
    let upper = name.to_ascii_uppercase();
    DRIVER_MANIFEST.iter().find(|d| d.name == upper)
}

pub fn find_descriptors_for_device(vendor: u16, device: u16, class: DeviceClass)
    -> Vec<&'static DriverDescriptor>
{
    let mut matches: Vec<&DriverDescriptor> = DRIVER_MANIFEST.iter()
        .filter(|d| {
            d.matches_pci(vendor, device) || (d.bus_type == BusType::Pci && d.pci_devices.is_empty() && d.device_class == class)
        })
        .collect();
    matches.sort_by_key(|d| -(d.priority as i32));
    matches
}

pub fn descriptors_for_class(class: DeviceClass) -> Vec<&'static DriverDescriptor> {
    DRIVER_MANIFEST.iter()
        .filter(|d| d.device_class == class)
        .collect()
}

pub fn all_descriptors() -> &'static [DriverDescriptor] {
    DRIVER_MANIFEST
}
