//! Safe(type-safe) CPU control-register and SMAP wrappers (HAL).
//!
//! All inline asm stays in `hal::raw`; this module is the only surface the rest
//! of the kernel (arch, syscall, …) should use for CR4 / STAC / CLAC / CPUID
//! (HAL-RAW-SAFE rule: zero asm outside `hal/`).

use crate::hal::raw;

/// CR4.SMAP (supervisor mode access prevention).
pub const CR4_SMAP: u64 = 1 << 21;
/// CR4.SMEP (supervisor mode execution prevention).
pub const CR4_SMEP: u64 = 1 << 20;
/// EFER.NXE (no-execute enable).
pub const EFER_NXE: u64 = 1 << 11;

#[inline]
pub fn read_cr4() -> u64 {
    unsafe { raw::raw_read_cr4() }
}

/// Write CR4. Privileged: the caller must only set bits the CPU advertises.
#[inline]
pub unsafe fn write_cr4(val: u64) {
    raw::raw_write_cr4(val);
}

/// Execute CPUID. `leaf`/`subleaf` in, `(eax, ebx, ecx, edx)` out.
#[inline]
pub fn cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    unsafe { raw::raw_cpuid(leaf, subleaf) }
}

/// Set RFLAGS.AC so the supervisor may touch user pages while SMAP is active.
/// #UD if SMAP is unsupported — callers must gate on [`smap_is_active`].
#[inline]
pub unsafe fn stac() {
    raw::raw_stac();
}

/// Clear RFLAGS.AC (SMAP).
#[inline]
pub unsafe fn clac() {
    raw::raw_clac();
}

/// Whether SMAP is currently active (CR4.SMAP set).
#[inline]
pub fn smap_is_active() -> bool {
    read_cr4() & CR4_SMAP != 0
}

/// Run `f` with RFLAGS.AC set when SMAP is active, so the kernel may legitimately
/// touch user pages (a validated syscall copy). No-op on CPUs without SMAP.
///
/// This keeps the SMAP mechanism inside the HAL: callers pass the closure that
/// performs validation + the user access, and the AC window is handled here.
#[inline]
pub fn with_user_access<R>(f: impl FnOnce() -> R) -> R {
    if smap_is_active() {
        unsafe { stac(); }
        let r = f();
        unsafe { clac(); }
        r
    } else {
        f()
    }
}
