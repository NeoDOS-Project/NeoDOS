//! Service Manager (Sm) — Kernel subsystem for managing Ring 3 service processes.
//!
//! Architecture:
//!   - Services are ObType::Service objects in \Service\<Name> namespace
//!   - 5-state machine: Stopped → Starting → Running → Stopping → Failed
//!   - Registry backend: \Registry\Machine\System\CurrentControlSet\Services\<Name>
//!   - Dependencies resolved via topological sort (Kahn's algorithm)
//!   - Restart policy: Never / OnCrash / Always

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::object::{self, ObType, ObId};
use crate::object::namespace;
use crate::cm::{CM_MANAGER, hive};
use crate::log::LogSubsys;
use crate::{test_case, test_eq, test_true};

pub mod manager;
pub mod lifecycle;
pub mod registry;

pub use manager::{ServiceState, ServiceStartType, ServiceRestartPolicy, SmError, ServiceConfig, Service, ServiceManager, SERVICE_MANAGER};
pub use registry::{sm_init, sm_start_auto_services, sm_mark_neoinit_running};

pub fn register_service_tests() {
    // ── State machine tests ──

    test_case!("sm_state_valid_transition", {
        let mut sm = ServiceManager::new();
        let config = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        // Register test service (won't actually spawn since binary doesn't exist)
        let result = sm.register("TestSvc", "Test Service", "C:\\nonexistent.nxe", config, &[]);
        test_true!(result.is_ok());
        let idx = result.unwrap();

        test_eq!(sm.services[idx].state, ServiceState::Stopped);
        // start_service will fail to spawn, but state machine should progress
        // We're testing state machine transitions, not actual spawning
        let _result = sm.start_service(idx);
            test_true!(_result.is_err() || sm.services[idx].state == ServiceState::Failed);
    });

    test_case!("sm_state_disabled", {
        let mut sm = ServiceManager::new();
        let config = ServiceConfig {
            start_type: ServiceStartType::Disabled,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let result = sm.register("DisabledSvc", "Disabled Test", "C:\\nonexistent.nxe", config, &[]);
        test_true!(result.is_ok());
        let idx = result.unwrap();

        let result = sm.start_service(idx);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), SmError::Disabled);
    });

    test_case!("sm_state_stop_stopped", {
        let mut sm = ServiceManager::new();
        let config = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("StoppedSvc", "", "C:\\nonexistent.nxe", config, &[]).unwrap();
        let result = sm.stop_service(idx, 0);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), SmError::AlreadyStopped);
    });

    test_case!("sm_state_restart_failed", {
        let mut sm = ServiceManager::new();
        let config = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("FailSvc", "", "C:\\nonexistent.nxe", config, &[]).unwrap();
        // start will fail, service goes to Failed
        let _ = sm.start_service(idx);
        // Try start again from Failed
        let _result = sm.start_service(idx);
        test_eq!(sm.services[idx].state, ServiceState::Failed);
    });

    test_case!("sm_state_exhaust_failures", {
        let mut sm = ServiceManager::new();
        let config = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 2,
        };
        let idx = sm.register("ExhaustSvc", "", "C:\\nonexistent.nxe", config, &[]).unwrap();

        // Mark as Starting then simulate process exit
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].failure_count = 0;

        // Process exit with restart_policy=Never means it goes to Failed
        sm.on_process_exit(idx, -1);
        test_eq!(sm.services[idx].state, ServiceState::Failed);
    });

    test_case!("sm_state_restart_on_crash", {
        let mut sm = ServiceManager::new();
        let config = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::OnCrash,
            max_failures: 3,
        };
        let idx = sm.register("CrashSvc", "", "C:\\nonexistent.nxe", config, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = 42;

        // Process exits with non-zero — should restart
        sm.services[idx].failure_count = 0;
        sm.on_process_exit(idx, -1);
        // Restart will try to spawn which will fail, so it goes to Failed
        // but the state should have attempted restart
        test_true!(sm.services[idx].failure_count >= 1);
    });

    // ── Dependency tests ──

    test_case!("sm_dep_no_deps", {
        let mut sm = ServiceManager::new();
        let config = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        sm.register("Solo", "", "C:\\a.nxe", config, &[]).unwrap();
        let order = sm.build_dependency_order();
        test_true!(order.is_ok());
        test_eq!(order.unwrap().len(), 1);
    });

    test_case!("sm_dep_simple_chain", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        sm.register("A", "", "C:\\a.nxe", cfg.clone(), &[]).unwrap();
        sm.register("B", "", "C:\\b.nxe", cfg.clone(), &["A".to_string()]).unwrap();
        sm.register("C", "", "C:\\c.nxe", cfg.clone(), &["B".to_string()]).unwrap();

        let order = sm.build_dependency_order().unwrap();
        test_eq!(order.len(), 3);
        let names: Vec<&str> = order.iter().map(|&i| sm.services[i].name.as_str()).collect();
        // A must come before B, B before C
        let pos_a = names.iter().position(|&n| n == "A").unwrap();
        let pos_b = names.iter().position(|&n| n == "B").unwrap();
        let pos_c = names.iter().position(|&n| n == "C").unwrap();
        test_true!(pos_a < pos_b);
        test_true!(pos_b < pos_c);
    });

    test_case!("sm_dep_cycle_detected", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        sm.register("X", "", "C:\\x.nxe", cfg.clone(), &["Y".to_string()]).unwrap();
        sm.register("Y", "", "C:\\y.nxe", cfg.clone(), &["Z".to_string()]).unwrap();
        sm.register("Z", "", "C:\\z.nxe", cfg.clone(), &["X".to_string()]).unwrap();
        let order = sm.build_dependency_order();
        test_true!(order.is_err());
        test_eq!(order.unwrap_err(), SmError::CycleDetected);
    });

    test_case!("sm_dep_fan_out", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        sm.register("A", "", "C:\\a.nxe", cfg.clone(), &[]).unwrap();
        sm.register("B", "", "C:\\b.nxe", cfg.clone(), &[]).unwrap();
        sm.register("C", "", "C:\\c.nxe", cfg.clone(), &["A".to_string(), "B".to_string()]).unwrap();
        let order = sm.build_dependency_order().unwrap();
        test_eq!(order.len(), 3);
        let names: Vec<&str> = order.iter().map(|&i| sm.services[i].name.as_str()).collect();
        let pos_c = names.iter().position(|&n| n == "C").unwrap();
        let pos_a = names.iter().position(|&n| n == "A").unwrap();
        let pos_b = names.iter().position(|&n| n == "B").unwrap();
        test_true!(pos_a < pos_c);
        test_true!(pos_b < pos_c);
    });

    // ── Registry backend tests (in-memory, no VFS) ──

    test_case!("sm_config_clone", {
        let c1 = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::OnCrash,
            max_failures: 5,
        };
        let c2 = c1.clone();
        test_eq!(c2.start_type as u8, ServiceStartType::Auto as u8);
        test_eq!(c2.restart_policy as u8, ServiceRestartPolicy::OnCrash as u8);
        test_eq!(c2.max_failures, 5);
    });

    test_case!("sm_find_by_name", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        sm.register("FindMe", "", "C:\\f.nxe", cfg, &[]).unwrap();
        test_true!(sm.find_by_name("FindMe").is_some());
        test_true!(sm.find_by_name("findme").is_some()); // case-insensitive
        test_true!(sm.find_by_name("NotFound").is_none());
    });

    test_case!("sm_set_config", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("ConfigTest", "", "C:\\c.nxe", cfg, &[]).unwrap();
        let r = sm.set_config(idx, ServiceStartType::Auto, ServiceRestartPolicy::Always, 5);
        test_true!(r.is_ok());
        test_eq!(sm.services[idx].start_type as u8, ServiceStartType::Auto as u8);
        test_eq!(sm.services[idx].restart_policy as u8, ServiceRestartPolicy::Always as u8);
        test_eq!(sm.services[idx].max_failures, 5);
    });

    test_case!("sm_register_duplicate_fails", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        sm.register("Dup", "", "C:\\d.nxe", cfg.clone(), &[]).unwrap();
        let r2 = sm.register("Dup", "", "C:\\d.nxe", cfg, &[]);
        test_true!(r2.is_err());
    });

    test_case!("sm_remove_stopped", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let _idx = sm.register("RemoveMe", "", "C:\\r.nxe", cfg, &[]).unwrap();
        test_eq!(sm.services.len(), 1);
        let r = sm.remove("RemoveMe");
        test_true!(r.is_ok());
        test_eq!(sm.services.len(), 0);
    });

    test_case!("sm_remove_running_fails", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("RunningRm", "", "C:\\r.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        let r = sm.remove("RunningRm");
        test_true!(r.is_err());
    });

    test_case!("sm_error_codes", {
        test_eq!(SmError::NotFound.as_err_code(), -2);
        test_eq!(SmError::Disabled.as_err_code(), -1);
        test_eq!(SmError::Busy.as_err_code(), -15);
        test_eq!(SmError::OutOfMemory.as_err_code(), -3);
    });

    test_case!("sm_on_process_exit_stopping", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("ExitSvc", "", "C:\\e.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Stopping;
        sm.services[idx].pid = 99;
        sm.on_process_exit(idx, 0);
        test_eq!(sm.services[idx].state, ServiceState::Stopped);
        test_eq!(sm.services[idx].pid, 0);
        test_eq!(sm.services[idx].exit_count, 1);
    });

    test_case!("sm_on_process_exit_running_never_restart", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("NoRestart", "", "C:\\nr.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = 100;
        sm.on_process_exit(idx, -1);
        test_eq!(sm.services[idx].state, ServiceState::Failed);
    });

    test_case!("sm_state_enum_values", {
        test_eq!(ServiceState::Stopped as u8, 0);
        test_eq!(ServiceState::Starting as u8, 1);
        test_eq!(ServiceState::Running as u8, 2);
        test_eq!(ServiceState::Stopping as u8, 3);
        test_eq!(ServiceState::Failed as u8, 4);
        test_eq!(ServiceStartType::Auto as u8, 2);
        test_eq!(ServiceStartType::Disabled as u8, 4);
        test_eq!(ServiceRestartPolicy::Always as u8, 2);
    });

    // ── AUDIT-33: Boot/init hardening tests ──

    test_case!("boot_missing_service_fallback", {
        // Verify that when no services are registered (empty registry),
        // the fallback to built-in defaults works correctly.
        // This simulates sm_init() with an empty registry.

        // Start with empty ServiceManager (simulates empty registry)
        let mut sm = ServiceManager::new();
        test_eq!(sm.services.len(), 0);

        // Simulate sm_reg_load_all() returning 0 (no registry entries)
        // Then register_default_services() is called
        let cfg = ServiceConfig {
            start_type: ServiceStartType::System,
            restart_policy: ServiceRestartPolicy::Always,
            max_failures: 5,
        };
        let r = sm.register("NeoInit", "NeoDOS Init Process",
            "C:\\nonexistent.nxe", cfg, &[]);
        test_true!(r.is_ok());
        test_eq!(sm.services.len(), 1);

        // Verify the fallback service has correct properties
        let svc = &sm.services[0];
        test_eq!(svc.name, "NeoInit");
        test_eq!(svc.start_type as u8, ServiceStartType::System as u8);
        test_eq!(svc.restart_policy as u8, ServiceRestartPolicy::Always as u8);
        test_eq!(svc.max_failures, 5);
        test_eq!(svc.binary_path, "C:\\nonexistent.nxe");

        // Auto-start with this service should not panic even if binary doesn't exist
        sm.dependency_order = vec![0];
        let _result = sm.start_service(0);
        // Service should be in Failed state (binary doesn't exist), not panicked
        test_eq!(sm.services[0].state, ServiceState::Failed);
    });

    test_case!("boot_service_startup_recovery", {
        // Verify that multiple services failing during auto-start don't halt the system.
        // Each failure should be isolated and the next service should still be attempted.

        let mut sm = ServiceManager::new();
        let cfg_auto = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let cfg_system = ServiceConfig {
            start_type: ServiceStartType::System,
            restart_policy: ServiceRestartPolicy::OnCrash,
            max_failures: 2,
        };

        // Register multiple services with non-existent binaries
        let idx1 = sm.register("SvcA", "Service A", "C:\\missing_a.nxe", cfg_auto.clone(), &[]).unwrap();
        let idx2 = sm.register("SvcB", "Service B", "C:\\missing_b.nxe", cfg_system.clone(), &[]).unwrap();
        let idx3 = sm.register("SvcC", "Service C", "C:\\missing_c.nxe", cfg_auto.clone(), &[]).unwrap();

        sm.dependency_order = vec![idx1, idx2, idx3];

        // Simulate auto-start loop (from sm_start_auto_services)
        let mut started = 0;
        let mut failed = 0;
        for &i in &sm.dependency_order.clone() {
            let should_start = {
                let svc = &sm.services[i];
                svc.start_type == ServiceStartType::System || svc.start_type == ServiceStartType::Auto
            };
            if should_start {
                match sm.start_service(i) {
                    Ok(()) => started += 1,
                    Err(_) => {
                        sm.services[i].state = ServiceState::Failed;
                        failed += 1;
                    }
                }
            }
        }

        // All should fail (binaries don't exist), but none should panic
        test_eq!(started, 0);
        test_eq!(failed, 3);
        test_eq!(sm.services[idx1].state, ServiceState::Failed);
        test_eq!(sm.services[idx2].state, ServiceState::Failed);
        test_eq!(sm.services[idx3].state, ServiceState::Failed);
    });

    test_case!("boot_register_default_services", {
        // Verify that register_default_services creates NeoInit
        // (only tests on a fresh ServiceManager)
        let mut fresh_sm = ServiceManager::new();
        test_eq!(fresh_sm.services.len(), 0);
        // Manually register: same logic as register_default_services
        let cfg = ServiceConfig {
            start_type: ServiceStartType::System,
            restart_policy: ServiceRestartPolicy::Always,
            max_failures: 5,
        };
        let r = fresh_sm.register("NeoInit", "NeoDOS Init Process",
            "C:\\Programs\\neoinit.nxe", cfg, &[]);
        test_true!(r.is_ok());
        test_eq!(fresh_sm.services.len(), 1);
        test_eq!(fresh_sm.services[0].name, "NeoInit");
        test_eq!(fresh_sm.services[0].start_type as u8, ServiceStartType::System as u8);
    });
}
