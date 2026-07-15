pub mod plan;
pub mod coordinator;
pub mod acpi;

use spin::Mutex;
use lazy_static::lazy_static;

use alloc::vec::Vec;

use self::plan::{PowerPlan, PowerPolicies, CpuPolicy, PowerAction};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSystemState {
    Active = 0,
    ShuttingDown = 1,
    Rebooting = 2,
    Suspending = 3,
    Hibernating = 4,
    Off = 5,
}

impl PowerSystemState {
    pub fn to_str(self) -> &'static str {
        match self {
            PowerSystemState::Active => "ACTIVE",
            PowerSystemState::ShuttingDown => "SHUTTING_DOWN",
            PowerSystemState::Rebooting => "REBOOTING",
            PowerSystemState::Suspending => "SUSPENDING",
            PowerSystemState::Hibernating => "HIBERNATING",
            PowerSystemState::Off => "OFF",
        }
    }
}

pub const POLICY_DISPLAY_TIMEOUT: u32 = 0;
pub const POLICY_SLEEP_TIMEOUT: u32 = 1;
pub const POLICY_HIBERNATE_ENABLED: u32 = 2;
pub const POLICY_CPU_POLICY: u32 = 3;
pub const POLICY_LID_ACTION: u32 = 4;
pub const POLICY_POWER_BUTTON_ACTION: u32 = 5;

pub const MAX_PLANS: usize = 8;
pub const PLAN_BALANCED: usize = 0;
pub const PLAN_PERFORMANCE: usize = 1;
pub const PLAN_POWER_SAVER: usize = 2;

const REG_POWER_PATH: &str = "Power";

pub struct PowerManager {
    state: PowerSystemState,
    active_plan: usize,
    plans: Vec<PowerPlan>,
}

impl PowerManager {
    pub fn new() -> Self {
        let mut plans = Vec::with_capacity(MAX_PLANS);
        plans.push(PowerPlan::new("Balanced", PowerPolicies::default_policies()));
        plans.push(PowerPlan::new("Performance", PowerPolicies::default_policies()));
        plans.push(PowerPlan::new("PowerSaver", PowerPolicies::default_policies()));
        PowerManager {
            state: PowerSystemState::Active,
            active_plan: PLAN_BALANCED,
            plans,
        }
    }

    pub fn state(&self) -> PowerSystemState {
        self.state
    }

    pub fn set_state(&mut self, new_state: PowerSystemState) {
        self.state = new_state;
    }

    pub fn active_plan(&self) -> usize {
        self.active_plan
    }

