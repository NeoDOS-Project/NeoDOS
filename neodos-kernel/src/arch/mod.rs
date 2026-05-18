pub mod x64;

/// Platform abstraction: operations that differ per architecture.
///
/// Each architecture target (x86_64, aarch64, riscv64) implements this
/// trait.  Generic kernel code calls `Platform::halt()` etc. instead of
/// calling arch-specific functions directly.
pub trait Platform {
    /// Halt the CPU indefinitely (interrupts may still wake it).
    fn halt() -> !;

    /// Attempt to power off the system.
    fn poweroff() -> !;

    /// Enable interrupts (STI on x86, equivalent on others).
    fn enable_interrupts();

    /// Disable interrupts (CLI on x86, equivalent on others).
    fn disable_interrupts();

    /// Read the CPU vendor / brand string.
    fn cpu_info() -> crate::cpu::CpuInfo;
}

/// The current platform implementation.
#[allow(unused_imports)]
pub use x64::X64Platform as CurrentPlatform;

// Re-export x64 submodule publicly (backward compat, will phase out)
pub use x64::*;
