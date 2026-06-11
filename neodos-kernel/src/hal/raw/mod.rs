pub mod msr;
pub mod cpu;
pub mod io;

pub use msr::*;
pub use cpu::*;
pub use io::*;

#[repr(C, packed)]
pub struct GdtDescriptor {
    pub limit: u16,
    pub base: u64,
}

#[repr(C, packed)]
pub struct IdtDescriptor {
    pub limit: u16,
    pub base: u64,
}

impl GdtDescriptor {
    #[inline]
    pub fn from_raw(limit: u16, base: u64) -> Self {
        GdtDescriptor { limit, base }
    }
}

impl IdtDescriptor {
    #[inline]
    pub fn from_raw(limit: u16, base: u64) -> Self {
        IdtDescriptor { limit, base }
    }
}
