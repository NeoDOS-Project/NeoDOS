use crate::hal::raw;
use crate::hal::safe;

/// IA32_GS_BASE — GS segment base address (used for per-CPU data).
pub const IA32_GS_BASE: u32 = 0xC0000101;

/// IA32_KERNEL_GS_BASE — GS base swap target (swapgs instruction).
pub const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;

/// IA32_FS_BASE — FS segment base address.
pub const IA32_FS_BASE: u32 = 0xC0000100;

/// IA32_APIC_BASE — Local APIC base address and control.
pub const IA32_APIC_BASE: u32 = 0x0000001B;

/// IA32_TSC_AUX — TSC auxiliary (RDX:RAX) for RDTSCP.
pub const IA32_TSC_AUX: u32 = 0xC0000103;

/// IA32_SYSENTER_CS — SYSENTER CS (legacy fast syscall).
pub const IA32_SYSENTER_CS: u32 = 0x174;
/// IA32_SYSENTER_ESP — SYSENTER ESP.
pub const IA32_SYSENTER_ESP: u32 = 0x175;
/// IA32_SYSENTER_EIP — SYSENTER EIP.
pub const IA32_SYSENTER_EIP: u32 = 0x176;

/// IA32_EFER — Extended Feature Enable Register.
pub const IA32_EFER: u32 = 0xC0000080;

/// IA32_MISC_ENABLE — Miscellaneous enable bits.
pub const IA32_MISC_ENABLE: u32 = 0x1A0;

/// Read a 64-bit value from the specified MSR on the current CPU.
///
/// # Safety
/// The caller must ensure the MSR is valid for the current CPU and
/// that reading it does not violate system invariants.
#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    raw::raw_read_msr(msr)
}

/// Write a 64-bit value to the specified MSR on the current CPU.
///
/// # Safety
/// The caller must ensure the MSR is valid for the current CPU and
/// that writing the given value does not violate system invariants.
#[inline]
pub unsafe fn wrmsr(msr: u32, val: u64) {
    raw::raw_write_msr(msr, val);
}

/// Read the current GS base address.
#[inline]
pub fn read_gs_base() -> u64 {
    safe::GsBase::read()
}

/// Set the GS base address on the current CPU.
///
/// # Safety
/// The caller must ensure `base` points to a valid, aligned KPRCB page
/// that will remain valid for the lifetime of the GS base setting.
#[inline]
pub unsafe fn write_gs_base(base: u64) {
    safe::GsBase::write(base);
}

/// Read the kernel GS base (used with `swapgs`).
#[inline]
pub fn read_kernel_gs_base() -> u64 {
    unsafe { raw::raw_read_msr(IA32_KERNEL_GS_BASE) }
}

/// Set the kernel GS base.
#[inline]
pub unsafe fn write_kernel_gs_base(base: u64) {
    raw::raw_write_msr(IA32_KERNEL_GS_BASE, base);
}

/// Read the FS base address.
#[inline]
pub fn read_fs_base() -> u64 {
    unsafe { raw::raw_read_msr(IA32_FS_BASE) }
}

/// Set the FS base address.
#[inline]
pub unsafe fn write_fs_base(base: u64) {
    raw::raw_write_msr(IA32_FS_BASE, base);
}

/// Read the Local APIC base from IA32_APIC_BASE MSR.
/// Returns the physical address with low 12 bits masked off.
pub fn read_apic_base_msr() -> u64 {
    safe::ApicBase::read()
}

/// Check if the current CPU is the Bootstrap Processor (BSP).
pub fn is_bsp() -> bool {
    safe::ApicBase::is_bsp()
}

/// Read the RDTSC timestamp counter (low 64 bits).
#[inline]
pub fn rdtsc() -> u64 {
    unsafe { raw::raw_read_tsc() }
}

/// Read the RDTSCP timestamp counter (low 64 bits + aux in ECX).
#[inline]
pub fn rdtscp() -> (u64, u32) {
    unsafe { raw::raw_read_tscp() }
}
