//! Syscall permission types — parallel to SSDT.

/// Permission requirements for a single syscall slot.
#[derive(Debug, Clone, Copy)]
pub struct SyscallPermission {
    /// Capability flags required (future use).
    pub caps: u64,
    /// Minimum ring level: 0 = kernel only, 3 = user allowed.
    pub ring_min: u8,
    /// Whether an admin token is required.
    pub admin: bool,
}

impl SyscallPermission {
    /// Slot with no access (default).
    pub const fn free() -> Self {
        Self { caps: 0, ring_min: 0, admin: false }
    }

    /// Accessible from user mode (Ring 3).
    pub const fn user() -> Self {
        Self { caps: 0, ring_min: 3, admin: false }
    }

    /// Admin-only syscall (requires admin token).
    pub const fn admin() -> Self {
        Self { caps: 0, ring_min: 0, admin: true }
    }
}

/// Synthetic capability bit for admin-only syscalls.
pub const CAP_ADMIN: u64 = 1;
