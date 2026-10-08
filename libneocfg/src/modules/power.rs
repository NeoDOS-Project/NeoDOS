//! Power management module (stub).
//!
//! The session probes `env.power()` at view time, so it degrades gracefully to
//! the `not available` message until the Power Manager lands (see #326 and
//! `docs/services/power-manager.md`). The ready path (plan select, restore
//! defaults, shutdown, reboot) is implemented behind [`PowerOps`] without any
//! change to this module.

use alloc::{boxed::Box, vec::Vec};

use crate::i18n_keys as k;
use crate::model::{Intent, Text, View};
use crate::module::{CfgModule, ModuleId, ModuleSession, Transition};
use crate::platform::CfgPlatform;

/// Power plans / actions.
pub struct PowerModule;

impl CfgModule for PowerModule {
    fn id(&self) -> ModuleId {
        ModuleId::Power
    }

    fn title_key(&self) -> u32 {
        k::MODULE_POWER_NAME
    }

    fn description_key(&self) -> u32 {
        k::MODULE_POWER_DESC
    }

    fn create(&self) -> Box<dyn ModuleSession> {
        Box::new(PowerSession)
    }
}

struct PowerSession;

impl ModuleSession for PowerSession {
    fn view(&self, env: &dyn CfgPlatform) -> View {
        let mut body: Vec<Text> = Vec::new();
        if env.power().is_none() {
            body.push(Text::Key(k::POWER_NOT_AVAILABLE));
        } else {
            body.push(Text::Key(k::MODULE_POWER_DESC));
            body.push(Text::Key(k::MODULE_PENDING));
        }
        body.push(Text::Key(k::PRESS_KEY));
        View::Message {
            title: Text::Key(k::MODULE_POWER_NAME),
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
