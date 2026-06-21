use core::sync::atomic::{AtomicU64, Ordering};
use core::ptr::{read_volatile, write_volatile};

/// ECAM (Enhanced Configuration Access Mechanism) base address.
/// Set from MCFG ACPI table or default QEMU value.
static ECAM_BASE: AtomicU64 = AtomicU64::new(0);

/// Whether ECAM is active (true) or we fall back to legacy PIO.
static ECAM_ACTIVE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Set the ECAM base address and mark ECAM as active.
pub fn set_ecam_base(base: u64) {
    ECAM_BASE.store(base, Ordering::SeqCst);
    ECAM_ACTIVE.store(true, Ordering::SeqCst);
}

/// Return the current ECAM base address (0 if not set).
pub fn ecam_base() -> u64 {
    ECAM_BASE.load(Ordering::Relaxed)
}

/// Check whether ECAM mode is active.
pub fn ecam_is_active() -> bool {
    ECAM_ACTIVE.load(Ordering::Relaxed)
}

/// Deactivate ECAM (fall back to legacy PIO).
#[allow(dead_code)]
pub fn ecam_deactivate() {
    ECAM_ACTIVE.store(false, Ordering::SeqCst);
}

/// Compute the MMIO address for a given PCI bus/device/function/offset
/// under the ECAM scheme.
///
/// ECAM addressing:
///   addr = ECAM_BASE + (bus << 20) + (dev << 15) + (func << 12) + offset
#[inline]
fn ecam_address(bus: u8, dev: u8, func: u8, offset: u8) -> u64 {
    let base = ECAM_BASE.load(Ordering::Relaxed);
    base | ((bus as u64) << 20) | ((dev as u64) << 15) | ((func as u64) << 12) | (offset as u64)
}

/// Read a 32-bit value from PCI config space via ECAM MMIO.
///
/// # Safety
/// - `bus` must be valid for the PCI segment (checked at init).
/// - `offset` must be 4-byte aligned; the function will align it.
pub unsafe fn ecam_read_config_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = ecam_address(bus, dev, func, offset & 0xFC);
    read_volatile(addr as *const u32)
}

/// Read a 16-bit value from PCI config space via ECAM MMIO.
pub unsafe fn ecam_read_config_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let dword = ecam_read_config_dword(bus, dev, func, offset & 0xFC);
    ((dword >> ((offset as u32 & 3) * 8)) & 0xFFFF) as u16
}

/// Read an 8-bit value from PCI config space via ECAM MMIO.
pub unsafe fn ecam_read_config_byte(bus: u8, dev: u8, func: u8, offset: u8) -> u8 {
    let dword = ecam_read_config_dword(bus, dev, func, offset & 0xFC);
    ((dword >> ((offset as u32 & 3) * 8)) & 0xFF) as u8
}

/// Write a 32-bit value to PCI config space via ECAM MMIO.
///
/// # Safety
/// - `bus` must be valid for the PCI segment.
/// - `offset` must be 4-byte aligned; the function will align it.
pub unsafe fn ecam_write_config_dword(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    let addr = ecam_address(bus, dev, func, offset & 0xFC);
    write_volatile(addr as *mut u32, value);
}

/// Write a 16-bit value to PCI config space via ECAM MMIO.
pub unsafe fn ecam_write_config_word(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = ecam_read_config_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);
    ecam_write_config_dword(bus, dev, func, aligned, new_dword);
}

/// Write an 8-bit value to PCI config space via ECAM MMIO.
pub unsafe fn ecam_write_config_byte(bus: u8, dev: u8, func: u8, offset: u8, value: u8) {
    let aligned = offset & 0xFC;
    let dword = ecam_read_config_dword(bus, dev, func, aligned);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFu32 << shift);
    let new_dword = (dword & mask) | ((value as u32) << shift);
    ecam_write_config_dword(bus, dev, func, aligned, new_dword);
}

// ── Tests ──────────────────────────────────────────────────────────

/// Register ECAM tests. Called from the kernel test harness.
pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    test_case!("ecam_base_default", {
        let active = ecam_is_active();
        let base = ecam_base();
        test_true!(!active);
        test_eq!(base, 0);
    });

    test_case!("ecam_address_calc", {
        set_ecam_base(0xE000_0000);
        let r0 = ecam_address(0, 0, 0, 0);
        let r1 = ecam_address(0, 0x1F, 0, 0);
        let r2 = ecam_address(1, 0, 0, 0);
        let r3 = ecam_address(0, 0, 7, 0xFF);
        ECAM_ACTIVE.store(false, Ordering::SeqCst);
        ECAM_BASE.store(0, Ordering::SeqCst);
        test_eq!(r0, 0xE000_0000);
        test_eq!(r1, 0xE000_0000 | ((0x1F as u64) << 15));
        test_eq!(r2, 0xE000_0000 | (1u64 << 20));
        test_eq!(r3, 0xE000_0000 | ((7 as u64) << 12) | 0xFF);
    });
}
