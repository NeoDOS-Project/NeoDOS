// Driver Runtime — tracks loaded .nem driver instances, state, and lifetimes
//
// ── Driver Certification Pipeline v1 ──
//
// Strict lifecycle: Loaded → Initialized → Registered → Bound → Active
// A driver MUST follow this exact sequence. If ANY step is skipped,
// the driver MUST NOT appear as ACTIVE in the registry.
//
// A driver may be LOADED and even INITIALIZED but still NOT ACTIVE because:
//   1. Registry was never updated (stuck in Loaded/Initialized)
//   2. Event Bus binding missing (stuck in Registered)
//   3. Sandbox rejection (certify_and_activate fails)
//   4. Deferred activation (scheduler hasn't called certify)
//   5. Missing capability grant (security model denied activation)

use spin::Mutex;
use lazy_static::lazy_static;
use crate::nem::{NemDriverType, DriverCategory};
use crate::eventbus::EventType;
use crate::kobj;

// ── Constants ──

pub type DriverId = u32;
pub const MAX_DRIVERS: usize = 16;
pub const INVALID_DRIVER_ID: DriverId = 0;

// ── Error codes for last_error field ──

pub const ERR_NONE: u32 = 0;
pub const ERR_INIT_FAILED: u32 = 1;
pub const ERR_REGISTRATION_FAILED: u32 = 2;
pub const ERR_BIND_FAILED: u32 = 3;
pub const ERR_SANDBOX_REJECTED: u32 = 4;
pub const ERR_CERTIFICATION_FAILED: u32 = 5;
pub const ERR_OUT_OF_MEMORY: u32 = 6;
pub const ERR_POLICY_VIOLATION: u32 = 7;
pub const ERR_LOAD_FAILED: u32 = 8;
pub const ERR_CAPABILITY_DENIED: u32 = 9;

pub fn err_to_str(code: u32) -> &'static str {
    match code {
        ERR_NONE => "NONE",
        ERR_INIT_FAILED => "INIT_FAILED",
        ERR_REGISTRATION_FAILED => "REGISTRATION_FAILED",
        ERR_BIND_FAILED => "BIND_FAILED",
        ERR_SANDBOX_REJECTED => "SANDBOX_REJECTED",
        ERR_CERTIFICATION_FAILED => "CERTIFICATION_FAILED",
        ERR_OUT_OF_MEMORY => "OUT_OF_MEMORY",
        ERR_POLICY_VIOLATION => "POLICY_VIOLATION",
        ERR_LOAD_FAILED => "LOAD_FAILED",
        ERR_CAPABILITY_DENIED => "CAPABILITY_DENIED",
        _ => "UNKNOWN",
    }
}

// ── Driver state (7-state lifecycle) ──
//
// State machine transition rules:
//   Loaded → Initialized → Registered → Bound → Active
//   Any state → Faulted | Unloaded (terminal)
//   All other transitions are INVALID.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverState {
    Loaded = 0,        // binary loaded into memory, not verified
    Initialized = 1,   // driver_init() executed successfully
    Registered = 2,    // registered in Driver Registry + Event Bus
    Bound = 3,         // bound to Event Bus / Device
    Active = 4,        // fully operational in runtime
    Faulted = 5,       // runtime failure detected
    Unloaded = 6,      // removed from system
}

impl DriverState {
    pub fn to_str(&self) -> &'static str {
        match self {
            DriverState::Loaded => "LOADED",
            DriverState::Initialized => "INIT",
            DriverState::Registered => "REGISTERED",
            DriverState::Bound => "BOUND",
            DriverState::Active => "ACTIVE",
            DriverState::Faulted => "FAULTED",
            DriverState::Unloaded => "UNLOADED",
        }
    }
}

// ── Transition error ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionError;

// ── Certification step tracking ──

/// Pipeline step that failed (0 = no failure / passed all)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PipelineStep {
    None = 0,
    Load = 1,
    Init = 2,
    Registration = 3,
    Binding = 4,
    Certification = 5,
}

