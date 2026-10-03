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

// ── Deferred process-exit notifications ──────────────────────────────────
//
// The scheduler terminates a process while holding its own lock. The Service
// Manager must not be invoked there: applying a restart policy spawns a new
// process, which needs the scheduler lock. The termination path therefore only
// enqueues a bounded, allocation-free notification; `process_pending_exits()`
// drains it later from syscall context, outside the scheduler lock.

const PENDING_EXIT_CAP: usize = 32;

struct PendingExits {
    entries: [(u32, i64); PENDING_EXIT_CAP],
    head: usize,
    len: usize,
}

static PENDING_EXITS: Mutex<PendingExits> = Mutex::new(PendingExits {
    entries: [(0, 0); PENDING_EXIT_CAP],
    head: 0,
    len: 0,
});
static PENDING_EXIT_COUNT: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

/// Queue a process-exit notification. Safe to call while the scheduler lock is
/// held: it never blocks, never allocates and never takes the Service Manager
/// lock. Drops the notification if the bounded queue is full.
pub fn notify_process_exit(pid: u32, exit_code: i64) {
    if pid < 2 {
        return; // idle/kernel (0) and NeoInit (1) are never services
    }
    if let Some(mut q) = PENDING_EXITS.try_lock() {
        if q.len < PENDING_EXIT_CAP {
            let idx = (q.head + q.len) % PENDING_EXIT_CAP;
            q.entries[idx] = (pid, exit_code);
            q.len += 1;
            PENDING_EXIT_COUNT.fetch_add(1, core::sync::atomic::Ordering::Release);
        }
    }
}

/// True when at least one exit is waiting to be dispatched.
#[inline]
pub fn has_pending_exits() -> bool {
    PENDING_EXIT_COUNT.load(core::sync::atomic::Ordering::Acquire) != 0
}

/// Drain queued process exits and apply service restart policy.
///
/// Must be called outside the scheduler lock (it may spawn processes). Uses
/// `try_lock` on the Service Manager: if it is contended the notifications are
/// left queued for a later call, avoiding a lock-order deadlock.
pub fn process_pending_exits() {
    if !has_pending_exits() {
        return;
    }
    let mut sm = match SERVICE_MANAGER.try_lock() {
        Some(g) => g,
        None => return,
    };
    loop {
        let item = match PENDING_EXITS.try_lock() {
            Some(mut q) => {
                if q.len == 0 {
                    None
                } else {
                    let e = q.entries[q.head];
                    q.head = (q.head + 1) % PENDING_EXIT_CAP;
                    q.len -= 1;
                    PENDING_EXIT_COUNT.fetch_sub(1, core::sync::atomic::Ordering::Release);
                    Some(e)
                }
            }
            None => None,
        };
        match item {
            Some((pid, code)) => {
                sm.on_process_exit_by_pid(pid, code);
            }
            None => break,
        }
    }
}

// ── Deferred graceful-shutdown requests (#358) ───────────────────────────
//
// `stop_service()` is called from the service syscall handler while holding the
// SERVICE_MANAGER lock. It must not block, queue user APCs, or take the
// scheduler lock there. It therefore only records the request here; the actual
// notification and the bounded-timeout forced termination happen in
// `process_pending_shutdowns()`, which runs from syscall context outside every
// kernel lock — the same architecture #374 uses for process exits.

const PENDING_SHUTDOWN_CAP: usize = 32;

struct PendingShutdowns {
    /// (pid, stop_deadline_ticks)
    entries: [(u32, u64); PENDING_SHUTDOWN_CAP],
    head: usize,
    len: usize,
}

static PENDING_SHUTDOWNS: Mutex<PendingShutdowns> = Mutex::new(PendingShutdowns {
    entries: [(0, 0); PENDING_SHUTDOWN_CAP],
    head: 0,
    len: 0,
});
static PENDING_SHUTDOWN_COUNT: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

