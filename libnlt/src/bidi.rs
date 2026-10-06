//! Right-to-left / bidirectional display helpers (`I18N-P11`).
//!
//! This is **not** a full UAX#9 implementation. It provides the operations a
//! console needs to display RTL locales:
//!
//! * detect whether a locale is RTL,
//! * classify characters as RTL / LTR / neutral,
//! * reorder a logical UTF-8 line into visual order (reverse run order for an
//!   RTL base direction while keeping embedded LTR runs readable).
//!
//! For a full bidi algorithm this module is the extension point: the run
//! segmentation is already computed and reusable.

/// Text base direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

/// Base direction for a language ID.
pub fn direction(language_id: u32) -> Direction {
    if crate::plural::is_rtl(language_id) {
        Direction::Rtl
    } else {
        Direction::Ltr
    }
}

/// Whether a character belongs to an RTL script.
pub fn is_rtl_char(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x05FF | // Hebrew
        0x0600..=0x06FF | // Arabic
        0x0700..=0x074F | // Syriac
        0x0750..=0x077F | // Arabic Supplement
        0x0780..=0x07BF | // Thaana
        0x07C0..=0x07FF | // NKo
        0x0800..=0x083F | // Samaritan
        0x0840..=0x085F | // Mandaic
        0x08A0..=0x08FF | // Arabic Extended-A
        0xFB1D..=0xFB4F | // Hebrew presentation forms
        0xFB50..=0xFDFF | // Arabic presentation forms A
        0xFE70..=0xFEFF | // Arabic presentation forms B
        0x10800..=0x10FFF | // various historic RTL scripts
        0x1E800..=0x1EFFF
    )
}

/// True for characters whose direction does not affect run order.
pub fn is_neutral_char(c: char) -> bool {
    c.is_ascii_whitespace()
        || c.is_ascii_punctuation()
        || matches!(c, '\u{00A0}' | '\u{00AB}' | '\u{00BB}')
}

#[derive(Clone, Copy)]
struct Run {
    start: usize,
    end: usize,
    rtl: bool,
}

const MAX_RUNS: usize = 256;

/// Reorder `input` into visual order for `dir`, writing into `out`.
///
/// For LTR this is a plain copy. For RTL the runs are emitted in reverse order
/// while RTL runs are also reversed character-by-character so embedded Latin
/// words, numbers and punctuation remain forward-readable.
pub fn reorder_visual(input: &str, dir: Direction, out: &mut [u8]) -> usize {
    if dir == Direction::Ltr {
        return copy(out, input.as_bytes());
    }

    let mut runs: [Run; MAX_RUNS] = [Run { start: 0, end: 0, rtl: false }; MAX_RUNS];
    let mut run_count = 0usize;
    let mut cur: Option<Run> = None;

    for (idx, c) in input.char_indices() {
        let next = idx + c.len_utf8();
        let rtl = is_rtl_char(c);
        let neutral = is_neutral_char(c);
        match cur {
            Some(mut r) => {
                // Neutrals join the current run (keeps spaces attached).
                if neutral || r.rtl == rtl {
                    r.end = next;
                    cur = Some(r);
                } else {
                    push_run(&mut runs, &mut run_count, r);
                    cur = Some(Run { start: idx, end: next, rtl });
                }
            }
            None => {
                cur = Some(Run { start: idx, end: next, rtl });
            }
        }
    }
    if let Some(r) = cur {
        push_run(&mut runs, &mut run_count, r);
    }

    let mut pos = 0usize;
    for ri in (0..run_count).rev() {
        let r = runs[ri];
        let slice = &input[r.start..r.end];
        if r.rtl {
            pos = copy_reversed(out, pos, slice);
        } else {
            pos = copy_at(out, pos, slice.as_bytes());
        }
    }
    pos
}

fn push_run(runs: &mut [Run; MAX_RUNS], count: &mut usize, r: Run) {
    if *count < MAX_RUNS {
        runs[*count] = r;
        *count += 1;
    } else if *count > 0 {
        // Coalesce overflow into the previous run rather than dropping text.
        runs[*count - 1].end = r.end;
    }
}

fn copy(out: &mut [u8], bytes: &[u8]) -> usize {
    copy_at(out, 0, bytes)
}

fn copy_at(out: &mut [u8], pos: usize, bytes: &[u8]) -> usize {
    let mut p = pos;
    for &b in bytes {
        if p < out.len() {
            out[p] = b;
            p += 1;
        }
    }
    p
}

fn copy_reversed(out: &mut [u8], pos: usize, s: &str) -> usize {
    let mut char_bounds: [(usize, usize); 256] = [(0, 0); 256];
    let mut n = 0usize;
    for (i, c) in s.char_indices() {
        if n < 256 {
            char_bounds[n] = (i, c.len_utf8());
            n += 1;
        }
    }
    let mut p = pos;
    for idx in (0..n).rev() {
        let (start, len) = char_bounds[idx];
        p = copy_at(out, p, &s.as_bytes()[start..start + len]);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltr_is_identity() {
        let mut out = [0u8; 32];
        let n = reorder_visual("hello", Direction::Ltr, &mut out);
        assert_eq!(&out[..n], b"hello");
    }

    #[test]
    fn rtl_reverses_hebrew() {
        // Hebrew aleph-bet-gimel (logical) -> visual gimel-bet-aleph
        let input = "\u{05D0}\u{05D1}\u{05D2}";
        let mut out = [0u8; 32];
        let n = reorder_visual(input, Direction::Rtl, &mut out);
        let expected = "\u{05D2}\u{05D1}\u{05D0}";
        assert_eq!(&out[..n], expected.as_bytes());
    }

    #[test]
    fn rtl_keeps_latin_run_readable() {
        // Logical: [hebrew][ "abc" ][hebrew]; visual: reverse run order,
        // Hebrew runs reversed, Latin kept forward.
        let input = "\u{05D0}abc\u{05D1}";
        let mut out = [0u8; 32];
        let n = reorder_visual(input, Direction::Rtl, &mut out);
        // last run (\u05D1) first, then "abc", then first run (\u05D0)
        let expected = "\u{05D1}abc\u{05D0}";
        assert_eq!(&out[..n], expected.as_bytes());
    }

    #[test]
    fn direction_detection() {
        assert_eq!(direction(1), Direction::Ltr);
        assert_eq!(direction(15), Direction::Rtl); // Arabic
    }
}
