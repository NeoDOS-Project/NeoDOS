#![allow(dead_code, unused_imports)]

mod cpu;
mod io;
mod irq;
pub mod irql;
mod mem;
mod time;

pub use cpu::*;
pub use io::*;
pub use irq::*;
pub use irql::*;
pub use mem::*;
pub use time::*;

/// Execute a closure with interrupts disabled, then restore the previous
/// interrupt state.  Uses the HAL's interrupt control functions internally
/// and saves RFLAGS via pushfq/popfq for correct nesting.
#[inline(never)]
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let flags = unsafe { crate::hal::raw::raw_read_rflags() };
    let enabled = (flags & 0x200) != 0;
    disable_interrupts();
    let result = f();
    if enabled {
        enable_interrupts();
    }
    result
}
