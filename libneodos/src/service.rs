//! Service-side helpers (#358).
//!
//! A Ring 3 service observes a graceful-shutdown request through the kernel's
//! authoritative per-service flag. The flag is set by the Service Manager when
//! `stop_service()` is called and is cleared when the process exits.
//!
//! The contract is deliberately minimal:
//!
//! * The Service Manager requests a graceful stop and never kills the process
//!   directly while it has time left.
//! * The request wakes the service via a user APC; the service polls with an
//!   alertable yield (`sys_sleep_ex`, RAX 3) and reads the flag.
//! * On observing the request — or on any polling interval — the service calls
//!   [`shutdown_requested`] to read the authoritative flag, performs its own
//!   cleanup, and exits voluntarily with `sys_exit`.
//! * If the service does not exit within the bounded timeout, the kernel forces
//!   termination.

use crate::syscall::{self, ObInfoClass};

/// Returns `true` when a graceful shutdown has been requested for the calling
/// service.
///
/// Safe to call from any valid process handle; the kernel answers for the
/// *calling* process. Returns `false` on any error (treat as "keep running").
pub fn shutdown_requested() -> bool {
    let fd = match syscall::sys_ob_open("\\Global\\Info\\Process", syscall::ob_access::READ) {
        Ok(fd) => fd,
        Err(_) => return false,
    };
    let mut buf = [0u8; 1];
    let res = syscall::sys_ob_query_info(fd, ObInfoClass::ProcessShutdownState, &mut buf);
    let _ = syscall::sys_close(fd);
    matches!(res, Ok(n) if n >= 1 && buf[0] != 0)
}