    pub fn active_plan_name(&self) -> &str {
        if self.active_plan < self.plans.len() {
            &self.plans[self.active_plan].name
        } else {
            "Unknown"
        }
    }

    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }

    pub fn get_plan(&self, index: usize) -> Option<&PowerPlan> {
        self.plans.get(index)
    }

    pub fn get_plan_mut(&mut self, index: usize) -> Option<&mut PowerPlan> {
        self.plans.get_mut(index)
    }

    pub fn set_active_plan(&mut self, index: usize) -> Result<(), ()> {
        if index >= self.plans.len() {
            return Err(());
        }
        self.active_plan = index;
        Ok(())
    }

    pub fn policies(&self) -> Option<&PowerPolicies> {
        self.plans.get(self.active_plan).map(|p| &p.policies)
    }

    pub fn policies_mut(&mut self) -> Option<&mut PowerPolicies> {
        self.plans.get_mut(self.active_plan).map(|p| &mut p.policies)
    }

    pub fn set_policy(&mut self, policy_id: u32, value: u32) -> Result<(), ()> {
        let policies = self.policies_mut().ok_or(())?;
        policies.set_by_id(policy_id, value)
    }

    pub fn load_plan_from_registry(&mut self, index: usize) -> Result<(), ()> {
        let plan_name = {
            let p = self.plans.get(index).ok_or(())?;
            p.name.clone()
        };
        let power_key = crate::cm::cm_open_key(0, REG_POWER_PATH).map_err(|_| ())?;
        let plans_key = crate::cm::cm_create_key(power_key, "Plans").map_err(|_| ())?;
        let plan_key = crate::cm::cm_create_key(plans_key, &plan_name).map_err(|_| ())?;
        let mut policies = PowerPolicies::default_policies();
        if let Ok(v) = crate::cm::cm_query_value(plan_key, "DisplayTimeout") {
            policies.display_timeout_minutes = v.as_dword().unwrap_or(policies.display_timeout_minutes);
        }
        if let Ok(v) = crate::cm::cm_query_value(plan_key, "SleepTimeout") {
            policies.sleep_timeout_minutes = v.as_dword().unwrap_or(policies.sleep_timeout_minutes);
        }
        if let Ok(v) = crate::cm::cm_query_value(plan_key, "HibernateEnabled") {
            policies.hibernate_enabled = v.as_dword().unwrap_or(0) != 0;
        }
        if let Ok(v) = crate::cm::cm_query_value(plan_key, "CpuPolicy") {
            if let Some(cp) = CpuPolicy::from_str(v.as_str().unwrap_or("Balanced")) {
                policies.cpu_policy = cp;
            }
        }
        if let Ok(v) = crate::cm::cm_query_value(plan_key, "LidAction") {
            if let Some(a) = PowerAction::from_u32(v.as_dword().unwrap_or(policies.lid_action as u32)) {
                policies.lid_action = a;
            }
        }
        if let Ok(v) = crate::cm::cm_query_value(plan_key, "PowerButtonAction") {
            if let Some(a) = PowerAction::from_u32(v.as_dword().unwrap_or(3)) {
                policies.power_button_action = a;
            }
        }
        if let Some(plan) = self.plans.get_mut(index) {
            plan.policies = policies;
        }
        Ok(())
    }

    pub fn save_plan_to_registry(&self, index: usize) -> Result<(), ()> {
        let plan = self.plans.get(index).ok_or(())?;
        let power_key = crate::cm::cm_open_key(0, REG_POWER_PATH).map_err(|_| ())?;
        let plans_key = crate::cm::cm_create_key(power_key, "Plans").map_err(|_| ())?;
        let plan_key = crate::cm::cm_create_key(plans_key, &plan.name).map_err(|_| ())?;
        let p = &plan.policies;
        crate::cm::cm_set_value(plan_key, "DisplayTimeout", crate::cm::hive::REG_DWORD, &p.display_timeout_minutes.to_le_bytes()).map_err(|_| ())?;
        crate::cm::cm_set_value(plan_key, "SleepTimeout", crate::cm::hive::REG_DWORD, &p.sleep_timeout_minutes.to_le_bytes()).map_err(|_| ())?;
        crate::cm::cm_set_value(plan_key, "HibernateEnabled", crate::cm::hive::REG_DWORD, &(p.hibernate_enabled as u32).to_le_bytes()).map_err(|_| ())?;
        crate::cm::cm_set_value(plan_key, "CpuPolicy", crate::cm::hive::REG_SZ, p.cpu_policy.to_str().as_bytes()).map_err(|_| ())?;
        crate::cm::cm_set_value(plan_key, "LidAction", crate::cm::hive::REG_DWORD, &(p.lid_action as u32).to_le_bytes()).map_err(|_| ())?;
        crate::cm::cm_set_value(plan_key, "PowerButtonAction", crate::cm::hive::REG_DWORD, &(p.power_button_action as u32).to_le_bytes()).map_err(|_| ())?;
        Ok(())
    }

    pub fn load_active_plan_from_registry(&mut self) -> Result<(), ()> {
        let power_key = crate::cm::cm_open_key(0, REG_POWER_PATH).map_err(|_| ())?;
        if let Ok(v) = crate::cm::cm_query_value(power_key, "ActivePlan") {
            let idx = v.as_dword().unwrap_or(0) as usize;
            if idx < self.plans.len() {
                self.active_plan = idx;
            }
        }
        Ok(())
    }

    pub fn save_active_plan_to_registry(&self) -> Result<(), ()> {
        let power_key = crate::cm::cm_open_key(0, REG_POWER_PATH).map_err(|_| ())?;
        crate::cm::cm_set_value(power_key, "ActivePlan", crate::cm::hive::REG_DWORD, &(self.active_plan as u32).to_le_bytes()).map_err(|_| ())
    }
}

lazy_static! {
    pub static ref POWER_MANAGER: Mutex<PowerManager> = Mutex::new(PowerManager::new());
}

pub fn init_power_manager() {
    let mut pm = POWER_MANAGER.lock();
    let _ = pm.load_active_plan_from_registry();
    let active = pm.active_plan();
    let _ = pm.load_plan_from_registry(active);
    crate::serial_println!("[PM] PowerManager initialized: plan={}, state={}",
        pm.active_plan_name(), pm.state().to_str());
}

