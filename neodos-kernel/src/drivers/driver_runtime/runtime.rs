//! Driver runtime registry and state machine.

use super::*;
use super::state::is_valid_transition;
use crate::object::{self, ObType};
use crate::eventbus::EventType;
use crate::nem::{NemDriverType, DriverCategory};

// ── Driver runtime ──

pub struct DriverRuntime {
    pub(super) drivers: [Option<DriverInstance>; MAX_DRIVERS],
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
        self.register_ext(name, driver_type, api_version, compat_flags,
            0, 0, 0, DriverCategory::Demand)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_ext(
        &mut self,
        name: &str,
        driver_type: NemDriverType,
        api_version: u16,
        compat_flags: u16,
        abi_min: u16,
        abi_target: u16,
        abi_max: u16,
        category: DriverCategory,
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

        let obj_id = object::ob_create_object(ObType::Driver, name, id as u64, 0, None).ok();

        let caps = crate::drivers::caps::capability_for_category(category).bits;
        let isolation_mode = crate::drivers::isolation::isolation_mode_for_category(category) as u8;

        let instance = DriverInstance {
            id,
            name: name_bytes,
            driver_type,
            state: DriverState::Loaded,
            api_version,
            compat_flags,
            abi_min,
            abi_target,
            abi_max,
            category,
            events_received: 0,
            tick_count: 0,
            last_event_type: 0,
            last_event_tick: 0,
            registered_at_tick: crate::hal::get_ticks(),
            last_error: 0,
            certification_step: PipelineStep::None as u8,
            obj_id,
            caps,
            isolation_mode,
            isolated_base: 0,
            isolated_size: 0,
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

    /// Transition a driver to a new state with validation.
    /// Returns Err(TransitionError) if the transition is invalid.
    pub fn try_transition(&mut self, id: DriverId, target: DriverState) -> Result<(), TransitionError> {
        let drv = self.get_mut(id).ok_or(TransitionError)?;
        if !is_valid_transition(drv.state, target) {
            return Err(TransitionError);
        }
        let previous = drv.state;
        drv.state = target;
        // If transitioning to Faulted, preserve the original state info
        if target == DriverState::Faulted && previous != DriverState::Faulted {
            // last_error should already be set by the caller
        }
        Ok(())
    }

    /// Set an error code on a driver and optionally transition to Faulted.
    pub fn set_error(&mut self, id: DriverId, error: u32, fault: bool) -> bool {
        if let Some(drv) = self.get_mut(id) {
            drv.last_error = error;
            if fault {
                drv.state = DriverState::Faulted;
            }
            true
        } else {
            false
        }
    }

    /// Mark which pipeline step failed.
    pub fn set_certification_step(&mut self, id: DriverId, step: PipelineStep) -> bool {
        if let Some(drv) = self.get_mut(id) {
            drv.certification_step = step as u8;
            true
        } else {
            false
        }
    }

    /// Set the capability bitmap for a driver.
    pub fn set_capabilities(&mut self, id: DriverId, caps: u64) -> bool {
        if let Some(drv) = self.get_mut(id) {
            drv.caps = caps;
            true
        } else {
            false
        }
    }

    /// Set the isolation region for a driver.
    pub fn set_isolation_region(&mut self, id: DriverId, base: u64, size: u64) -> bool {
        if let Some(drv) = self.get_mut(id) {
            drv.isolated_base = base;
            drv.isolated_size = size;
            true
        } else {
            false
        }
    }

    /// Get the capability bitmap for a driver.
    pub fn get_capabilities(&self, id: DriverId) -> Option<u64> {
        self.get(id).map(|d| d.caps)
    }

    /// Check whether a driver holds all of the required capabilities.
    /// Returns Ok(()) or an error string.
    pub fn check_driver_cap(&self, id: DriverId, required: u64) -> Result<(), &'static str> {
        match self.get(id) {
            Some(drv) => crate::drivers::caps::check_capabilities(drv.caps, required),
            None => Err("Driver not found"),
        }
    }

    /// ── Certification Pipeline ──
    ///
    /// A driver is ONLY ACTIVE if:
    ///   Loaded AND Initialized AND Registered AND Bound AND SandboxApproved
    ///
    /// This function checks all preconditions and transitions the driver to Active
    /// only when all criteria are met.
    pub fn certify_and_activate(&mut self, id: DriverId) -> Result<(), &'static str> {
        let drv = self.get_mut(id).ok_or("Driver not found")?;

        // Must be in Bound state — proves pipeline sequence was followed
        if drv.state != DriverState::Bound {
            drv.last_error = ERR_CERTIFICATION_FAILED;
            drv.certification_step = PipelineStep::Certification as u8;
            return Err("Not in Bound state — pipeline incomplete, cannot activate");
        }

        // Check no prior errors
        if drv.last_error != 0 {
            return Err("Driver has unresolved error — cannot activate");
        }

        // Check no fault
        if drv.state == DriverState::Faulted {
            return Err("Driver is faulted — cannot activate");
        }

        // All checks passed: promote to Active
        drv.state = DriverState::Active;
        drv.last_error = 0;
        drv.certification_step = PipelineStep::None as u8;
        Ok(())
    }

