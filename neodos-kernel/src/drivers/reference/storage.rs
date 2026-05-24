// src/drivers/reference/storage.rs
// Reference Rust Storage Driver — .nem module
//
// A Rust boot-critical driver for block storage I/O.
// Follows the extern "C" entrypoint contract for NeoDOS HAL ABI v0.3.
//
// Responsible for:
//   - Registering with the Device Model as a storage controller
//   - Mediating block read/write requests via the HAL binding layer
//   - Reporting device health and geometry
//
// Compile target: x86_64-unknown-none, #![no_std]

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// NeoEvent repr(C) matching the Event Bus event structure
#[repr(C)]
pub struct NeoEvent {
    pub event_id: u64,
    pub event_type: u32,
    pub source: u32,
    pub timestamp: u64,
    pub device_id: u32,
    pub driver_target: u32,
    pub data0: u64,
    pub data1: u64,
    pub flags: u32,
}

/// Block device geometry
#[repr(C)]
pub struct BlockGeometry {
    pub num_sectors: u64,
    pub sector_size: u32,
    pub max_transfer: u32,
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static SECTOR_COUNT: AtomicU64 = AtomicU64::new(0);
static IO_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Initialize the storage driver.
/// Returns 0 on success, non-zero on failure.
pub fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) {
        return -1;
    }

    // In a real implementation:
    //   let controller = hal::device_query_storage(device_id);
    //   SECTOR_COUNT.store(controller.num_sectors, Ordering::Release);
    //   Register DMA buffers via HAL binding layer

    INITIALIZED.store(true, Ordering::Release);
    0
}

/// Event handler — storage handles DISK_IO_COMPLETE events from DMA IRQs.
pub fn driver_on_event(event: *const NeoEvent) -> i32 {
    if !ACTIVE.load(Ordering::Relaxed) {
        return -1;
    }
    if event.is_null() {
        return -1;
    }

    let ev = unsafe { &*event };

    // Handle DISK_IO_COMPLETE (type 3)
    if ev.event_type == 3 {
        // data0 = LBA, data1 = status (0 = success, non-zero = error)
        let status = ev.data1;
        if status != 0 {
            IO_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        return 0;
    }

    1
}

/// Driver finalization.
pub fn driver_fini() {
    ACTIVE.store(false, Ordering::Release);
    INITIALIZED.store(false, Ordering::Release);
    SECTOR_COUNT.store(0, Ordering::Release);
    IO_ERRORS.store(0, Ordering::Release);
}

/// Activate after Event Bus binding.
pub fn driver_activate() -> i32 {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return -1;
    }
    ACTIVE.store(true, Ordering::Release);
    0
}

/// Query device geometry.
pub fn driver_get_geometry(geo: *mut BlockGeometry) -> i32 {
    if geo.is_null() {
        return -1;
    }
    if !ACTIVE.load(Ordering::Relaxed) {
        return -1;
    }
    unsafe {
        (*geo).num_sectors = SECTOR_COUNT.load(Ordering::Relaxed);
        (*geo).sector_size = 512;
        (*geo).max_transfer = 8; // 8 sectors max per DMA transfer
    }
    0
}

/// Read blocks via HAL binding layer.
pub fn driver_read_blocks(lba: u64, count: u32, buf: *mut u8) -> i32 {
    if buf.is_null() {
        return -1;
    }
    if !ACTIVE.load(Ordering::Relaxed) {
        return -1;
    }
    if count == 0 || count > 8 {
        return -1;
    }

    // In a real implementation:
    //   let result = hal::device_read(device_handle, lba, count, buf);
    //   if result.is_err() { IO_ERRORS.fetch_add(1, ...); return -1; }

    0
}

/// Write blocks via HAL binding layer.
pub fn driver_write_blocks(lba: u64, count: u32, buf: *const u8) -> i32 {
    if buf.is_null() {
        return -1;
    }
    if !ACTIVE.load(Ordering::Relaxed) {
        return -1;
    }
    if count == 0 || count > 8 {
        return -1;
    }

    // In a real implementation:
    //   let result = hal::device_write(device_handle, lba, count, buf);
    //   if result.is_err() { IO_ERRORS.fetch_add(1, ...); return -1; }

    0
}

#[cfg(test)]
mod tests {
    // These tests are for standalone compilation of this .nem module.
}

/// Register kernel-compatible tests for the storage reference driver.
pub fn register_ref_storage_tests() {
    crate::test_case!("ref_stor_init_success", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_init(), 0);
        crate::test_eq!(INITIALIZED.load(Ordering::Relaxed), true);
    });

    crate::test_case!("ref_stor_double_init_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        crate::test_eq!(driver_init(), -1);
    });

    crate::test_case!("ref_stor_activate_success", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        crate::test_eq!(driver_activate(), 0);
        crate::test_eq!(ACTIVE.load(Ordering::Relaxed), true);
    });

    crate::test_case!("ref_stor_activate_before_init_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_activate(), -1);
    });

    crate::test_case!("ref_stor_null_geo_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_get_geometry(core::ptr::null_mut()), -1);
    });

    crate::test_case!("ref_stor_geo_before_active_fails", {
        INITIALIZED.store(true, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        let mut geo = BlockGeometry { num_sectors: 0, sector_size: 0, max_transfer: 0 };
        crate::test_eq!(driver_get_geometry(&mut geo as *mut _), -1);
    });

    crate::test_case!("ref_stor_read_null_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        crate::test_eq!(driver_read_blocks(0, 1, core::ptr::null_mut()), -1);
    });

    crate::test_case!("ref_stor_write_null_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        crate::test_eq!(driver_write_blocks(0, 1, core::ptr::null()), -1);
    });

    crate::test_case!("ref_stor_read_zero_count_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        let mut buf = [0u8; 512];
        crate::test_eq!(driver_read_blocks(0, 0, buf.as_mut_ptr()), -1);
    });

    crate::test_case!("ref_stor_read_overflow_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        let mut buf = [0u8; 512];
        crate::test_eq!(driver_read_blocks(0, 9, buf.as_mut_ptr()), -1);
    });

    crate::test_case!("ref_stor_fini_clears", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        driver_fini();
        crate::test_eq!(ACTIVE.load(Ordering::Relaxed), false);
        crate::test_eq!(INITIALIZED.load(Ordering::Relaxed), false);
    });

    crate::test_case!("ref_stor_null_event_rejected", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        crate::test_eq!(driver_on_event(core::ptr::null()), -1);
    });

    crate::test_case!("ref_stor_io_complete_event", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        let ev = NeoEvent {
            event_id: 1, event_type: 3, source: 0, timestamp: 0,
            device_id: 1, driver_target: 0, data0: 100, data1: 0, flags: 0,
        };
        crate::test_eq!(driver_on_event(&ev as *const _), 0);
    });

    crate::test_case!("ref_stor_io_error_recorded", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        IO_ERRORS.store(0, Ordering::Relaxed);
        let ev = NeoEvent {
            event_id: 2, event_type: 3, source: 0, timestamp: 0,
            device_id: 1, driver_target: 0, data0: 200, data1: 1, flags: 0,
        };
        driver_on_event(&ev as *const _);
        crate::test_eq!(IO_ERRORS.load(Ordering::Relaxed), 1);
    });
}