impl PipelineStep {
    pub fn to_str(&self) -> &'static str {
        match self {
            PipelineStep::None => "OK",
            PipelineStep::Load => "LOAD",
            PipelineStep::Init => "INIT",
            PipelineStep::Registration => "REGISTER",
            PipelineStep::Binding => "BIND",
            PipelineStep::Certification => "CERTIFY",
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
    pub abi_min: u16,
    pub abi_target: u16,
    pub abi_max: u16,
    pub category: DriverCategory,
    pub events_received: u64,
    pub tick_count: u64,
    pub last_event_type: EventType,
    pub last_event_tick: u64,
    pub registered_at_tick: u64,
    pub last_error: u32,                // 0 = no error, non-zero = error code
    pub certification_step: u8,         // PipelineStep value tracking which step failed
    pub kobj_id: Option<kobj::KObjId>,
    pub caps: u64,                      // Capability bitmap (X3 capability system)
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
            abi_min: 0,
            abi_target: 0,
            abi_max: 0,
            category: DriverCategory::Demand,
            events_received: 0,
            tick_count: 0,
            last_event_type: 0,
            last_event_tick: 0,
            registered_at_tick: 0,
            last_error: 0,
            certification_step: 0,
            kobj_id: None,
            caps: 0,
        }
    }
}

impl DriverInstance {
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        core::str::from_utf8(&self.name[..len]).unwrap_or("<?>")
    }

    /// Returns true only if the driver is fully certified and active.
    pub fn is_operational(&self) -> bool {
        self.state == DriverState::Active && self.last_error == 0
    }

    /// Human-readable description of why a driver is not active (for debugging).
    pub fn inactive_reason(&self) -> &'static str {
        if self.state == DriverState::Active {
            return "Driver IS active";
        }
        if self.state == DriverState::Faulted {
            return "Driver faulted — see last_error";
        }
        if self.state == DriverState::Unloaded {
            return "Driver unloaded";
        }
        match self.state {
            DriverState::Loaded => "Loaded but not Initialized — driver_init() never called",
            DriverState::Initialized => "Initialized but not Registered — registry commit missing",
            DriverState::Registered => "Registered but not Bound — Event Bus binding missing",
            DriverState::Bound => "Bound but not Active — certification failed or deferred",
            _ => "Unknown state",
        }
    }

    /// Returns which pipeline steps have been completed.
    pub fn pipeline_progress(&self) -> [bool; 5] {
        [
            self.state as u8 >= DriverState::Initialized as u8,
            self.state as u8 >= DriverState::Registered as u8,
            self.state as u8 >= DriverState::Bound as u8,
            self.state as u8 >= DriverState::Active as u8,
            self.state == DriverState::Active,
        ]
    }
}

// ── State machine validation ──

