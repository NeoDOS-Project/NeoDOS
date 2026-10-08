//! Module registry and the shared scaffold stub session.

mod about;
mod keyboard;
mod locale;
mod power;
mod system;

pub use about::AboutModule;
pub use keyboard::KeyboardModule;
pub use locale::LocaleModule;
pub use power::PowerModule;
pub use system::SystemModule;

use alloc::{boxed::Box, vec, vec::Vec};

use crate::model::{Intent, Text, View};
use crate::module::{ModuleSession, Transition};
use crate::platform::CfgPlatform;

/// Minimal navigable placeholder shared by the module stubs.
///
/// Issues #323–#326 replace the session bodies with real screens; the module
/// registry and navigation plumbing do not change.
pub(crate) struct StubSession {
    title_key: u32,
    body: Vec<Text>,
}

impl StubSession {
    pub(crate) fn new(title_key: u32, desc_key: u32) -> Self {
        StubSession {
            title_key,
            body: vec![
                Text::Key(desc_key),
                Text::Key(crate::i18n_keys::MODULE_PENDING),
                Text::Key(crate::i18n_keys::PRESS_KEY),
            ],
        }
    }
}

impl ModuleSession for StubSession {
    fn view(&self, _env: &dyn CfgPlatform) -> View {
        View::Message {
            title: Text::Key(self.title_key),
            body: self.body.clone(),
        }
    }

    fn update(&mut self, _env: &dyn CfgPlatform, intent: Intent) -> Transition {
        // An informational screen is dismissed by any key (Esc is handled by
        // the App before reaching here).
        match intent {
            Intent::Tick => Transition::Stay,
            _ => Transition::Back,
        }
    }
}

/// Helper for the stub `create()` implementations.
pub(crate) fn boxed_stub(title_key: u32, desc_key: u32) -> Box<dyn ModuleSession> {
    Box::new(StubSession::new(title_key, desc_key))
}
