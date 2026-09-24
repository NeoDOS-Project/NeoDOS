//! Service registry — extracted from mod.rs (free functions)
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::object::{self, ObType, ObId};
use crate::object::namespace;
use crate::cm::{CM_MANAGER, hive};
use crate::log::LogSubsys;
use crate::services::manager::{ServiceManager, Service, ServiceConfig, ServiceState, ServiceStartType, ServiceRestartPolicy, SmError, SERVICE_MANAGER};

fn register_default_services() {
    let mut sm = SERVICE_MANAGER.lock();
    if !sm.services.is_empty() {
        return; // already have services
    }

    // NeoInit — PID 1 init process (System start type)
    let neoinit_cfg = ServiceConfig {
        start_type: ServiceStartType::System,
        restart_policy: ServiceRestartPolicy::Always,
        max_failures: 5,
    };
    let _ = sm.register("NeoInit", "NeoDOS Init Process",
        "C:\\Programs\\neoinit.nxe", neoinit_cfg, &[]);
}

/// Initialize the Service Manager. Called during Phase 3.882 (after Registry init).
/// Creates \Service\ namespace directory and loads configured services from Registry.
pub fn sm_init() {
    let _ = namespace::ob_create_directory("\\Service");

    // Load services from Registry
    let loaded = sm_reg_load_all();
    if loaded == 0 {
        // Tolerance: if registry is empty/missing, register built-in defaults
        kwarn!(LogSubsys::Services, "Registry has no services — registering built-in defaults");
        register_default_services();
    }
    kinfo!(LogSubsys::Services, "Service Manager initialized ({} services loaded)", loaded);

    // Build dependency order
    let mut sm = SERVICE_MANAGER.lock();
    match sm.build_dependency_order() {
        Ok(order) => {
            let len = order.len();
            sm.dependency_order = order;
            kinfo!(LogSubsys::Services, "Dependency order resolved ({} services)", len);
        }
        Err(e) => {
            kerror!(LogSubsys::Services, "Dependency resolution failed: {:?}", e);
        }
    }
}

/// Load all services from Registry into the ServiceManager.
fn sm_reg_load_all() -> usize {
    let service_names: Vec<String> = {
        let cm = CM_MANAGER.lock();
        if cm.hives.is_empty() { return 0; }
        let hm = &cm.hives[0];
        let root = hm.hive.root_cell();
        let svc_key = match hm.hive.open_key_by_path(root, "CurrentControlSet\\Services") {
            Some(k) => k,
            None => return 0,
        };
        let mut names = Vec::new();
        let count = hm.hive.key_count(svc_key);
        for i in 0..count {
            if let Some(n) = hm.hive.enum_key(svc_key, i) {
                if !n.is_empty() {
                    names.push(n);
                }
            }
        }
        names
    };

    let mut count = 0;
    for name in &service_names {
        if let Some((display_name, binary_path, config, deps)) =
            ServiceManager::read_registry_config(name)
        {
            if !binary_path.is_empty() {
                let mut sm = SERVICE_MANAGER.lock();
                if sm.register(name, &display_name, &binary_path, config, &deps).is_ok() {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Start all auto-start and system services in dependency order.
pub fn sm_start_auto_services() {
    let order = {
        let sm = SERVICE_MANAGER.lock();
        sm.dependency_order.clone()
    };

    if !order.is_empty() {
        kinfo!(LogSubsys::Services, "Starting auto/system services...");
        let mut started = 0;
        let mut failed = 0;

        for &idx in &order {
            let should_start = {
                let sm = SERVICE_MANAGER.lock();
                if idx >= sm.services.len() {
                    continue;
                }
                let svc = &sm.services[idx];
                svc.start_type == ServiceStartType::System || svc.start_type == ServiceStartType::Auto
            };
            if should_start {
                crate::serial_println!("[SM] start service idx={}", idx);
                let mut sm = SERVICE_MANAGER.lock();
                match sm.start_service(idx) {
                    Ok(()) => {
                        crate::serial_println!("[SM] started: {}", sm.services[idx].name);
                        kinfo!(LogSubsys::Services, "Started: {}", sm.services[idx].name);
                        started += 1;
                    }
                    Err(e) => {
                        sm.services[idx].state = ServiceState::Failed;
                        crate::serial_println!("[SM] FAILED to start {}: {:?}", sm.services[idx].name, e);
                        kerror!(LogSubsys::Services, "Failed to start {}: {:?}", sm.services[idx].name, e);
                        failed += 1;
                    }
                }
            }
        }
        crate::serial_println!("[SM] auto-start complete: {} started, {} failed", started, failed);
        kinfo!(LogSubsys::Services, "Auto-start complete: {} started, {} failed", started, failed);
    } else {
        kwarn!(LogSubsys::Services, "No services in dependency order (dependency resolution may have failed)");
    }
}

/// Mark the NeoInit service as Running with the given PID.
/// Called by the kernel after manually spawning NeoInit as PID 1,
/// before sm_start_auto_services() tries to start it again.
pub fn sm_mark_neoinit_running(pid: u32) {
    let mut sm = SERVICE_MANAGER.lock();
    if let Some(idx) = sm.find_by_name("NeoInit") {
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = pid;
        kinfo!(LogSubsys::Services, "NeoInit already running (PID {}) via manual kernel spawn", pid);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