    pub fn unregister(&mut self, id: DriverId) -> bool {
        for drv in self.drivers.iter_mut().flatten() {
            if drv.id == id {
                drv.state = DriverState::Unloaded;
                return true;
            }
        }
        false
    }

    pub fn remove(&mut self, id: DriverId) -> Option<DriverInstance> {
        for drv in self.drivers.iter_mut().flatten() {
            if drv.id == id {
                if let Some(kid) = drv.obj_id {
                    let _ = object::ob_destroy_object(kid);
                }
                let removed = core::mem::take(drv);
                self.count -= 1;
                return Some(removed);
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
        self.drivers.iter().flatten().find(|d| d.name_str().eq_ignore_ascii_case(name))
    }

    pub fn get_by_name_mut(&mut self, name: &str) -> Option<&mut DriverInstance> {
        self.drivers.iter_mut().flatten().find(|d| d.name_str() == name)
    }

    pub fn get_by_driver_type(&self, dt: NemDriverType) -> Option<&DriverInstance> {
        self.drivers.iter().flatten().find(|d| d.driver_type == dt && d.state != DriverState::Unloaded)
    }

    /// Deprecated: use try_transition() instead.
    /// Kept for compatibility with legacy loader code.
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

    /// Count of drivers in ACTIVE state only.
    pub fn active_count(&self) -> usize {
        self.drivers.iter().flatten()
            .filter(|d| d.state == DriverState::Active)
            .count()
    }

    /// Count of drivers that are loaded but NOT yet active (excludes Unloaded).
    pub fn loaded_count(&self) -> usize {
        self.drivers.iter().flatten()
            .filter(|d| d.state != DriverState::Unloaded && d.state != DriverState::Active)
            .count()
    }

    /// Count of faulted drivers.
    pub fn faulted_count(&self) -> usize {
        self.drivers.iter().flatten()
            .filter(|d| d.state == DriverState::Faulted)
            .count()
    }

    /// Breakdown of drivers by state (for driver diagnostics).
    pub fn state_counts(&self) -> alloc::vec::Vec<(DriverState, usize)> {
        let mut counts = [0usize; 8];
        for d in self.drivers.iter().flatten() {
            counts[d.state as usize] += 1;
        }
        let mut result = alloc::vec::Vec::new();
        for (i, &c) in counts.iter().enumerate() {
            if c > 0 {
                let state = match i {
                    0 => DriverState::Loaded,
                    1 => DriverState::Initialized,
                    2 => DriverState::Registered,
                    3 => DriverState::Bound,
                    4 => DriverState::Active,
                    5 => DriverState::Faulted,
                    6 => DriverState::Unloaded,
                    7 => DriverState::Unloading,
                    _ => continue,
                };
                result.push((state, c));
            }
        }
        result
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
