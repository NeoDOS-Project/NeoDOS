pub mod x64;
pub mod raw;
pub mod safe;
pub mod pci;
pub mod tests;

pub use x64::*;
pub use safe::read_cr2;

/// Check if the CPU supports RDRAND.
pub fn has_rdrand() -> bool {
    unsafe { raw::raw_has_rdrand() }
}

/// Generate a 64-bit random value using RDRAND.
/// Returns `None` if the CPU doesn't support RDRAND or if the instruction fails.
pub fn rdrand() -> Option<u64> {
    if !has_rdrand() {
        return None;
    }
    // Retry up to 10 times per Intel recommendation
    for _ in 0..10 {
        if let Some(val) = unsafe { raw::raw_rdrand() } {
            return Some(val);
        }
    }
    None
}
