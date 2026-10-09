//! Ordered registry of NeoCfg modules.

use crate::module::CfgModule;
use crate::modules::{AboutModule, KeyboardModule, LocaleModule, PowerModule, SystemModule};

/// The registered modules, in main-menu order.
///
/// Adding a module is a one-line change here plus a new `modules/*.rs` file;
/// no existing module is touched.
pub static MODULES: &[&dyn CfgModule] = &[
    &SystemModule,
    &PowerModule,
    &LocaleModule,
    &KeyboardModule,
    &AboutModule,
];
