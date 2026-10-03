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

// ═══════════════════════════════════════════════════════════════════════
// Enums
// ═══════════════════════════════════════════════════════════════════════

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Stopped   = 0,
    Starting  = 1,
    Running   = 2,
    Stopping  = 3,
    Failed    = 4,
    /// #358: a graceful stop has been requested and the shutdown notification
    /// has been (or is about to be) delivered. The service is still alive and
    /// is expected to clean up and exit voluntarily. If it does not exit before
    /// `stop_deadline`, the forced-termination fallback runs.
    StopPending = 5,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStartType {
    Boot     = 0,
    System   = 1,
    Auto     = 2,
    Demand   = 3,
    Disabled = 4,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRestartPolicy {
    Never   = 0,
    OnCrash = 1,
    Always  = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmError {
    InvalidTransition,
    Disabled,
    AlreadyRunning,
    AlreadyStopped,
    Busy,
    NotFound,
    OutOfMemory,
    DependencyFailed,
    CycleDetected,
}

impl SmError {
    pub fn as_err_code(self) -> i64 {
        match self {
            SmError::InvalidTransition => -1,
            SmError::Disabled => -1,
            SmError::AlreadyRunning => -15,
            SmError::AlreadyStopped => -1,
            SmError::Busy => -15,
            SmError::NotFound => -2,
            SmError::OutOfMemory => -3,
            SmError::DependencyFailed => -1,
            SmError::CycleDetected => -1,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Service struct
// ═══════════════════════════════════════════════════════════════════════

/// #358: default graceful-shutdown timeout used when the caller passes 0.
/// Matches the Service Manager design document's 5 s start/stop handshake
/// convention.
pub const DEFAULT_STOP_TIMEOUT_MS: u32 = 5000;

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub start_type: ServiceStartType,
    pub restart_policy: ServiceRestartPolicy,
    pub max_failures: u32,
}

#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    pub display_name: String,
    pub binary_path: String,
    pub state: ServiceState,
    pub start_type: ServiceStartType,
    pub restart_policy: ServiceRestartPolicy,
    pub pid: u32,
    pub obj_id: ObId,
    pub exit_count: u32,
    pub last_exit_code: i64,
    pub dependencies: Vec<String>,
    pub failure_count: u32,
    pub max_failures: u32,
    pub start_tick: u64,
    /// #358: set while a graceful shutdown has been requested and the service
    /// has not yet exited. Cleared when the process exits (or restarts).
    pub shutdown_requested: bool,
    /// #358: absolute tick deadline by which a `StopPending` service must exit
    /// before forced termination. Only meaningful while `shutdown_requested`.
    pub stop_deadline: u64,
}

impl Service {
    pub fn config(&self) -> ServiceConfig {
        ServiceConfig {
            start_type: self.start_type,
            restart_policy: self.restart_policy,
            max_failures: self.max_failures,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ServiceManager
// ═══════════════════════════════════════════════════════════════════════

pub struct ServiceManager {
    pub services: Vec<Service>,
    pub dependency_order: Vec<usize>,
}

impl ServiceManager {
    pub fn new() -> Self {
        ServiceManager {
            services: Vec::new(),
            dependency_order: Vec::new(),
        }
    }

    /// Find service index by name.
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        self.services.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Find service index by ObId.
    pub fn find_by_obj_id(&self, obj_id: ObId) -> Option<usize> {
        self.services.iter().position(|s| s.obj_id == obj_id)
    }

    /// Find service index by the PID of its running process.
    ///
    /// Only services that currently have a live process (`pid != 0`) are
    /// considered, so a stale PID from a previous run never matches.
    pub fn find_by_pid(&self, pid: u32) -> Option<usize> {
        if pid == 0 {
            return None;
        }
        self.services
            .iter()
            .position(|s| s.pid != 0 && s.pid == pid)
    }

    /// Register a service from config. Creates Ob object in \Service\<Name>.
    pub fn register(&mut self, name: &str, display_name: &str, binary_path: &str,
                    config: ServiceConfig, deps: &[String]) -> Result<usize, SmError> {
        if self.find_by_name(name).is_some() {
            return Err(SmError::AlreadyRunning);
        }

        // Create Ob object in \Service\<Name>
        let svc_dir = alloc::format!("\\Service\\{}", name);
        let _ = namespace::ob_create_directory("\\Service");
        let ob_id = object::ob_create_object(
            ObType::Service, name, 0, 0, None,
        ).map_err(|_| SmError::OutOfMemory)?;

        // The Ob namespace entry may fail if it already exists; that's OK
        // since we just need the ObId for handle operations.
        let _ = namespace::ob_create_directory_tree(&svc_dir);
        let _ = namespace::ob_insert_object(&svc_dir, ob_id);

        let idx = self.services.len();
        self.services.push(Service {
            name: name.to_string(),
            display_name: display_name.to_string(),
            binary_path: binary_path.to_string(),
            state: ServiceState::Stopped,
            start_type: config.start_type,
            restart_policy: config.restart_policy,
            pid: 0,
            obj_id: ob_id,
            exit_count: 0,
            last_exit_code: 0,
            dependencies: deps.to_vec(),
            failure_count: 0,
            max_failures: config.max_failures,
            start_tick: 0,
            shutdown_requested: false,
            stop_deadline: 0,
        });

        // Write config to Registry
        Self::write_registry_config(name, &config).ok();

        Ok(idx)
    }

    /// Read service configuration from Registry.
    pub fn read_registry_config(name: &str) -> Option<(String, String, ServiceConfig, Vec<String>)> {
        let cm = CM_MANAGER.lock();
        if cm.hives.is_empty() { return None; }
        let hm = &cm.hives[0];

        let root = hm.hive.root_cell();
        let key = hm.hive.open_key_by_path(root, &format!("CurrentControlSet\\Services\\{}", name))?;

        let display_name = hm.hive.query_value(key, "DisplayName")
            .and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
        let binary_path = hm.hive.query_value(key, "BinaryPath")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| {
                hm.hive.query_value(key, "ImagePath")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_default();
        let start_type_val = hm.hive.query_value(key, "StartType")
            .and_then(|v| v.as_dword()).unwrap_or(3);
        let restart_val = hm.hive.query_value(key, "RestartPolicy")
            .and_then(|v| v.as_dword()).unwrap_or(0);
        let max_fail = hm.hive.query_value(key, "MaxFailures")
            .and_then(|v| v.as_dword()).unwrap_or(3);
        let deps_str = hm.hive.query_value(key, "Dependencies")
            .and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();

        let start_type = match start_type_val {
            0 => ServiceStartType::Boot,
            1 => ServiceStartType::System,
            2 => ServiceStartType::Auto,
            3 => ServiceStartType::Demand,
            _ => ServiceStartType::Disabled,
        };
        let restart_policy = match restart_val {
            1 => ServiceRestartPolicy::OnCrash,
            2 => ServiceRestartPolicy::Always,
            _ => ServiceRestartPolicy::Never,
        };
        let config = ServiceConfig {
            start_type,
            restart_policy,
            max_failures: max_fail,
        };

        let deps: Vec<String> = if deps_str.is_empty() {
            Vec::new()
        } else {
            deps_str.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        };

        Some((display_name, binary_path, config, deps))
    }

    /// Write service config back to Registry.
    fn write_registry_config(name: &str, config: &ServiceConfig) -> Result<(), ()> {
        let mut cm = CM_MANAGER.lock();
        if cm.hives.is_empty() { return Err(()); }
        let hm = &mut cm.hives[0];
        let root = hm.hive.root_cell();
        let key_path = format!("CurrentControlSet\\Services\\{}", name);

        let key = crate::cm::ensure_key_path(&mut hm.hive, root, &key_path)
            .ok_or(())?;

        hm.hive.set_value(key, "StartType", hive::REG_DWORD,
                          &(config.start_type as u32).to_le_bytes());
        hm.hive.set_value(key, "RestartPolicy", hive::REG_DWORD,
                          &(config.restart_policy as u32).to_le_bytes());
        hm.hive.set_value(key, "MaxFailures", hive::REG_DWORD,
                          &config.max_failures.to_le_bytes());
        Ok(())
    }

    /// Remove a service's Ob object and Registry entry.
    pub fn remove(&mut self, name: &str) -> Result<(), SmError> {
        let idx = self.find_by_name(name).ok_or(SmError::NotFound)?;
        let svc = &self.services[idx];
        if svc.state != ServiceState::Stopped && svc.state != ServiceState::Failed {
            return Err(SmError::Busy);
        }
        let obj_id = svc.obj_id;
        let svc_path = alloc::format!("\\Service\\{}", svc.name);
        let _ = namespace::ob_remove_object(&svc_path);
        let _ = object::ob_destroy_object(obj_id);
        self.services.remove(idx);
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════
    // Dependency resolution
    // ═══════════════════════════════════════════════════════════════

    /// Build dependency graph and compute topological order.
    /// Returns indices in start order (dependencies first).
    pub fn build_dependency_order(&self) -> Result<Vec<usize>, SmError> {
        let n = self.services.len();
        let name_to_idx: BTreeMap<String, usize> = self.services.iter()
            .enumerate().map(|(i, s)| (s.name.to_lowercase(), i)).collect();

        // adjacency: edge deps[i] -> i means i depends on deps[i]
        let mut in_degree = vec![0u32; n];
        let mut graph: Vec<Vec<usize>> = vec![Vec::new(); n]; // reverse: prereq -> dependents

        for (i, svc) in self.services.iter().enumerate() {
            for dep in &svc.dependencies {
                if let Some(&dep_idx) = name_to_idx.get(&dep.to_lowercase()) {
                    graph[dep_idx].push(i);
                    in_degree[i] += 1;
                } else {
                    // Missing dependency — mark as unresolved
                    return Err(SmError::DependencyFailed);
                }
            }
        }

        // Kahn's algorithm
        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(idx) = queue.pop() {
            order.push(idx);
            for &dep_idx in &graph[idx] {
                in_degree[dep_idx] -= 1;
                if in_degree[dep_idx] == 0 {
                    queue.push(dep_idx);
                }
            }
        }

        if order.len() != n {
            return Err(SmError::CycleDetected);
        }
        Ok(order)
    }

    /// Start a service by index. Spawns the process and transitions state.
    pub fn start_service(&mut self, idx: usize) -> Result<(), SmError> {
        let (name, binary_path) = {
            let svc = &self.services[idx];
            if svc.start_type == ServiceStartType::Disabled {
                return Err(SmError::Disabled);
            }
            if svc.state == ServiceState::Running || svc.state == ServiceState::Starting {
                return Err(SmError::AlreadyRunning);
            }
            if svc.state == ServiceState::Stopping {
                return Err(SmError::Busy);
            }
            (svc.name.clone(), svc.binary_path.clone())
        };

        // Dependencies must be running first
        {
            let deps = self.services[idx].dependencies.clone();
            for dep in &deps {
                if let Some(dep_idx) = self.find_by_name(dep) {
                    let dep_state = self.services[dep_idx].state;
                    if dep_state != ServiceState::Running {
                        // Start it first (recursive)
                        // To avoid infinite recursion in case of cycles, the
                        // dependency graph should already have been resolved.
                        self.start_service(dep_idx)?;
                    }
                } else {
                    return Err(SmError::DependencyFailed);
                }
            }
        }

        // Transition to Starting
        self.services[idx].state = ServiceState::Starting;
        self.services[idx].start_tick = crate::hal::get_ticks();

        // Spawn the process
        match self.spawn_process(&name, &binary_path) {
            Ok(pid) => {
                self.services[idx].pid = pid;
                self.services[idx].state = ServiceState::Running;
                Ok(())
            }
            Err(e) => {
                self.services[idx].state = ServiceState::Failed;
                Err(e)
            }
        }
    }

    /// Spawn a process for a service via the shared kernel process-creation
    /// path (the same one used by `ObCreate(Process)`).
    fn spawn_process(&mut self, name: &str, binary_path: &str) -> Result<u32, SmError> {
        // Build Ob path: \Global\FileSystem\<path>
        let ob_path = if binary_path.starts_with("\\Global\\FileSystem\\") {
            binary_path.to_string()
        } else if binary_path.contains(':') {
            alloc::format!("\\Global\\FileSystem\\{}", binary_path)
        } else {
            return Err(SmError::NotFound);
        };

        let created = crate::usermode::create_process_from_ob_path(
            &ob_path, 2, "\\", 0, name, // cwd_drive=C, cwd_path=\, parent_pid=0 (kernel)
        )
        .map_err(|e| match e {
            crate::usermode::CreateProcessError::NotFound => SmError::NotFound,
            crate::usermode::CreateProcessError::InvalidElf => SmError::InvalidTransition,
            crate::usermode::CreateProcessError::NoMemory => SmError::OutOfMemory,
        })?;

        // The ObWait hand-off activates a Suspended child; services are not
        // spawned through ObWait, so publish the initial thread Ready now via
        // the shared activation path (single implementation).
        crate::usermode::activate_process(created.pid);

        Ok(created.pid)
    }

    /// Request a graceful stop of a service by index.
    ///
    /// #358: this does **not** terminate the process. It transitions the service
    /// to `StopPending`, marks `shutdown_requested` and hands the request to the
    /// deferred shutdown queue. The actual notification (a user APC to the
    /// service thread) and the bounded-timeout forced termination are performed
    /// by `process_pending_shutdowns`, which runs in syscall context outside all
    /// kernel locks. This mirrors the #374 deferred process-exit architecture and
    /// keeps `SERVICE_MANAGER` safety intact.
    ///
    /// `timeout_ms == 0` selects [`DEFAULT_STOP_TIMEOUT_MS`].
    pub fn stop_service(&mut self, idx: usize, timeout_ms: u32) -> Result<(), SmError> {
        let state = self.services[idx].state;
        match state {
            ServiceState::Stopped | ServiceState::Failed => {
                return Err(SmError::AlreadyStopped);
            }
            ServiceState::StopPending | ServiceState::Stopping => {
                return Err(SmError::Busy);
            }
            _ => {}
        }

        let pid = self.services[idx].pid;
        // A service with no live process can transition straight to Stopped:
        // there is nothing to notify or wait for.
        if pid == 0 {
            self.services[idx].state = ServiceState::Stopped;
            self.services[idx].shutdown_requested = false;
            self.services[idx].stop_deadline = 0;
            return Ok(());
        }

        let timeout_ms = if timeout_ms == 0 { DEFAULT_STOP_TIMEOUT_MS } else { timeout_ms };

        self.services[idx].state = ServiceState::StopPending;
        self.services[idx].shutdown_requested = true;
        self.services[idx].stop_deadline = Self::deadline_from_now(timeout_ms);

        crate::services::request_service_shutdown(pid);
        Ok(())
    }

    /// Compute an absolute tick deadline `timeout_ms` in the future.
    #[inline]
    pub fn deadline_from_now(timeout_ms: u32) -> u64 {
        let rate = crate::hal::get_tick_rate().max(1);
        let ticks = (timeout_ms as u64).saturating_mul(rate) / 1000;
        crate::hal::get_ticks().saturating_add(ticks.max(1))
    }

    /// True when the service is past its graceful-shutdown deadline.
    #[inline]
    pub fn stop_deadline_elapsed(&self, idx: usize) -> bool {
        let svc = &self.services[idx];
        svc.shutdown_requested && crate::hal::get_ticks() >= svc.stop_deadline
    }

    /// Restart a service by index.
    ///
    /// #358: a restart is an immediate administrative operation, not a graceful
    /// stop: it must complete within the syscall. It therefore force-terminates
    /// the current process (if any) and starts a fresh one, exactly as before.
    /// Graceful shutdown semantics belong to `stop_service` only.
    pub fn restart_service(&mut self, idx: usize, _timeout_ms: u32) -> Result<(), SmError> {
        let state = self.services[idx].state;
        if state == ServiceState::StopPending || state == ServiceState::Stopping {
            return Err(SmError::Busy);
        }
        let pid = self.services[idx].pid;
        if pid != 0 {
            let _ = self.kill_process(pid);
            self.services[idx].pid = 0;
        }
        self.services[idx].state = ServiceState::Stopped;
        self.services[idx].shutdown_requested = false;
        self.services[idx].stop_deadline = 0;
        self.start_service(idx)
    }

    /// Kill a process by PID (internal).
    pub fn kill_process(&self, pid: u32) -> Result<(), SmError> {
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut lock = s.lock();
            if lock.kill_pid(pid) {
                lock.wake_waiters(pid);
                Ok(())
            } else {
                Err(SmError::NotFound)
            }
        })
    }

    /// Handle process exit for a service (called from process tracker).
    pub fn on_process_exit(&mut self, idx: usize, exit_code: i64) {
        let (name, bin_path, state, restart_policy, failure_count, max_failures) = {
            let svc = &self.services[idx];
            (svc.name.clone(), svc.binary_path.clone(), svc.state,
             svc.restart_policy, svc.failure_count, svc.max_failures)
        };

        self.services[idx].exit_count += 1;
        self.services[idx].last_exit_code = exit_code;
        self.services[idx].pid = 0;
        // #358: any exit (graceful or forced) ends the shutdown request. This is
        // what prevents an intentional administrative stop from being mistaken
        // for a crash and triggering a restart below.
        let was_shutdown_requested = self.services[idx].shutdown_requested;
        self.services[idx].shutdown_requested = false;
        self.services[idx].stop_deadline = 0;

        match state {
            ServiceState::Stopping | ServiceState::StopPending => {
                // Intentional administrative stop: never restart, regardless of
                // restart policy or exit code.
                self.services[idx].state = ServiceState::Stopped;
                self.services[idx].failure_count = 0;
            }
            ServiceState::Running | ServiceState::Starting => {
                // A shutdown request that raced with a natural exit still counts
                // as intentional: honour the administrative stop.
                if was_shutdown_requested {
                    self.services[idx].state = ServiceState::Stopped;
                    self.services[idx].failure_count = 0;
                    return;
                }
                let should_restart = match restart_policy {
                    ServiceRestartPolicy::Never => false,
                    ServiceRestartPolicy::OnCrash => exit_code != 0,
                    ServiceRestartPolicy::Always => true,
                };
                if should_restart && failure_count < max_failures {
                    self.services[idx].failure_count = failure_count + 1;
                    self.services[idx].state = ServiceState::Stopped;
                    if let Ok(pid) = self.spawn_process(&name, &bin_path) {
                        self.services[idx].pid = pid;
                        self.services[idx].state = ServiceState::Running;
                        self.services[idx].start_tick = crate::hal::get_ticks();
                    } else {
                        self.services[idx].state = ServiceState::Failed;
                    }
                } else {
                    self.services[idx].state = ServiceState::Failed;
                }
            }
            _ => {}
        }
    }

    /// Handle the exit of the process identified by `pid` (0 if unknown).
    ///
    /// This is the production entry point: it is invoked when the scheduler
    /// observes that a service process has fully exited, routes it to the
    /// owning service and applies the restart policy. Returns `true` if a
    /// service was found for `pid`.
    pub fn on_process_exit_by_pid(&mut self, pid: u32, exit_code: i64) -> bool {
        match self.find_by_pid(pid) {
            Some(idx) => {
                self.on_process_exit(idx, exit_code);
                true
            }
            None => false,
        }
    }

    /// Set service configuration.
    pub fn set_config(&mut self, idx: usize,
                      start_type: ServiceStartType,
                      restart_policy: ServiceRestartPolicy,
                      max_failures: u32) -> Result<(), SmError> {
        let svc = &mut self.services[idx];
        svc.start_type = start_type;
        svc.restart_policy = restart_policy;
        svc.max_failures = max_failures;

        // Write back to Registry
        let config = ServiceConfig { start_type, restart_policy, max_failures };
        Self::write_registry_config(&svc.name, &config).ok();
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Global + Init
// ═══════════════════════════════════════════════════════════════════════

lazy_static! {
    pub static ref SERVICE_MANAGER: Mutex<ServiceManager> = Mutex::new(ServiceManager::new());
}

