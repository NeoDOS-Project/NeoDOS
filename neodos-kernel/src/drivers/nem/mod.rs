//! NEM dynamic driver framework: loader, certification runtime and management.

pub mod format;
pub mod loader;
pub mod management;
pub mod runtime;

// Historical submodule paths (`crate::drivers::nem::driver`, `.hst`, ...)
// now live under `loader/`.
pub use loader::{driver, event, hst, net_bridge, v3loader};

pub use loader::load_nem as load_nem_driver;
