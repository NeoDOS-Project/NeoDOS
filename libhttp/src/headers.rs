//! Ordered, case-insensitive HTTP header map.
//!
//! Duplicates are preserved in wire order (the spec allows repeated headers),
//! and lookups are ASCII case-insensitive. Values are validated so a header can
//! never smuggle CR/LF into the request.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::HttpError;

/// Ordered list of header name/value pairs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderMap {
    entries: Vec<(String, String)>,
}

impl HeaderMap {
    /// Empty map.
    pub fn new() -> Self {
        HeaderMap {
            entries: Vec::new(),
        }
    }

    /// Number of header lines (duplicates count separately).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when there are no headers.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Validate and append a header, trimming optional whitespace from the
    /// value.
    pub fn insert(&mut self, name: &str, value: &str) -> Result<(), HttpError> {
        if !is_valid_name(name) || !is_valid_value(value) {
            return Err(HttpError::MalformedHeader);
        }
        self.entries.push((
            name.to_string(),
            value.trim_matches([' ', '\t']).to_string(),
        ));
        Ok(())
    }

    /// Append an already-validated pair (used by the response parser).
    pub(crate) fn push_validated(&mut self, name: String, value: String) {
        self.entries.push((name, value));
    }

    /// First value for `name`, case-insensitively.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// All values for `name`, in wire order.
    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.entries
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Iterate `(name, value)` in wire order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    /// True if any value of `name` is `token` (case-insensitive).
    pub fn contains_token(&self, name: &str, token: &str) -> bool {
        self.get_all(name).any(|value| {
            value
                .split(',')
                .any(|part| part.trim_matches([' ', '\t']).eq_ignore_ascii_case(token))
        })
    }
}

/// RFC 7230 `token`: one or more `tchar` (no separators, no whitespace).
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(is_tchar)
}

fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Header value: printable ASCII plus HTAB and obs-text; CR, LF and NUL are
/// rejected.
pub fn is_valid_value(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b == b'\t' || (0x20..=0x7e).contains(&b) || b >= 0x80)
}
