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

/// Scan the PCI capability list for a capability with the given ID (e.g.
/// 0x05 = MSI, 0x11 = MSI-X). Returns the byte offset within the PCI
/// configuration space where the capability header begins, or `None` if the
/// device does not expose the capability or has no capability list at all.
pub fn find_capability(bus: u8, dev: u8, func: u8, cap_id: u8) -> Option<u8> {
    // Bit 4 of the Status register (offset 0x06) indicates that the capability
    // list is present.
    let status = pci_config_read_word(bus, dev, func, 0x06);
    if (status & (1 << 4)) == 0 {
        return None;
    }

    // The first capability pointer is at offset 0x34 (low 8 bits only).
    let mut ptr = (pci_config_read_word(bus, dev, func, 0x34) & 0xFF) as u8;

    let mut guard = 0u8; // prevent infinite loops on malformed lists
    while ptr != 0 && guard < 48 {
        // Each capability header: [7:0] = Capability ID, [15:8] = Next Ptr
        let header = pci_config_read_word(bus, dev, func, ptr);
        let id = (header & 0xFF) as u8;
        if id == cap_id {
            return Some(ptr);
        }
        ptr = ((header >> 8) & 0xFF) as u8;
        guard += 1;
    }
    None
}
