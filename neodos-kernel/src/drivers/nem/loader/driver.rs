// src/drivers/nem/driver.rs
//! Runtime representation of a loaded .nem driver and public registration API.

use crate::eventbus::{self, Event, EventCallback, EventType};
use crate::drivers::driver_runtime::{DriverId};
use alloc::vec::Vec;
use spin::Mutex;

/// Global mutable reference to the driver currently being initialized.
/// Used by driver binaries to register callbacks without passing a reference.
static mut CURRENT_DRIVER_ID: DriverId = 0;

/// Set the driver ID that is considered "current" for registration calls.
/// This is only safe when called from the scheduler task that executes the driver entry.
pub unsafe fn set_current_driver(id: DriverId) {
    CURRENT_DRIVER_ID = id;
}

/// Clear the current driver after the driver entry returns.
pub unsafe fn clear_current_driver() {
    CURRENT_DRIVER_ID = 0;
}

/// Read the current driver ID (0 = kernel context, no driver active).
/// Used by the capability system to check per-call permissions in hst_* exports.
pub fn current_driver_id() -> DriverId {
    unsafe { CURRENT_DRIVER_ID }
}

/// Public function used by driver binaries to register an event callback.
/// The callback receives only the `&Event`; the driver can obtain its own runtime state
/// via `driver_runtime::get_driver(id)` if needed.
pub fn register_event(event_type: EventType, callback: fn(&Event)) -> Result<(), ()> {
    // Directly register the raw callback; driver state must be accessed via static globals if needed.
    eventbus::EVENT_BUS.register_handler(event_type, callback as EventCallback, "nem_driver")
}

/// Runtime representation of a loaded NEM driver.
pub struct NemDriver {
    pub id: DriverId,
    pub driver_type: u8,
    // Simple mutable state container for driver‑specific counters, etc.
    pub state: Mutex<DriverState>,
    // Keep the names of all registered handlers for clean unload.
    pub callbacks: Mutex<Vec<&'static str>>,
}

#[derive(Default, Debug)]
pub struct DriverState {
    /// Example counter used by timer_listener.nem.
    pub tick_counter: u64,
}

impl core::fmt::Debug for NemDriver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NemDriver")
            .field("id", &self.id)
            .field("type", &self.driver_type)
            // State is under a mutex, so we just skip it or print <locked>
            .field("state", &"<mutex>")
            .finish()
    }
}

impl NemDriver {
    /// Construct a new driver instance after the loader has parsed the header.
    pub fn new(id: DriverId, driver_type: u8) -> Self {
        Self {
            id,
            driver_type,
            state: Mutex::new(DriverState::default()),
            callbacks: Mutex::new(Vec::new()),
        }
    }

    /// Register a callback on behalf of this driver.
    /// This stores the handler name so that `unregister_all` can remove the
    /// registration when the driver is unloaded.
    pub fn register_callback(
        &self,
        event_type: EventType,
        callback: fn(&Event),
    ) -> Result<(), ()> {
        let name_str = alloc::format!("nem_driver_{}_{}", self.id, self.callbacks.lock().len());
        let leaked_name = alloc::boxed::Box::leak(name_str.into_boxed_str());
        eventbus::EVENT_BUS.register_handler(event_type, callback as EventCallback, leaked_name)?;
        self.callbacks.lock().push(leaked_name);
        Ok(())
    }

}
