// src/drivers/reference/framebuffer.rs
// Reference Rust Framebuffer Driver — .nem module
//
// A Rust boot-critical driver that manages the framebuffer for display output.
// Follows the extern "C" entrypoint contract for NeoDOS HAL ABI v0.3.
//
// Responsible for:
//   - Managing framebuffer memory (MMIO via HAL binding layer)
//   - Providing a standardized display API for higher-level graphics
//   - Registering as a boot-critical device with the Device Model
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

/// Framebuffer info structure
#[repr(C)]
pub struct FramebufferInfo {
    pub base_addr: u64,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub pitch: u32,
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static FB_ADDR: AtomicU64 = AtomicU64::new(0);
static FB_WIDTH: AtomicU64 = AtomicU64::new(0);
static FB_HEIGHT: AtomicU64 = AtomicU64::new(0);
static FB_BPP: AtomicU64 = AtomicU64::new(0);

/// Initialize the framebuffer driver.
/// Returns 0 on success, non-zero on failure.
pub fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) {
        return -1;
    }

    // Framebuffer MMIO region would be obtained from the HAL binding layer.
    // The bootloader has already set up the framebuffer; this driver claims
    // ownership and provides a standardized API.

    // In a real implementation:
    //   let fb = hal::device_query_framebuffer(device_id);
    //   FB_ADDR.store(fb.base_addr, Ordering::Release);
    //   FB_WIDTH.store(fb.width as u64, Ordering::Release);
    //   FB_HEIGHT.store(fb.height as u64, Ordering::Release);
    //   FB_BPP.store(fb.bpp as u64, Ordering::Release);

    INITIALIZED.store(true, Ordering::Release);
    0
}

/// Event handler — framebuffer typically does not process input events,
/// so this mostly returns 1 (pass through). Future versions may handle
/// display reconfiguration events.
pub fn driver_on_event(_event: *const NeoEvent) -> i32 {
    if !ACTIVE.load(Ordering::Relaxed) {
        return -1;
    }
    // Framebuffer is a passive device — no event handling needed
    1
}

/// Driver finalization.
pub fn driver_fini() {
    ACTIVE.store(false, Ordering::Release);
    INITIALIZED.store(false, Ordering::Release);
    FB_ADDR.store(0, Ordering::Release);
    FB_WIDTH.store(0, Ordering::Release);
    FB_HEIGHT.store(0, Ordering::Release);
    FB_BPP.store(0, Ordering::Release);
}

/// Activate after Event Bus binding.
pub fn driver_activate() -> i32 {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return -1;
    }
    ACTIVE.store(true, Ordering::Release);
    0
}

/// Query screen dimensions (data through memory, not registers).
pub fn driver_get_info(info: *mut FramebufferInfo) -> i32 {
    if info.is_null() {
        return -1;
    }
    if !ACTIVE.load(Ordering::Relaxed) {
        return -1;
    }
    unsafe {
        (*info).base_addr = FB_ADDR.load(Ordering::Relaxed);
        (*info).width = FB_WIDTH.load(Ordering::Relaxed) as u32;
        (*info).height = FB_HEIGHT.load(Ordering::Relaxed) as u32;
        (*info).bpp = FB_BPP.load(Ordering::Relaxed) as u8;
        (*info).pitch = FB_WIDTH.load(Ordering::Relaxed) as u32 * (FB_BPP.load(Ordering::Relaxed) as u32 / 8);
    }
    0
}

#[cfg(test)]
mod tests {
    // These tests are for standalone compilation of this .nem module.
}

/// Register kernel-compatible tests for the framebuffer reference driver.
pub fn register_ref_framebuffer_tests() {
    crate::test_case!("ref_fb_init_success", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_init(), 0);
        crate::test_eq!(INITIALIZED.load(Ordering::Relaxed), true);
    });

    crate::test_case!("ref_fb_double_init_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        crate::test_eq!(driver_init(), -1);
    });

    crate::test_case!("ref_fb_activate_success", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        crate::test_eq!(driver_activate(), 0);
        crate::test_eq!(ACTIVE.load(Ordering::Relaxed), true);
    });

    crate::test_case!("ref_fb_activate_before_init_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_activate(), -1);
    });

    crate::test_case!("ref_fb_fini_clears", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        driver_fini();
        crate::test_eq!(ACTIVE.load(Ordering::Relaxed), false);
        crate::test_eq!(INITIALIZED.load(Ordering::Relaxed), false);
    });

    crate::test_case!("ref_fb_info_before_activate_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        let mut info = FramebufferInfo {
            base_addr: 0, width: 0, height: 0, bpp: 0, pitch: 0,
        };
        crate::test_eq!(driver_get_info(&mut info as *mut _), -1);
    });

    crate::test_case!("ref_fb_null_info_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_get_info(core::ptr::null_mut()), -1);
    });

    crate::test_case!("ref_fb_event_passthrough", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        let ev = NeoEvent {
            event_id: 0, event_type: 0, source: 0, timestamp: 0,
            device_id: 0, driver_target: 0, data0: 0, data1: 0, flags: 0,
        };
        crate::test_eq!(driver_on_event(&ev as *const _), 1);
    });
}
