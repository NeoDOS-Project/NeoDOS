//! Pure, dependency-free helpers for neotop.
//!
//! No syscalls and no ABI access, so these can be unit-tested on the host with
//! `rustc --test src/logic.rs` (the neotop binary itself is no_std and its
//! Cargo test target is disabled).

/// Number of spaces required to pad a field of `len` bytes up to `width`.
///
/// Saturating by construction: a value wider than its column never underflows.
/// The previous neotop used `width - len`, which wrapped to a huge value in
/// release builds (e.g. `"ABOVE_NORMAL"` = 12 in a 4-wide column) and turned
/// the render loop into a near-infinite space flood.
pub fn pad_width(len: usize, width: usize) -> usize {
    width.saturating_sub(len)
}

/// Map `Kthread::state` (`ThreadState::to_u8`) to a display string.
///
/// This is the *thread* encoding. It differs from the process encoding in
/// `ObProcessInfo` (where 3 means Terminated), so the two are kept separate.
pub fn thread_state_str(state: u8) -> &'static str {
    match state {
        0 => "Ready",
        1 => "Running",
        2 => "Blocked",
        3 => "Suspended",
        4 => "Terminated",
        _ => "?",
    }
}

/// A snapshot is truncated when the kernel had more objects than fit.
pub fn is_truncated(returned: u32, total: u32) -> bool {
    returned < total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_width_never_underflows() {
        assert_eq!(pad_width(0, 4), 4);
        assert_eq!(pad_width(2, 4), 2);
        assert_eq!(pad_width(4, 4), 0);
        // Regression: len > width clamps to 0 instead of wrapping.
        assert_eq!(pad_width(6, 4), 0);
        assert_eq!(pad_width(12, 4), 0);
        assert_eq!(pad_width(usize::MAX, 3), 0);
    }

    #[test]
    fn thread_states_cover_kernel_encoding() {
        assert_eq!(thread_state_str(0), "Ready");
        assert_eq!(thread_state_str(1), "Running");
        assert_eq!(thread_state_str(2), "Blocked");
        assert_eq!(thread_state_str(3), "Suspended");
        assert_eq!(thread_state_str(4), "Terminated");
        assert_eq!(thread_state_str(255), "?");
    }

    #[test]
    fn truncation_detection() {
        assert!(is_truncated(1, 2));
        assert!(is_truncated(0, 5));
        assert!(!is_truncated(5, 5));
        assert!(!is_truncated(6, 5));
    }
}
