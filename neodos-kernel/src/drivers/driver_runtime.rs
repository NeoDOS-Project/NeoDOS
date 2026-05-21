// Driver Runtime — tracks loaded .nem driver instances, state, and lifetimes

use spin::Mutex;
use lazy_static::lazy_static;
use crate::nem::NemDriverType;
use crate::eventbus::EventType;

// ── Constants ──

pub type DriverId = u32;
pub const MAX_DRIVERS: usize = 16;
pub const INVALID_DRIVER_ID: DriverId = 0;

// ── Driver state ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverState {
    Loaded = 0,
    Registered = 1,
    Active = 2,
    Faulted = 3,
    Unloaded = 4,
}

impl DriverState {
    pub fn to_str(&self) -> &'static str {
        match self {
            DriverState::Loaded => "LOADED",
            DriverState::Registered => "REGISTERED",
            DriverState::Active => "ACTIVE",
            DriverState::Faulted => "FAULTED",
            DriverState::Unloaded => "UNLOADED",
        }
    }
}

// ── Driver instance ──

#[derive(Debug, Clone, Copy)]
pub struct DriverInstance {
    pub id: DriverId,
    pub name: [u8; 8],
    pub driver_type: NemDriverType,
    pub state: DriverState,
    pub api_version: u16,
    pub compat_flags: u16,
    pub events_received: u64,
    pub tick_count: u64,
    pub last_event_type: EventType,
    pub last_event_tick: u64,
    pub registered_at_tick: u64,
}

impl Default for DriverInstance {
    fn default() -> Self {
        Self {
            id: 0,
            name: [0u8; 8],
            driver_type: NemDriverType::Null,
            state: DriverState::Unloaded,
            api_version: 0,
            compat_flags: 0,
            events_received: 0,
            tick_count: 0,
            last_event_type: 0,
            last_event_tick: 0,
            registered_at_tick: 0,
        }
    }
}

impl DriverInstance {
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        core::str::from_utf8(&self.name[..len]).unwrap_or("<?>")
    }
}

// ── Driver runtime ──

pub struct DriverRuntime {
    drivers: [Option<DriverInstance>; MAX_DRIVERS],
    count: usize,
    next_id: DriverId,
}

impl DriverRuntime {
    pub const fn new() -> Self {
        const INIT: Option<DriverInstance> = None;
        DriverRuntime {
            drivers: [INIT; MAX_DRIVERS],
            count: 0,
            next_id: 1,
        }
    }

    pub fn register(
        &mut self,
        name: &str,
        driver_type: NemDriverType,
        api_version: u16,
        compat_flags: u16,
    ) -> Result<DriverId, &'static str> {
        if self.count >= MAX_DRIVERS {
            return Err("Driver limit reached");
        }
        let id = self.next_id;
        self.next_id += 1;

        let mut name_bytes = [0u8; 8];
        let nb = name.as_bytes();
        let len = nb.len().min(8);
        name_bytes[..len].copy_from_slice(&nb[..len]);

        let instance = DriverInstance {
            id,
            name: name_bytes,
            driver_type,
            state: DriverState::Loaded,
            api_version,
            compat_flags,
            events_received: 0,
            tick_count: 0,
            last_event_type: 0,
            last_event_tick: 0,
            registered_at_tick: crate::hal::get_ticks(),
        };

