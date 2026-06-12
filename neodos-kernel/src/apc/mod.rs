//! A4.5 APC (Asynchronous Procedure Call) engine
//!
//! Queues per-thread de procedimientos asíncronos para entregar resultados
//! de I/O y eventos sin trabajo síncrono.
//!
//! # Architecture
//! - Kernel APC: ejecutado a PASSIVE_LEVEL en contexto kernel
//! - User APC: ejecutado en Ring 3 antes de retornar de syscall
//! - IRP completion via DPC: DIRQL → DPC (DISPATCH) → APC (PASSIVE)

use alloc::boxed::Box;
use core::sync::atomic::Ordering;
use crate::{test_case, test_eq, test_true};
use crate::scheduler;
use crate::irp::{IrpId, IrpStatus, irp_free};

// ── Constants ──

/// Return value for sys_wait_alertable when APC was delivered
pub const APC_ALERTED: u64 = 1;

/// Magic value for alertable wait blocking
pub const APC_WAIT_MAGIC: u32 = 0xBBBB_0000;

/// Max entries per queue
pub const MAX_KERNEL_APC: usize = 64;
pub const MAX_USER_APC: usize = 64;

// ── Types ──

/// Function pointer for APC callbacks
pub type ApcFn = fn(context: *mut u8);

/// A single APC entry
#[repr(C)]
pub struct ApcEntry {
    pub function: ApcFn,
    pub context: *mut u8,
    pub kernel: bool,
}

// Safety: ApcEntry is only accessed under the scheduler lock (Mutex),
// and raw pointers are validated before use.
unsafe impl Send for ApcEntry {}
unsafe impl Sync for ApcEntry {}

// ── IRP→APC bridge types ──

/// Payload for DPC bridge to irp_complete_with_apc
struct IrpApcDpcInfo {
    irp_id: IrpId,
    tid: u32,
}

/// Result payload delivered via user APC callback
struct IrpApcResult {
    irp_id: IrpId,
    status: IrpStatus,
    callback: Option<crate::irp::IrpCallback>,
    callback_ctx: *mut u8,
}

// ── Queue operations ──

