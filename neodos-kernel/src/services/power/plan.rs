use alloc::string::String;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuPolicy {
    Performance = 0,
    Balanced = 1,
    PowerSave = 2,
}

impl CpuPolicy {
    pub fn to_str(self) -> &'static str {
        match self {
            CpuPolicy::Performance => "Performance",
            CpuPolicy::Balanced => "Balanced",
            CpuPolicy::PowerSave => "PowerSave",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Performance" => Some(CpuPolicy::Performance),
            "Balanced" => Some(CpuPolicy::Balanced),
            "PowerSave" => Some(CpuPolicy::PowerSave),
            _ => None,
        }
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(CpuPolicy::Performance),
            1 => Some(CpuPolicy::Balanced),
            2 => Some(CpuPolicy::PowerSave),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Nothing = 0,
    Sleep = 1,
    Hibernate = 2,
    Shutdown = 3,
}

impl PowerAction {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(PowerAction::Nothing),
            1 => Some(PowerAction::Sleep),
            2 => Some(PowerAction::Hibernate),
            3 => Some(PowerAction::Shutdown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PowerPolicies {
    pub display_timeout_minutes: u32,
    pub sleep_timeout_minutes: u32,
    pub hibernate_enabled: bool,
    pub cpu_policy: CpuPolicy,
    pub lid_action: PowerAction,
    pub power_button_action: PowerAction,
}

impl PowerPolicies {
    pub fn default_policies() -> Self {
        PowerPolicies {
            display_timeout_minutes: 0,
            sleep_timeout_minutes: 0,
            hibernate_enabled: false,
            cpu_policy: CpuPolicy::Balanced,
            lid_action: PowerAction::Nothing,
            power_button_action: PowerAction::Shutdown,
        }
    }

    pub fn set_by_id(&mut self, id: u32, value: u32) -> Result<(), ()> {
        match id {
            0 => { self.display_timeout_minutes = value; Ok(()) }
            1 => { self.sleep_timeout_minutes = value; Ok(()) }
            2 => { self.hibernate_enabled = value != 0; Ok(()) }
            3 => { self.cpu_policy = CpuPolicy::from_u32(value).ok_or(())?; Ok(()) }
            4 => { self.lid_action = PowerAction::from_u32(value).ok_or(())?; Ok(()) }
            5 => { self.power_button_action = PowerAction::from_u32(value).ok_or(())?; Ok(()) }
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PowerPlan {
    pub name: String,
    pub policies: PowerPolicies,
}

impl PowerPlan {
    pub fn new(name: &str, policies: PowerPolicies) -> Self {
        PowerPlan {
            name: alloc::string::ToString::to_string(name),
            policies,
        }
    }
}
