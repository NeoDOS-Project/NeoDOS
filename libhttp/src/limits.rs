//! Resource limits and timeouts for one request.
//!
//! Defaults are sized for a 2 MiB process heap: the response head is bounded at
//! 16 KiB and the body at 1 MiB unless the caller streams it.

use crate::error::HttpError;

/// Hard caps applied to a single request/response exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum bytes of the response head (status line + headers + CRLFCRLF).
    pub max_header_bytes: usize,
    /// Maximum number of header lines in the response.
    pub max_headers: usize,
    /// Maximum body bytes; `0` means "no limit" (streaming sinks).
    pub max_body_bytes: u64,
    /// Connect deadline, in milliseconds.
    pub connect_timeout_ms: u32,
    /// Read inactivity deadline, in milliseconds.
    pub read_timeout_ms: u32,
    /// Write deadline, in milliseconds.
    pub write_timeout_ms: u32,
    /// Whole-exchange deadline, in milliseconds.
    pub total_timeout_ms: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_header_bytes: 16 * 1024,
            max_headers: 100,
            max_body_bytes: 1024 * 1024,
            connect_timeout_ms: 10_000,
            read_timeout_ms: 10_000,
            write_timeout_ms: 10_000,
            total_timeout_ms: 30_000,
        }
    }
}

impl Limits {
    /// Reject limits that would disable the safety properties (zero caps or
    /// zero timeouts).
    pub fn validate(&self) -> Result<(), HttpError> {
        if self.max_header_bytes == 0
            || self.max_headers == 0
            || self.connect_timeout_ms == 0
            || self.read_timeout_ms == 0
            || self.write_timeout_ms == 0
            || self.total_timeout_ms == 0
        {
            return Err(HttpError::InvalidRequest);
        }
        Ok(())
    }

    /// True when the body cap is enabled.
    pub fn has_body_limit(&self) -> bool {
        self.max_body_bytes != 0
    }
}
