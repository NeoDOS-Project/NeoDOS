use crate::hal::safe;

// MSR address constants (canonical reference values).
#[allow(dead_code)]
pub const IA32_GS_BASE: u32 = 0xC0000101;
#[allow(dead_code)]
pub const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
#[allow(dead_code)]
pub const IA32_FS_BASE: u32 = 0xC0000100;
#[allow(dead_code)]
pub const IA32_APIC_BASE: u32 = 0x0000001B;
#[allow(dead_code)]
pub const IA32_TSC_AUX: u32 = 0xC0000103;
#[allow(dead_code)]
pub const IA32_SYSENTER_CS: u32 = 0x174;
#[allow(dead_code)]
pub const IA32_SYSENTER_ESP: u32 = 0x175;
#[allow(dead_code)]
pub const IA32_SYSENTER_EIP: u32 = 0x176;
#[allow(dead_code)]
pub const IA32_EFER: u32 = 0xC0000080;
#[allow(dead_code)]
pub const IA32_MISC_ENABLE: u32 = 0x1A0;

/// Set the GS base address on the current CPU.
///
/// # Safety
/// The caller must ensure `base` points to a valid, aligned KPRCB page
/// that will remain valid for the lifetime of the GS base setting.
#[inline]
pub unsafe fn write_gs_base(base: u64) {
    safe::GsBase::write(base);
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