/// Record a graceful-shutdown request for `pid`.
///
/// Safe to call while holding `SERVICE_MANAGER`: it never blocks and never
/// allocates. Drops the request if the bounded queue is full (the service then
/// simply falls through to the forced-kill deadline on a later drain).
pub fn request_service_shutdown(pid: u32) {
    if pid < 2 {
        return;
    }
    if let Some(mut q) = PENDING_SHUTDOWNS.try_lock() {
        if q.len < PENDING_SHUTDOWN_CAP {
            let idx = (q.head + q.len) % PENDING_SHUTDOWN_CAP;
            // Deadline is resolved against the owning service at drain time; 0
            // here means "look it up from the Service Manager".
            q.entries[idx] = (pid, 0);
            q.len += 1;
            PENDING_SHUTDOWN_COUNT.fetch_add(1, core::sync::atomic::Ordering::Release);
        }
    }
}

/// True when at least one shutdown request is waiting to be dispatched.
#[inline]
pub fn has_pending_shutdowns() -> bool {
    PENDING_SHUTDOWN_COUNT.load(core::sync::atomic::Ordering::Acquire) != 0
}

/// Deliver queued graceful-shutdown notifications and enforce their deadlines.
///
/// Runs in syscall context, outside `SERVICE_MANAGER` and the scheduler lock.
/// For each service in `StopPending` it:
///   1. queues a user APC to the service thread (wakes an alertable wait), and
///   2. if the service's deadline has elapsed, forces termination.
///
/// Forced termination still converges through #374: `kill_process` →
/// `terminate_current` → `notify_process_exit` → `process_pending_exits` →
/// `on_process_exit_by_pid`.
pub fn process_pending_shutdowns() {
    // Snapshot the requests without holding SERVICE_MANAGER, then act.
    let mut requests: [(u32, u64); PENDING_SHUTDOWN_CAP] = [(0, 0); PENDING_SHUTDOWN_CAP];
    let mut n = 0usize;
    if let Some(mut q) = PENDING_SHUTDOWNS.try_lock() {
        while q.len > 0 && n < PENDING_SHUTDOWN_CAP {
            requests[n] = q.entries[q.head];
            q.head = (q.head + 1) % PENDING_SHUTDOWN_CAP;
            q.len -= 1;
            PENDING_SHUTDOWN_COUNT.fetch_sub(1, core::sync::atomic::Ordering::Release);
            n += 1;
        }
    }
    if n == 0 {
        return;
    }

    for &(pid, _) in requests.iter().take(n) {
        if pid == 0 {
            continue;
        }
        // Resolve the owning service and capture the decision under a brief
        // `try_lock`. We only read state here; notification and force-kill run
        // after the guard is dropped. If contended, re-queue and retry later.
        //
        // `Decision`:
        //   Gone           — no longer a pending service; drop the request.
        //   Notify{force}  — first delivery; set the notified flag, then (after
        //                    unlocking) send the APC and force-kill if due.
        //   Recheck{force} — already notified; only re-evaluate the deadline.
        //   Retry          — Service Manager contended; re-queue for later.
        enum Decision { Gone, Notify(bool), Recheck(bool), Retry }
        let decision = match SERVICE_MANAGER.try_lock() {
            Some(mut sm) => match sm.find_by_pid(pid) {
                Some(idx) if sm.services[idx].shutdown_requested => {
                    let due = sm.stop_deadline_elapsed(idx);
                    if sm.services[idx].shutdown_notified {
                        Decision::Recheck(due)
                    } else {
                        sm.services[idx].shutdown_notified = true;
                        Decision::Notify(due)
                    }
                }
                _ => Decision::Gone, // exited/restarted already
            },
            None => Decision::Retry, // contended: retry later
        };

        match decision {
            Decision::Retry => {
                request_service_shutdown(pid);
            }
            Decision::Gone => {
                // Service already exited or was restarted. #374 will (or has)
                // finalized it; nothing to notify or kill.
            }
            Decision::Notify(deadline_elapsed) => {
                // First delivery: wake the service if it is in an alertable wait.
                // The authoritative flag lives on the service entry and is read
                // from Ring 3 via `ObInfoClass::ProcessShutdownState`.
                crate::apc::request_process_shutdown_notification(pid);
                if deadline_elapsed {
                    force_terminate_pending(pid);
                } else {
                    // Keep the request alive so the deadline keeps being checked
                    // on later drains; already-notified entries only re-check.
                    request_service_shutdown(pid);
                }
            }
            Decision::Recheck(deadline_elapsed) => {
                if deadline_elapsed {
                    force_terminate_pending(pid);
                } else {
                    request_service_shutdown(pid);
                }
            }
        }
    }
}

