//! Syscall permission types — parallel to SSDT.

/// Permission requirements for a single syscall slot.
///
/// The dispatch boundary is always INT 0x80 from Ring 3; the only additional
/// gate is the administrator token. `ring_min`/`caps` were declared but never
/// evaluated, so they were removed rather than implying a defence that does
/// not exist (#822).
#[derive(Debug, Clone, Copy)]
pub struct SyscallPermission {
    /// Whether an admin token is required.
    pub admin: bool,
}

impl SyscallPermission {
    /// Slot with no access (default).
    pub const fn free() -> Self {
        Self { admin: false }
    }

    /// Accessible from user mode (Ring 3).
    pub const fn user() -> Self {
        Self { admin: false }
    }

    /// Admin-only syscall (requires admin token).
    pub const fn admin() -> Self {
        Self { admin: true }
    }
}
