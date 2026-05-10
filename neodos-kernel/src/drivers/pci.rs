// src/drivers/pci.rs

use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

pub struct IdeController {
    pub bus: u8,
    pub device: u8,
    pub func: u8,
    pub bus_master_base: u16,
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

fn pci_config_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
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

fn pci_config_read_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
}

fn pci_config_write_dword(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
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

fn pci_config_write_word(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);
    pci_config_write_dword(bus, dev, func, aligned, new_dword);
}
