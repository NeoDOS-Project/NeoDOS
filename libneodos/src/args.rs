const ARGS_ADDR: u64 = 0x41F000;

/// Read command-line arguments from the shared buffer at 0x41F000.
/// Returns only the bytes up to the first null terminator.
pub fn read_args() -> [u8; 256] {
    let mut buf = [0u8; 256];
    let mut i = 0usize;
    unsafe {
        let ptr = ARGS_ADDR as *const u8;
        while i < 255 {
            let b = ptr.add(i).read();
            if b == 0 { break; }
            buf[i] = b;
            i += 1;
        }
    }
    buf
}

/// Check if the given args slice matches any help flag pattern (/?, -h, --help).
pub fn is_help_flag(args: &[u8]) -> bool {
    let trimmed = trim_ascii(args);
    trimmed == b"/?" || trimmed == b"-h" || trimmed == b"--help"
}

/// Trim leading/trailing whitespace and null bytes from a byte slice.
pub fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && matches!(s[start], b' ' | b'\t' | b'\r' | b'\n' | 0) {
        start += 1;
    }
    let mut end = s.len();
    while end > start && matches!(s[end - 1], b' ' | b'\t' | b'\r' | b'\n' | 0) {
        end -= 1;
    }
    &s[start..end]
}
