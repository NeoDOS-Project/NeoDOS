//! System language module (stub).
//!
//! The session probes `env.locale()` at view time, so it degrades gracefully to
//! the `not available` message until the i18n runtime ships (see #326). The
//! ready path (change language, set default) is implemented behind
//! [`LocaleOps`](crate::LocaleOps) without changing this module.

use alloc::{boxed::Box, vec::Vec};

use crate::i18n_keys as k;
use crate::model::{Intent, Text, View};
use crate::module::{CfgModule, ModuleId, ModuleSession, Transition};
use crate::platform::CfgPlatform;

/// System language.
pub struct LocaleModule;

impl CfgModule for LocaleModule {
    fn id(&self) -> ModuleId {
        ModuleId::Locale
    }

    fn title_key(&self) -> u32 {
        k::MODULE_LOCALE_NAME
    }

    fn description_key(&self) -> u32 {
        k::MODULE_LOCALE_DESC
    }

    fn create(&self) -> Box<dyn ModuleSession> {
        Box::new(LocaleSession)
    }
}

struct LocaleSession;

impl ModuleSession for LocaleSession {
    fn view(&self, env: &dyn CfgPlatform) -> View {
        let mut body: Vec<Text> = Vec::new();
        if env.locale().is_none() {
            body.push(Text::Key(k::LOCALE_NOT_AVAILABLE));
        } else {
            body.push(Text::Key(k::MODULE_LOCALE_DESC));
            body.push(Text::Key(k::MODULE_PENDING));
        }
        body.push(Text::Key(k::PRESS_KEY));
        View::Message {
            title: Text::Key(k::MODULE_LOCALE_NAME),
            body,
        }
    }

    fn update(&mut self, _env: &dyn CfgPlatform, intent: Intent) -> Transition {
        match intent {
            Intent::Tick => Transition::Stay,
            _ => Transition::Back,
        }
    }
}
