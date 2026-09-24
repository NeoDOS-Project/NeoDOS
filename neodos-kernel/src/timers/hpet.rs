use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use crate::log::LogSubsys;

// ── ACPI table signatures ──────────────────────────────────────────
const RSDP_SIGNATURE: [u8; 8] = *b"RSD PTR ";
const HPET_SIGNATURE: [u8; 4] = *b"HPET";
const RSDT_SIGNATURE: [u8; 4] = *b"RSDT";
const XSDT_SIGNATURE: [u8; 4] = *b"XSDT";
const MCFG_SIGNATURE: [u8; 4] = *b"MCFG";
const APIC_SIGNATURE: [u8; 4] = *b"APIC";

// ── HPET MMIO register offsets ─────────────────────────────────────
const HPET_GEN_CAP_ID: u64       = 0x000;
const HPET_GEN_CONFIG: u64       = 0x010;
const HPET_GEN_INT_STATUS: u64   = 0x020;
const HPET_MAIN_COUNTER: u64     = 0x0F0;
const HPET_TIMER0_CFG: u64       = 0x100;
const HPET_TIMER0_COMPARATOR: u64 = 0x108;

// ── HPET configuration bits ────────────────────────────────────────
const HPET_CFG_ENABLE: u64       = 1 << 0;
const HPET_CFG_LEGACY: u64       = 1 << 1;

const HPET_TIMER_TYPE: u64       = 1 << 1;   // 0=edge, 1=level
const HPET_TIMER_INT_ENABLE: u64 = 1 << 2;
const HPET_TIMER_PERIODIC: u64   = 1 << 3;
const HPET_TIMER_SET_VAL: u64    = 1 << 6;   // force timer to use comparator value

// ── ACPI RSDP ──────────────────────────────────────────────────────
#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_addr: u32,
    // ACPI v2.0+ fields (revision >= 2)
    length: u32,
    xsdt_addr: u64,
    ext_checksum: u8,
    reserved: [u8; 3],
}

// ── ACPI SDT header ────────────────────────────────────────────────
#[repr(C, packed)]
struct AcpiSdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

// ── ACPI HPET table ────────────────────────────────────────────────
#[repr(C, packed)]
struct AcpiHpet {
    header: AcpiSdtHeader,
    hardware_rev_id: u32,
    base_addr: GenericAddr,
    hpet_number: u8,
    min_period: u16,
    page_protect: u8,
}

#[repr(C, packed)]
struct GenericAddr {
    address_space_id: u8,
    register_bit_width: u8,
    register_bit_offset: u8,
    access_size: u8,
    address: u64,
}

// ── MMIO access helpers ────────────────────────────────────────────

#[inline]
unsafe fn hpet_read(base: u64, offset: u64) -> u64 {
    let ptr = (base + offset) as *const u64;
    fence(Ordering::SeqCst);
    read_volatile(ptr)
}

#[inline]
unsafe fn hpet_write(base: u64, offset: u64, val: u64) {
    let ptr = (base + offset) as *mut u64;
    fence(Ordering::SeqCst);
    write_volatile(ptr, val);
}

// ── ACPI table scanning ────────────────────────────────────────────

/// Validate ACPI table checksum.
fn acpi_checksum(data: &[u8]) -> bool {
    let mut sum: u8 = 0;
    for &byte in data {
        sum = sum.wrapping_add(byte);
    }
    sum == 0
}

/// Scan a memory range for the RSDP (aligned to 16 bytes).
fn scan_range_for_rsdp(start: u64, end: u64) -> Option<&'static Rsdp> {
    let mut addr = start;
    while addr + 16 <= end {
        let ptr = addr as *const Rsdp;
        unsafe {
            if (*ptr).signature == RSDP_SIGNATURE
                && acpi_checksum(core::slice::from_raw_parts(ptr as *const u8, 20)) {
                return Some(&*ptr);
            }
        }
        addr += 16;
    }
    None
}

