// src/drivers/reference/ps2kbd.rs
// Reference Rust PS/2 Keyboard Driver — .nem module
//
// A Rust boot-critical driver that integrates with the Event Bus.
// Follows the extern "C" entrypoint contract for NeoDOS HAL ABI v0.3.
//
// When compiled as a .nem module and loaded by the Boot Driver Loader,
// this driver:
//   1. Receives KEYBOARD_INPUT events from the Event Bus
//   2. Validates scan codes via the HAL binding layer
//   3. Forwards keystrokes to the input ring buffer
//
// Compile target: x86_64-unknown-none, #![no_std]

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};

/// NeoEvent repr(C) matching the Event Bus event structure (56 bytes)
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

/// Driver state: tracks whether the keyboard is initialized and active
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Driver entry point — called once when the .nem driver is loaded.
/// Returns 0 on success, non-zero on failure.
pub fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) {
        return -1; // Already initialized
    }

    // PS/2 controller initialization is already done by the kernel at boot.
    // This driver hooks into the Event Bus to receive keyboard events.

    INITIALIZED.store(true, Ordering::Release);
    0
}

/// Event handler — called for each KEYBOARD_INPUT event.
/// Returns 0 if the event was handled, non-zero to pass to next handler.
pub fn driver_on_event(event: *const NeoEvent) -> i32 {
    if !ACTIVE.load(Ordering::Relaxed) {
        return -1; // Not yet active
    }
    if event.is_null() {
        return -1;
    }

    // SAFETY: Event pointer is guaranteed valid by the Event Bus dispatcher.
    let ev = unsafe { &*event };

    // Only handle keyboard input events (type 1)
    if ev.event_type != 1 {
        return 1; // Not our event, pass through
    }

    // data0 contains the scan code from the PS/2 interrupt handler
    let scan_code = ev.data0 as u8;
    let _pressed = (ev.data0 >> 8) & 1; // 0 = release, 1 = press

    // Validate scan code range
    if scan_code == 0x00 || scan_code > 0x83 {
        return 1; // Invalid scan code, pass through
    }

    // In a real implementation, this would:
    // 1. Translate scan code to ASCII via keyboard layout table
    // 2. Handle modifier keys (Shift, Ctrl, Alt)
    // 3. Push resulting character to the input ring buffer
    // All via HAL ABI v0.3 — no direct port I/O

    0
}

/// Driver finalization — called when the driver is unloaded.
pub fn driver_fini() {
    ACTIVE.store(false, Ordering::Release);
    INITIALIZED.store(false, Ordering::Release);
}

/// Called by the loader to activate the driver after Event Bus binding.
pub fn driver_activate() -> i32 {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return -1;
    }
    ACTIVE.store(true, Ordering::Release);
    0
}

/// Query whether this driver is active (used by NDREG DEBUG).
pub fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) {
        1
    } else {
        0
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    // These tests are for standalone compilation of this .nem module.
    // They are not part of the kernel test suite (see register_ref_ps2kbd_tests).
}

/// Register kernel-compatible tests for the ps2kbd reference driver.
pub fn register_ref_ps2kbd_tests() {
    crate::test_case!("ref_ps2kbd_init_not_active", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(ACTIVE.load(Ordering::Relaxed), false);
    });

    crate::test_case!("ref_ps2kbd_init_success", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_init(), 0);
        crate::test_eq!(INITIALIZED.load(Ordering::Relaxed), true);
    });

    crate::test_case!("ref_ps2kbd_double_init_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        crate::test_eq!(driver_init(), -1);
    });

    crate::test_case!("ref_ps2kbd_null_event_rejected", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        crate::test_eq!(driver_on_event(core::ptr::null()), -1);
    });

    crate::test_case!("ref_ps2kbd_wrong_event_passthrough", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        let ev = NeoEvent {
            event_id: 1, event_type: 99, source: 0, timestamp: 0,
            device_id: 0, driver_target: 0, data0: 0, data1: 0, flags: 0,
        };
        crate::test_eq!(driver_on_event(&ev as *const _), 1);
    });

    crate::test_case!("ref_ps2kbd_valid_scancode", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        let ev = NeoEvent {
            event_id: 1, event_type: 1, source: 0, timestamp: 100,
            device_id: 2, driver_target: 0, data0: 0x1E, data1: 0, flags: 0,
        };
        crate::test_eq!(driver_on_event(&ev as *const _), 0);
    });

    crate::test_case!("ref_ps2kbd_invalid_scancode", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        let ev = NeoEvent {
            event_id: 2, event_type: 1, source: 0, timestamp: 100,
            device_id: 2, driver_target: 0, data0: 0x00, data1: 0, flags: 0,
        };
        crate::test_eq!(driver_on_event(&ev as *const _), 1);
    });

    crate::test_case!("ref_ps2kbd_fini_clears_state", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        driver_activate();
        driver_fini();
        crate::test_eq!(ACTIVE.load(Ordering::Relaxed), false);
        crate::test_eq!(INITIALIZED.load(Ordering::Relaxed), false);
    });

    crate::test_case!("ref_ps2kbd_activate_before_init_fails", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        crate::test_eq!(driver_activate(), -1);
    });

    crate::test_case!("ref_ps2kbd_is_active_query", {
        INITIALIZED.store(false, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        driver_init();
        crate::test_eq!(driver_is_active(), 0);
        driver_activate();
        crate::test_eq!(driver_is_active(), 1);
    });
}
