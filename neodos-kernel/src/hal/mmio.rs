//! Minimal device-register MMIO access.
//!
//! Covers exactly the register widths used by the kernel's real MMIO
//! consumers found in the HAL audit: 8, 32 and 64 bits. Callers are
//! responsible for having the address mapped (typically as uncacheable /
//! UC- device memory); this module performs no mapping.
//!
//! Deliberately NOT covered here:
//! - DMA descriptor memory (virtio vring, NVMe/AHCI command+completion
//!   queues): it is coherent RAM, not device registers.
//! - ACPI tables (firmware RAM), the KPRCB, lock-free rings and user buffers.
//! - The framebuffer surface.
//! - 16-bit accesses: no current consumer needs them.

use core::sync::atomic::{compiler_fence, Ordering};

/// Read an 8-bit device register at `addr`.
///
/// # Safety
/// `addr` must be a valid, mapped MMIO address of an 8-bit register.
#[inline]
pub unsafe fn read8(addr: usize) -> u8 {
    compiler_fence(Ordering::SeqCst);
    core::ptr::read_volatile(addr as *const u8)
}

/// Read a 32-bit device register at `addr`.
///
/// # Safety
/// `addr` must be a valid, mapped, 4-byte-aligned MMIO address.
#[inline]
pub unsafe fn read32(addr: usize) -> u32 {
    compiler_fence(Ordering::SeqCst);
    core::ptr::read_volatile(addr as *const u32)
}

/// Read a 64-bit device register at `addr`.
///
/// # Safety
/// `addr` must be a valid, mapped, 8-byte-aligned MMIO address.
#[inline]
pub unsafe fn read64(addr: usize) -> u64 {
    compiler_fence(Ordering::SeqCst);
    core::ptr::read_volatile(addr as *const u64)
}

/// Write an 8-bit device register at `addr`.
///
/// # Safety
/// `addr` must be a valid, mapped MMIO address of an 8-bit register.
#[inline]
pub unsafe fn write8(addr: usize, val: u8) {
    compiler_fence(Ordering::SeqCst);
    core::ptr::write_volatile(addr as *mut u8, val);
}

/// Write a 32-bit device register at `addr`.
///
/// # Safety
/// `addr` must be a valid, mapped, 4-byte-aligned MMIO address.
#[inline]
pub unsafe fn write32(addr: usize, val: u32) {
    compiler_fence(Ordering::SeqCst);
    core::ptr::write_volatile(addr as *mut u32, val);
}

/// Write a 64-bit device register at `addr`.
///
/// # Safety
/// `addr` must be a valid, mapped, 8-byte-aligned MMIO address.
#[inline]
pub unsafe fn write64(addr: usize, val: u64) {
    compiler_fence(Ordering::SeqCst);
    core::ptr::write_volatile(addr as *mut u64, val);
}

/// Verify the MMIO helpers round-trip through normal (mapped) memory.
///
/// This validates the pointer/width plumbing; real device-register access is
/// exercised by the individual consumers (LAPIC, HPET, IOAPIC, ...).
pub fn register_tests() {
    crate::testing::register("hal_mmio_roundtrip", || {
        let mut buf = [0u64; 2];
        let addr = buf.as_mut_ptr() as usize;
        unsafe {
            write8(addr, 0xAB);
            crate::test_eq!(read8(addr), 0xAB);

            write32(addr + 4, 0xDEAD_BEEF);
            crate::test_eq!(read32(addr + 4), 0xDEAD_BEEF);

            write64(addr, 0x0123_4567_89AB_CDEF);
            crate::test_eq!(read64(addr), 0x0123_4567_89AB_CDEF);
        }
        let _ = buf;
        Ok(())
    });
}
