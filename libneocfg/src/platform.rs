//! Data/effects seam: **all** system access goes through [`CfgPlatform`].
//!
//! The types here are pure (owned `String`s, plain integers) so the core can be
//! host-tested. The NeoDOS target implements the trait in
//! `userbin/neocfg/src/neodos_platform.rs` over `libneodos::syscall`.

use alloc::{string::String, vec::Vec};

/// Error returned by any [`CfgPlatform`] operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgError {
    /// The required subsystem (Power Manager, i18n runtime) is not present.
    ModuleUnavailable,
    /// Access was denied by the security subsystem.
    PermissionDenied,
    /// Raw syscall error code.
    Io(i64),
    /// The user cancelled the operation.
    Cancelled,
}

impl From<i64> for CfgError {
    fn from(code: i64) -> Self {
        CfgError::Io(code)
    }
}

/// Kernel version string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: String,
}

/// Physical memory usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemInfo {
    pub total_kib: u64,
    pub used_kib: u64,
    pub free_kib: u64,
}

/// CPU summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    pub vendor: String,
    pub brand: String,
    pub family: u32,
    pub model: u32,
    pub cores: u32,
}

/// A mounted volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveInfo {
    pub letter: u8,
    pub fs_type: String,
    pub label: String,
    pub total_kib: u64,
}

/// A registered service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInfo {
    pub name: String,
    pub running: bool,
}

/// Power plan (mirrors the Power Manager's plan index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPlan {
    Balanced,
    Performance,
    PowerSaver,
}

impl PowerPlan {
    /// Plan index as used by the Power Manager ABI.
    pub fn index(self) -> u32 {
        match self {
            PowerPlan::Balanced => 0,
            PowerPlan::Performance => 1,
            PowerPlan::PowerSaver => 2,
        }
    }

    /// Parse a plan index; unknown values fall back to `Balanced`.
    pub fn from_index(index: u32) -> Self {
        match index {
            1 => PowerPlan::Performance,
            2 => PowerPlan::PowerSaver,
            _ => PowerPlan::Balanced,
        }
    }
}

/// Optional Power Manager operations. `None` while the subsystem is absent.
pub trait PowerOps {
    fn active_plan(&self) -> Result<PowerPlan, CfgError>;
    fn set_active_plan(&self, plan: PowerPlan) -> Result<(), CfgError>;
    fn shutdown(&self) -> Result<(), CfgError>;
    fn reboot(&self) -> Result<(), CfgError>;
}

/// Optional locale/i18n operations. `None` while the runtime is absent.
pub trait LocaleOps {
    fn active_locale(&self) -> Result<String, CfgError>;
    fn available_locales(&self) -> Result<Vec<String>, CfgError>;
    fn set_locale(&self, tag: &str) -> Result<(), CfgError>;
}

/// Everything NeoCfg needs from the host system.
pub trait CfgPlatform {
    fn version(&self) -> Result<VersionInfo, CfgError>;
    fn memory(&self) -> Result<MemInfo, CfgError>;
    fn cpu(&self) -> Result<CpuInfo, CfgError>;
    fn drives(&self) -> Result<Vec<DriveInfo>, CfgError>;
    fn process_count(&self) -> Result<u32, CfgError>;
    fn services(&self) -> Result<Vec<ServiceInfo>, CfgError>;

    fn keyboard_layout(&self) -> Result<String, CfgError>;
    fn set_keyboard_layout(&self, layout: &str) -> Result<(), CfgError>;

    /// Power Manager operations, or `None` when the subsystem is not present.
    fn power(&self) -> Option<&dyn PowerOps>;
    /// Locale/i18n operations, or `None` when the runtime is not present.
    fn locale(&self) -> Option<&dyn LocaleOps>;
}
