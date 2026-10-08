//! About module — static identity/version information (issue #325).
//!
//! Read-only. The platform supplies [`AboutInfo`](crate::AboutInfo); this module
//! only lays it out. Version comes from `\Global\Info\Version`; ABI/arch/NeoFS
//! are compile-time facts supplied by the platform.

use alloc::{boxed::Box, format, vec, vec::Vec};

use crate::i18n_keys as k;
use crate::model::{Field, FieldValue, Intent, Text, View};
use crate::module::{CfgModule, ModuleId, ModuleSession, Transition};
use crate::platform::CfgPlatform;

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
        Box::new(AboutSession)
    }
}

struct AboutSession;

impl ModuleSession for AboutSession {
    fn view(&self, env: &dyn CfgPlatform) -> View {
        let info = match env.about() {
            Ok(info) => info,
            Err(_) => {
                return View::Message {
                    title: Text::Key(k::ABOUT_TITLE),
                    body: vec![Text::Key(k::ABOUT_UNAVAILABLE), Text::Key(k::PRESS_KEY)],
                };
            }
        };

        let mut fields: Vec<Field> = Vec::new();
        fields.push(field(
            k::ABOUT_NEODOS,
            Text::Owned(info.neodos_version),
        ));
        fields.push(field(
            k::ABOUT_ABI,
            Text::Owned(format!("v{} (syscall)", info.syscall_abi)),
        ));
        fields.push(field(k::ABOUT_ARCH, Text::Owned(info.arch)));
        fields.push(field(k::ABOUT_NEOFS, Text::Owned(info.neofs)));
        fields.push(field(
            k::ABOUT_LIBNEODOS,
            Text::Owned(format!("v{} (NXL ABI)", info.libneodos_abi)),
        ));
        if let Some(date) = info.build_date {
            fields.push(field(k::ABOUT_BUILD, Text::Owned(date)));
        }

        View::Detail {
            title: Text::Key(k::ABOUT_TITLE),
            fields,
            footer: Text::Key(k::NEOCFG_BACK),
        }
    }

    fn update(&mut self, _env: &dyn CfgPlatform, intent: Intent) -> Transition {
        // The footer says "Back": any key returns to the main menu (Esc is
        // handled by the App before reaching here).
        match intent {
            Intent::Tick => Transition::Stay,
            _ => Transition::Back,
        }
    }
}

fn field(label_key: u32, value: Text) -> Field {
    Field::new(Text::Key(label_key), FieldValue::Text(value))
}
