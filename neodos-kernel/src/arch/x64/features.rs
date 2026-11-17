//! CPU feature detection and enablement: SMEP / SMAP / NX (NEODOS-09 / #639).
//!
//! All enablement is CPUID-gated: on hardware that does not advertise a feature
//! the corresponding CR4/EFER bit is left clear, so the kernel still boots (the
//! "si el HW los soporta" clause of the acceptance criteria).
//!
//! This module contains NO inline asm: CR4/CPUID access goes through the
//! `hal::safe` wrappers (HAL-RAW-SAFE: asm stays in `hal/raw`).

use core::sync::atomic::{AtomicU8, Ordering};
use crate::hal::safe::{cpuid, read_cr4, write_cr4, CR4_SMAP, CR4_SMEP, EFER_NXE};

pub const F_SMEP: u8 = 1 << 0;
pub const F_SMAP: u8 = 1 << 1;
pub const F_NX: u8 = 1 << 2; // CPU advertises NX (CPUID.80000001h:EDX[20])
pub const F_NXE: u8 = 1 << 3; // EFER.NXE currently set
pub const F_SMEP_ON: u8 = 1 << 4; // CR4.SMEP currently set
pub const F_SMAP_ON: u8 = 1 << 5; // CR4.SMAP currently set

static STATE: AtomicU8 = AtomicU8::new(0);
static DETECTED: AtomicU8 = AtomicU8::new(0);

#[inline]
pub fn state() -> u8 { STATE.load(Ordering::Relaxed) }
#[inline]
pub fn supported() -> u8 { DETECTED.load(Ordering::Relaxed) }
#[inline]
pub fn smep_supported() -> bool { supported() & F_SMEP != 0 }
#[inline]
pub fn smap_supported() -> bool { supported() & F_SMAP != 0 }
#[inline]
pub fn nx_supported() -> bool { supported() & F_NX != 0 }
#[inline]
pub fn smep_enabled() -> bool { state() & F_SMEP_ON != 0 }
#[inline]
pub fn smap_enabled() -> bool { state() & F_SMAP_ON != 0 }
#[inline]
pub fn nx_enabled() -> bool { state() & F_NXE != 0 }

/// Detect CPU features once (BSP). Read-only; safe to call before enabling.
pub fn detect() {
    let mut f = 0u8;
    let (max_leaf, _, _, _) = cpuid(0, 0);
    if max_leaf >= 7 {
        let (_, ebx7, _, _) = cpuid(7, 0);
        if ebx7 & (1 << 7) != 0 { f |= F_SMEP; }
        if ebx7 & (1 << 20) != 0 { f |= F_SMAP; }
    }
    let (max_ext, _, _, _) = cpuid(0x8000_0000, 0);
    if max_ext >= 0x8000_0001 {
        let (_, _, _, edx_ext) = cpuid(0x8000_0001, 0);
        if edx_ext & (1 << 20) != 0 { f |= F_NX; }
    }
    DETECTED.store(f, Ordering::Relaxed);
    crate::serial_println!(
        "[CPU_FEAT] supported: SMEP={} SMAP={} NX={}",
        f & F_SMEP != 0, f & F_SMAP != 0, f & F_NX != 0);
}

/// Reflect the *live* CR4/EFER bits into `state()` and log them.
pub fn refresh_state() {
    let cr4 = read_cr4();
    let efer = crate::hal::safe::msr::Efer::read();
    let mut s = 0u8;
    if cr4 & CR4_SMEP != 0 { s |= F_SMEP_ON; }
    if cr4 & CR4_SMAP != 0 { s |= F_SMAP_ON; }
    if efer & EFER_NXE != 0 { s |= F_NXE; }
    STATE.store(s, Ordering::Relaxed);
    crate::serial_println!(
        "[CPU_FEAT] enabled: SMEP={} SMAP={} EFER.NXE={}",
        s & F_SMEP_ON != 0, s & F_SMAP_ON != 0, s & F_NXE != 0);
}

/// Enable SMEP / SMAP / EFER.NXE on the current CPU, gated by CPUID support.
/// Safe to call on BSP and on each AP after it reaches long mode.
pub unsafe fn enable_on_this_cpu() {
    let sup = supported();
    let mut cr4 = read_cr4();
    if sup & F_SMEP != 0 { cr4 |= CR4_SMEP; }
    if sup & F_SMAP != 0 { cr4 |= CR4_SMAP; }
    write_cr4(cr4);
    if sup & F_NX != 0 {
        let efer = crate::hal::safe::msr::Efer::read();
        if efer & EFER_NXE == 0 {
            crate::hal::safe::msr::Efer::write(efer | EFER_NXE);
        }
    }
    refresh_state();
}