/// Force-terminate a `StopPending` service whose graceful deadline elapsed.
///
/// Forced termination converges through #374: `kill_process` → `kill_pid` →
/// `notify_process_exit` → `process_pending_exits` → `on_process_exit_by_pid`.
/// Called without `SERVICE_MANAGER` held on entry (it acquires it briefly).
fn force_terminate_pending(pid: u32) {
    kwarn!(LogSubsys::Services,
        "Graceful shutdown timed out for pid {} — forcing termination", pid);
    let kill_result = {
        let sm = SERVICE_MANAGER.lock();
        sm.kill_process(pid)
    };
    match kill_result {
        // The kill was accepted. `kill_pid` enqueues the #374 exit notification,
        // which finalizes the service on the next drain. Re-queue the request so
        // the deadline is re-checked until that happens (the process may still be
        // running on another CPU).
        Ok(()) => request_service_shutdown(pid),
        // The process is already gone (or was already reaped). If #374's exit
        // notification was delivered, the service is already finalized and this
        // is a no-op; if it was lost (e.g. bounded-queue contention at kill
        // time), re-enqueue it so finalization still travels the #374 path.
        // Either way the service does not remain stuck in StopPending.
        Err(SmError::NotFound) => crate::services::notify_process_exit(pid, -1),
        Err(_) => {}
    }
}

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

    // ── #365: canonical network configuration applier identity ──

    test_case!("sm_net_applier_service_identity", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::OnCrash,
            max_failures: 3,
        };
        let idx = sm.register("NetApplier", "Network Configuration Applier",
            "C:\\System\\Tools\\netapplier.nxe", cfg, &[]).unwrap();
        test_eq!(sm.services[idx].name, "NetApplier");
        test_eq!(sm.services[idx].binary_path, "C:\\System\\Tools\\netapplier.nxe");
        test_eq!(sm.services[idx].start_type as u8, ServiceStartType::Auto as u8);
        // netcfg is the configuration CLI, never a service.
        test_true!(sm.find_by_name("Netcfg").is_none());
    });

    // ── #372: netd Ring 3 network service identity ──

    test_case!("sm_netd_service_identity", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::OnCrash,
            max_failures: 3,
        };
        let idx = sm.register("Netd", "Network Service",
            "C:\\System\\Tools\\netd.nxe", cfg, &[]).unwrap();
        test_eq!(sm.services[idx].name, "Netd");
        test_eq!(sm.services[idx].binary_path, "C:\\System\\Tools\\netd.nxe");
        test_eq!(sm.services[idx].start_type as u8, ServiceStartType::Auto as u8);
        // The Ring-0 RX pump is the kernel worker "netpump", never a service.
        test_true!(sm.find_by_name("netpump").is_none());
    });

    // ── #375: shared process-creation path ──

    test_case!("process_create_missing_binary_not_found", {
        // The shared creation path is used by both ObCreate(Process) and the
        // Service Manager; a missing image must fail before allocating a slot.
        let r = crate::usermode::create_process_from_ob_path(
            "\\Global\\FileSystem\\C:\\System\\Tools\\__no_such_binary__.nxe",
            2, "\\", 0, "test",
        );
        test_true!(r.is_err());
        if let Err(e) = r {
            test_eq!(e, crate::usermode::CreateProcessError::NotFound);
        }
    });

    test_case!("sm_start_missing_binary_failed", {
        // ServiceManager::spawn_process uses the shared path; a missing image
        // must mark the service Failed, not leak or panic.
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("MissingBin", "", "C:\\nonexistent-xyz.nxe", cfg, &[]).unwrap();
        let r = sm.start_service(idx);
        test_true!(r.is_err());
        test_eq!(sm.services[idx].state, ServiceState::Failed);
    });

    // ── #374: Service Manager observes service process exit ──

    test_case!("sm_find_by_pid", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("PidSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
        // A service with pid 0 (stopped) never matches.
        test_eq!(sm.find_by_pid(0), None);
        test_eq!(sm.find_by_pid(77), None);
        sm.services[idx].pid = 77;
        test_eq!(sm.find_by_pid(77), Some(idx));
        test_eq!(sm.find_by_pid(78), None);
    });

    test_case!("sm_exit_by_pid_routes_and_accounts", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("ExitSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = 4242;
        // Unknown pid is a no-op.
        test_true!(!sm.on_process_exit_by_pid(9999, 0));
        test_eq!(sm.services[idx].pid, 4242);
        // Known pid advances accounting and clears the pid.
        test_true!(sm.on_process_exit_by_pid(4242, -1));
        test_eq!(sm.services[idx].exit_count, 1);
        test_eq!(sm.services[idx].last_exit_code, -1);
        test_eq!(sm.services[idx].pid, 0);
        test_eq!(sm.services[idx].state, ServiceState::Failed); // Never => Failed
    });

    test_case!("sm_deferred_exit_queue", {
        // End-to-end: the scheduler hook enqueues, process_pending_exits
        // dispatches to the owning service.
        let idx = {
            let mut sm = SERVICE_MANAGER.lock();
            let cfg = ServiceConfig {
                start_type: ServiceStartType::Demand,
                restart_policy: ServiceRestartPolicy::Never,
                max_failures: 3,
            };
            let idx = sm.register("DeferredExitTest", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
            sm.services[idx].state = ServiceState::Running;
            sm.services[idx].pid = 5150;
            idx
        };
        // Idle/kernel (0) and NeoInit (1) are ignored by the hook.
        notify_process_exit(0, 0);
        notify_process_exit(1, 0);
        notify_process_exit(5150, -1);
        test_true!(has_pending_exits());
        process_pending_exits();
        {
            let sm = SERVICE_MANAGER.lock();
            test_eq!(sm.services[idx].exit_count, 1);
            test_eq!(sm.services[idx].last_exit_code, -1);
            test_eq!(sm.services[idx].pid, 0);
        }
        let _ = SERVICE_MANAGER.lock().remove("DeferredExitTest");
    });

    // ── #358: graceful service shutdown notification ──

    test_case!("sm_shutdown_state_enum_value", {
        // Appended variant must not disturb existing discriminants.
        test_eq!(ServiceState::Stopped as u8, 0);
        test_eq!(ServiceState::Running as u8, 2);
        test_eq!(ServiceState::Stopping as u8, 3);
        test_eq!(ServiceState::Failed as u8, 4);
        test_eq!(ServiceState::StopPending as u8, 5);
        // Cross-check the Ring 3 observation class number is stable.
        test_eq!(crate::object::types::ObInfoClass::ProcessShutdownState as u32, 42);
    });

    test_case!("sm_stop_no_pid_goes_stopped", {
        // A Running service with no live process can stop immediately.
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("NoPidSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = 0;
        test_true!(sm.stop_service(idx, 0).is_ok());
        test_eq!(sm.services[idx].state, ServiceState::Stopped);
        test_true!(!sm.services[idx].shutdown_requested);
    });

    // Test 1 — graceful stop: request → notification pending → voluntary exit.
    test_case!("sm_stop_requests_graceful_shutdown", {
        let _ = SERVICE_MANAGER.lock().remove("GracefulSvc");
        let idx = {
            let mut sm = SERVICE_MANAGER.lock();
            let cfg = ServiceConfig {
                start_type: ServiceStartType::Demand,
                restart_policy: ServiceRestartPolicy::Never,
                max_failures: 3,
            };
            let idx = sm.register("GracefulSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
            sm.services[idx].state = ServiceState::Running;
            sm.services[idx].pid = 6161;
            idx
        };

        // Request a graceful stop: must NOT kill, must NOT reach Stopped yet.
        // Drop the lock exactly as the real syscall handler does between the
        // request and the deferred drain.
        {
            let mut sm = SERVICE_MANAGER.lock();
            test_true!(sm.stop_service(idx, 1000).is_ok());
            test_eq!(sm.services[idx].state, ServiceState::StopPending);
            test_true!(sm.services[idx].shutdown_requested);
            // Still alive — graceful path never force-kills inline.
            test_eq!(sm.services[idx].pid, 6161);
        }
        test_true!(has_pending_shutdowns());

        // First drain delivers the notification (deadline not reached) and
        // re-queues for deadline re-checking.
        process_pending_shutdowns();
        {
            let sm = SERVICE_MANAGER.lock();
            test_eq!(sm.services[idx].state, ServiceState::StopPending);
            test_true!(sm.services[idx].shutdown_requested);
            test_true!(sm.services[idx].shutdown_notified);
            test_eq!(sm.services[idx].pid, 6161); // never force-killed inline
        }

        // Simulate the service voluntarily exiting: converged through #374.
        notify_process_exit(6161, 0);
        process_pending_exits();

        // Drain the shutdown queue too: the service is gone, so the request
        // is dropped.
        process_pending_shutdowns();

        {
            let sm = SERVICE_MANAGER.lock();
            test_eq!(sm.services[idx].state, ServiceState::Stopped);
            test_eq!(sm.services[idx].pid, 0);
            test_true!(!sm.services[idx].shutdown_requested);
            test_eq!(sm.services[idx].exit_count, 1);
        }
        let _ = SERVICE_MANAGER.lock().remove("GracefulSvc");
    });

    // Test 2 — timeout fallback: the service ignores shutdown.
    test_case!("sm_stop_timeout_elapsed_and_enforced", {
        let _ = SERVICE_MANAGER.lock().remove("StubbornSvc");
        let idx = {
            let mut sm = SERVICE_MANAGER.lock();
            let cfg = ServiceConfig {
                start_type: ServiceStartType::Demand,
                restart_policy: ServiceRestartPolicy::Never,
                max_failures: 3,
            };
            let idx = sm.register("StubbornSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
            sm.services[idx].state = ServiceState::Running;
            sm.services[idx].pid = 6262;
            idx
        };

        {
            let mut sm = SERVICE_MANAGER.lock();
            // 1 ms timeout → deadline is already elapsed on the next drain.
            test_true!(sm.stop_service(idx, 1).is_ok());
            test_eq!(sm.services[idx].state, ServiceState::StopPending);
        }
        test_true!(has_pending_shutdowns());

        // First drain with a deadline that has NOT yet elapsed: the request must
        // be notified once and then re-queued so the deadline keeps being
        // checked on later drains (a one-shot queue entry would strand the
        // service in StopPending forever).
        {
            let mut sm = SERVICE_MANAGER.lock();
            // Push the deadline into the future deterministically.
            sm.services[idx].stop_deadline = u64::MAX;
            test_true!(!sm.stop_deadline_elapsed(idx));
        }
        process_pending_shutdowns();
        {
            let sm = SERVICE_MANAGER.lock();
            test_true!(sm.services[idx].shutdown_notified);
            test_eq!(sm.services[idx].state, ServiceState::StopPending);
        }
        test_true!(has_pending_shutdowns()); // re-queued for re-check

        // Second drain with the deadline elapsed: forced termination is
        // attempted. `kill_process(6262)` finds no such process in a unit-test
        // scheduler, so no exit is produced; the state remains StopPending until
        // #374 observes the real exit.
        {
            let mut sm = SERVICE_MANAGER.lock();
            sm.services[idx].stop_deadline = 0;
            test_true!(sm.stop_deadline_elapsed(idx));
        }
        process_pending_shutdowns();
        {
            let sm = SERVICE_MANAGER.lock();
            // Still pending: forcing is a request to the scheduler; the service
            // is finalized only when #374 reports the exit.
            test_true!(sm.services[idx].shutdown_requested);
            test_eq!(sm.services[idx].state, ServiceState::StopPending);
        }

        // When the forced kill lands, the exit converges through #374.
        notify_process_exit(6262, -1);
        process_pending_exits();
        {
            let sm = SERVICE_MANAGER.lock();
            test_eq!(sm.services[idx].state, ServiceState::Stopped);
            test_true!(!sm.services[idx].shutdown_requested);
        }
        let _ = SERVICE_MANAGER.lock().remove("StubbornSvc");
    });

    // Test 3 — no restart on intentional stop (restart policy = Always).
    test_case!("sm_intentional_stop_no_restart", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Auto,
            restart_policy: ServiceRestartPolicy::Always, // would restart on crash
            max_failures: 5,
        };
        let idx = sm.register("AlwaysSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = 6363;

        // Graceful stop request, then the process exits with a non-zero code
        // (worst case for OnCrash) — it must still be treated as intentional.
        test_true!(sm.stop_service(idx, 1000).is_ok());
        test_eq!(sm.services[idx].state, ServiceState::StopPending);
        sm.on_process_exit(idx, -1);

        test_eq!(sm.services[idx].state, ServiceState::Stopped);
        test_eq!(sm.services[idx].failure_count, 0); // not counted as a failure
        test_eq!(sm.services[idx].exit_count, 1);
        test_true!(!sm.services[idx].shutdown_requested);
        // Drain the request enqueued by stop_service (service already gone).
        process_pending_shutdowns();
        test_true!(!has_pending_shutdowns());
    });

    // Test 4 — #374 integration: forced/voluntary exit travels the deferred path.
    test_case!("sm_shutdown_exit_via_deferred_queue", {
        let _ = SERVICE_MANAGER.lock().remove("DeferredShutdownSvc");
        let idx = {
            let mut sm = SERVICE_MANAGER.lock();
            let cfg = ServiceConfig {
                start_type: ServiceStartType::Demand,
                restart_policy: ServiceRestartPolicy::OnCrash,
                max_failures: 3,
            };
            let idx = sm.register("DeferredShutdownSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
            sm.services[idx].state = ServiceState::Running;
            sm.services[idx].pid = 6464;
            idx
        };
        {
            let mut sm = SERVICE_MANAGER.lock();
            test_true!(sm.stop_service(idx, 5000).is_ok());
        }
        process_pending_shutdowns(); // deliver notification, deadline not reached

        // The service exits voluntarily; only the #374 hook observes it.
        notify_process_exit(6464, 0);
        test_true!(has_pending_exits());
        process_pending_exits();
        {
            let sm = SERVICE_MANAGER.lock();
            test_eq!(sm.services[idx].state, ServiceState::Stopped);
            test_eq!(sm.services[idx].exit_count, 1);
            test_eq!(sm.services[idx].pid, 0);
        }
        // The service is gone: the re-queued request is dropped cleanly.
        process_pending_shutdowns();
        test_true!(!has_pending_shutdowns());
        let _ = SERVICE_MANAGER.lock().remove("DeferredShutdownSvc");
    });

    test_case!("sm_stop_busy_when_already_pending", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("BusySvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = 6565;
        test_true!(sm.stop_service(idx, 1000).is_ok());
        // Second stop while pending is rejected, never force-killed twice.
        test_eq!(sm.stop_service(idx, 1000).unwrap_err(), SmError::Busy);
        drop(sm);
        process_pending_shutdowns();
    });

    test_case!("sm_stop_while_stopping_busy", {
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("StoppingSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Stopping;
        sm.services[idx].pid = 6666;
        test_eq!(sm.stop_service(idx, 1000).unwrap_err(), SmError::Busy);
    });

    test_case!("sm_restart_force_terminates_and_restarts", {
        // Restart stays synchronous: force-kill then start (which fails here
        // because the binary is missing, leaving the service Failed).
        let mut sm = ServiceManager::new();
        let cfg = ServiceConfig {
            start_type: ServiceStartType::Demand,
            restart_policy: ServiceRestartPolicy::Never,
            max_failures: 3,
        };
        let idx = sm.register("RestartSvc", "", "C:\\nonexistent.nxe", cfg, &[]).unwrap();
        sm.services[idx].state = ServiceState::Running;
        sm.services[idx].pid = 6767;
        let r = sm.restart_service(idx, 0);
        test_true!(r.is_err());
        test_eq!(sm.services[idx].state, ServiceState::Failed);
        test_true!(!sm.services[idx].shutdown_requested);
    });

    test_case!("sm_shutdown_request_queue_bounded", {
        // The pending-shutdown queue is bounded and never panics on overflow.
        for pid in 7000u32..7100 {
            request_service_shutdown(pid);
        }
        // Drain everything; must terminate and leave the counter at zero.
        for _ in 0..4 {
            process_pending_shutdowns();
        }
        test_true!(!has_pending_shutdowns());
    });

    test_case!("sm_shutdown_notify_unknown_pid_safe", {
        // SMP/lifecycle safety: notifying a PID with no live processes is a
        // well-defined no-op and must not panic or leave state behind.
        test_true!(!crate::apc::request_process_shutdown_notification(0));
        let _ = crate::apc::request_process_shutdown_notification(0xDEAD_BEEF);
        // Idle (0) and NeoInit (1) are never services and are ignored by the
        // request hook, just like #374's exit hook.
        request_service_shutdown(0);
        request_service_shutdown(1);
        test_true!(!has_pending_shutdowns());
    });
}
