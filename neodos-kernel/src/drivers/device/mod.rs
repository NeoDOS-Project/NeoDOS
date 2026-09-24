use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    Pci,
    Acpi,
    Virtio,
    Platform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Network,
    Storage,
    Display,
    Input,
    Bridge,
    System,
    Serial,
    Rtc,
    Audio,
    Usb,
    Unknown,
}

impl DeviceClass {
    pub fn from_pci_class(base: u8, sub: u8, _prog_if: u8) -> Self {
        match (base, sub) {
            (0x01, _) => DeviceClass::Storage,
            (0x02, _) => DeviceClass::Network,
            (0x03, _) => DeviceClass::Display,
            (0x04, _) => DeviceClass::Audio,
            (0x06, 0x00) => DeviceClass::Bridge,
            (0x06, 0x01) => DeviceClass::Bridge,
            (0x06, 0x04) => DeviceClass::Bridge,
            (0x0C, 0x03) => DeviceClass::Usb,
            (0x0C, 0x05) => DeviceClass::System,
            _ => DeviceClass::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceLocation {
    Pci { bus: u8, device: u8, function: u8 },
}

impl DeviceLocation {
    pub fn bus_type(&self) -> BusType {
        match self {
            DeviceLocation::Pci { .. } => BusType::Pci,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub bus: BusType,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: DeviceClass,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub location: DeviceLocation,
}

impl DeviceInfo {
    pub fn pci_device(bus: u8, dev: u8, func: u8, vendor: u16, device: u16,
                       class_code: u8, subclass: u8, prog_if: u8, revision: u8) -> Self {
        DeviceInfo {
            bus: BusType::Pci,
            vendor_id: vendor,
            device_id: device,
            class: DeviceClass::from_pci_class(class_code, subclass, prog_if),
            subclass,
            prog_if,
            revision,
            location: DeviceLocation::Pci { bus, device: dev, function: func },
        }
    }

    pub fn pci_location(&self) -> Option<(u8, u8, u8)> {
        match self.location {
            DeviceLocation::Pci { bus, device, function } => Some((bus, device, function)),
        }
    }
}

pub struct DeviceRegistry {
    devices: Vec<DeviceInfo>,
}

impl DeviceRegistry {
    pub const fn new() -> Self {
        DeviceRegistry { devices: Vec::new() }
    }

    pub fn add_device(&mut self, dev: DeviceInfo) {
        self.devices.push(dev);
    }

    pub fn add_devices(&mut self, devs: &[DeviceInfo]) {
        self.devices.extend_from_slice(devs);
    }

    pub fn devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    pub fn count(&self) -> usize {
        self.devices.len()
    }

    pub fn find_by_vendor_device(&self, vendor: u16, device: u16) -> Vec<&DeviceInfo> {
        self.devices.iter()
            .filter(|d| d.vendor_id == vendor && d.device_id == device)
            .collect()
    }

    pub fn find_by_class(&self, class: DeviceClass) -> Vec<&DeviceInfo> {
        self.devices.iter()
            .filter(|d| d.class == class)
            .collect()
    }

    pub fn pci_devices(&self) -> Vec<&DeviceInfo> {
        self.devices.iter()
            .filter(|d| d.bus == BusType::Pci)
            .collect()
    }

    pub fn devices_without_driver(&self, loaded_vendor_devices: &[(u16, u16)]) -> Vec<&DeviceInfo> {
        self.devices.iter()
            .filter(|d| !loaded_vendor_devices.contains(&(d.vendor_id, d.device_id)))
            .collect()
    }
}

pub fn scan_all_devices() -> DeviceRegistry {
    let mut registry = DeviceRegistry::new();
    let pci_devices = pci_scan::scan_pci_bus();
    registry.add_devices(&pci_devices);
    registry
}

mod pci_scan;