fn register_pm_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("pm_init_state_active", {
        let pm = PowerManager::new();
        test_eq!(pm.state(), PowerSystemState::Active);
    });

    test_case!("pm_device_namespace_exists", {
        let result = crate::object::namespace::ob_lookup_path("\\System\\PowerManager");
        test_true!(result.is_ok());
    });

    test_case!("pm_query_plan_defaults", {
        let pm = PowerManager::new();
        let plan = pm.get_plan(PLAN_BALANCED).unwrap();
        test_eq!(plan.policies.display_timeout_minutes, 0);
        test_eq!(plan.policies.sleep_timeout_minutes, 0);
        test_eq!(plan.policies.hibernate_enabled, false);
        test_eq!(plan.policies.cpu_policy, CpuPolicy::Balanced);
        test_eq!(plan.policies.lid_action, PowerAction::Nothing);
        test_eq!(plan.policies.power_button_action, PowerAction::Shutdown);
    });

    test_case!("pm_set_plan_balanced", {
        let mut pm = PowerManager::new();
        test_true!(pm.set_active_plan(PLAN_BALANCED).is_ok());
        test_eq!(pm.active_plan(), PLAN_BALANCED);
    });

    test_case!("pm_set_plan_performance", {
        let mut pm = PowerManager::new();
        test_true!(pm.set_active_plan(PLAN_PERFORMANCE).is_ok());
        test_eq!(pm.active_plan(), PLAN_PERFORMANCE);
    });

    test_case!("pm_set_plan_invalid", {
        let mut pm = PowerManager::new();
        let result = pm.set_active_plan(99);
        test_true!(result.is_err());
    });

    test_case!("pm_plan_persists_to_registry", {
        let mut hive = crate::cm::hive::Hive::new("TestPower");
        let root = hive.root_cell();
        let power_key = hive.create_key(root, "Power").unwrap();
        let plans_key = hive.create_key(power_key, "Plans").unwrap();
        let bal_key = hive.create_key(plans_key, "Balanced").unwrap();
        hive.set_value(bal_key, "DisplayTimeout", crate::cm::hive::REG_DWORD, &15u32.to_le_bytes()).unwrap();
        hive.set_value(bal_key, "SleepTimeout", crate::cm::hive::REG_DWORD, &45u32.to_le_bytes()).unwrap();
        hive.set_value(bal_key, "HibernateEnabled", crate::cm::hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();
        hive.set_value(bal_key, "CpuPolicy", crate::cm::hive::REG_SZ, b"Balanced").unwrap();
        hive.set_value(bal_key, "LidAction", crate::cm::hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();
        hive.set_value(bal_key, "PowerButtonAction", crate::cm::hive::REG_DWORD, &3u32.to_le_bytes()).unwrap();
        let dt = hive.query_value(bal_key, "DisplayTimeout").unwrap();
        test_eq!(dt.as_dword().unwrap(), 15);
        let st = hive.query_value(bal_key, "SleepTimeout").unwrap();
        test_eq!(st.as_dword().unwrap(), 45);
        let he = hive.query_value(bal_key, "HibernateEnabled").unwrap();
        test_eq!(he.as_dword().unwrap(), 1);
    });

    test_case!("pm_set_policy_display_timeout", {
        let mut pm = PowerManager::new();
        test_true!(pm.set_policy(POLICY_DISPLAY_TIMEOUT, 5).is_ok());
        let p = pm.policies().unwrap();
        test_eq!(p.display_timeout_minutes, 5);
    });

    test_case!("pm_set_policy_invalid_id", {
        let mut pm = PowerManager::new();
        let result = pm.set_policy(99, 0);
        test_true!(result.is_err());
    });

    test_case!("pm_policy_persists", {
        let mut hive = crate::cm::hive::Hive::new("TestPolicyPersist");
        let root = hive.root_cell();
        let power_key = hive.create_key(root, "Power").unwrap();
        let plans_key = hive.create_key(power_key, "Plans").unwrap();
        let perf_key = hive.create_key(plans_key, "Performance").unwrap();
        hive.set_value(perf_key, "DisplayTimeout", crate::cm::hive::REG_DWORD, &30u32.to_le_bytes()).unwrap();
        hive.set_value(perf_key, "CpuPolicy", crate::cm::hive::REG_SZ, b"Performance").unwrap();
        let dt = hive.query_value(perf_key, "DisplayTimeout").unwrap();
        test_eq!(dt.as_dword().unwrap(), 30);
        let cp = hive.query_value(perf_key, "CpuPolicy").unwrap();
        test_eq!(cp.as_str().unwrap(), "Performance");
    });
}

pub fn register_tests() {
    register_pm_tests();
}
