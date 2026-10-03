//! Scheduler diagnostic rings (lock-free forensic instrumentation).
//!
//! Split by ring; the public API is preserved via glob re-exports, so
//! `crate::scheduler::diag::*` paths are unchanged.

mod events;
mod syscall;
mod frames;
mod ctx;
mod dr;
mod stress;
mod vfs_owner;

pub use events::*;
pub use syscall::*;
pub use frames::*;
pub use ctx::*;
pub use dr::*;
pub use stress::*;
pub use vfs_owner::*;