/// Validate a potential RSDP pointer.
fn validate_rsdp(ptr: *const Rsdp) -> bool {
    unsafe {
        if (*ptr).signature != RSDP_SIGNATURE {
            return false;
        }
        // For ACPI v1, checksum first 20 bytes.
        // For ACPI v2+, checksum first 20 bytes, then full 36 bytes.
        if !acpi_checksum(core::slice::from_raw_parts(ptr as *const u8, 20)) {
            return false;
        }
        if (*ptr).revision >= 2
            && !acpi_checksum(core::slice::from_raw_parts(ptr as *const u8, 36)) {
            return false;
        }
        true
    }
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
fn find_rsdp() -> Option<&'static Rsdp> {
    // 0. Boot-provided RSDP (from UEFI configuration table, most reliable)
    let boot_rsdp = unsafe { crate::timers::BOOT_RSDP_ADDR };
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
    let ebda_seg = crate::hal::inw(0x40E) as u64;
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
fn find_table_in_rsdt(rsdt: &'static [u32], signature: &[u8; 4]) -> Option<&'static AcpiSdtHeader> {
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
fn find_table_in_xsdt(xsdt: &'static [u64], signature: &[u8; 4]) -> Option<&'static AcpiSdtHeader> {
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

// ── ACPI MCFG (PCI Express MMIO config space) table ────────────────

#[repr(C, packed)]
struct AcpiMcfg {
    header: AcpiSdtHeader,
    reserved: [u8; 8],
    // Followed by one or more McfgEntry
}

#[repr(C, packed)]
struct McfgEntry {
    base_addr: u64,
    segment_group: u16,
    start_bus: u8,
    end_bus: u8,
    reserved: [u8; 4],
}

/// Find the MCFG table by scanning RSDP → RSDT/XSDT.
fn find_mcfg_table() -> Option<&'static AcpiMcfg> {
    let rsdp = find_rsdp()?;
    let sdt: Option<&'static AcpiSdtHeader>;

    if rsdp.revision >= 2 && rsdp.xsdt_addr != 0 {
        let xsdt_ptr = rsdp.xsdt_addr as *const AcpiSdtHeader;
        unsafe {
            let xsdt = &*xsdt_ptr;
            if xsdt.signature != XSDT_SIGNATURE { return None; }
            let entry_count = (xsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 8;
            let entries = core::slice::from_raw_parts(
                (rsdp.xsdt_addr + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u64,
                entry_count,
            );
            sdt = find_table_in_xsdt(entries, &MCFG_SIGNATURE);
        }
    } else {
        let rsdt_ptr = rsdp.rsdt_addr as u64 as *const AcpiSdtHeader;
        unsafe {
            let rsdt = &*rsdt_ptr;
            if rsdt.signature != RSDT_SIGNATURE { return None; }
            let entry_count = (rsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 4;
            let entries = core::slice::from_raw_parts(
                (rsdp.rsdt_addr as u64 + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u32,
                entry_count,
            );
            sdt = find_table_in_rsdt(entries, &MCFG_SIGNATURE);
        }
    }

    unsafe {
        let header = sdt?;
        Some(&*(header as *const AcpiSdtHeader as *const AcpiMcfg))
    }
}

/// Extract the first ECAM base address from the MCFG table.
/// Returns (base_addr, segment_group, start_bus, end_bus) or None.
pub fn get_ecam_info() -> Option<(u64, u16, u8, u8)> {
    let mcfg = find_mcfg_table()?;
    let header_size = core::mem::size_of::<AcpiSdtHeader>() as u32;
    let mcfg_fixed = core::mem::size_of::<AcpiMcfg>() as u32;
    let entry_size = core::mem::size_of::<McfgEntry>() as u32;
    let data_avail = mcfg.header.length.saturating_sub(header_size);
    let entries_bytes = data_avail.saturating_sub(mcfg_fixed - header_size);
    if entries_bytes < entry_size {
        return None;
    }
    unsafe {
        let entry_ptr = (mcfg as *const AcpiMcfg as u64 + mcfg_fixed as u64) as *const McfgEntry;
        let entry = &*entry_ptr;
        if entry.base_addr == 0 {
            return None;
        }
        Some((entry.base_addr, entry.segment_group, entry.start_bus, entry.end_bus))
    }
}

// ── ACPI MADT (Multiple APIC Description Table) ────────────────────

#[repr(C, packed)]
struct AcpiMadt {
    header: AcpiSdtHeader,
    local_apic_addr: u32,
    flags: u32,
    // Followed by interrupt controller structures (MADT entries)
}

#[repr(C, packed)]
struct MadtEntryHeader {
    entry_type: u8,
    record_length: u8,
}

#[repr(C, packed)]
struct MadtIoApic {
    header: MadtEntryHeader,
    ioapic_id: u8,
    _reserved: u8,
    ioapic_addr: u32,
    gsi_base: u32,
}

#[repr(C, packed)]
struct MadtIsoOverride {
    header: MadtEntryHeader,
    bus: u8,
    source: u8,
    gsi: u32,
    flags: u16,
}

/// Find the MADT table.
fn find_madt_table() -> Option<&'static AcpiMadt> {
    let rsdp = find_rsdp()?;
    let sdt: Option<&'static AcpiSdtHeader>;

    if rsdp.revision >= 2 && rsdp.xsdt_addr != 0 {
        let xsdt_ptr = rsdp.xsdt_addr as *const AcpiSdtHeader;
        unsafe {
            let xsdt = &*xsdt_ptr;
            if xsdt.signature != XSDT_SIGNATURE { return None; }
            let entry_count = (xsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 8;
            let entries = core::slice::from_raw_parts(
                (rsdp.xsdt_addr + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u64,
                entry_count,
            );
            sdt = find_table_in_xsdt(entries, &APIC_SIGNATURE);
        }
    } else {
        let rsdt_ptr = rsdp.rsdt_addr as u64 as *const AcpiSdtHeader;
        unsafe {
            let rsdt = &*rsdt_ptr;
            if rsdt.signature != RSDT_SIGNATURE { return None; }
            let entry_count = (rsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 4;
            let entries = core::slice::from_raw_parts(
                (rsdp.rsdt_addr as u64 + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u32,
                entry_count,
            );
            sdt = find_table_in_rsdt(entries, &APIC_SIGNATURE);
        }
    }

    unsafe {
        let header = sdt?;
        Some(&*(header as *const AcpiSdtHeader as *const AcpiMadt))
    }
}

/// Iterate MADT entries and return the first I/O APIC found.
/// Returns (ioapic_addr, gsi_base).
pub fn find_ioapic() -> Option<(u32, u32)> {
    let madt = find_madt_table()?;
    let madt_header_size = core::mem::size_of::<AcpiMadt>() as u32;
    let total_len = madt.header.length;
    if total_len <= madt_header_size {
        return None;
    }
    let data_ptr = madt as *const AcpiMadt as u64;
    let mut offset = madt_header_size as u64;
    while offset + 2 <= total_len as u64 {
        unsafe {
            let entry = (data_ptr + offset) as *const MadtEntryHeader;
            let entry_type = (*entry).entry_type;
            let record_length = (*entry).record_length as u64;
            if record_length < 2 { break; }
            if entry_type == 1 && record_length >= 12 {
                let ioapic = (data_ptr + offset) as *const MadtIoApic;
                if (*ioapic).ioapic_addr != 0 {
                    return Some(((*ioapic).ioapic_addr, (*ioapic).gsi_base));
                }
            }
            offset += record_length;
        }
    }
    None
}

/// Return all ISA interrupt source overrides from the MADT.
pub fn get_isa_overrides() -> alloc::vec::Vec<(u8, u32, u16)> {
    let mut overrides = alloc::vec::Vec::new();
    let madt = match find_madt_table() {
        Some(m) => m,
        None => return overrides,
    };
    let madt_header_size = core::mem::size_of::<AcpiMadt>() as u32;
    let total_len = madt.header.length;
    if total_len <= madt_header_size {
        return overrides;
    }
    let data_ptr = madt as *const AcpiMadt as u64;
    let mut offset = madt_header_size as u64;
    while offset + 2 <= total_len as u64 {
        unsafe {
            let entry = (data_ptr + offset) as *const MadtEntryHeader;
            let entry_type = (*entry).entry_type;
            let record_length = (*entry).record_length as u64;
            if record_length < 2 { break; }
            if entry_type == 2 && record_length >= 10 {
                let iso = (data_ptr + offset) as *const MadtIsoOverride;
                overrides.push(((*iso).source, (*iso).gsi, (*iso).flags));
            }
            offset += record_length;
        }
    }
    overrides
}

/// Find the HPET ACPI table by scanning RSDP → RSDT/XSDT → HPET.
fn find_hpet_table() -> Option<&'static AcpiHpet> {
    let rsdp = find_rsdp()?;
    let hpetsdt: Option<&'static AcpiSdtHeader>;

    if rsdp.revision >= 2 && rsdp.xsdt_addr != 0 {
        // Use XSDT (64-bit table pointers)
        let xsdt_ptr = rsdp.xsdt_addr as *const AcpiSdtHeader;
        unsafe {
            let xsdt = &*xsdt_ptr;
            if xsdt.signature != XSDT_SIGNATURE {
                return None;
            }
            let entry_count = (xsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 8;
            let entries = core::slice::from_raw_parts(
                (rsdp.xsdt_addr + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u64,
                entry_count,
            );
            hpetsdt = find_table_in_xsdt(entries, &HPET_SIGNATURE);
        }
    } else {
        // Use RSDT (32-bit table pointers)
        let rsdt_ptr = rsdp.rsdt_addr as u64 as *const AcpiSdtHeader;
        unsafe {
            let rsdt = &*rsdt_ptr;
            if rsdt.signature != RSDT_SIGNATURE {
                return None;
            }
            let entry_count = (rsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 4;
            let entries = core::slice::from_raw_parts(
                (rsdp.rsdt_addr as u64 + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u32,
                entry_count,
            );
            hpetsdt = find_table_in_rsdt(entries, &HPET_SIGNATURE);
        }
    }

    unsafe {
        let header = hpetsdt?;
        Some(&*(header as *const AcpiSdtHeader as *const AcpiHpet))
    }
}

// ── Public API ─────────────────────────────────────────────────────

/// Global HPET MMIO base address, programmed into HPET_BASE on init.
static mut HPET_MMIO_BASE: u64 = 0;

/// Counter period in femtoseconds, from HPET capabilities.
pub static mut HPET_FS_PERIOD: u64 = 0;

/// Return the HPET MMIO base address (0 if not initialized).
pub fn hpet_mmio_base() -> u64 {
    unsafe { HPET_MMIO_BASE }
}

/// Return the HPET counter period in femtoseconds (0 if not initialized).
pub fn hpet_fs_period() -> u64 {
    unsafe { HPET_FS_PERIOD }
}

/// Read the raw HPET main counter value at a given MMIO base.
/// Used by the APIC timer calibration code.
pub unsafe fn read_raw_counter(base: u64) -> u64 {
    if base != 0 {
        hpet_read(base, HPET_MAIN_COUNTER)
    } else {
        0
    }
}

/// Read the HPET general configuration register.
pub unsafe fn read_hpet_config(base: u64) -> u64 {
    hpet_read(base, HPET_GEN_CONFIG)
}

/// Write the HPET general configuration register.
pub unsafe fn write_hpet_config(base: u64, val: u64) {
    hpet_write(base, HPET_GEN_CONFIG, val);
}

/// Initialize the HPET timer.
///
/// 1. Locate the HPET ACPI table via RSDP → RSDT/XSDT.
/// 2. Read the MMIO base address and counter period.
/// 3. Configure timer 0 in periodic mode.
/// 4. Route interrupt to IRQ0 (legacy replacement).
/// 5. Set comparator for `TICK_INTERVAL_US` (1 ms at 1 KHz).
/// 6. Enable the main counter.
///
/// Returns `true` if HPET was successfully initialized.
pub fn init_hpet() -> bool {
    let hpet = match find_hpet_table() {
        Some(h) => h,
        None => {
            kerror!(LogSubsys::Hpet, "ACPI HPET table not found");
            return false;
        }
    };

    let base_addr;
    let fs_period;
    unsafe {
        base_addr = hpet.base_addr.address;
        fs_period = read_hpet_fs_period(base_addr);
    }

    if base_addr == 0 {
        kerror!(LogSubsys::Hpet, "Invalid HPET base address");
        return false;
    }

    kinfo!(LogSubsys::Hpet, "Found at MMIO 0x{:x}, period={} fs",
        base_addr, fs_period);

    // Calculate comparator value for TICK_INTERVAL_US
    // counter_period = fs_period * 10^-15 seconds per tick
    // ticks_needed = (TICK_INTERVAL_US * 10^-6) / (fs_period * 10^-15)
    //            = TICK_INTERVAL_US * 10^9 / fs_period
    let counter_hz = 1_000_000_000_000_000u64 / fs_period;
    let ticks_needed = (counter_hz * crate::timers::TICK_INTERVAL_US) / 1_000_000;

    kdebug!(LogSubsys::Hpet, "Counter freq: {} Hz, ticks/interval: {}",
        counter_hz, ticks_needed);

    if ticks_needed == 0 || ticks_needed > 0xFFFF_FFFF {
        kerror!(LogSubsys::Hpet, "Invalid comparator value {}", ticks_needed);
        return false;
    }

    unsafe {
        // Disable HPET while configuring
        hpet_write(base_addr, HPET_GEN_CONFIG, 0);

        // Check that timer 0 supports periodic mode.
        // Bit 4 of timer config = periodic mode capable.
        let t0_cap = hpet_read(base_addr, HPET_TIMER0_CFG);
        if t0_cap & (1u64 << 4) == 0 {
            kerror!(LogSubsys::Hpet, "Timer 0 does not support periodic mode");
            return false;
        }

        // Configure timer 0:
        //   - Periodic mode (bit 3 = 1)
        //   - Interrupt enable (bit 2 = 1)
        //   - Use 32-bit mode for easier compat (clear bit 5)
        //   - Clear routing bits; defaults to IRQ0
        let t0_cfg = HPET_TIMER_INT_ENABLE | HPET_TIMER_PERIODIC;
        hpet_write(base_addr, HPET_TIMER0_CFG, t0_cfg);

        // Set comparator to target interval.
        hpet_write(base_addr, HPET_TIMER0_COMPARATOR, ticks_needed);

        // Enable legacy replacement: HPET timer 0 -> IRQ0, timer 1 -> IRQ8 (RTC).
        // This disables the PIT and RTC via PIC directly.
        // Enable main counter.
        hpet_write(base_addr, HPET_GEN_CONFIG, HPET_CFG_ENABLE | HPET_CFG_LEGACY);

        // Store for later use (e.g., APIC calibration)
        HPET_MMIO_BASE = base_addr;
        HPET_FS_PERIOD = fs_period;
    }

    kinfo!(LogSubsys::Hpet, "Timer 0 configured: {} Hz ({} µs per tick)",
        counter_hz / ticks_needed, crate::timers::TICK_INTERVAL_US);

    true
}

/// Read the HPET counter period in femtoseconds from the capabilities register.
unsafe fn read_hpet_fs_period(base_addr: u64) -> u64 {
    let cap = hpet_read(base_addr, HPET_GEN_CAP_ID);
    // Bits 31:32 = counter clock period in femtoseconds
    cap >> 32
}

/// Read the current HPET main counter value (for high-resolution timing).
pub fn read_counter() -> u64 {
    unsafe {
        if HPET_MMIO_BASE != 0 {
            hpet_read(HPET_MMIO_BASE, HPET_MAIN_COUNTER)
        } else {
            0
        }
    }
}

/// Convert HPET counter ticks to microseconds.
/// Requires HPET to be initialized.
pub fn ticks_to_us(ticks: u64) -> u64 {
    unsafe {
        if HPET_FS_PERIOD == 0 { return 0; }
        // ticks * fs_period * 10^-15 seconds / 10^-6 = ticks * fs_period / 10^9
        (ticks * HPET_FS_PERIOD) / 1_000_000_000
    }
}

/// Sleep for approximately `us` microseconds using the HPET counter.
/// Busy-waits; only suitable for short delays.
pub fn sleep_us(us: u64) {
    unsafe {
        if HPET_MMIO_BASE == 0 {
            crate::hal::sleep_hint(us as u32);
            return;
        }
        // Calculate target tick count for the requested delay
        let counter_hz = 1_000_000_000_000_000u64 / HPET_FS_PERIOD;
        let ticks_needed = (counter_hz * us) / 1_000_000;
        let start = hpet_read(HPET_MMIO_BASE, HPET_MAIN_COUNTER);
        loop {
            let now = hpet_read(HPET_MMIO_BASE, HPET_MAIN_COUNTER);
            if now.wrapping_sub(start) >= ticks_needed {
                break;
            }
            // Pause hint for hyperthreading
            crate::hal::raw::raw_pause();
        }
    }
}


