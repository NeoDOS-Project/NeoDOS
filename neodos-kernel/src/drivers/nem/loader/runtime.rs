use super::hst::{HalServiceTable, build_hst};
use crate::drivers::driver_runtime::{self, DriverId, ERR_INIT_FAILED};
use crate::eventbus;
use alloc::vec::Vec;
use spin::Mutex;

pub type DriverInitFn = unsafe extern "C" fn(*const HalServiceTable) -> i32;
pub type DriverEventFn = unsafe extern "C" fn(event_type: u32, data0: u64, data1: u64) -> i32;
pub type DriverFiniFn = unsafe extern "C" fn();

pub struct LoadedDriver {
    pub id: DriverId,
    pub name: Vec<u8>,
    pub init_fn: Option<DriverInitFn>,
    pub event_fn: Option<DriverEventFn>,
    pub fini_fn: Option<DriverFiniFn>,
    pub hst: HalServiceTable,
}

// Single driver registry, lock-protected (no `static mut`): NEODOS-07 / #637.
static LOADED_DRIVERS: Mutex<Vec<LoadedDriver>> = Mutex::new(Vec::new());

pub fn register_inline(id: DriverId, name: &str,
                       init_fn: Option<DriverInitFn>,
                       event_fn: Option<DriverEventFn>,
                       fini_fn: Option<DriverFiniFn>) {
    let hst = build_hst();
    let loaded = LoadedDriver {
        id,
        name: name.as_bytes().to_vec(),
        init_fn,
        event_fn,
        fini_fn,
        hst,
    };
    LOADED_DRIVERS.lock().push(loaded);
}

pub fn call_init(id: DriverId) -> Result<(), &'static str> {
    // Snapshot the fn + HST under the lock, then call outside it (disjoint from
    // any re-entrant registry access).
    let (init, hst) = {
        let g = LOADED_DRIVERS.lock();
        let d = g.iter().find(|d| d.id == id)
            .ok_or("Driver not loaded in runtime")?;
        (d.init_fn, d.hst)
    };
    if let Some(init) = init {
        let result = unsafe { init(&hst as *const HalServiceTable) };
        if result != 0 {
            driver_runtime::DRIVER_RUNTIME.lock()
                .set_error(id, ERR_INIT_FAILED, true);
            return Err("driver_init() failed");
        }
    }
    Ok(())
}

pub fn call_event_by_id(id: DriverId, event_type: u32, data0: u64, data1: u64) -> Result<i32, &'static str> {
    let event_fn = {
        let g = LOADED_DRIVERS.lock();
        g.iter().find(|d| d.id == id).ok_or("Driver not loaded")?.event_fn
    };
    if let Some(event_fn) = event_fn {
        Ok(unsafe { event_fn(event_type, data0, data1) })
    } else {
        Ok(0)
    }
}

pub fn call_fini(id: DriverId) {
    let fini = {
        let g = LOADED_DRIVERS.lock();
        g.iter().find(|d| d.id == id).and_then(|d| d.fini_fn)
    };
    if let Some(fini) = fini {
        unsafe { fini(); }
    }
}

pub fn register_event_bus_handler(_id: DriverId, event_type: u32) -> Result<(), ()> {
    fn dispatch_wrapper(event: &eventbus::Event) {
        let _ = call_event_by_id(
            event.driver_target,
            event.event_type,
            event.data0,
            event.data1,
        );
    }
    eventbus::EVENT_BUS.register_handler(
        event_type,
        dispatch_wrapper,
        "nem_runtime_dispatch",
    )
}
