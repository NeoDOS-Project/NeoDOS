//! Module contracts: [`CfgModule`] (registry entry) and [`ModuleSession`]
//! (per-visit state machine).
//!
//! Both are UI-agnostic and never import a terminal, `libneodos`, Ob or the
//! Registry.

use alloc::boxed::Box;

use crate::model::{Intent, View};
use crate::platform::CfgPlatform;

/// Stable identity of a NeoCfg module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleId {
    System,
    Power,
    Locale,
    Keyboard,
    About,
}

/// What a [`ModuleSession::update`] wants to happen next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Stay in the module and repaint.
    Stay,
    /// Leave the module and return to the main menu.
    Back,
    /// Leave NeoCfg entirely.
    Exit,
}

/// A registered configuration module.
///
/// Modules are stateless singletons; per-visit state lives in the
/// [`ModuleSession`] returned by [`CfgModule::create`].
pub trait CfgModule: Sync {
    /// Stable id.
    fn id(&self) -> ModuleId;

    /// i18n id of the short display name.
    fn title_key(&self) -> u32;

    /// i18n id of the one-line description.
    fn description_key(&self) -> u32;

    /// Create the session state for one visit to this module.
    fn create(&self) -> Box<dyn ModuleSession>;
}

/// Per-visit module state machine.
pub trait ModuleSession {
    /// Build the current screen. Must not mutate state.
    fn view(&self, env: &dyn CfgPlatform) -> View;

    /// Reduce a user intent. `Stay` repaints, `Back`/`Exit` leave.
    fn update(&mut self, env: &dyn CfgPlatform, intent: Intent) -> Transition;
}
