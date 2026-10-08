//! Progress bar / spinner rendering.

use alloc::{format, string::String};

use crate::screen::Screen;

/// A simple ASCII progress bar redrawn in place with `\r`.
pub struct Progress<'a> {
    screen: &'a mut Screen<'a>,
    title: String,
    total: u64,
    current: u64,
    message: String,
    width: usize,
}

impl<'a> Progress<'a> {
    pub fn new(screen: &'a mut Screen<'a>, title: impl Into<String>, total: u64) -> Self {
        Progress {
            screen,
            title: title.into(),
            total,
            current: 0,
            message: String::new(),
            width: 40,
        }
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.render();
    }

    pub fn update(&mut self, current: u64) {
        self.current = current.min(self.total);
        self.render();
    }

    pub fn finish(&mut self) {
        self.current = self.total;
        self.render();
        self.screen.write("\r\n");
    }

    fn render(&mut self) {
        let filled = if self.total == 0 {
            self.width
        } else {
            (self.current as usize * self.width) / self.total as usize
        }
        .min(self.width);
        let pct = if self.total == 0 {
            100
        } else {
            (self.current * 100) / self.total
        };

        let mut bar = String::new();
        for i in 0..self.width {
            bar.push(if i < filled { '#' } else { '.' });
        }

        let line = if self.message.is_empty() {
            format!("\r{} [{}] {:>3}%", self.title, bar, pct)
        } else {
            format!("\r{} [{}] {:>3}% {}", self.title, bar, pct, self.message)
        };
        self.screen.write(&line);
    }
}
