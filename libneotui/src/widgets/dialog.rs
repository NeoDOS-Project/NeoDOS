//! Modal dialogs: message, error, confirm and input.

use alloc::string::String;

use crate::keys::Key;
use crate::screen::Screen;
use crate::widgets::input::read_line;

/// Stateless dialog helpers.
pub struct Dialog;

impl Dialog {
    /// Informational message; waits for a key.
    pub fn message(screen: &mut Screen, title: &str, body: &[String], hint: &str) {
        screen.clear();
        screen.title(title);
        screen.separator();
        for line in body {
            screen.line(line);
        }
        screen.pause(hint);
    }

    /// Error message; waits for a key.
    pub fn error(screen: &mut Screen, title: &str, message: &str, hint: &str) {
        let body = [String::from(message)];
        Self::message(screen, title, &body, hint);
    }

    /// Yes/no confirmation. Returns the user's answer.
    pub fn confirm(screen: &mut Screen, title: &str, prompt: &str, default_yes: bool) -> bool {
        screen.clear();
        screen.title(title);
        screen.separator();
        screen.line(prompt);
        screen.blank();
        let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
        loop {
            screen.prompt(&alloc::format!("{suffix} "));
            match screen.read_key() {
                Key::Char('y') | Key::Char('Y') => return true,
                Key::Char('n') | Key::Char('N') => return false,
                Key::Enter => return default_yes,
                Key::Esc => return false,
                _ => {}
            }
        }
    }

    /// Prompt for a line of text. Returns `None` if the user cancelled.
    pub fn input(screen: &mut Screen, title: &str, prompt: &str) -> Option<String> {
        screen.clear();
        screen.title(title);
        screen.separator();
        let mut out = String::new();
        if read_line(screen, prompt, &mut out) {
            Some(out)
        } else {
            None
        }
    }
}
