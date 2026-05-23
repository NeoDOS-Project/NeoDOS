//! Event registration helpers for NEM drivers.
//!
//! Drivers must not interact with the EventBus directly; they should use the
//! `register_event_handler!` macro which forwards to the driver instance. The
//! macro guarantees that the callback is wrapped as an `EventCallback` and
//! registered with the global `EVENT_BUS`.
//!
//! The macro expands to a call of `NemDriver::register_event_handler`.
//!
//! Example usage inside a driver binary:
//! ```rust
//! use neodos_kernel::drivers::nem::NemDriver;
//! use neodos_kernel::eventbus::EventType;
//!
//! fn on_tick(event: &neodos_kernel::eventbus::Event, driver: &NemDriver) {
//!     // driver‑specific handling
//! }
//!
//! pub extern "C" fn driver_entry(driver: &NemDriver) {
//!     register_event_handler!(driver, EventType::TimerTick, on_tick);
//! }
//! ```
//!
//! The macro is deliberately lightweight – it does not allocate or perform any
//! runtime checks beyond delegating to `NemDriver::register_event_handler`.

#[macro_export]
macro_rules! register_event_handler {
    ($driver:expr, $event_type:expr, $callback:path) => {{
        $driver.register_event_handler($event_type, $callback)
    }};
}

// Re‑export for external crates that may want to import it directly.
pub use register_event_handler;
