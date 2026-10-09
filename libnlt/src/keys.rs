//! Symbolic key → numeric id resolution for the generated keymap (#578).
//!
//! The compiler (`nltc --generate-keymap`) emits a sorted `&[(&str, u32)]`
//! table. This module resolves `"<app>.<NAME>"` keys against it, both at
//! runtime ([`find`]) and at compile time ([`find_const`]), so `tr!("…")` can
//! fail the build on an unknown key.
//!
//! Zero-dependency and `no_std`; the comparison is byte-wise (lexicographic),
//! which matches the sort order `nltc` produces.

/// Const-equal two strings byte-wise.
pub const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Const-lexicographic `a < b`.
pub const fn str_lt(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0;
    while i < n {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
        i += 1;
    }
    a.len() < b.len()
}

/// Binary-search `map` (sorted by key) for `key`.
///
/// `const`-evaluable: usable in a `const` context to reject unknown keys at
/// compile time.
pub const fn find_const(map: &[(&str, u32)], key: &str) -> Option<u32> {
    let mut lo = 0usize;
    let mut hi = map.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (k, id) = map[mid];
        if str_eq(k, key) {
            return Some(id);
        }
        if str_lt(k, key) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    None
}

/// Runtime lookup (same algorithm as [`find_const`]).
pub fn find(map: &[(&str, u32)], key: &str) -> Option<u32> {
    find_const(map, key)
}

/// Look up `key` and return its id, panicking when missing.
///
/// Intended for use inside a `const` initialiser so a mistyped key becomes a
/// compile-time error rather than a runtime surprise.
pub const fn require(map: &[(&str, u32)], key: &str) -> u32 {
    match find_const(map, key) {
        Some(id) => id,
        None => panic!("unknown i18n key"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static MAP: &[(&str, u32)] = &[
        ("app.ALPHA", 10),
        ("app.BETA", 20),
        ("neocfg.TITLE", 1001),
    ];

    #[test]
    fn finds_existing_keys() {
        assert_eq!(find(MAP, "app.ALPHA"), Some(10));
        assert_eq!(find(MAP, "app.BETA"), Some(20));
        assert_eq!(find(MAP, "neocfg.TITLE"), Some(1001));
    }

    #[test]
    fn missing_key_is_none() {
        assert_eq!(find(MAP, "app.GAMMA"), None);
        assert_eq!(find(MAP, "neocfg"), None);
        assert_eq!(find(MAP, ""), None);
    }

    #[test]
    fn const_evaluates() {
        const ID: u32 = require(MAP, "app.BETA");
        assert_eq!(ID, 20);
    }

    #[test]
    fn ordering_helpers() {
        assert!(str_lt("a", "b"));
        assert!(str_lt("app.A", "app.B"));
        assert!(str_lt("app", "app.A"));
        assert!(!str_lt("b", "a"));
        assert!(str_eq("x", "x"));
        assert!(!str_eq("x", "y"));
    }
}
