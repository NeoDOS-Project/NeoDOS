//! System information module (stub).
//!
//! Full implementation is tracked in #323.

use alloc::boxed::Box;

use crate::i18n_keys as k;
use crate::module::{CfgModule, ModuleId, ModuleSession};
use crate::modules::boxed_stub;

/// Read-only system information.
pub struct SystemModule;

impl CfgModule for SystemModule {
    fn id(&self) -> ModuleId {
        ModuleId::System
    }

    fn title_key(&self) -> u32 {
        k::MODULE_SYSTEM_NAME
    }

    fn description_key(&self) -> u32 {
        k::MODULE_SYSTEM_DESC
    }

    fn create(&self) -> Box<dyn ModuleSession> {
        boxed_stub(k::MODULE_SYSTEM_NAME, k::MODULE_SYSTEM_DESC)
    }
}
