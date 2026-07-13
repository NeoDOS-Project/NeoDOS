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

/// Translate a string literal using the current locale.
///
/// Expands to a call to `i18n_get(key)`.  If no translation is found the
/// literal `$key` is returned — **never panics**, never produces garbage.
///
/// # Usage
/// ```ignore
/// write_str(tr!("file.notfound"));
/// ```
///
/// The macro accepts only string literals (not variables) so the fallback
/// path can return the key text directly without a function call.
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::i18n_get($key)
    };
}
