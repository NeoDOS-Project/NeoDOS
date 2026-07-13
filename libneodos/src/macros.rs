#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::io::_print(core::format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\r\n")
    };
    ($fmt:expr) => {
        $crate::print!(concat!($fmt, "\r\n"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::print!(concat!($fmt, "\r\n"), $($arg)*)
    };
}

#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {
        $crate::io::_eprint(core::format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! eprintln {
    () => {
        $crate::eprint!("\r\n")
    };
    ($fmt:expr) => {
        $crate::eprint!(concat!($fmt, "\r\n"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::eprint!(concat!($fmt, "\r\n"), $($arg)*)
    };
}

/// Translate a string ID using the current locale.
///
/// Expands to `i18n_get_id(id)`. Returns `"?"` on miss — **never panics**.
#[macro_export]
macro_rules! tr_id {
    ($id:expr) => {
        $crate::i18n::i18n_get_id($id)
    };
}

/// Legacy translation (string-key). No longer performs lookups.
/// Always returns the key string literal (no-op during migration).
/// Use `tr_id!()` with numeric IDs instead.
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $key
    };
}