        for slot in self.drivers.iter_mut() {
            if slot.is_none() {
                *slot = Some(instance);
                self.count += 1;
                return Ok(id);
            }
        }
        Err("No free driver slot")
    }

    pub fn unregister(&mut self, id: DriverId) -> bool {
        for slot in self.drivers.iter_mut() {
            if let Some(drv) = slot {
                if drv.id == id {
                    drv.state = DriverState::Unloaded;
                    return true;
                }
            }
        }
        false
    }

    pub fn remove(&mut self, id: DriverId) -> Option<DriverInstance> {
        for slot in self.drivers.iter_mut() {
            if let Some(drv) = slot {
                if drv.id == id {
                    let removed = core::mem::take(drv);
                    self.count -= 1;
                    return Some(removed);
                }
            }
        }
        None
    }

    pub fn get(&self, id: DriverId) -> Option<&DriverInstance> {
        self.drivers.iter().flatten().find(|d| d.id == id)
    }

    pub fn get_mut(&mut self, id: DriverId) -> Option<&mut DriverInstance> {
        self.drivers.iter_mut().flatten().find(|d| d.id == id)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&DriverInstance> {
        self.drivers.iter().flatten().find(|d| d.name_str() == name)
    }

    pub fn get_by_name_mut(&mut self, name: &str) -> Option<&mut DriverInstance> {
        self.drivers.iter_mut().flatten().find(|d| d.name_str() == name)
    }

    pub fn get_by_driver_type(&self, dt: NemDriverType) -> Option<&DriverInstance> {
        self.drivers.iter().flatten().find(|d| d.driver_type == dt && d.state != DriverState::Unloaded)
    }

    pub fn set_state(&mut self, id: DriverId, state: DriverState) -> bool {
        if let Some(drv) = self.get_mut(id) {
            drv.state = state;
            true
        } else {
            false
        }
    }

    pub fn record_event(&mut self, id: DriverId, event_type: EventType, tick: u64) {
        if let Some(drv) = self.get_mut(id) {
            drv.events_received += 1;
            drv.last_event_type = event_type;
            drv.last_event_tick = tick;
        }
    }

    pub fn increment_tick(&mut self, id: DriverId) {
        if let Some(drv) = self.get_mut(id) {
            drv.tick_count += 1;
        }
    }

    pub fn record_event_and_tick(&mut self, id: DriverId, event_type: EventType, tick: u64) {
        if let Some(drv) = self.get_mut(id) {
            drv.events_received += 1;
            drv.last_event_type = event_type;
            drv.last_event_tick = tick;
            if event_type == crate::eventbus::EVENT_TIMER_TICK {
                drv.tick_count += 1;
            }
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn active_count(&self) -> usize {
        self.drivers.iter().flatten()
            .filter(|d| d.state != DriverState::Unloaded)
            .count()
    }

    pub fn next_driver_id(&self) -> DriverId {
        self.next_id
    }

    pub fn driver_ids(&self) -> alloc::vec::Vec<DriverId> {
        self.drivers.iter().flatten().map(|d| d.id).collect()
    }

    pub fn driver_names(&self) -> alloc::vec::Vec<(&str, DriverId, DriverState)> {
        self.drivers.iter().flatten()
            .map(|d| (d.name_str(), d.id, d.state))
            .collect()
    }
}

// ── Global singleton ──

lazy_static! {
    pub static ref DRIVER_RUNTIME: Mutex<DriverRuntime> = Mutex::new(DriverRuntime::new());
}

// ── Convenience wrappers ──

pub fn register_driver(
    name: &str,
    driver_type: NemDriverType,
    api_version: u16,
    compat_flags: u16,
) -> Result<DriverId, &'static str> {
    DRIVER_RUNTIME.lock().register(name, driver_type, api_version, compat_flags)
}

pub fn unregister_driver(id: DriverId) -> bool {
    DRIVER_RUNTIME.lock().unregister(id)
}

pub fn get_driver(id: DriverId) -> Option<DriverInstance> {
    DRIVER_RUNTIME.lock().get(id).copied()
}

pub fn get_driver_by_name(name: &str) -> Option<DriverInstance> {
    DRIVER_RUNTIME.lock().get_by_name(name).copied()
}

pub fn driver_count() -> usize {
    DRIVER_RUNTIME.lock().count()
}

pub fn driver_names() -> alloc::vec::Vec<(&'static str, DriverId, DriverState)> {
    // The names come from DriverInstance.name_str() which returns &str,
    // but we can't easily return references from a Mutex lock.
    // Instead return a static representation.
    let mut results = alloc::vec::Vec::new();
    let runtime = DRIVER_RUNTIME.lock();
    for drv in runtime.drivers.iter().flatten() {
        // We can't use name_str() here because it returns a temporary.
        // Use a fixed list of known names instead.
        let static_name: &'static str = match drv.driver_type {
            NemDriverType::Null => "null",
            NemDriverType::Echo => "echo",
            NemDriverType::Lifecycle => "timer_listener",
            NemDriverType::Mutation => "mutation",
            NemDriverType::Fault => "fault",
            NemDriverType::Burst => "burst",
        };
        results.push((static_name, drv.id, drv.state));
    }
    results
}
