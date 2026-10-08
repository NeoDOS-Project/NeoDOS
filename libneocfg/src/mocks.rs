//! Test doubles for the three seams, usable from host tests.
//!
//! These are compiled into the library (not `#[cfg(test)]`-gated) so both unit
//! tests inside the crate and integration tests under `tests/` can use them.

use alloc::{borrow::ToOwned, collections::BTreeMap, string::String, vec, vec::Vec};
use core::cell::{Cell, RefCell};

use crate::i18n::Translator;
use crate::model::{Intent, View};
use crate::platform::{
    CfgError, CfgPlatform, CpuInfo, DriveInfo, LocaleOps, MemInfo, PowerOps, PowerPlan,
    ServiceInfo, VersionInfo,
};
use crate::ui::CfgUi;

/// A [`Translator`] backed by an explicit id → text table.
#[derive(Default)]
pub struct MockTranslator {
    table: BTreeMap<u32, String>,
}

impl MockTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a message.
    pub fn with(mut self, id: u32, text: &str) -> Self {
        self.table.insert(id, text.to_owned());
        self
    }

    pub fn set(&mut self, id: u32, text: &str) {
        self.table.insert(id, text.to_owned());
    }
}

impl Translator for MockTranslator {
    fn tr(&self, id: u32) -> &str {
        self.table.get(&id).map(|s| s.as_str()).unwrap_or("?")
    }
}

/// Power-op recorder.
#[derive(Default)]
pub struct MockPower {
    plan: Cell<PowerPlanCell>,
    pub shutdown_called: Cell<bool>,
    pub reboot_called: Cell<bool>,
}

// `Cell<PowerPlan>` would need `PowerPlan: Copy + Default`; keep a tiny wrapper
// so `MockPower: Default` works without deriving Default on the enum.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PowerPlanCell(u32);

impl Default for PowerPlanCell {
    fn default() -> Self {
        PowerPlanCell(PowerPlan::Balanced.index())
    }
}

impl MockPower {
    pub fn new(plan: PowerPlan) -> Self {
        MockPower {
            plan: Cell::new(PowerPlanCell(plan.index())),
            shutdown_called: Cell::new(false),
            reboot_called: Cell::new(false),
        }
    }
}

impl PowerOps for MockPower {
    fn active_plan(&self) -> Result<PowerPlan, CfgError> {
        Ok(PowerPlan::from_index(self.plan.get().0))
    }

    fn set_active_plan(&self, plan: PowerPlan) -> Result<(), CfgError> {
        self.plan.set(PowerPlanCell(plan.index()));
        Ok(())
    }

    fn shutdown(&self) -> Result<(), CfgError> {
        self.shutdown_called.set(true);
        Ok(())
    }

    fn reboot(&self) -> Result<(), CfgError> {
        self.reboot_called.set(true);
        Ok(())
    }
}

/// Locale-op recorder.
pub struct MockLocale {
    active: RefCell<String>,
    available: Vec<String>,
    pub writes: RefCell<Vec<String>>,
}

impl MockLocale {
    pub fn new(active: &str, available: &[&str]) -> Self {
        MockLocale {
            active: RefCell::new(active.to_owned()),
            available: available.iter().map(|s| String::from(*s)).collect(),
            writes: RefCell::new(Vec::new()),
        }
    }
}

impl LocaleOps for MockLocale {
    fn active_locale(&self) -> Result<String, CfgError> {
        Ok(self.active.borrow().clone())
    }

    fn available_locales(&self) -> Result<Vec<String>, CfgError> {
        Ok(self.available.clone())
    }

    fn set_locale(&self, tag: &str) -> Result<(), CfgError> {
        *self.active.borrow_mut() = tag.to_owned();
        self.writes.borrow_mut().push(tag.to_owned());
        Ok(())
    }
}