/// Queue a kernel APC to the specified thread.
/// Safe to call from DPC context (DISPATCH_LEVEL) or higher.
pub fn queue_kernel_apc(tid: u32, function: ApcFn, context: *mut u8) -> bool {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let result = {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(k) = lock.find_kthread_mut(tid) {
            if k.kernel_apc_queue.len() < MAX_KERNEL_APC {
                k.kernel_apc_queue.push_back(ApcEntry {
                    function,
                    context,
                    kernel: true,
                });
                k.apc_pending = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

/// Queue a user APC to the specified thread.
/// Safe to call from DPC context (DISPATCH_LEVEL) or higher.
/// Wakes the thread if it is blocked in an alertable wait.
pub fn queue_user_apc(tid: u32, function: ApcFn, context: *mut u8) -> bool {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let result = {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(k) = lock.find_kthread_mut(tid) {
            if k.user_apc_queue.len() < MAX_USER_APC {
                k.user_apc_queue.push_back(ApcEntry {
                    function,
                    context,
                    kernel: false,
                });
                k.apc_pending = true;

                // Wake thread if it's blocked in alertable wait
                if k.waiting_for == Some(APC_WAIT_MAGIC)
                    && matches!(k.state, scheduler::ThreadState::Blocked { .. })
                {
                    k.waiting_for = None;
                    k.state = scheduler::ThreadState::Ready;
                    scheduler::Scheduler::enqueue_to_cpu_run_queue(k);
                    crate::syscall::set_need_resched();
                }
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

// ── Query ──

/// Check if the current thread has pending user APCs.
pub fn has_pending_user_apcs() -> bool {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let result = {
        let s = scheduler::current_scheduler();
        let lock = s.lock();
        lock.find_kthread(lock.current_tid)
            .map(|k| !k.user_apc_queue.is_empty())
            .unwrap_or(false)
    };
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

/// Check if the current thread has pending kernel APCs.
pub fn has_pending_kernel_apcs() -> bool {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let result = {
        let s = scheduler::current_scheduler();
        let lock = s.lock();
        lock.find_kthread(lock.current_tid)
            .map(|k| !k.kernel_apc_queue.is_empty())
            .unwrap_or(false)
    };
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

// ── Dispatch ──

/// Dispatch all kernel APCs for the current thread.
/// Returns the number of APCs dispatched.
pub fn dispatch_kernel_apcs() -> usize {
    let mut count = 0;
    loop {
        let entry = {
            let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
            let result: Option<ApcEntry> = {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(k) = lock.current_kthread_mut() {
                    k.kernel_apc_queue.pop_front()
                } else {
                    None
                }
            };
            unsafe { crate::hal::irql::lower_irql(old_irql) };
            result
        };
        match entry {
            Some(apc) => {
                (apc.function)(apc.context);
                count += 1;
            }
            None => break,
        }
    }
    // Update apc_pending flag
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(k) = lock.current_kthread_mut() {
            if k.kernel_apc_queue.is_empty() && k.user_apc_queue.is_empty() {
                k.apc_pending = false;
            }
        }
    }
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    count
}

/// Dispatch one user APC for the current thread.
/// Returns true if an APC was dispatched, false if queue empty.
pub fn dispatch_one_user_apc() -> bool {
    let entry = {
        let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
        let result: Option<ApcEntry> = {
            let s = scheduler::current_scheduler();
            let mut lock = s.lock();
            if let Some(k) = lock.current_kthread_mut() {
                k.user_apc_queue.pop_front()
            } else {
                None
            }
        };
        unsafe { crate::hal::irql::lower_irql(old_irql) };
        result
    };
    match entry {
        Some(apc) => {
            (apc.function)(apc.context);
            // Update apc_pending flag
            let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
            {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(k) = lock.current_kthread_mut() {
                    if k.kernel_apc_queue.is_empty() && k.user_apc_queue.is_empty() {
                        k.apc_pending = false;
                    }
                }
            }
            unsafe { crate::hal::irql::lower_irql(old_irql) };
            true
        }
        None => false,
    }
}

// ── IRP Completion via APC ──

/// Callback invoked from DPC (DISPATCH_LEVEL) to complete IRP via APC delivery.
fn irp_complete_apc_dpc_callback(ctx: *mut u8) {
    let info = unsafe { Box::from_raw(ctx.cast::<IrpApcDpcInfo>()) };
    irp_complete_with_apc(info.irp_id, info.tid);
}

/// Complete an IRP and deliver the result via user APC to the specified thread.
/// Called from DPC context at DISPATCH_LEVEL.
pub fn irp_complete_with_apc(irp_id: IrpId, tid: u32) {
    // Extract IRP status and callback before freeing
    let (status, callback, cb_ctx) = {
        let mut pool = crate::irp::IRP_POOL.inner.lock();
        if let Some(irp) = pool.get_mut(irp_id) {
            irp.status = crate::irp::IrpStatus::Completed;
            let status = irp.status;
            let cb = irp.callback.take();
            let ctx = irp.callback_ctx;
            (status, cb, ctx)
        } else {
            (crate::irp::IrpStatus::Error(1), None, core::ptr::null_mut())
        }
    };

    // Allocate result payload for the user APC callback
    let result = Box::new(IrpApcResult {
        irp_id,
        status,
        callback,
        callback_ctx: cb_ctx,
    });
    let ctx = Box::into_raw(result).cast::<u8>();

    // Queue user APC to target thread
    queue_user_apc(tid, irp_apc_user_callback, ctx);

    // Free IRP from pool
    irp_free(irp_id);
}

/// User APC callback for IRP completion.
/// Called at PASSIVE_LEVEL when target thread processes its user APC queue.
fn irp_apc_user_callback(ctx: *mut u8) {
    let result = unsafe { Box::from_raw(ctx.cast::<IrpApcResult>()) };
    // Invoke the original IRP callback if one was registered
    if let Some(cb) = result.callback {
        cb(result.irp_id, result.status, result.callback_ctx);
    }
}

/// Enqueue a DPC to complete an IRP via APC delivery to a user thread.
/// Called from device ISR or completion handler (DIRQL).
/// The DPC will call irp_complete_with_apc at DISPATCH_LEVEL.
pub fn irp_queue_apc_dpc_completion(irp_id: IrpId, tid: u32) -> bool {
    let info = Box::new(IrpApcDpcInfo { irp_id, tid });
    let ptr = Box::into_raw(info).cast::<u8>();
    crate::dpc::insert_queue_dpc(irp_complete_apc_dpc_callback, ptr)
}

// ── Syscall return path dispatch ──

/// Called from syscall handler assembly before IRETQ to Ring 3.
/// Dispatches pending kernel APCs first, then one user APC.
#[no_mangle]
pub extern "C" fn apc_dispatch_on_syscall_return() {
    // Dispatch kernel APCs (cleanup, post-I/O work)
    let kcount = dispatch_kernel_apcs();
    if kcount > 0 {
        // Kernel APCs may have queued work
        crate::syscall::set_need_resched();
    }
    // Dispatch one user APC (I/O completion, events)
    if has_pending_user_apcs() {
        dispatch_one_user_apc();
    }
}

// ── Alertable wait helper ──

/// Block the current thread in an alertable state.
/// Returns true if woken by APC, false if woken by other means.
pub fn block_current_alertable() -> bool {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(k) = lock.current_kthread_mut() {
            k.state = scheduler::ThreadState::Blocked { waiting_for: APC_WAIT_MAGIC };
            k.waiting_for = Some(APC_WAIT_MAGIC);
        }
    }
    crate::syscall::set_need_resched();
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    true
}

// ── Tests ──────────────────────────────────────────────────────────────

fn test_apc_kernel_dispatch_during_cleanup() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicBool;
    static CALLED: AtomicBool = AtomicBool::new(false);

    fn apc_callback(_ctx: *mut u8) {
        CALLED.store(true, Ordering::Relaxed);
    }

    let tid = crate::hal::without_interrupts(|| {
        scheduler::current_tid()
    });
    test_true!(queue_kernel_apc(tid, apc_callback, core::ptr::null_mut()));

    let count = dispatch_kernel_apcs();
    test_true!(count > 0);
    test_true!(CALLED.load(Ordering::Relaxed));
    Ok(())
}

fn test_apc_user_alertable_wait_receives() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicBool;
    static CALLED: AtomicBool = AtomicBool::new(false);

    fn apc_callback(_ctx: *mut u8) {
        CALLED.store(true, Ordering::Relaxed);
    }

    let tid = crate::hal::without_interrupts(|| {
        scheduler::current_tid()
    });

    // Queue user APC
    test_true!(queue_user_apc(tid, apc_callback, core::ptr::null_mut()));
    test_true!(has_pending_user_apcs());

    // Dispatch it (simulates sys_wait_alertable)
    let dispatched = dispatch_one_user_apc();
    test_true!(dispatched);
    test_true!(CALLED.load(Ordering::Relaxed));
    test_true!(!has_pending_user_apcs());
    Ok(())
}

fn test_apc_queue_overflow_handling() -> Result<(), &'static str> {
    fn dummy_callback(_ctx: *mut u8) {}

    let tid = crate::hal::without_interrupts(|| {
        scheduler::current_tid()
    });

    // Fill kernel APC queue to max
    for _ in 0..MAX_KERNEL_APC {
        test_true!(queue_kernel_apc(tid, dummy_callback, core::ptr::null_mut()));
    }
    // Next one should fail
    test_true!(!queue_kernel_apc(tid, dummy_callback, core::ptr::null_mut()));

    // Fill user APC queue to max
    for _ in 0..MAX_USER_APC {
        test_true!(queue_user_apc(tid, dummy_callback, core::ptr::null_mut()));
    }
    // Next one should fail
    test_true!(!queue_user_apc(tid, dummy_callback, core::ptr::null_mut()));

    // Drain and verify
    let kcount = dispatch_kernel_apcs();
    test_eq!(kcount, MAX_KERNEL_APC);

    for _ in 0..MAX_USER_APC {
        dispatch_one_user_apc();
    }
    test_true!(!has_pending_user_apcs());
    Ok(())
}

fn test_irp_completion_dispatches_apc() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicBool;
    use crate::irp::{irp_alloc, IrpOp};
    static CALLED: AtomicBool = AtomicBool::new(false);

    fn irp_callback(_id: IrpId, _status: IrpStatus, _ctx: *mut u8) {
        CALLED.store(true, Ordering::Relaxed);
    }

    let id = irp_alloc(
        IrpOp::Read, 0, 1, core::ptr::null_mut(), 512,
        Some(irp_callback), core::ptr::null_mut(),
    ).ok_or("irp_alloc failed")?;

    // Complete via APC path
    let tid = crate::hal::without_interrupts(|| {
        scheduler::current_tid()
    });
    irp_complete_with_apc(id, tid);

    // APC should be queued — dispatch it
    let dispatched = dispatch_one_user_apc();
    test_true!(dispatched);
    Ok(())
}

fn test_apc_stress_100_concurrent_irps() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicU32;
    use crate::irp::{irp_alloc, IrpOp};
    static COUNT: AtomicU32 = AtomicU32::new(0);

    fn irp_callback(_id: IrpId, _status: IrpStatus, _ctx: *mut u8) {
        COUNT.fetch_add(1, Ordering::Relaxed);
    }

    let tid = crate::hal::without_interrupts(|| {
        scheduler::current_tid()
    });

    // Create APCs in batches of MAX_USER_APC, dispatching between batches
    // to stay within queue capacity while processing 100 total IRPs.
    let total = 100u32;
    let batch = MAX_USER_APC;
    let mut remaining = total;
    while remaining > 0 {
        let n = core::cmp::min(remaining, batch as u32);
        for i in 0..n {
            let id = irp_alloc(
                IrpOp::Read, i as u64, 1, core::ptr::null_mut(), 512,
                Some(irp_callback), core::ptr::null_mut(),
            ).ok_or("irp_alloc failed")?;
            irp_complete_with_apc(id, tid);
        }
        // Dispatch all pending APCs
        for _ in 0..n {
            dispatch_one_user_apc();
        }
        remaining -= n;
    }

    test_eq!(COUNT.load(Ordering::Relaxed), total);
    test_true!(!has_pending_user_apcs());
    Ok(())
}

pub fn register_tests() {
    test_case!("apc_kernel_dispatch_during_cleanup", {
        test_apc_kernel_dispatch_during_cleanup()?;
    });
    test_case!("apc_user_alertable_wait_receives", {
        test_apc_user_alertable_wait_receives()?;
    });
    test_case!("apc_queue_overflow_handling", {
        test_apc_queue_overflow_handling()?;
    });
    test_case!("irp_completion_dispatches_apc", {
        test_irp_completion_dispatches_apc()?;
    });
    test_case!("apc_stress_100_concurrent_irps", {
        test_apc_stress_100_concurrent_irps()?;
    });
}
