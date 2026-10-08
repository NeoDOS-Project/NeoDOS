//! Presentation seam.

use crate::model::{Intent, View};

/// Renders a [`View`] and returns the user's [`Intent`].
///
/// The TUI target implements this over `libneotui`; a future GUI implements it
/// over `libneogui`. Host tests implement it with a scripted intent list.
pub trait CfgUi {
    /// Present `view` and block until the user acts.
    fn present(&mut self, view: &View) -> Intent;

    /// Optional non-blocking tick for progress/animation. Default: no-op.
    fn tick(&mut self) {}
}
