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

/// Translate a format template with positional placeholders `{0}`, `{0:d}`,
/// `{0:x}`, `{0:X}`.
///
/// Expands to `i18n_format(id, args)`. Returns `"?"` on miss.
#[macro_export]
macro_rules! tr_fmt {
    ($id:expr, $args:expr) => {
        $crate::i18n::i18n_format($id, $args)
    };
}

/// Select the plural form of a translated string for a count.
///
/// Expands to `i18n_plural(id, n)`. Returns `"?"` on miss.
#[macro_export]
macro_rules! plural_id {
    ($id:expr, $n:expr) => {
        $crate::i18n::i18n_plural($id, $n as u64)
    };
}
