//! Minimal ACPI table discovery shared by the HPET and power subsystems.
//!
//! Only firmware-table walking lives here: the RSDP scan, checksum validation
//! and RSDT/XSDT lookup. Table-specific parsers stay in their owning modules.

/// ACPI RSDP (Root System Description Pointer).
#[repr(C, packed)]
pub struct Rsdp {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_addr: u32,
    // ACPI v2.0+ fields (revision >= 2)
    pub length: u32,
    pub xsdt_addr: u64,
    pub ext_checksum: u8,
    pub reserved: [u8; 3],
}

/// Common ACPI system description table header.
#[repr(C, packed)]
pub struct AcpiSdtHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

pub const RSDP_SIGNATURE: [u8; 8] = *b"RSD PTR ";

/// Validate an ACPI table checksum (all bytes must sum to zero).
pub fn checksum(data: &[u8]) -> bool {
    let mut sum: u8 = 0;
    for &byte in data {
        sum = sum.wrapping_add(byte);
    }
    sum == 0
}

/// Validate a potential RSDP pointer.
pub fn validate_rsdp(ptr: *const Rsdp) -> bool {
    unsafe {
        if (*ptr).signature != RSDP_SIGNATURE {
            return false;
        }
        // For ACPI v1, checksum first 20 bytes.
        // For ACPI v2+, checksum first 20 bytes, then full 36 bytes.
        if !checksum(core::slice::from_raw_parts(ptr as *const u8, 20)) {
            return false;
        }
        if (*ptr).revision >= 2 && !checksum(core::slice::from_raw_parts(ptr as *const u8, 36)) {
            return false;
        }
        true
    }
}

/// Scan a memory range for the RSDP (aligned to 16 bytes).
pub fn scan_range_for_rsdp(start: u64, end: u64) -> Option<&'static Rsdp> {
    let mut addr = start;
    while addr + 16 <= end {
        let ptr = addr as *const Rsdp;
        unsafe {
            if (*ptr).signature == RSDP_SIGNATURE
                && checksum(core::slice::from_raw_parts(ptr as *const u8, 20))
            {
                return Some(&*ptr);
            }
        }
        addr += 16;
    }
    None
}

/// Scan for the ACPI RSDP.
///
/// Strategy:
/// 0. Boot-provided RSDP address (from UEFI config table) — most reliable.
/// 1. Legacy BIOS area (0xE0000..0x100000) — works on SeaBIOS / real HW.
/// 2. EBDA (Extended BIOS Data Area at segment 0x40E) — early BIOS area.
/// 3. Option ROM area (0xC0000..0xE0000) — some UEFI puts RSDP here.
/// 4. Low memory (0x80000..0xA0000) — some firmware places RSDP here.
/// 5. First 64 KB (0x0..0x10000) — rare but worth checking.
pub fn find_rsdp(boot_rsdp: u64) -> Option<&'static Rsdp> {
    // 0. Boot-provided RSDP (from UEFI configuration table, most reliable)
    if boot_rsdp != 0 {
        let ptr = boot_rsdp as *const Rsdp;
        if validate_rsdp(ptr) {
            return Some(unsafe { &*ptr });
        }
    }

    // 1. Standard legacy BIOS area
    if let Some(rsdp) = scan_range_for_rsdp(0xE0000, 0x100000) {
        return Some(rsdp);
    }

    // 2. EBDA
    let ebda_seg = unsafe { crate::hal::raw::raw_inw(0x40E) } as u64;
    if ebda_seg > 0 {
        let ebda_addr = ebda_seg << 4;
        if let Some(rsdp) = scan_range_for_rsdp(ebda_addr, ebda_addr + 1024) {
            return Some(rsdp);
        }
    }

    // 3. Option ROM area
    if let Some(rsdp) = scan_range_for_rsdp(0xC0000, 0xE0000) {
        return Some(rsdp);
    }

    // 4. Extended BIOS data area
    if let Some(rsdp) = scan_range_for_rsdp(0x80000, 0xA0000) {
        return Some(rsdp);
    }

    // 5. First 64 KB (BDA area)
    if let Some(rsdp) = scan_range_for_rsdp(0x0, 0x10000) {
        return Some(rsdp);
    }

    None
}

/// Scan RSDT for a table with the given signature.
pub fn find_table_in_rsdt(
    rsdt: &[u32],
    signature: &[u8; 4],
) -> Option<&'static AcpiSdtHeader> {
    for &entry in rsdt {
        let sdt = entry as u64 as *const AcpiSdtHeader;
        unsafe {
            if (*sdt).signature == *signature {
                return Some(&*sdt);
            }
        }
    }
    None
}

/// Scan XSDT for a table with the given signature.
pub fn find_table_in_xsdt(
    xsdt: &[u64],
    signature: &[u8; 4],
) -> Option<&'static AcpiSdtHeader> {
    for &entry in xsdt {
        let sdt = entry as *const AcpiSdtHeader;
        unsafe {
            if (*sdt).signature == *signature {
                return Some(&*sdt);
            }
        }
    }
    None
}
