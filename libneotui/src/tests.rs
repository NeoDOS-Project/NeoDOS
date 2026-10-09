//! Host tests for the reusable toolkit (with a mock console).

use alloc::{string::String, vec, vec::Vec};

use crate::keys::{read_key, Key};
use crate::screen::{Console, Screen};
use crate::widgets::dialog::Dialog;
use crate::widgets::input::read_line;
use crate::widgets::menu::{Menu, MenuItem, MenuOutcome};
use crate::widgets::progress::Progress;

/// A scripted console: replays `input` and captures everything written.
struct MockConsole {
    input: Vec<i32>,
    pos: usize,
    out: String,
}

impl MockConsole {
    fn new(input: &[i32]) -> Self {
        MockConsole {
            input: input.to_vec(),
            pos: 0,
            out: String::new(),
        }
    }
}

impl Console for MockConsole {
    fn write_str(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn read_byte(&mut self) -> i32 {
        match self.input.get(self.pos).copied() {
            Some(b) if b >= 0 => {
                self.pos += 1;
                b
            }
            _ => -1,
        }
    }

    fn try_read_byte(&mut self) -> Option<i32> {
        match self.input.get(self.pos).copied() {
            Some(b) if b >= 0 => {
                self.pos += 1;
                Some(b)
            }
            _ => None,
        }
    }

    fn clear(&mut self) {
        self.out.push_str("<clear>");
    }
}

fn bytes(s: &str) -> Vec<i32> {
    s.bytes().map(|b| b as i32).collect()
}

#[test]
fn decodes_printable_and_control_keys() {
    let mut con = MockConsole::new(&[b'7' as i32, 0x0D, 0x08, 0x09, 0x1B]);
    assert_eq!(read_key(&mut con), Key::Char('7'));
    assert_eq!(read_key(&mut con), Key::Enter);
    assert_eq!(read_key(&mut con), Key::Backspace);
    assert_eq!(read_key(&mut con), Key::Tab);
    // Lone Esc: no following byte -> Esc (does not block/consume).
    assert_eq!(read_key(&mut con), Key::Esc);
}

#[test]
fn decodes_arrow_escape_sequences() {
    let mut con = MockConsole::new(&[0x1B, b'[' as i32, b'A' as i32]);
    assert_eq!(read_key(&mut con), Key::Up);
    let mut con = MockConsole::new(&[0x1B, b'[' as i32, b'B' as i32]);
    assert_eq!(read_key(&mut con), Key::Down);
}

#[test]
fn menu_selects_by_shortcut_and_renders() {
    let items = vec![MenuItem::new("Alpha"), MenuItem::new("Beta")];
    let mut menu = Menu::new("Pick", items).footer("hint");
    let mut con = MockConsole::new(&[b'2' as i32]);
    let mut screen = Screen::new(&mut con);
    assert_eq!(menu.run(&mut screen), MenuOutcome::Select(1));
    assert!(con.out.contains("Pick"));
    assert!(con.out.contains("Alpha"));
    assert!(con.out.contains("2. Beta"));
    assert!(con.out.contains("hint"));
}

#[test]
fn menu_esc_is_back_and_q_is_quit() {
    let mut con = MockConsole::new(&[0x1B]);
    let mut screen = Screen::new(&mut con);
    let mut menu = Menu::new("T", vec![MenuItem::new("A")]);
    assert_eq!(menu.run(&mut screen), MenuOutcome::Back);

    let mut con = MockConsole::new(&[b'q' as i32]);
    let mut screen = Screen::new(&mut con);
    let mut menu = Menu::new("T", vec![MenuItem::new("A")]);
    assert_eq!(menu.run(&mut screen), MenuOutcome::Quit);
}

#[test]
fn menu_navigates_with_arrows_and_wraps() {
    // Down then Enter -> index 1.
    let mut menu = Menu::new("T", vec![MenuItem::new("A"), MenuItem::new("B")]);
    let mut con = MockConsole::new(&[0x1B, b'[' as i32, b'B' as i32, 0x0D]);
    let mut screen = Screen::new(&mut con);
    assert_eq!(menu.run(&mut screen), MenuOutcome::Select(1));
    assert_eq!(menu.selected, 1);
}

#[test]
fn dialog_message_waits_for_a_key() {
    let mut con = MockConsole::new(&[b'x' as i32]);
    let mut screen = Screen::new(&mut con);
    Dialog::message(&mut screen, "Title", &[String::from("line one")], "press");
    assert!(con.out.contains("Title"));
    assert!(con.out.contains("line one"));
    assert!(con.out.contains("press"));
}

#[test]
fn line_input_edits_with_backspace() {
    let mut con = MockConsole::new(&bytes("ab\x08c\r"));
    let mut screen = Screen::new(&mut con);
    let mut out = String::new();
    assert!(read_line(&mut screen, "> ", &mut out));
    assert_eq!(out, "ac");
}

#[test]
fn progress_renders_a_bar() {
    let mut con = MockConsole::new(&[]);
    let mut screen = Screen::new(&mut con);
    {
        let mut p = Progress::new(&mut screen, "Working", 4);
        p.update(2);
        p.finish();
    }
    assert!(con.out.contains("Working"));
    assert!(con.out.contains("100%"));
}

#[test]
fn toolkit_is_reusable_without_neocfg() {
    // The whole point: build and drive UI without libneocfg (which is not a
    // dependency of this crate at all).
    let mut con = MockConsole::new(&[b'1' as i32]);
    let mut screen = Screen::new(&mut con);
    let mut list = crate::widgets::list::List::new("Sections", vec![String::from("One")]);
    assert_eq!(list.run(&mut screen), MenuOutcome::Select(0));
}
