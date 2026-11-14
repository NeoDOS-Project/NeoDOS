//! Transport seam: the byte stream HTTP runs over.
//!
//! `libhttp` never touches a socket. It asks a [`Transport`] for a connection
//! and then reads/writes with an explicit [`Deadline`]. `libnet` implements the
//! real TCP version; tests implement scripted ones.
//!
//! `Deadline` is a plain millisecond value on a monotonic clock. The transport
//! supplies `now_ms` (userland uses `ob_uptime_ms`, #467), which keeps this
//! crate free of syscalls and host-testable.

use crate::error::HttpError;

/// Absolute point in time after which an operation must fail with
/// [`HttpError::Timeout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    /// Monotonic milliseconds.
    pub at_ms: u64,
}

impl Deadline {
    /// Deadline `timeout_ms` after `now_ms` (saturating).
    pub fn after(now_ms: u64, timeout_ms: u32) -> Self {
        Deadline {
            at_ms: now_ms.saturating_add(timeout_ms as u64),
        }
    }

    /// True once `now_ms` has reached the deadline.
    pub fn expired_at(&self, now_ms: u64) -> bool {
        now_ms >= self.at_ms
    }

    /// Milliseconds left (0 when expired), capped to `u32`.
    pub fn remaining_ms(&self, now_ms: u64) -> u32 {
        self.at_ms.saturating_sub(now_ms).min(u32::MAX as u64) as u32
    }
}

/// Read half of a connection.
pub trait Read {
    /// Read up to `buf.len()` bytes; `Ok(0)` means EOF. Blocks until data
    /// arrives or `dl` expires.
    fn read(&mut self, buf: &mut [u8], dl: Deadline) -> Result<usize, HttpError>;
}

/// Write half of a connection.
pub trait Write {
    /// Write all of `buf`, retrying partial writes until `dl` expires.
    fn write_all(&mut self, buf: &[u8], dl: Deadline) -> Result<(), HttpError>;
}

/// Close the connection. Must be idempotent and must not fail the caller.
pub trait Close {
    /// Close the underlying connection.
    fn close(self);
}

/// Factory for connections, one per request.
pub trait Transport {
    /// Connection type returned by [`Transport::connect`].
    type Conn: Read + Write + Close;

    /// Open a connection to `host:port` within `dl`.
    fn connect(&mut self, host: &str, port: u16, dl: Deadline) -> Result<Self::Conn, HttpError>;
}
