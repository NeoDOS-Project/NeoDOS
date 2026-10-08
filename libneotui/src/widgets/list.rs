//! Scrollable, read-only list.
//!
//! A thin specialization of [`Menu`] where every row is selectable and the
//! outcome is used for navigation rather than configuration.

use alloc::{string::String, vec::Vec};

use crate::screen::Screen;
use crate::widgets::menu::{Menu, MenuItem, MenuOutcome};

/// A scrollable list of labels.
pub struct List {
    menu: Menu,
}

impl List {
    pub fn new(title: impl Into<String>, items: Vec<String>) -> Self {
        let items = items.into_iter().map(MenuItem::new).collect();
        List {
            menu: Menu::new(title, items),
        }
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.menu.selected = index;
        self
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.menu.footer = Some(footer.into());
        self
    }

    /// Render and interact; see [`MenuOutcome`].
    pub fn run(&mut self, screen: &mut Screen) -> MenuOutcome {
        self.menu.run(screen)
    }
}
