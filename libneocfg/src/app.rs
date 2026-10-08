//! The single navigation loop shared by every UI.
//!
//! `App` renders the main menu, dispatches to module sessions, and translates
//! [`Intent::Back`] at the top level into "exit NeoCfg".

use alloc::boxed::Box;

use crate::i18n::Translator;
use crate::i18n_keys as k;
use crate::model::{Intent, MenuItem, Text, View};
use crate::module::{CfgModule, ModuleSession, Transition};
use crate::platform::{CfgError, CfgPlatform};
use crate::ui::CfgUi;

/// Configuration panel application driver.
pub struct App<'a> {
    modules: &'a [&'a dyn CfgModule],
    translator: &'a dyn Translator,
    ui: &'a mut dyn CfgUi,
    selected: usize,
}

impl<'a> App<'a> {
    /// Build a driver over a module registry, a translator and a UI backend.
    pub fn new(
        modules: &'a [&'a dyn CfgModule],
        translator: &'a dyn Translator,
        ui: &'a mut dyn CfgUi,
    ) -> Self {
        App {
            modules,
            translator,
            ui,
            selected: 0,
        }
    }

    /// Run the main loop until the user quits.
    pub fn run(&mut self, env: &dyn CfgPlatform) -> Result<(), CfgError> {
        loop {
            let view = self.main_menu_view();
            match self.ui.present(&view) {
                Intent::Quit | Intent::Back => return Ok(()),
                Intent::Up => {
                    if self.selected > 0 {
                        self.selected -= 1;
                    }
                }
                Intent::Down => {
                    if self.selected + 1 < self.modules.len() {
                        self.selected += 1;
                    }
                }
                Intent::Activate => {
                    let idx = self.selected;
                    if idx < self.modules.len()
                        && self.run_module(self.modules[idx], env)? == Transition::Exit
                    {
                        return Ok(());
                    }
                }
                Intent::Select(idx) if idx < self.modules.len() => {
                    if self.run_module(self.modules[idx], env)? == Transition::Exit {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    }

    /// Build the main-menu [`View`] from the registry.
    pub fn main_menu_view(&self) -> View {
        let items = self
            .modules
            .iter()
            .map(|m| MenuItem::new(Text::Key(m.title_key())))
            .collect();
        View::Menu {
            title: Text::Key(k::NEOCFG_TITLE),
            items,
            selected: self.selected,
            footer: Some(Text::Key(k::NEOCFG_SELECT_HINT)),
        }
    }

    /// Drive one module session to completion.
    fn run_module(
        &mut self,
        module: &dyn CfgModule,
        env: &dyn CfgPlatform,
    ) -> Result<Transition, CfgError> {
        let mut session: Box<dyn ModuleSession> = module.create();
        // Touch the translator so the field is exercised for all UIs; the view
        // data itself carries ids, the UI resolves them.
        let _ = self.translator;
        loop {
            let view = session.view(env);
            match self.ui.present(&view) {
                Intent::Back => return Ok(Transition::Back),
                Intent::Quit => return Ok(Transition::Exit),
                intent => match session.update(env, intent) {
                    Transition::Stay => {}
                    other => return Ok(other),
                },
            }
        }
    }
}