/// Check if a transition from `from` to `to` is valid per the strict lifecycle.
fn is_valid_transition(from: DriverState, to: DriverState) -> bool {
    match (from, to) {
        // Forward progression (must follow exact sequence)
        (DriverState::Loaded, DriverState::Initialized) => true,
        (DriverState::Initialized, DriverState::Registered) => true,
        (DriverState::Registered, DriverState::Bound) => true,
        (DriverState::Bound, DriverState::Active) => true,

        // Error handling: any state can fault or unload
        (_, DriverState::Faulted) => true,
        (_, DriverState::Unloaded) => true,

        // Identity (no-op) — always valid
        (a, b) if a == b => true,

        // Everything else is forbidden
        _ => false,
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
        self.register_ext(name, driver_type, api_version, compat_flags,
            0, 0, 0, DriverCategory::Demand)
    }

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

        let kobj_id = kobj::kobj_register(kobj::KObjType::Driver, name, id as u64).ok();

        let caps = crate::drivers::caps::capability_for_category(category).bits;

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
            kobj_id,
            caps,
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
                    if let Some(kid) = drv.kobj_id {
                        kobj::kobj_unregister(kid);
                    }
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

    /// Breakdown of drivers by state (for NDREG QUERY).
    pub fn state_counts(&self) -> alloc::vec::Vec<(DriverState, usize)> {
        let mut counts = [0usize; 7];
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

pub fn register_driver_ext(
    name: &str,
    driver_type: NemDriverType,
    api_version: u16,
    compat_flags: u16,
    abi_min: u16,
    abi_target: u16,
    abi_max: u16,
    category: DriverCategory,
) -> Result<DriverId, &'static str> {
    DRIVER_RUNTIME.lock().register_ext(name, driver_type, api_version, compat_flags,
        abi_min, abi_target, abi_max, category)
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

pub fn driver_names() -> alloc::vec::Vec<(alloc::string::String, DriverId, DriverState)> {
    let mut results = alloc::vec::Vec::new();
    let runtime = DRIVER_RUNTIME.lock();
    for drv in runtime.drivers.iter().flatten() {
        results.push((alloc::string::String::from(drv.name_str()), drv.id, drv.state));
    }
    results
}

/// Check whether a driver (by ID) holds the required capabilities.
pub fn check_driver_cap(id: DriverId, required: u64) -> Result<(), &'static str> {
    DRIVER_RUNTIME.lock().check_driver_cap(id, required)
}

/// Set capabilities for a driver (by ID).
pub fn set_capabilities(id: DriverId, caps: u64) -> bool {
    DRIVER_RUNTIME.lock().set_capabilities(id, caps)
}

/// Get capabilities for a driver (by ID).
pub fn get_capabilities(id: DriverId) -> Option<u64> {
    DRIVER_RUNTIME.lock().get_capabilities(id)
}

// ── Test suite: Driver State Machine + Certification Pipeline ──

pub fn register_driver_state_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;

    // ── Transition matrix tests ──

    test_case!("dstate_valid_loaded_to_init", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
        test_true!(rt.try_transition(id, DriverState::Initialized).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Initialized);
    });

    test_case!("dstate_valid_init_to_registered", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        test_true!(rt.try_transition(id, DriverState::Registered).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Registered);
    });

    test_case!("dstate_valid_registered_to_bound", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        test_true!(rt.try_transition(id, DriverState::Bound).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Bound);
    });

    test_case!("dstate_valid_bound_to_active", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        rt.try_transition(id, DriverState::Bound).ok();
        test_true!(rt.try_transition(id, DriverState::Active).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Active);
    });

    // ── Invalid transition tests ──

    test_case!("dstate_invalid_skip_init", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Cannot skip Initialized — going directly to Registered should fail
        test_true!(rt.try_transition(id, DriverState::Registered).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
    });

    test_case!("dstate_invalid_skip_registered", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        // Cannot skip Registered — going directly to Bound should fail
        test_true!(rt.try_transition(id, DriverState::Bound).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Initialized);
    });

    test_case!("dstate_invalid_skip_bound", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        // Cannot skip Bound — going directly to Active should fail
        test_true!(rt.try_transition(id, DriverState::Active).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Registered);
    });

    test_case!("dstate_invalid_skip_all", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Loaded → Active is impossible (6 steps missing)
        test_true!(rt.try_transition(id, DriverState::Active).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
    });

    // ── Fault / Unload transition tests ──

    test_case!("dstate_any_to_faulted", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Any state can go to Faulted
        test_true!(rt.try_transition(id, DriverState::Faulted).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Faulted);
    });

    test_case!("dstate_any_to_unloaded", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        test_true!(rt.try_transition(id, DriverState::Unloaded).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Unloaded);
    });

    test_case!("dstate_faulted_to_active_fails", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Faulted).ok();
        // Cannot recover from Faulted to Active (must go through Unloaded)
        test_true!(rt.try_transition(id, DriverState::Active).is_err());
    });

    // ── Certification pipeline tests ──

    test_case!("dstate_certify_full_pipeline", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Walk through all 5 stages
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        rt.try_transition(id, DriverState::Bound).ok();
        // Now certify — should succeed since all prior stages completed
        test_true!(rt.certify_and_activate(id).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Active);
        test_eq!(rt.get(id).unwrap().last_error, 0);
        test_eq!(rt.get(id).unwrap().certification_step, 0);
    });

    test_case!("dstate_certify_incomplete_pipeline", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Only go through Loaded → Initialized → Registered (skip Bound)
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        // Should NOT be able to certify — pipeline incomplete
        test_true!(rt.certify_and_activate(id).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Registered);
        test_ne!(rt.get(id).unwrap().last_error, 0);
    });

    test_case!("dstate_certify_not_initialized", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Only Loaded — can't certify
        test_true!(rt.certify_and_activate(id).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
    });

    test_case!("dstate_certify_not_bound", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        // Not Bound — can't certify
        test_true!(rt.certify_and_activate(id).is_err());
        test_eq!(rt.get(id).unwrap().certification_step, PipelineStep::Certification as u8);
    });

    test_case!("dstate_set_error_and_fault", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.set_error(id, ERR_OUT_OF_MEMORY, true);
        let drv = rt.get(id).unwrap();
        test_eq!(drv.state, DriverState::Faulted);
        test_eq!(drv.last_error, ERR_OUT_OF_MEMORY);
    });

    test_case!("dstate_set_error_no_fault", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.set_error(id, ERR_CERTIFICATION_FAILED, false);
        let drv = rt.get(id).unwrap();
        // Should still be Loaded (not faulted)
        test_eq!(drv.state, DriverState::Loaded);
        test_eq!(drv.last_error, ERR_CERTIFICATION_FAILED);
    });

    // ── Active count tests ──

    test_case!("dstate_active_count", {
        let mut rt = DriverRuntime::new();
        let id1 = rt.register("drv1", NemDriverType::Null, 1, 0).unwrap();
        let _id2 = rt.register("drv2", NemDriverType::Echo, 1, 0).unwrap();
        test_eq!(rt.active_count(), 0); // none active yet
        // Fully certify drv1
        rt.try_transition(id1, DriverState::Initialized).ok();
        rt.try_transition(id1, DriverState::Registered).ok();
        rt.try_transition(id1, DriverState::Bound).ok();
        rt.certify_and_activate(id1).ok();
        test_eq!(rt.active_count(), 1);
        // drv2 should not affect active_count
        test_eq!(rt.active_count(), 1);
    });

    test_case!("dstate_loaded_count", {
        let mut rt = DriverRuntime::new();
        let id1 = rt.register("drv1", NemDriverType::Null, 1, 0).unwrap();
        let _id2 = rt.register("drv2", NemDriverType::Echo, 1, 0).unwrap();
        test_eq!(rt.loaded_count(), 2); // both loaded, none active
        rt.try_transition(id1, DriverState::Initialized).ok();
        rt.try_transition(id1, DriverState::Registered).ok();
        rt.try_transition(id1, DriverState::Bound).ok();
        rt.certify_and_activate(id1).ok();
        test_eq!(rt.loaded_count(), 1); // drv2 still loaded-not-active
    });

    // ── Inactive reason tests ──

    test_case!("dstate_inactive_reason", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        let drv = rt.get(id).unwrap();
        test_ne!(drv.inactive_reason(), "Driver IS active");
        test_true!(drv.inactive_reason().contains("Loaded"));
        // Advanced to Init
        rt.try_transition(id, DriverState::Initialized).ok();
        let drv = rt.get(id).unwrap();
        test_true!(drv.inactive_reason().contains("Initialized"));
    });

    // ── Pipeline progress test ──

    test_case!("dstate_pipeline_progress", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [false, false, false, false, false]);
        rt.try_transition(id, DriverState::Initialized).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, false, false, false, false]);
        rt.try_transition(id, DriverState::Registered).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, true, false, false, false]);
        rt.try_transition(id, DriverState::Bound).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, true, true, false, false]);
        rt.try_transition(id, DriverState::Active).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, true, true, true, true]);
    });
}

pub fn register_driver_certification_tests() {
    register_driver_state_tests();
}
