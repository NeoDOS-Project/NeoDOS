//! Pure view / intent data model. No formatting, no I/O.
//!
//! Modules build [`View`]s out of [`Text`] pieces and [`Field`]s. A `Text` is
//! either an i18n message id or already-materialized text (dynamic values such
//! as a version string or a count). The UI resolves ids through the
//! [`Translator`](crate::Translator).

use alloc::{string::String, vec::Vec};

/// A user-visible piece of text.
///
/// `Key` is the normal case: the module emits a numeric i18n id and the UI
/// resolves it. `Owned` carries dynamic data (version strings, counts, names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    /// Numeric i18n message id (see [`crate::i18n_keys`]).
    Key(u32),
    /// Already-materialized text (dynamic value).
    Owned(String),
}

impl Text {
    /// Build a translated text from an i18n id.
    pub const fn key(id: u32) -> Self {
        Text::Key(id)
    }

    /// Build a literal text from anything string-like.
    pub fn owned(s: impl Into<String>) -> Self {
        Text::Owned(s.into())
    }
}

/// One entry of a [`View::Menu`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: Text,
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(label: Text) -> Self {
        MenuItem {
            label,
            enabled: true,
        }
    }

    pub fn disabled(label: Text) -> Self {
        MenuItem {
            label,
            enabled: false,
        }
    }
}

/// A field in a [`View::Detail`] screen: `label: value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: Text,
    pub value: FieldValue,
}

impl Field {
    pub fn new(label: Text, value: FieldValue) -> Self {
        Field { label, value }
    }
}

/// The value half of a [`Field`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// A translated or literal text value.
    Text(Text),
    /// A numeric count, rendered by the UI.
    Count(u64),
    /// A boolean, rendered by the UI as yes/no.
    Bool(bool),
}

/// A renderable screen. Pure data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// A selectable menu. `selected` is the highlighted index.
    Menu {
        title: Text,
        items: Vec<MenuItem>,
        selected: usize,
        footer: Option<Text>,
    },
    /// A read-only list of `label: value` fields.
    Detail {
        title: Text,
        fields: Vec<Field>,
        footer: Text,
    },
    /// A yes/no confirmation.
    Confirm {
        title: Text,
        prompt: Text,
        default_yes: bool,
    },
    /// A line input.
    Input {
        title: Text,
        prompt: Text,
        buf: String,
        mask: bool,
    },
    /// An informational message; the UI waits for a key.
    Message { title: Text, body: Vec<Text> },
}

impl View {
    /// Title text of any view.
    pub fn title(&self) -> &Text {
        match self {
            View::Menu { title, .. }
            | View::Detail { title, .. }
            | View::Confirm { title, .. }
            | View::Input { title, .. }
            | View::Message { title, .. } => title,
        }
    }
}

/// A normalized user intent produced by the UI.
///
/// `Back` means "leave the current screen / module"; the top-level [`App`]
/// treats `Back` at the main menu as `Quit`. `Quit` is an explicit request to
/// exit NeoCfg.
///
/// [`App`]: crate::App
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// The user picked the item at this index (digit/letter shortcut).
    Select(usize),
    /// The user activated the currently highlighted item (Enter).
    Activate,
    /// Leave the current module / screen (Esc).
    Back,
    /// Exit NeoCfg (Q).
    Quit,
    Up,
    Down,
    PageUp,
    PageDown,
    Next,
    Prev,
    /// A printable character.
    Char(char),
    /// A whole line of text (input dialog).
    Text(String),
    /// A non-blocking repaint tick (progress bars/animations).
    Tick,
}
