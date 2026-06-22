//! SSDT — Syscall Service Dispatch Table types.

/// Snapshot of user registers passed to every syscall handler.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Registers {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub r8: u64,
    pub r9: u64,
}

impl Registers {
    pub const fn new(rax: u64, rbx: u64, rcx: u64, rdx: u64, r8: u64, r9: u64) -> Self {
        Self { rax, rbx, rcx, rdx, r8, r9 }
    }
}

/// Type of a syscall handler function.
pub type SyscallFn = fn(Registers) -> u64;

/// Highest assigned syscall number (0..=MAX_SYSCALL are expected to have handlers).
pub const MAX_SYSCALL: u64 = 58;
