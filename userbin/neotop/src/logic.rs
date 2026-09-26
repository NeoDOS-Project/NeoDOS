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

/// Validate a `ProcSnapshotHeader` against the ABI this tool was built with.
/// Guards against kernel/libneodos layout drift instead of misparsing records.
pub fn proc_header_matches(
    version: u32,
    process_entry_size: u32,
    thread_entry_size: u32,
    expected_version: u32,
    expected_process_entry_size: u32,
    expected_thread_entry_size: u32,
) -> bool {
    version == expected_version
        && process_entry_size == expected_process_entry_size
        && thread_entry_size == expected_thread_entry_size
}

/// True when the Phase 15-A process/thread snapshot is truncated: the explicit
/// flag is set, or either section returned fewer records than were available.
pub fn proc_truncated(
    flags: u32,
    process_returned: u32,
    process_total: u32,
    thread_returned: u32,
    thread_total: u32,
) -> bool {
    flags & 1 != 0 || process_returned < process_total || thread_returned < thread_total
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

    #[test]
    fn proc_header_validation() {
        assert!(proc_header_matches(1, 40, 48, 1, 40, 48));
        assert!(!proc_header_matches(2, 40, 48, 1, 40, 48));
        assert!(!proc_header_matches(1, 41, 48, 1, 40, 48));
        assert!(!proc_header_matches(1, 40, 47, 1, 40, 48));
    }

    #[test]
    fn proc_truncation_detection() {
        assert!(!proc_truncated(0, 2, 2, 6, 6));
        assert!(proc_truncated(1, 2, 2, 6, 6));
        assert!(proc_truncated(0, 1, 2, 6, 6));
        assert!(proc_truncated(0, 2, 2, 5, 6));
    }
}
