//! Cross-cutting kernel infrastructure.
//!
//! These modules were historically top-level singletons; they are grouped here
//! for discoverability. Historical `crate::<name>` paths are preserved via
//! re-exports in `main.rs`.

pub mod abi_freeze;
pub mod boot_benchmark;
pub mod cpu;
pub mod elf;
pub mod globals;
pub mod handle;
pub mod invariants;
pub mod lock_order;
pub mod nxl;
pub mod panic_classification;
pub mod trace;
pub mod usermode;
pub mod work_queue;
