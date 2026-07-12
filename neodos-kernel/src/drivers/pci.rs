// src/drivers/pci.rs
// Low-level PCI config space access primitives.
// Uses ECAM (MMIO) when available, falls back to legacy port I/O (0xCF8/0xCFC).

#![allow(dead_code)]

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// ECAM-backed read (MMIO) or legacy PIO fallback.
pub fn pci_config_read_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    if crate::hal::pci::ecam_is_active() {
        unsafe { crate::hal::pci::ecam_read_config_dword(bus, dev, func, offset) }
    } else {
        let addr = 0x8000_0000u32
            | ((bus as u32) << 16)
            | ((dev as u32) << 11)
            | ((func as u32) << 8)
            | (offset as u32 & 0xFC);
        crate::hal::outl(CONFIG_ADDRESS, addr);
        crate::hal::inl(CONFIG_DATA)
    }
}

pub fn pci_config_read_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    ((dword >> ((offset & 3) * 8)) & 0xFFFF) as u16
}

pub fn pci_config_read_byte(bus: u8, dev: u8, func: u8, offset: u8) -> u8 {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    ((dword >> ((offset & 3) * 8)) & 0xFF) as u8
}

pub fn pci_config_write_dword(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    if crate::hal::pci::ecam_is_active() {
        unsafe { crate::hal::pci::ecam_write_config_dword(bus, dev, func, offset, value); }
    } else {
        let addr = 0x8000_0000u32
            | ((bus as u32) << 16)
            | ((dev as u32) << 11)
            | ((func as u32) << 8)
            | (offset as u32 & 0xFC);
        crate::hal::outl(CONFIG_ADDRESS, addr);
        crate::hal::outl(CONFIG_DATA, value);
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

pub fn pci_config_write_byte(bus: u8, dev: u8, func: u8, offset: u8, value: u8) {
    let aligned = offset & 0xFC;
    let dword = pci_config_read_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFu32 << shift);
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

/// Initialize PCIe ECAM (Enhanced Configuration Access Mechanism).
///
/// Reads the MCFG ACPI table to get the ECAM base address for segment 0,
/// maps the MMIO region as uncacheable (UC-), and activates ECAM mode.
/// If no MCFG table is found, logs a warning and keeps legacy PIO fallback.
pub fn init_ecam() {
    use crate::timers::hpet::get_ecam_info;

    if let Some((base, segment, start_bus, end_bus)) = get_ecam_info() {
        crate::serial_println!(
            "[PCI] MCFG found: ECAM base=0x{:x}, segment={}, bus {}-{}",
            base, segment, start_bus, end_bus
        );

        // Map ECAM region as UC- (uncacheable) for MMIO config access.
        // ECAM region is typically 256 MB (256 buses × 32 devices × 8 funcs × 4 KB).
        let ecam_size = ((end_bus as u64 - start_bus as u64) + 1) << 20;
        let ecam_size_aligned = (ecam_size + 0x1F_FFFF) & !0x1F_FFFF;
        let size_to_map = core::cmp::min(ecam_size_aligned, 0x1000_0000); // max 256 MB

        unsafe {
            if !map_ecam_region(base, size_to_map) {
                crate::serial_println!("[PCI] WARNING: Failed to map ECAM MMIO region, using legacy PIO");
                return;
            }
        }

        crate::hal::pci::set_ecam_base(base);
        crate::serial_println!(
            "[PCI] ECAM active: 0x{:x} ({} MB mapped, UC-)",
            base, size_to_map / 0x100000
        );
    } else {
        crate::serial_println!("[PCI] No MCFG table found, using legacy PIO (0xCF8/0xCFC)");
    }
}

/// Map the ECAM MMIO region as uncacheable (UC-) in the page tables.
/// The ECAM region must reside within the identity-mapped 4 GiB window.
///
/// # Safety
/// Must be called after custom page tables are active.
unsafe fn map_ecam_region(phys_base: u64, size: u64) -> bool {
    use x86_64::structures::paging::PageTableFlags;

    if phys_base + size > 0x1_0000_0000 {
        // ECAM region extends beyond 4 GiB — need to map via PML4[1]
        crate::serial_println!("[PCI] ECAM above 4 GiB, not yet supported for mapping");
        return false;
    }

    // The region is covered by 2 MB huge pages. We need to split them
    // into 4 KB pages and mark them as uncacheable.
    let start_aligned = phys_base & !0x1F_FFFF;
    let end_aligned = ((phys_base + size + 0x1F_FFFF) & !0x1F_FFFF).min(0x1_0000_0000);

    let mut addr = start_aligned;
    while addr < end_aligned {
        if crate::arch::x64::paging::split_2mb_page(addr).is_err() {
            crate::serial_println!("[PCI] Failed to split 2MB page @ 0x{:x}", addr);
            return false;
        }
        addr += 0x200_000;
    }

    // Now mark each 4 KB page as UC- (NO_CACHE)
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;
    let rc = crate::arch::x64::paging::map_mmio_4k(phys_base, phys_base, size, flags);
    if !rc {
        crate::serial_println!("[PCI] map_mmio_4k failed for ECAM @ 0x{:x}", phys_base);
        return false;
    }

    true
}

/// Read a BAR (Base Address Register) from a PCI device.
/// Returns the raw 32-bit value at the given bar_index (0-5).
pub fn read_bar(bus: u8, dev: u8, func: u8, bar_index: u8) -> u32 {
    let offset = 0x10 + bar_index * 4;
    pci_config_read_dword(bus, dev, func, offset)
}

/// Read a 64-bit BAR (two consecutive 32-bit registers).
pub fn read_bar64(bus: u8, dev: u8, func: u8, bar_index: u8) -> u64 {
    let low = read_bar(bus, dev, func, bar_index) as u64;
    let high = read_bar(bus, dev, func, bar_index + 1) as u64;
    low | (high << 32)
}

/// Map a PCI BAR's MMIO region into the kernel's virtual address space.
/// Returns (virtual_address, size) or None.
/// `bar_raw` is the raw BAR value; this function extracts the address and
/// determines the size by writing all-ones and reading back.
pub fn map_bar_mmio(bus: u8, dev: u8, func: u8, bar_index: u8) -> Option<(u64, u64)> {
    let offset = 0x10 + bar_index * 4;
    let raw = pci_config_read_dword(bus, dev, func, offset);
    if raw == 0 || raw == u32::MAX {
        return None;
    }
    // Check if this is an MMIO BAR (bit 0 = 0) or IO BAR (bit 0 = 1)
    if raw & 1 != 0 {
        // IO BAR — not handled here
        return None;
    }
    // Determine 32-bit or 64-bit (bits 2:1 = 00 → 32-bit, 10 → 64-bit)
    let is_64bit = (raw & 0x6) == 0x4;
    let base_phys = if is_64bit {
        let high = pci_config_read_dword(bus, dev, func, offset + 4) as u64;
        ((raw & 0xFFFF_FFF0) as u64) | (high << 32)
    } else {
        (raw & 0xFFFF_FFF0) as u64
    };

    // Determine size by writing all-ones, reading back, restoring
    pci_config_write_dword(bus, dev, func, offset, 0xFFFF_FFFF);
    let mask_raw = pci_config_read_dword(bus, dev, func, offset);
    pci_config_write_dword(bus, dev, func, offset, raw);
    let size = !(mask_raw & 0xFFFF_FFF0) as u64 + 1;

    Some((base_phys, size))
}

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_true;

    test_case!("pci_bus0_has_qemu_devices", {
        let mut count = 0u16;
        let mut found_vga = false;
        let mut found_ahci = false;
        let mut found_net = false;
        let mut found_isa = false;
        for dev in 0..32 {
            let vendor = pci_config_read_word(0, dev, 0, 0);
            if vendor == 0xFFFF || vendor == 0 {
                continue;
            }
            let header_type = pci_config_read_word(0, dev, 0, 0x0E);
            let is_multi = (header_type & 0x80) != 0;
            let max_func = if is_multi { 8 } else { 1 };
            for func in 0..max_func {
                let vendor = pci_config_read_word(0, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    continue;
                }
                let device = pci_config_read_word(0, dev, func, 2);
                if vendor == 0x1234 && device == 0x1111 { found_vga = true; }
                if vendor == 0x8086 && device == 0x100E { found_net = true; }
                if vendor == 0x8086 && device == 0x10D3 { found_net = true; }
                if vendor == 0x8086 && device == 0x2922 { found_ahci = true; }
                if vendor == 0x8086 && device == 0x2918 { found_isa = true; }
                if vendor == 0x8086 && device == 0x1237 { found_isa = true; }
                if vendor == 0x8086 && device == 0x7000 { found_isa = true; }
                count += 1;
            }
        }
        test_true!(found_vga);
        test_true!(found_ahci);
        test_true!(found_net);
        test_true!(found_isa);
        test_true!(count >= 5);
    });

    test_case!("pci_bus1_empty", {
        let mut found = false;
        for dev in 0..32 {
            let vendor = pci_config_read_word(1, dev, 0, 0);
            if vendor != 0xFFFF && vendor != 0 {
                found = true;
                break;
            }
        }
        test_true!(!found);
    });

    test_case!("pci_algo_no_false_bridges", {
        let mut bridges = 0u16;
        let mut multi_devs = 0u16;
        for dev in 0..32 {
            let vendor = pci_config_read_word(0, dev, 0, 0);
            if vendor == 0xFFFF || vendor == 0 {
                continue;
            }
            let header_type = pci_config_read_word(0, dev, 0, 0x0E);
            let is_multi = (header_type & 0x80) != 0;
            if is_multi { multi_devs += 1; }
            let max_func = if is_multi { 8usize } else { 1usize };
            for func in 0..max_func {
                let vendor = pci_config_read_word(0, dev, func as u8, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    continue;
                }
                let class_rev = pci_config_read_dword(0, dev, func as u8, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;
                if class == 0x06 && subclass == 0x04 {
                    bridges += 1;
                }
            }
        }
        test_true!(bridges == 0);
        test_true!(multi_devs >= 1);
    });
}
