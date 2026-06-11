use crate::hal::raw;

pub trait Msr {
    type Value: Copy;
    const ADDR: u32;
    const IS_SAFE: bool;

    fn from_raw(raw: u64) -> Self::Value;
    fn into_raw(val: Self::Value) -> u64;
}

pub struct GsBase;
impl Msr for GsBase {
    type Value = u64;
    const ADDR: u32 = 0xC0000101;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct KernelGsBase;
impl Msr for KernelGsBase {
    type Value = u64;
    const ADDR: u32 = 0xC0000102;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct FsBase;
impl Msr for FsBase {
    type Value = u64;
    const ADDR: u32 = 0xC0000100;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct ApicBaseMsr;
impl Msr for ApicBaseMsr {
    type Value = u64;
    const ADDR: u32 = 0x0000001B;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct Efer;
impl Msr for Efer {
    type Value = u64;
    const ADDR: u32 = 0xC0000080;
    const IS_SAFE: bool = false;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct MiscEnable;
impl Msr for MiscEnable {
    type Value = u64;
    const ADDR: u32 = 0x1A0;
    const IS_SAFE: bool = false;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct SysenterCs;
impl Msr for SysenterCs {
    type Value = u64;
    const ADDR: u32 = 0x174;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct SysenterEsp;
impl Msr for SysenterEsp {
    type Value = u64;
    const ADDR: u32 = 0x175;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct SysenterEip;
impl Msr for SysenterEip {
    type Value = u64;
    const ADDR: u32 = 0x176;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct TscAux;
impl Msr for TscAux {
    type Value = u64;
    const ADDR: u32 = 0xC0000103;
    const IS_SAFE: bool = true;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub struct Ia32FeatureControl;
impl Msr for Ia32FeatureControl {
    type Value = u64;
    const ADDR: u32 = 0x0000003A;
    const IS_SAFE: bool = false;
    fn from_raw(raw: u64) -> u64 { raw }
    fn into_raw(val: u64) -> u64 { val }
}

pub const GS_BASE: GsBase = GsBase;
pub const KERNEL_GS_BASE: KernelGsBase = KernelGsBase;
pub const FS_BASE: FsBase = FsBase;
pub const APIC_BASE_MSR: ApicBaseMsr = ApicBaseMsr;
pub const EFER: Efer = Efer;
pub const MISC_ENABLE: MiscEnable = MiscEnable;
pub const SYSENTER_CS: SysenterCs = SysenterCs;
pub const SYSENTER_ESP: SysenterEsp = SysenterEsp;
pub const SYSENTER_EIP: SysenterEip = SysenterEip;
pub const TSC_AUX: TscAux = TscAux;
pub const IA32_FEATURE_CONTROL: Ia32FeatureControl = Ia32FeatureControl;

#[inline]
pub fn read_msr<T: Msr>(_msr: &T) -> T::Value {
    let raw_val = unsafe { raw::raw_read_msr(T::ADDR) };
    T::from_raw(raw_val)
}

#[inline]
pub unsafe fn write_msr<T: Msr>(_msr: &T, val: T::Value) {
    let raw_val = T::into_raw(val);
    raw::raw_write_msr(T::ADDR, raw_val);
}

impl GsBase {
    pub fn read() -> u64 {
        read_msr(&GS_BASE)
    }

    pub unsafe fn write(val: u64) {
        write_msr(&GS_BASE, val);
    }
}

pub struct ApicBase;

impl ApicBase {
    pub fn read() -> u64 {
        let raw = read_msr(&APIC_BASE_MSR);
        raw & 0xFFFF_FFFF_FFFF_F000
    }

    pub fn is_enabled() -> bool {
        let raw = read_msr(&APIC_BASE_MSR);
        (raw & (1 << 11)) != 0
    }

    pub fn is_bsp() -> bool {
        let raw = read_msr(&APIC_BASE_MSR);
        (raw & (1 << 8)) != 0
    }
}

impl Efer {
    pub unsafe fn write(val: u64) {
        write_msr(&EFER, val);
    }

    pub fn read() -> u64 {
        read_msr(&EFER)
    }
}

pub fn read_cr2() -> u64 {
    unsafe { raw::raw_read_cr2() }
}
