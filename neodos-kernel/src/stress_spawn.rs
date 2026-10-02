//! #345 Phase 2B — VFS-contention isolation harness (diagnostic).
//!
//! The Phase 2A spawn storm stalls on the VFS spinlock (`with_vfs`) while
//! `preempt_disable()` is held. This harness isolates whether the stall is in
//! the pure creation path (VFS read + ELF load) or only appears once created
//! children start executing and hitting VFS themselves.
//!
//! Selectable mode (no cargo feature; edit the constant):
//!   * `MODE = CreateOnly` : one creator thread, `create_process_from_ob_path`
//!     only, children left `Suspended` (never activated, never run);
//!   * `MODE = CreateActivate` : one creator thread, create + activate + yield.
//!
//! Pure diagnostic: no scheduler/lifecycle/allocator/NeoInit change.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::log::LogSubsys;

/// Master switch for the diagnostic build.
pub const ENABLED: bool = false;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Create only: children stay Suspended (never scheduled).
    CreateOnly,
    /// Create + activate + yield (one creator).
    CreateActivate,
}

/// Selected mode for this build.
pub const MODE: Mode = Mode::CreateActivate;

/// Bounded iteration count (Phase A/B/C only need a small observable number).
const STRESS_ITERS: u64 = 64;

/// Number of concurrent creator threads.
const STRESS_THREADS: u32 = 2;

/// A NXE that runs briefly and exits (no interactive input).
const STRESS_PATH: &str = "\\Global\\FileSystem\\C:\\System\\Tools\\stresscmd.nxe";

static ACTIVATED: AtomicU64 = AtomicU64::new(0);

/// Start the stress threads. Called from the BSP boot path after the Service
/// Manager has started the auto services, before entering NeoInit.
pub fn start() {
    if !ENABLED {
        return;
    }
    let mode = match MODE {
        Mode::CreateOnly => "create-only",
        Mode::CreateActivate => "create+activate",
    };
    crate::serial_println!("[SPAWN_STRESS] harness enabled mode={} threads={} iters={}",
        mode, STRESS_THREADS, STRESS_ITERS);
    for i in 0..STRESS_THREADS {
        let name = match i {
            0 => "spawnstress0",
            1 => "spawnstress1",
            2 => "spawnstress2",
            _ => "spawnstress3",
        };
        let entry = stress_entry as *const () as u64;
        let tid = crate::hal::without_interrupts(|| {
            crate::scheduler::current_scheduler()
                .lock()
                .spawn_kthread_named(entry, crate::scheduler::PRIORITY_NORMAL, name)
        });
        crate::serial_println!("[SPAWN_STRESS] started name={} tid={:?}", name, tid);
    }
}

extern "C" fn stress_entry() -> ! {
    let me = crate::scheduler::current_pid();
    let cpu0 = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
    crate::serial_println!("[SPAWN_STRESS] entry pid={} cpu={} mode={:?}", me, cpu0, MODE);

    let mut i: u64 = 0;
    let mut ok: u64 = 0;
    let mut err: u64 = 0;
    while i < STRESS_ITERS {
        i += 1;
        crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_ITER_BEGIN, i, 0);
        let t = crate::hal::get_ticks();
        crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_CREATE_BEGIN, i, 0);
        match crate::usermode::create_process_from_ob_path(
            STRESS_PATH, 2, "\\", me, "ss-child",
        ) {
            Ok(c) => {
                crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_CREATE_OK, i, c.pid);
                ok += 1;
                if i <= 20 || i % 8 == 0 {
                    crate::serial_println!(
                        "[SPAWN_STRESS] pid={} i={} res=OK child={} t={} ok={} err={} mode={:?}",
                        me, i, c.pid, t, ok, err, MODE);
                }
                if MODE == Mode::CreateActivate {
                    let _ = ACTIVATED.fetch_add(1, Ordering::Relaxed);
                    crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_ACTIVATE_BEGIN, i, c.pid);
                    crate::usermode::activate_process(c.pid);
                    crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_ACTIVATE_OK, i, c.pid);
                } else {
                    // Create-only: leave the child `Suspended` (never scheduled).
                }
            }
            Err(_) => {
                err += 1;
                crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_CREATE_ERR, i, 0);
                if err <= 40 || err % 8 == 0 {
                    crate::serial_println!(
                        "[SPAWN_STRESS] pid={} i={} res=ERR t={} ok={} err={} mode={:?}",
                        me, i, t, ok, err, MODE);
                }
            }
        }
        crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_YIELD_BEGIN, i, 0);
        crate::scheduler::yield_current_thread();
        crate::scheduler::diag::st_ev(crate::scheduler::diag::ST_RESUME, i, 0);
        if me == 7 && i <= 8 {
            crate::scheduler::diag::st_dump_raw();
        }
    }
    crate::serial_println!("[SPAWN_STRESS] done pid={} ok={} err={} mode={:?}", me, ok, err, MODE);
    crate::scheduler::diag::st_dump_raw();
    crate::raw_serial_println!(
        "[VFS_STATE] after-stress owner_cpu={} owner_tid={} owner_pid={} owner_rip=0x{:x} owner_acq={} waiter_cpu={} waiter=0x{:x} waits={}",
        crate::scheduler::diag::vfs_owner_cpu(),
        crate::scheduler::diag::vfs_owner_tid(),
        crate::scheduler::diag::vfs_owner_pid(),
        crate::scheduler::diag::vfs_owner_rip(),
        crate::scheduler::diag::vfs_owner_acq(),
        crate::scheduler::diag::vfs_waiter_cpu(),
        crate::scheduler::diag::vfs_waiter_word(),
        crate::scheduler::diag::vfs_wait_count());
    loop {
        crate::scheduler::yield_current_thread();
    }
}

/// Register the harness in the kernel's module tree (no-op unless enabled).
pub fn init() {
    let _ = LogSubsys::Sched;
}
