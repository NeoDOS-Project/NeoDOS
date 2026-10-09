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

pub mod manager;
pub mod lifecycle;
pub mod registry;
pub mod power;

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

mod tests;
pub use tests::register_service_tests;
