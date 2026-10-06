//! Plural-form selection (`I18N-P9`).
//!
//! Plural groups store six CLDR categories in fixed order:
//! `[zero, one, two, few, many, other]`. A category that does not apply to a
//! locale is stored as an empty string. Selection picks the category for the
//! active locale/count and falls back to `other`.

use crate::{str_at, PLURAL_CATEGORIES};

/// CLDR category indices.
pub const ZERO: usize = 0;
pub const ONE: usize = 1;
pub const TWO: usize = 2;
pub const FEW: usize = 3;
pub const MANY: usize = 4;
pub const OTHER: usize = 5;

/// Return the CLDR plural category index for `n` in the given language.
pub fn category(language_id: u32, n: u64) -> usize {
    match language_id {
        // Arabic: zero, one, two, few, many, other
        15 => {
            if n == 0 {
                ZERO
            } else if n == 1 {
                ONE
            } else if n == 2 {
                TWO
            } else if n % 100 >= 3 && n % 100 <= 10 {
                FEW
            } else {
                MANY
            }
        }
        // Russian / Polish / Czech style (simplified CLDR)
        14 | 17 | 24 => {
            let n10 = n % 10;
            let n100 = n % 100;
            if n10 == 1 && n100 != 11 {
                ONE
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                FEW
            } else if n10 == 0 || (5..=9).contains(&n10) || (11..=14).contains(&n100) {
                MANY
            } else {
                OTHER
            }
        }
        // French / Brazilian Portuguese treat 0 as singular.
        3 | 7 => {
            if n == 0 || n == 1 {
                ONE
            } else {
                OTHER
            }
        }
        // Languages with no plural distinction.
        12 | 13 | 22 | 23 => OTHER,
        // Default (English, Spanish, Catalan, German, Italian, …)
        _ => {
            if n == 1 {
                ONE
            } else {
                OTHER
            }
        }
    }
}

/// Select the plural form for `id`'s group located at `entry_off`.
///
/// `entry_off` points at `u8 count, u8 rel[count], strings…`. Falls back to the
/// `other` slot, then to any non-empty slot.
pub fn select(payload: &[u8], entry_off: u32, language_id: u32, n: u64) -> Option<&str> {
    let base = entry_off as usize;
    let count = *payload.get(base)? as usize;
    if count == 0 {
        return None;
    }
    let cat = category(language_id, n).min(count - 1);
    let mut candidate = cat;

    for _ in 0..count {
        let rel = *payload.get(base + 1 + candidate)? as usize;
        if let Some(s) = str_at(payload, (base + rel) as u32) {
            if !s.is_empty() {
                return Some(s);
            }
        }
        // fall back: this category -> other -> any
        candidate = if candidate == OTHER { (candidate + 1) % count } else { OTHER.min(count - 1) };
    }
    None
}

/// Whether a language is right-to-left.
pub fn is_rtl(language_id: u32) -> bool {
    matches!(language_id, 15 | 26 | 27 | 28)
}

/// Ensure the constant is referenced (documents the fixed-order contract).
pub const CATEGORY_COUNT: usize = PLURAL_CATEGORIES;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_rule() {
        assert_eq!(category(1, 0), OTHER);
        assert_eq!(category(1, 1), ONE);
        assert_eq!(category(1, 2), OTHER);
    }

    #[test]
    fn french_zero_is_singular() {
        assert_eq!(category(3, 0), ONE);
        assert_eq!(category(3, 1), ONE);
        assert_eq!(category(3, 2), OTHER);
    }

    #[test]
    fn russian_rule() {
        assert_eq!(category(14, 1), ONE);
        assert_eq!(category(14, 2), FEW);
        assert_eq!(category(14, 5), MANY);
        assert_eq!(category(14, 11), MANY);
        assert_eq!(category(14, 21), ONE);
    }

    #[test]
    fn japanese_has_no_plural() {
        assert_eq!(category(12, 0), OTHER);
        assert_eq!(category(12, 1), OTHER);
    }

    #[test]
    fn select_falls_back_to_other() {
        // group with 6 slots, only "other" populated
        let mut p = [0u8; 64];
        p[0] = 6;
        // rel offsets
        let mut cursor = 1 + 6;
        for c in 0..6 {
            p[1 + c] = cursor as u8;
            let text: &[u8] = if c == OTHER { b"files\0" } else { b"\0" };
            p[cursor..cursor + text.len()].copy_from_slice(text);
            cursor += text.len();
        }
        assert_eq!(select(&p, 0, 1, 1), Some("files"));
        assert_eq!(select(&p, 0, 1, 5), Some("files"));
    }
}
