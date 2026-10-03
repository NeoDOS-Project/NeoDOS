//! Driver Runtime — loaded .nem driver registry, lifecycle and certification.

// Driver Runtime — tracks loaded .nem driver instances, state, and lifetimes
//
// ── Driver Certification Pipeline v1 ──
//
// Strict lifecycle: Loaded → Initialized → Registered → Bound → Active
// A driver MUST follow this exact sequence. If ANY step is skipped,
// the driver MUST NOT appear as ACTIVE in the registry.
//
// A driver may be LOADED and even INITIALIZED but still NOT ACTIVE because:
//   1. Registry was never updated (stuck in Loaded/Initialized)
//   2. Event Bus binding missing (stuck in Registered)
//   3. Sandbox rejection (certify_and_activate fails)
//   4. Deferred activation (scheduler hasn't called certify)
//   5. Missing capability grant (security model denied activation)


use spin::Mutex;
use lazy_static::lazy_static;
use crate::nem::{NemDriverType, DriverCategory};

mod state;
mod pipeline;
mod instance;
mod runtime;
mod tests;

pub use state::{DriverState, TransitionError};
pub use pipeline::PipelineStep;
pub use instance::DriverInstance;
pub use runtime::DriverRuntime;
pub use tests::register_driver_certification_tests;

// ── Constants ──

pub type DriverId = u32;
pub const MAX_DRIVERS: usize = 16;
pub const INVALID_DRIVER_ID: DriverId = 0;

// ── Error codes for last_error field ──

pub const ERR_NONE: u32 = 0;
pub const ERR_INIT_FAILED: u32 = 1;
pub const ERR_REGISTRATION_FAILED: u32 = 2;
pub const ERR_BIND_FAILED: u32 = 3;
pub const ERR_SANDBOX_REJECTED: u32 = 4;
pub const ERR_CERTIFICATION_FAILED: u32 = 5;
pub const ERR_OUT_OF_MEMORY: u32 = 6;
pub const ERR_POLICY_VIOLATION: u32 = 7;
pub const ERR_LOAD_FAILED: u32 = 8;
pub const ERR_CAPABILITY_DENIED: u32 = 9;
pub const ERR_UNLOAD_FAILED: u32 = 10;
pub const ERR_UNLOAD_TIMEOUT: u32 = 11;

pub fn err_to_str(code: u32) -> &'static str {
    match code {
        ERR_NONE => "NONE",
        ERR_INIT_FAILED => "INIT_FAILED",
        ERR_REGISTRATION_FAILED => "REGISTRATION_FAILED",
        ERR_BIND_FAILED => "BIND_FAILED",
        ERR_SANDBOX_REJECTED => "SANDBOX_REJECTED",
        ERR_CERTIFICATION_FAILED => "CERTIFICATION_FAILED",
        ERR_OUT_OF_MEMORY => "OUT_OF_MEMORY",
        ERR_POLICY_VIOLATION => "POLICY_VIOLATION",
        ERR_LOAD_FAILED => "LOAD_FAILED",
        ERR_CAPABILITY_DENIED => "CAPABILITY_DENIED",
        ERR_UNLOAD_FAILED => "UNLOAD_FAILED",
        ERR_UNLOAD_TIMEOUT => "UNLOAD_TIMEOUT",
        _ => "UNKNOWN",
    }
}

// ── Global singleton ──

lazy_static! {
    pub static ref DRIVER_RUNTIME: Mutex<DriverRuntime> = Mutex::new(DriverRuntime::new());
}

// ── Convenience wrappers ──

pub fn register_driver(
    name: &str,
    driver_type: NemDriverType,
    api_version: u16,
    compat_flags: u16,
) -> Result<DriverId, &'static str> {
    DRIVER_RUNTIME.lock().register(name, driver_type, api_version, compat_flags)
}

#[allow(clippy::too_many_arguments)]
pub fn register_driver_ext(
    name: &str,
    driver_type: NemDriverType,
    api_version: u16,
    compat_flags: u16,
    abi_min: u16,
    abi_target: u16,
    abi_max: u16,
    category: DriverCategory,
) -> Result<DriverId, &'static str> {
    DRIVER_RUNTIME.lock().register_ext(name, driver_type, api_version, compat_flags,
        abi_min, abi_target, abi_max, category)
}

pub fn unregister_driver(id: DriverId) -> bool {
    DRIVER_RUNTIME.lock().unregister(id)
}

pub fn get_driver(id: DriverId) -> Option<DriverInstance> {
    DRIVER_RUNTIME.lock().get(id).copied()
}

pub fn get_driver_by_name(name: &str) -> Option<DriverInstance> {
    DRIVER_RUNTIME.lock().get_by_name(name).copied()
}

pub fn driver_count() -> usize {
    DRIVER_RUNTIME.lock().count()
}

pub fn driver_names() -> alloc::vec::Vec<(alloc::string::String, DriverId, DriverState)> {
    let mut results = alloc::vec::Vec::new();
    let runtime = DRIVER_RUNTIME.lock();
    for drv in runtime.drivers.iter().flatten() {
        results.push((alloc::string::String::from(drv.name_str()), drv.id, drv.state));
    }
    results
}

/// Check whether a driver (by ID) holds the required capabilities.
pub fn check_driver_cap(id: DriverId, required: u64) -> Result<(), &'static str> {
    DRIVER_RUNTIME.lock().check_driver_cap(id, required)
}

/// Set capabilities for a driver (by ID).
pub fn set_capabilities(id: DriverId, caps: u64) -> bool {
    DRIVER_RUNTIME.lock().set_capabilities(id, caps)
}

/// Get capabilities for a driver (by ID).
pub fn get_capabilities(id: DriverId) -> Option<u64> {
    DRIVER_RUNTIME.lock().get_capabilities(id)
}
