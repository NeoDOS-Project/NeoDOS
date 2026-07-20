use alloc::vec::Vec;
use super::{BusType, DeviceInfo};
use crate::drivers::pci;

pub fn scan_pci_bus() -> Vec<DeviceInfo> {
    let mut devices = Vec::new();

    for bus in 0u8..=0u8 {
        for dev in 0u8..32 {
            let vendor = pci::pci_config_read_word(bus, dev, 0, 0);
            if vendor == 0xFFFF {
                continue;
            }
            let device_id = pci::pci_config_read_word(bus, dev, 0, 2);
            if device_id == 0xFFFF || device_id == 0 {
                continue;
            }

            let class_subclass = pci::pci_config_read_dword(bus, dev, 0, 8);
            let revision = (class_subclass & 0xFF) as u8;
            let prog_if = ((class_subclass >> 8) & 0xFF) as u8;
            let subclass = ((class_subclass >> 16) & 0xFF) as u8;
            let class_code = ((class_subclass >> 24) & 0xFF) as u8;

            let header_type = pci::pci_config_read_byte(bus, dev, 0, 0x0E);
            let is_multifunction = (header_type & 0x80) != 0;

            devices.push(DeviceInfo::pci_device(
                bus, dev, 0,
                vendor, device_id,
                class_code, subclass, prog_if, revision,
            ));

            if is_multifunction {
                for func in 1u8..8 {
                    let vf = pci::pci_config_read_word(bus, dev, func, 0);
                    if vf == 0xFFFF {
                        continue;
                    }
                    let df = pci::pci_config_read_word(bus, dev, func, 2);
                    if df == 0xFFFF || df == 0 {
                        continue;
                    }
                    let cs = pci::pci_config_read_dword(bus, dev, func, 8);
                    let rev = (cs & 0xFF) as u8;
                    let pif = ((cs >> 8) & 0xFF) as u8;
                    let sub = ((cs >> 16) & 0xFF) as u8;
                    let cls = ((cs >> 24) & 0xFF) as u8;

                    devices.push(DeviceInfo::pci_device(
                        bus, dev, func,
                        vf, df,
                        cls, sub, pif, rev,
                    ));
                }
            }
        }
    }

    devices
}

pub fn print_pci_devices(devices: &[DeviceInfo]) {
    crate::serial_println!("[DEVICE] === PCI Device Scan ===");
    for d in devices {
        crate::serial_println!(
            "[DEVICE]   {:02x}:{:02x}.{} vendor=0x{:04x} device=0x{:04x} class={:02x}:{:02x}",
            d.pci_location().map(|(b, _, _)| b).unwrap_or(0),
            d.pci_location().map(|(_, d, _)| d).unwrap_or(0),
            d.pci_location().map(|(_, _, f)| f).unwrap_or(0),
            d.vendor_id,
            d.device_id,
            d.subclass,
            d.prog_if,
        );
    }
    crate::serial_println!("[DEVICE]   {} device(s) found", devices.len());
}
