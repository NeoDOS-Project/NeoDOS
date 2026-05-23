// userbin/nem_drivers/test_driver.rs
#![no_std]
#![no_main]

extern crate neodos_kernel as nd;

use nd::drivers::nem::NemDriver;
use nd::eventbus::{Event, EventType};

// Simple callback that prints a message on each timer tick.
fn on_tick(_event: &Event) {
    nd::serial_println!("[TEST DRIVER] timer tick received");
}

// The driver entry point required by the NEM loader.
#[no_mangle]
pub extern "C" fn driver_entry(driver: &NemDriver) {
    // Register the timer‑tick callback via the macro, which ensures
    // the handler is added through the EventBus and runs under the scheduler.
    register_event_handler!(driver, EventType::TimerTick, on_tick);
}
