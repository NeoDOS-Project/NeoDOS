// src/drivers/pci.rs

use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

pub struct IdeController {
    pub bus: u8,
    pub device: u8,
    pub func: u8,
    pub bus_master_base: u16,
    #[allow(dead_code)]
    pub prog_if: u8,
}

pub fn find_ide_controller() -> Option<IdeController> {
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 {
                        break;
                    }
                    continue;
                }

                let class_rev = pci_config_read_dword(bus, dev, func, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;

                if class == 0x01 && subclass == 0x01 {
                    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
                    let bar4 = pci_config_read_dword(bus, dev, func, 0x20);

                    if (prog_if & 0x80) != 0 && (bar4 & 0x01) != 0 {
                        let bmba = (bar4 & 0xFFF0) as u16;
                        if bmba != 0 {
                            return Some(IdeController {
                                bus,
                                device: dev,
                                func,
                                bus_master_base: bmba,
                                prog_if,
                            });
                        }
                    }
                    break;
                }
            }
        }
    }
    None
}

pub fn enable_bus_master(ide: &IdeController) {
    let cmd = pci_config_read_word(ide.bus, ide.device, ide.func, 0x04);
    pci_config_write_word(ide.bus, ide.device, ide.func, 0x04, cmd | 0x04);
}

/// Find the ACPI PM1a_CNT I/O port by scanning PCI for known ACPI controllers.
/// Returns `None` if no known controller is detected.
pub fn find_acpi_pm1_cnt_port() -> Option<u16> {
    for bus in 0..=0 {
        for dev in 0..32 {
            for func in 0..8 {
                let vendor = pci_config_read_word(bus, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    if func == 0 {
                        break;
                    }
                    continue;
                }
                if vendor != 0x8086 {
                    continue;
                }
                let device = pci_config_read_word(bus, dev, func, 2);

                // PIIX4 ACPI: device 0x7113
                if device == 0x7113 {
                    let gpbase = pci_config_read_dword(bus, dev, func, 0x40);
                    if gpbase & 1 != 0 {
                        // PM1a_CNT_BLK = GPBASE + 0x04 (16-byte aligned)
                        return Some(((gpbase & 0xFFF0) as u16) + 0x04);
                    }
                }

                // ICH9 LPC: device 0x2918 (ICH9) or 0x2916 (ICH9M)
                if device == 0x2918 || device == 0x2916 {
                    let abase = pci_config_read_dword(bus, dev, func, 0x40);
                    if abase & 1 != 0 {
                        return Some(((abase & 0xFFFE) as u16) + 0x04);
                    }
                }
            }
        }
    }
    None
}

pub fn pci_config_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);

    unsafe {
        let mut addr_port: Port<u32> = Port::new(CONFIG_ADDRESS);
        let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
        addr_port.write(addr);
        data_port.read()
    }
}

pub fn pci_config_read_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
}

pub fn pci_config_write_dword(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);

    unsafe {
        let mut addr_port: Port<u32> = Port::new(CONFIG_ADDRESS);
        let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
        addr_port.write(addr);
        data_port.write(value);
    }
}

pub fn pci_config_write_word(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);
    pci_config_write_dword(bus, dev, func, aligned, new_dword);
}
