// src/drivers/pci.rs
// Low-level PCI config space access primitives.
// Higher-level scanning functions moved inline to their callers.

#![allow(dead_code)]

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

pub fn pci_config_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);

    crate::hal::outl(CONFIG_ADDRESS, addr);
    crate::hal::inl(CONFIG_DATA)
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

    crate::hal::outl(CONFIG_ADDRESS, addr);
    crate::hal::outl(CONFIG_DATA, value);
}

pub fn pci_config_write_word(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);
    pci_config_write_dword(bus, dev, func, aligned, new_dword);
}
