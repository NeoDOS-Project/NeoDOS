//! Driver instance record.

use crate::nem::{NemDriverType, DriverCategory};
use crate::eventbus::EventType;
use crate::object::ObId;
use super::{DriverId, DriverState};

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
    pub obj_id: Option<ObId>,
    pub caps: u64,                      // Capability bitmap (X3 capability system)
    pub isolation_mode: u8,             // X4: IsolationMode enum value (0=None, 1=Basic, 2=Sandbox)
    pub isolated_base: u64,             // X4: Base address in isolated region, or 0
    pub isolated_size: u64,             // X4: Allocated size in isolated region, or 0
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
            obj_id: None,
            caps: 0,
            isolation_mode: 0,
            isolated_base: 0,
            isolated_size: 0,
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
        if self.state == DriverState::Unloading {
            return "Driver unloading — graceful drain in progress";
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
