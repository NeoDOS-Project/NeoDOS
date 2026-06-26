pub mod vt;
pub mod manager;

pub use vt::VtInputQueue;
pub use manager::{init, active_vt, switch_vt, push_byte, pop_byte_from_vt};

pub type InputBuffer = VtInputQueue;
