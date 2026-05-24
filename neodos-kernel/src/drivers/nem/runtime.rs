use super::hst::{HalServiceTable, build_hst};
use crate::drivers::driver_runtime::{self, DriverId, DriverState, ERR_INIT_FAILED};
use crate::eventbus;
use alloc::vec::Vec;

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

static mut LOADED_DRIVERS: Vec<LoadedDriver> = Vec::new();

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
    unsafe { LOADED_DRIVERS.push(loaded); }
}

pub fn call_init(id: DriverId) -> Result<(), &'static str> {
    let loaded = unsafe {
        LOADED_DRIVERS.iter_mut().find(|d| d.id == id)
            .ok_or("Driver not loaded in runtime")?
    };
    let hst_ptr = &loaded.hst as *const HalServiceTable;
    if let Some(init) = loaded.init_fn {
        let result = unsafe { init(hst_ptr) };
        if result != 0 {
            driver_runtime::DRIVER_RUNTIME.lock()
                .set_error(id, ERR_INIT_FAILED, true);
            return Err("driver_init() failed");
        }
    }
    Ok(())
}

pub fn call_event_by_id(id: DriverId, event_type: u32, data0: u64, data1: u64) -> Result<i32, &'static str> {
    let loaded = unsafe {
        LOADED_DRIVERS.iter().find(|d| d.id == id)
            .ok_or("Driver not loaded")?
    };
    if let Some(event_fn) = loaded.event_fn {
        let result = unsafe { event_fn(event_type, data0, data1) };
        Ok(result)
    } else {
        Ok(0)
    }
}

pub fn call_fini(id: DriverId) {
    if let Some(loaded) = unsafe { LOADED_DRIVERS.iter_mut().find(|d| d.id == id) } {
        if let Some(fini) = loaded.fini_fn {
            unsafe { fini(); }
        }
    }
}

pub fn register_event_bus_handler(id: DriverId, event_type: u32) -> Result<(), ()> {
    fn dispatch_wrapper(event: &eventbus::Event) {
        let _ = call_event_by_id(
            event.driver_target as u32,
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
