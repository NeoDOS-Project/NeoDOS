//! A3.4 SEH + Exception Dispatcher — Unified exception handling for Ring 0 and Ring 3.
//!
//! Kernel exceptions → crash dump + panic
//! User exceptions → TEB exception handler chain → handler decide Continue/Terminate/Reevaluate

pub mod dispatcher;

pub use dispatcher::*;
