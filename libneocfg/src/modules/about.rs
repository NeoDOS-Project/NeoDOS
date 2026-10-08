//! About module (stub).
//!
//! Full implementation is tracked in #325.

use alloc::boxed::Box;

use crate::i18n_keys as k;
use crate::module::{CfgModule, ModuleId, ModuleSession};
use crate::modules::boxed_stub;

/// Version information.
pub struct AboutModule;

impl CfgModule for AboutModule {
    fn id(&self) -> ModuleId {
        ModuleId::About
    }

    fn title_key(&self) -> u32 {
        k::MODULE_ABOUT_NAME
    }

    fn description_key(&self) -> u32 {
        k::MODULE_ABOUT_DESC
    }

    fn create(&self) -> Box<dyn ModuleSession> {
        boxed_stub(k::MODULE_ABOUT_NAME, k::MODULE_ABOUT_DESC)
    }
}
