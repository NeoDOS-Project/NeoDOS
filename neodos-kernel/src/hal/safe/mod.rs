// The MSR catalog is a typed safe API: entries are added ahead of their first
// caller and kept stable for drivers/diagnostics, so unused entries are expected.
#[allow(dead_code)]
pub mod msr;
pub mod cpu;

pub use msr::*;
pub use cpu::*;
