use core::mem::size_of;
use core::slice;

const RSDP_SIG: &[u8; 8] = b"RSD PTR ";

/// Describes how to reach the ACPI PM1a_CNT register.
pub enum Pm1aCntTarget {
    /// System I/O port address (use `out` instruction).
    IoPort(u16),
    /// MMIO physical address (use volatile memory write).
    Mmio(u64),
}

#[repr(C, packed)]
struct Rsdp {
    sig: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_addr: u32,
    length: u32,
    xsdt_addr: u64,
    ext_checksum: u8,
    _reserved: [u8; 3],
}

#[repr(C, packed)]
struct SdtHeader {
    sig: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

fn checksum_region(ptr: *const u8, len: usize) -> bool {
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

unsafe fn find_rsdp() -> Option<*const Rsdp> {
    for addr in (0x0..0x100000).step_by(16) {
        let ptr = addr as *const u8;
        if ptr.cast::<[u8; 8]>().read_unaligned() == *RSDP_SIG {
            if checksum_region(ptr, size_of::<Rsdp>().min(20)) {
                return Some(ptr.cast::<Rsdp>());
            }
        }
    }
    None
}

unsafe fn find_fadt_in_rsdt(rsdt_addr: u32) -> Option<*const SdtHeader> {
    let header = (rsdt_addr as *const SdtHeader).read_unaligned();
    let entry_count = (header.length as usize - size_of::<SdtHeader>()) / 4;
    let entries = (rsdt_addr as usize + size_of::<SdtHeader>()) as *const u32;
    for i in 0..entry_count {
        let entry_addr = entries.add(i).read_unaligned();
        let sdt = entry_addr as *const SdtHeader;
        if sdt.read_unaligned().sig == *b"FACP" {
            return Some(sdt);
        }
    }
    None
}

unsafe fn find_fadt_in_xsdt(xsdt_addr: u64) -> Option<*const SdtHeader> {
    let header = (xsdt_addr as *const SdtHeader).read_unaligned();
    let entry_count = (header.length as usize - size_of::<SdtHeader>()) / 8;
    let entries = (xsdt_addr as usize + size_of::<SdtHeader>()) as *const u64;
    for i in 0..entry_count {
        let entry_addr = entries.add(i).read_unaligned();
        if entry_addr > 0xFFFFFFFF {
            continue;
        }
        let sdt = (entry_addr as u32) as *const SdtHeader;
        if sdt.read_unaligned().sig == *b"FACP" {
            return Some(sdt);
        }
    }
    None
}

fn read_fadt_field(fadt: *const SdtHeader, offset: usize) -> u32 {
    unsafe { ((fadt as *const u8).add(offset) as *const u32).read_unaligned() }
}

unsafe fn find_fadt() -> Option<*const SdtHeader> {
    let rsdp = find_rsdp()?;
    if (*rsdp).revision >= 2 && (*rsdp).xsdt_addr != 0 {
        find_fadt_in_xsdt((*rsdp).xsdt_addr)
    } else {
        find_fadt_in_rsdt((*rsdp).rsdt_addr)
    }
}

/// Returns the ACPI PM1a_CNT_BLK target (I/O port or MMIO address) by parsing
/// ACPI tables. Scans memory for RSDP -> RSDT/XSDT -> FADT -> PM1a_CNT_BLK.
pub fn find_pm1a_cnt_target() -> Option<Pm1aCntTarget> {
    unsafe {
        let fadt = find_fadt()?;

        // Extract X_PM1a_CNT_BLK (GAS at offset 0xAA, ACPI 2.0+)
        if (*fadt).length >= 0xB6 {
            let gp = (fadt as *const u8).add(0xAA);
            let gas_space = *gp;
            let gas_addr = (gp.add(4) as *const u64).read_unaligned();
            if gas_addr != 0 {
                return match gas_space {
                    1 if gas_addr <= 0xFFFF => Some(Pm1aCntTarget::IoPort(gas_addr as u16)),
                    _ => Some(Pm1aCntTarget::Mmio(gas_addr)),
                };
            }
        }

        // Fallback: legacy PM1a_CNT_BLK (DWORD at offset 0x3E)
        let legacy = read_fadt_field(fadt, 0x3E);
        if legacy != 0 && legacy <= 0xFFFF {
            return Some(Pm1aCntTarget::IoPort(legacy as u16));
        }

        None
    }
}
