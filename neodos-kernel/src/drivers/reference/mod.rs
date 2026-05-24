// src/drivers/reference/mod.rs
// Reference Rust .nem driver implementations
//
// These are example implementations of Rust-based .nem drivers following
// the NeoDOS HAL ABI v0.3 extern "C" entrypoint contract.
//
// Every Rust .nem driver MUST expose:
//   #[no_mangle] pub extern "C" fn driver_init() -> i32;
//   #[no_mangle] pub extern "C" fn driver_on_event(event: *const NeoEvent) -> i32;
//   #[no_mangle] pub extern "C" fn driver_fini();
//
// Rules:
//   - #![no_std] only
//   - No heap in IRQ context
//   - No raw hardware access (use HAL binding layer)
//   - Deterministic lifecycle

pub mod ps2kbd;
pub mod framebuffer;
pub mod storage;