/// In-memory [`CfgPlatform`] with sensible defaults.
pub struct MockPlatform {
    pub version: String,
    pub mem: MemInfo,
    pub cpu: CpuInfo,
    pub drives: Vec<DriveInfo>,
    pub processes: u32,
    pub services: Vec<ServiceInfo>,
    pub keyboard: RefCell<String>,
    pub power: Option<MockPower>,
    pub locale: Option<MockLocale>,
    /// When set, `version()` fails with this error (to exercise error paths).
    pub version_error: Option<CfgError>,
}

impl Default for MockPlatform {
    fn default() -> Self {
        MockPlatform {
            version: "NeoDOS v0.0.0".to_owned(),
            mem: MemInfo {
                total_kib: 1024 * 1024,
                used_kib: 256 * 1024,
                free_kib: 768 * 1024,
            },
            cpu: CpuInfo {
                vendor: "MockVendor".to_owned(),
                brand: "MockCPU".to_owned(),
                family: 6,
                model: 42,
                cores: 2,
            },
            drives: vec![DriveInfo {
                letter: b'C',
                fs_type: "NE2".to_owned(),
                label: "NeoDOS".to_owned(),
                total_kib: 256 * 1024,
            }],
            processes: 7,
            services: vec![ServiceInfo {
                name: "mock-svc".to_owned(),
                running: true,
            }],
            keyboard: RefCell::new("us".to_owned()),
            power: None,
            locale: None,
            version_error: None,
        }
    }
}

impl MockPlatform {
    pub fn with_power(plan: PowerPlan) -> Self {
        let mut p = Self::default();
        p.power = Some(MockPower::new(plan));
        p
    }

    pub fn with_locale(active: &str, available: &[&str]) -> Self {
        let mut p = Self::default();
        p.locale = Some(MockLocale::new(active, available));
        p
    }
}

impl CfgPlatform for MockPlatform {
    fn version(&self) -> Result<VersionInfo, CfgError> {
        if let Some(err) = &self.version_error {
            return Err(err.clone());
        }
        Ok(VersionInfo {
            version: self.version.clone(),
        })
    }

    fn memory(&self) -> Result<MemInfo, CfgError> {
        Ok(self.mem.clone())
    }

    fn cpu(&self) -> Result<CpuInfo, CfgError> {
        Ok(self.cpu.clone())
    }

    fn drives(&self) -> Result<Vec<DriveInfo>, CfgError> {
        Ok(self.drives.clone())
    }

    fn process_count(&self) -> Result<u32, CfgError> {
        Ok(self.processes)
    }

    fn services(&self) -> Result<Vec<ServiceInfo>, CfgError> {
        Ok(self.services.clone())
    }

    fn keyboard_layout(&self) -> Result<String, CfgError> {
        Ok(self.keyboard.borrow().clone())
    }

    fn set_keyboard_layout(&self, layout: &str) -> Result<(), CfgError> {
        *self.keyboard.borrow_mut() = layout.to_owned();
        Ok(())
    }

    fn power(&self) -> Option<&dyn PowerOps> {
        self.power.as_ref().map(|p| p as &dyn PowerOps)
    }

    fn locale(&self) -> Option<&dyn LocaleOps> {
        self.locale.as_ref().map(|l| l as &dyn LocaleOps)
    }
}

/// A [`CfgUi`] that plays back a scripted list of [`Intent`]s and records every
/// [`View`] it is given.
pub struct MockUi {
    script: Vec<Intent>,
    next: usize,
    pub views: Vec<View>,
}

impl MockUi {
    pub fn new(script: Vec<Intent>) -> Self {
        MockUi {
            script,
            next: 0,
            views: Vec::new(),
        }
    }

    /// Intent returned once the script is exhausted.
    pub const FALLBACK: Intent = Intent::Quit;

    pub fn recorded(&self) -> &[View] {
        &self.views
    }
}

impl CfgUi for MockUi {
    fn present(&mut self, view: &View) -> Intent {
        self.views.push(view.clone());
        let intent = self.script.get(self.next).cloned().unwrap_or(Self::FALLBACK);
        self.next += 1;
        intent
    }
}
