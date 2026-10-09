//! Menu widget.

use alloc::{format, string::String, vec::Vec};

use crate::keys::Key;
use crate::screen::Screen;

/// One menu entry.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        MenuItem {
            label: label.into(),
            enabled: true,
        }
    }

    pub fn disabled(label: impl Into<String>) -> Self {
        MenuItem {
            label: label.into(),
            enabled: false,
        }
    }
}

/// What a [`Menu`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// The user chose an item.
    Select(usize),
    /// The user pressed `Esc`.
    Back,
    /// The user pressed `Q`.
    Quit,
}

/// A selectable menu with number/letter shortcuts.
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub selected: usize,
    pub footer: Option<String>,
    /// Maximum rows rendered at once (default 10).
    pub max_visible: usize,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Menu {
            title: title.into(),
            items,
            selected: 0,
            footer: None,
            max_visible: 10,
        }
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    /// Render the menu once.
    pub fn render(&self, screen: &mut Screen) {
        screen.clear();
        screen.title(&self.title);
        screen.separator();

        let total = self.items.len();
        let visible = self.max_visible.max(1);
        let start = if total > visible {
            let half = visible / 2;
            let s = self.selected.saturating_sub(half);
            s.min(total - visible)
        } else {
            0
        };

        for i in start..(start + visible).min(total) {
            let item = &self.items[i];
            let marker = if i == self.selected { ">" } else { " " };
            let label = if item.enabled {
                item.label.clone()
            } else {
                format!("({})", item.label)
            };
            screen.line(&format!("{} {}. {}", marker, i + 1, label));
        }

        screen.separator();
        if let Some(f) = &self.footer {
            screen.line(f);
        }
    }

    /// Render and interact until the user makes a terminal choice.
    pub fn run(&mut self, screen: &mut Screen) -> MenuOutcome {
        if self.items.is_empty() {
            return MenuOutcome::Back;
        }
        self.selected = self.clamp_to_enabled(self.selected);
        loop {
            self.render(screen);
            match screen.read_key() {
                Key::Esc => return MenuOutcome::Back,
                Key::Char('q') | Key::Char('Q') => return MenuOutcome::Quit,
                Key::Enter => {
                    if self.items[self.selected].enabled {
                        return MenuOutcome::Select(self.selected);
                    }
                }
                Key::Up | Key::BackTab => self.selected = self.step(false),
                Key::Down | Key::Tab => self.selected = self.step(true),
                Key::Home | Key::PageUp => self.selected = self.clamp_to_enabled(0),
                Key::End | Key::PageDown => {
                    self.selected = self.clamp_to_enabled(self.items.len() - 1)
                }
                Key::Char(c) => {
                    if let Some(idx) = shortcut_index(c) {
                        if idx < self.items.len() {
                            if self.items[idx].enabled {
                                return MenuOutcome::Select(idx);
                            }
                            self.selected = idx;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Move the selection to the next/previous enabled item (wrapping).
    fn step(&self, forward: bool) -> usize {
        let n = self.items.len();
        if n == 0 {
            return 0;
        }
        let mut i = self.selected;
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if self.items[i].enabled {
                return i;
            }
        }
        self.selected
    }

    /// Nearest enabled index at or after the given preference.
    fn clamp_to_enabled(&self, preferred: usize) -> usize {
        if self.items.is_empty() {
            return 0;
        }
        let preferred = preferred.min(self.items.len() - 1);
        if self.items[preferred].enabled {
            return preferred;
        }
        for up in preferred + 1..self.items.len() {
            if self.items[up].enabled {
                return up;
            }
        }
        for down in (0..preferred).rev() {
            if self.items[down].enabled {
                return down;
            }
        }
        preferred
    }
}

/// Map a shortcut character to a zero-based item index.
///
/// `1`–`9` map to indices `0`–`8`; letters `a`–`z` (except `q`, reserved for
/// quit) map to `9` onward.
pub fn shortcut_index(c: char) -> Option<usize> {
    match c {
        '1'..='9' => Some((c as u8 - b'1') as usize),
        'a'..='z' if c != 'q' => Some(9 + (c as u8 - b'a') as usize),
        'A'..='Z' if c != 'Q' => Some(9 + (c as u8 - b'A') as usize),
        _ => None,
    }
}
