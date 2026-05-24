// src/drivers/nem/policy.rs
//! Policy engine for NEM drivers with ABI validation.
//! Ensures only known, well‑formed binaries with compatible ABI are accepted.

use crate::nem::{ParsedNem, ABI_MIN_VALID, ABI_MAX_VALID};

/// Validate a parsed NEM driver according to the current policy.
///
/// Returns `Ok(())` if the driver is allowed, otherwise an error string.
pub fn validate_driver(parsed: &ParsedNem) -> Result<(), &'static str> {
    // 1️⃣ Allowed driver types (the three first ones we ship)
    match parsed.driver_type as u8 {
        0 | 1 | 2 => {},
        _ => return Err("Disallowed driver type by policy"),
    }

    // 2️⃣ Require v2 header for boot drivers (ABI fields mandatory)
    if parsed.category as u8 <= 1 && !parsed.is_v2 {
        return Err("Boot/System drivers must use NEM v2 format with ABI fields");
    }

    Ok(())
}

/// Validate ABI compatibility for a parsed NEM driver.
/// Only meaningful for v2 headers (v1 has no ABI fields).
pub fn validate_abi(parsed: &ParsedNem) -> Result<(), &'static str> {
    if !parsed.is_v2 {
        // v1 drivers: accept if type is allowed (relaxed policy for test drivers)
        return Ok(());
    }

    if parsed.abi_min == 0 || parsed.abi_target == 0 || parsed.abi_max == 0 {
        return Err("ABI fields cannot be zero");
    }

    // Kernel ABI window: [ABI_MIN_VALID, ABI_MAX_VALID]
    // Driver's required ABI window: [abi_min, abi_max]
    //
    // Compatible if:
    //   driver.abi_min <= ABI_MAX_VALID  (driver is not too new)
    //   driver.abi_max >= ABI_MIN_VALID  (kernel is not too new for driver)
    //   driver.abi_target within [ABI_MIN_VALID, ABI_MAX_VALID]

    if parsed.abi_min > ABI_MAX_VALID {
        return Err("Driver requires newer ABI than kernel supports (abi_min > ABI_MAX_VALID)");
    }

    if parsed.abi_max < ABI_MIN_VALID {
        return Err("Driver ABI is too old for this kernel (abi_max < ABI_MIN_VALID)");
    }

    if parsed.abi_target < ABI_MIN_VALID || parsed.abi_target > ABI_MAX_VALID {
        return Err("Driver ABI target is outside kernel's supported range");
    }

    Ok(())
}
