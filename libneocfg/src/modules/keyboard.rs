//! Keyboard layout module (stub).
//!
//! Full implementation is tracked in #324.

use alloc::boxed::Box;

use crate::i18n_keys as k;
use crate::module::{CfgModule, ModuleId, ModuleSession};
use crate::modules::boxed_stub;

/// Keyboard layout selection.
pub struct KeyboardModule;

impl CfgModule for KeyboardModule {
    fn id(&self) -> ModuleId {
        ModuleId::Keyboard
    }

    fn title_key(&self) -> u32 {
        k::MODULE_KEYBOARD_NAME
    }

    fn description_key(&self) -> u32 {
        k::MODULE_KEYBOARD_DESC
    }

    fn create(&self) -> Box<dyn ModuleSession> {
        boxed_stub(k::MODULE_KEYBOARD_NAME, k::MODULE_KEYBOARD_DESC)
    }
}
