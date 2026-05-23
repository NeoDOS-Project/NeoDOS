// src/drivers/nem/policy.rs
//! Minimal policy engine for NEM drivers.
//! Ensures only known, well‑formed binaries are accepted.

use crate::nem::ParsedNem;

/// Validate a parsed NEM driver according to the current policy.
///
/// Returns `Ok(())` if the driver is allowed, otherwise an error string.
pub fn validate_driver(parsed: &ParsedNem) -> Result<(), &'static str> {
    // 1️⃣ Allowed driver types (the three first ones we ship)
    match parsed.driver_type as u8 {
        0 | 1 | 2 => Ok(()), // Null, Echo, Lifecycle
        _ => Err("Disallowed driver type by policy"),
    }
}
