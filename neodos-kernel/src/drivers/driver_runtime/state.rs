//! Driver lifecycle state machine.

// ── Driver state (8-state lifecycle, W2 Hot Reload) ──
//
// State machine transition rules:
//   Loaded → Initialized → Registered → Bound → Active
//   Active → Unloading (graceful drain in progress)
//   Unloading → Unloaded (completed drain)
//   Unloaded → Loaded (reload path)
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
    Unloading = 7,     // graceful drain in progress (hot reload)
}

impl DriverState {
    pub fn to_str(self) -> &'static str {
        match self {
            DriverState::Loaded => "LOADED",
            DriverState::Initialized => "INIT",
            DriverState::Registered => "REGISTERED",
            DriverState::Bound => "BOUND",
            DriverState::Active => "ACTIVE",
            DriverState::Faulted => "FAULTED",
            DriverState::Unloaded => "UNLOADED",
            DriverState::Unloading => "UNLOADING",
        }
    }
}

// ── Transition error ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionError;

// ── State machine validation ──

/// Check if a transition from `from` to `to` is valid per the strict lifecycle.
pub(super) fn is_valid_transition(from: DriverState, to: DriverState) -> bool {
    match (from, to) {
        // Forward progression (must follow exact sequence)
        (DriverState::Loaded, DriverState::Initialized) => true,
        (DriverState::Initialized, DriverState::Registered) => true,
        (DriverState::Registered, DriverState::Bound) => true,
        (DriverState::Bound, DriverState::Active) => true,

        // Hot reload: Active → Unloading → Unloaded → Loaded (reload)
        (DriverState::Active, DriverState::Unloading) => true,
        (DriverState::Unloading, DriverState::Unloaded) => true,
        (DriverState::Unloaded, DriverState::Loaded) => true,

        // Error handling: any state can fault or unload
        (_, DriverState::Faulted) => true,
        (_, DriverState::Unloaded) => true,

        // Identity (no-op) — always valid
        (a, b) if a == b => true,

        // Everything else is forbidden
        _ => false,
    }
}
