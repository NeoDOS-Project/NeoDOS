//! NeoCfg configuration **logic** — the UI-agnostic core (issue #322).
//!
//! NeoCfg is split into three crates (see `docs/design/neocfg-design.md`,
//! Addendum A):
//!
//! ```text
//! libneocfg   — this crate. Pure logic + seams.
//! libneotui   — generic console-UI toolkit (does not know about NeoCfg).
//! userbin/neocfg — glue binary implementing the seams over libneodos.
//! ```
//!
//! This crate never touches a terminal, a syscall or a locale. Everything
//! external goes through three injectable seams:
//!
//! * [`CfgUi`] — presentation: renders a [`View`] and returns an [`Intent`].
//! * [`CfgPlatform`] — data/effects: all system access.
//! * [`Translator`] — i18n: resolves numeric message ids to text.
//!
//! Modules build **data** ([`View`]), never formatted strings, and emit numeric
//! i18n ids. A single [`App`] loop is shared by every UI.
//!
//! When built for NeoDOS this crate is `no_std`; under `cargo test` it uses the
//! host standard library (same pattern as `libnet-config`).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod app;
pub mod i18n;
pub mod i18n_keys;
pub mod mocks;
pub mod model;
pub mod module;
pub mod modules;
pub mod platform;
pub mod registry;
pub mod ui;

#[cfg(test)]
mod tests;

pub use app::App;
pub use i18n::Translator;
pub use model::{Field, FieldValue, Intent, MenuItem, Text, View};
pub use module::{CfgModule, ModuleId, ModuleSession, Transition};
pub use platform::{
    AboutInfo, CfgError, CfgPlatform, CpuInfo, DriveInfo, LocaleOps, MemInfo, PowerOps, PowerPlan,
    ServiceInfo, VersionInfo,
};
pub use registry::MODULES;
pub use ui::CfgUi;
