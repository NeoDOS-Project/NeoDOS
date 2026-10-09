//! `CfgUi` seam over `libneotui`.
//!
//! Converts the UI-agnostic [`View`] model into `libneotui` widgets, resolves
//! i18n ids through the [`NeodosTranslator`], and maps widget outcomes back to
//! [`Intent`]s.

use alloc::{format, string::String, vec::Vec};

use libneocfg::i18n_keys as k;
use libneocfg::{CfgUi, FieldValue, Intent, Text, Translator, View};
use libneodos::{console, syscall};
use libneotui::{Console, Key, Menu, MenuItem, MenuOutcome, Screen};

use crate::neodos_i18n::NeodosTranslator;

/// `Console` backend over `libneodos::console`.
struct NeodosConsole;

impl Console for NeodosConsole {
    fn write_str(&mut self, s: &str) {
        let _ = console::write(s.as_bytes());
    }

    fn read_byte(&mut self) -> i32 {
        // `console::read_byte` may return -1 when no input is ready; yield and
        // retry until a byte arrives.
        loop {
            let b = console::read_byte();
            if b >= 0 {
                return b;
            }
            let _ = syscall::sys_sleep_ex();
        }
    }

    fn try_read_byte(&mut self) -> Option<i32> {
        // Non-blocking look-ahead used to disambiguate a lone ESC from an ANSI
        // escape sequence.
        let mut fds = [syscall::PollFd {
            fd: 0,
            events: syscall::POLLIN,
            revents: 0,
        }];
        match syscall::sys_poll(&mut fds, 0) {
            Ok(n) if n > 0 => {
                let b = console::read_byte();
                if b >= 0 {
                    Some(b)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn clear(&mut self) {
        console::clear_screen();
        console::cursor_home();
    }

    fn set_color(&mut self, fg: u8, bg: u8) {
        console::set_color(fg, bg);
    }

    fn reset_color(&mut self) {
        console::reset_color();
    }
}

/// TUI implementation of [`CfgUi`].
pub struct TuiUi {
    console: NeodosConsole,
    translator: NeodosTranslator,
}

impl TuiUi {
    pub fn new(translator: NeodosTranslator) -> Self {
        TuiUi {
            console: NeodosConsole,
            translator,
        }
    }
}

impl CfgUi for TuiUi {
    fn present(&mut self, view: &View) -> Intent {
        let tr = &self.translator;
        let mut screen = Screen::new(&mut self.console);
        render(tr, &mut screen, view)
    }
}

fn resolve(tr: &NeodosTranslator, text: &Text) -> String {
    match text {
        Text::Key(id) => String::from(tr.tr(*id)),
        Text::Owned(s) => s.clone(),
    }
}

fn wait_intent(screen: &mut Screen) -> Intent {
    match screen.read_key() {
        Key::Esc => Intent::Back,
        Key::Char('q') | Key::Char('Q') => Intent::Quit,
        Key::Enter => Intent::Activate,
        Key::Char(c) => Intent::Char(c),
        _ => Intent::Tick,
    }
}

fn render(tr: &NeodosTranslator, screen: &mut Screen, view: &View) -> Intent {
    match view {
        View::Menu {
            title,
            items,
            selected,
            footer,
        } => {
            let widget_items: Vec<MenuItem> = items
                .iter()
                .map(|i| MenuItem {
                    label: resolve(tr, &i.label),
                    enabled: i.enabled,
                })
                .collect();
            let mut menu = Menu::new(resolve(tr, title), widget_items).selected(*selected);
            if let Some(f) = footer {
                menu = menu.footer(resolve(tr, f));
            }
            match menu.run(screen) {
                MenuOutcome::Select(i) => Intent::Select(i),
                MenuOutcome::Back => Intent::Back,
                MenuOutcome::Quit => Intent::Quit,
            }
        }
        View::Message { title, body } => {
            screen.clear();
            screen.title(&resolve(tr, title));
            screen.separator();
            for line in body {
                screen.line(&resolve(tr, line));
            }
            screen.blank();
            screen.prompt(&resolve(tr, &Text::Key(k::PRESS_KEY)));
            wait_intent(screen)
        }
        View::Detail {
            title,
            fields,
            footer,
        } => {
            screen.clear();
            screen.title(&resolve(tr, title));
            screen.separator();
            for field in fields {
                let label = resolve(tr, &field.label);
                let value = match &field.value {
                    FieldValue::Text(t) => resolve(tr, t),
                    FieldValue::Count(n) => format!("{n}"),
                    FieldValue::Bool(b) => {
                        if *b {
                            resolve(tr, &Text::Key(k::NEOCFG_YES))
                        } else {
                            resolve(tr, &Text::Key(k::NEOCFG_NO))
                        }
                    }
                };
                screen.line(&format!("{label}: {value}"));
            }
            screen.separator();
            screen.line(&resolve(tr, footer));
            wait_intent(screen)
        }
        View::Confirm {
            title,
            prompt,
            default_yes,
        } => {
            if libneotui::Dialog::confirm(
                screen,
                &resolve(tr, title),
                &resolve(tr, prompt),
                *default_yes,
            ) {
                Intent::Activate
            } else {
                Intent::Back
            }
        }
        View::Input {
            title, prompt, ..
        } => {
            screen.clear();
            screen.title(&resolve(tr, title));
            screen.separator();
            let mut out = String::new();
            if libneotui::read_line(screen, &resolve(tr, prompt), &mut out) {
                Intent::Text(out)
            } else {
                Intent::Back
            }
        }
    }
}
