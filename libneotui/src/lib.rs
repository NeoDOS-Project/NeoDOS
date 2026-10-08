//! `libneotui` — a generic, reusable console-UI toolkit for NeoDOS.
//!
//! It provides a small widget set (menu, dialogs, input, list, progress) and
//! key decoding on top of an abstract [`Console`]. It is **not** specific to
//! NeoCfg: any `.NXE` console tool can use it, and nothing here knows about
//! configuration or system objects.
//!
//! Design constraints (see `docs/design/neocfg-design.md`, Addendum A.2.1):
//!
//! * no dependency on `libneocfg`, Ob, the Registry or any app;
//! * no global state — every use instantiates its own backend;
//! * host-testable with a mock [`Console`].

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod keys;
pub mod screen;
pub mod widgets;

pub use keys::{read_key, Key};
pub use screen::{Console, Screen};
pub use widgets::dialog::Dialog;
pub use widgets::input::read_line;
pub use widgets::list::List;
pub use widgets::menu::{Menu, MenuItem, MenuOutcome};
pub use widgets::progress::Progress;

#[cfg(test)]
mod tests;
