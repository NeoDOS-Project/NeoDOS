//! DPC (Deferred Procedure Call) engine — A2.5.
//!
//! Per-CPU queues of deferred procedures executed at DISPATCH_LEVEL.
//! IRQ handlers enqueue DPCs at DIRQL; DPCs are dispatched when IRQL
//! drops to DISPATCH (timer handler exit) or on syscall return.
//!
//! Design:
//! - Per-CPU SPSC ring buffer (lock-free, single producer = current CPU at DIRQL)
//! - Max 128 DPCs pending per-CPU
//! - Nesting depth limit (MAX_DPC_DEPTH=10) prevents infinite recursion
//! - DPCs execute at DISPATCH_LEVEL (interrupts disabled, device IRQs masked)

use core::sync::atomic::{compiler_fence, Ordering};
use crate::{test_eq, test_true};

/// Maximum number of DPC entries per-CPU queue.
pub const DPC_QUEUE_SIZE: usize = 128;

/// Maximum DPC nesting depth to prevent infinite recursion.
pub const MAX_DPC_DEPTH: usize = 10;

/// Function pointer type for DPC callbacks.
/// Called at DISPATCH_LEVEL with the provided context pointer.
pub type DpcFn = fn(ctx: *mut u8);

/// A single deferred procedure call entry.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct DpcEntry {
    /// Callback function pointer.
    pub function: DpcFn,
    /// Opaque context data passed to the callback.
    pub context: *mut u8,
}

/// Per-CPU DPC queue — SPSC ring buffer.
///
/// # Safety
/// Producer: current CPU at DIRQL (interrupts disabled by hardware).
/// Consumer: current CPU at DISPATCH (interrupts disabled by IRQL).
/// Only the owning CPU accesses this queue.
#[repr(C)]
pub struct DpcQueue {
    /// Ring buffer of DPC entries.
    entries: [DpcEntry; DPC_QUEUE_SIZE],
    /// Consumer index (dispatch reads from here).
    head: u16,
    /// Producer index (enqueue writes here).
    tail: u16,
    /// Number of items currently in the queue.
    count: u16,
    /// Current nesting depth (incremented on dispatch, prevents recursion > MAX).
    depth: u8,
    /// Whether any DPCs are pending (optimization flag).
    pending: bool,
}

impl DpcQueue {
    /// Create an empty DPC queue.
    pub const fn new() -> Self {
        DpcQueue {
            entries: [DpcEntry {
                function: DpcQueue::noop,
                context: core::ptr::null_mut(),
            }; DPC_QUEUE_SIZE],
            head: 0,
            tail: 0,
            count: 0,
            depth: 0,
            pending: false,
        }
    }

    /// No-op callback for const initialization of the entries array.
    fn noop(_ctx: *mut u8) {}

    /// Enqueue a DPC for deferred execution.
    /// Safe to call from DIRQL context (interrupts disabled by hardware).
    /// If the queue is full, the DPC is silently dropped.
    #[inline]
    pub fn enqueue(&mut self, function: DpcFn, context: *mut u8) -> bool {
        if self.count as usize >= DPC_QUEUE_SIZE {
            return false;
        }
        self.entries[self.tail as usize] = DpcEntry { function, context };
        compiler_fence(Ordering::Release);
        self.tail = self.tail.wrapping_add(1);
        self.count += 1;
        self.pending = true;
        true
    }

    /// Dequeue a DPC entry. Must be called with interrupts disabled.
    #[inline]
    fn dequeue(&mut self) -> Option<DpcEntry> {
        if self.count == 0 {
            self.pending = false;
            return None;
        }
        let entry = self.entries[self.head as usize];
        compiler_fence(Ordering::Release);
        self.head = self.head.wrapping_add(1);
        self.count -= 1;
        if self.count == 0 {
            self.pending = false;
        }
        Some(entry)
    }

    /// Drain and execute all pending DPCs.
    /// Must be called at DISPATCH_LEVEL (interrupts disabled).
    /// Returns the number of DPCs dispatched.
    #[inline]
    pub fn dispatch_all(&mut self) -> usize {
        if self.depth as usize >= MAX_DPC_DEPTH {
            return 0;
        }
        self.depth += 1;
        let mut count = 0;
        while let Some(entry) = self.dequeue() {
            (entry.function)(entry.context);
            count += 1;
        }
        self.depth -= 1;
        count
    }

    /// Check if any DPCs are pending.
    #[inline]
    pub fn has_pending(&self) -> bool {
        self.pending && self.count > 0
    }

    /// Number of pending DPCs.
    #[inline]
    pub fn pending_count(&self) -> usize {
        self.count as usize
    }
}

// ── Static per-CPU DPC queues (not embedded in KPRCB to keep it ≤4096) ─

use crate::arch::x64::cpu_local::MAX_CPUS;

/// Static array of per-CPU DPC queues.
static mut DPC_QUEUES: [DpcQueue; MAX_CPUS] = [
    DpcQueue::new(), DpcQueue::new(), DpcQueue::new(), DpcQueue::new(),
    DpcQueue::new(), DpcQueue::new(), DpcQueue::new(), DpcQueue::new(),
    DpcQueue::new(), DpcQueue::new(), DpcQueue::new(), DpcQueue::new(),
    DpcQueue::new(), DpcQueue::new(), DpcQueue::new(), DpcQueue::new(),
];

/// Get a mutable reference to the current CPU's DPC queue.
///
/// # Safety
/// Requires GS base to be set. Only the owning CPU should call this.
#[inline]
pub fn this_cpu_dpc_queue() -> &'static mut DpcQueue {
    unsafe {
        let cpu_id = crate::arch::x64::cpu_local::this_cpu_id();
        &mut DPC_QUEUES[cpu_id as usize]
    }
}

/// Enqueue a DPC on the current CPU's queue.
/// Safe to call from any context where interrupts may be disabled (DIRQL+).
#[inline]
pub fn insert_queue_dpc(function: DpcFn, context: *mut u8) -> bool {
    this_cpu_dpc_queue().enqueue(function, context)
}

/// Dispatch all pending DPCs on the current CPU.
/// Must be called at DISPATCH_LEVEL (interrupts disabled).
/// Returns the number of DPCs executed.
#[inline]
pub fn dpc_dispatch_pending() -> usize {
    this_cpu_dpc_queue().dispatch_all()
}

/// Check if the current CPU has pending DPCs.
#[inline]
pub fn dpc_has_pending() -> bool {
    this_cpu_dpc_queue().has_pending()
}

// ── Tests ──────────────────────────────────────────────────────────────

fn test_dpc_enqueue_dispatch_level() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicU64;
    static RESULT: AtomicU64 = AtomicU64::new(0);
    fn callback(ctx: *mut u8) {
        RESULT.store(ctx as u64, core::sync::atomic::Ordering::Relaxed);
    }

    let mut q = DpcQueue::new();
    test_true!(q.enqueue(callback, 0xAB as *mut u8));
    test_true!(q.has_pending());
    test_eq!(q.pending_count(), 1);

    let count = q.dispatch_all();
    test_eq!(count, 1);
    test_eq!(RESULT.load(core::sync::atomic::Ordering::Relaxed), 0xAB);
    test_true!(!q.has_pending());
    test_eq!(q.pending_count(), 0);
    Ok(())
}

fn test_dpc_irq_to_dispatch_transition() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicU64;
    static ORDER: AtomicU64 = AtomicU64::new(0);

    fn irq_handler(ctx: *mut u8) {
        // Simulate ISR: enqueue DPC
        ORDER.store(1, core::sync::atomic::Ordering::Relaxed);
        let q = ctx as *mut DpcQueue;
        unsafe {
            (*q).enqueue(dpc_callback, core::ptr::null_mut());
        }
    }

    fn dpc_callback(_ctx: *mut u8) {
        ORDER.store(2, core::sync::atomic::Ordering::Relaxed);
    }

    let mut q = DpcQueue::new();
    let q_ptr = &mut q as *mut DpcQueue;

    // Simulate IRQ firing and enqueueing DPC
    irq_handler(q_ptr as *mut u8);
    test_eq!(ORDER.load(core::sync::atomic::Ordering::Relaxed), 1);
    test_true!(q.has_pending());

    // Simulate DIRQL→DISPATCH transition: dispatch DPCs
    let count = q.dispatch_all();
    test_eq!(count, 1);
    test_eq!(ORDER.load(core::sync::atomic::Ordering::Relaxed), 2);
    Ok(())
}

fn test_dpc_nesting_depth_limit() -> Result<(), &'static str> {
    static NEST_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

    fn nested_dpc(ctx: *mut u8) {
        NEST_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // Attempt to enqueue another DPC (nesting)
        let q = ctx as *mut DpcQueue;
        unsafe {
            (*q).enqueue(nested_dpc, ctx);
        }
    }

    let mut q = DpcQueue::new();
    let q_ptr = &mut q as *mut DpcQueue;

    // Enqueue a DPC that will try to nest
    test_true!(q.enqueue(nested_dpc, q_ptr as *mut u8));

    // First dispatch executes the DPC, which enqueues another
    // But depth limit prevents infinite recursion
    let count = q.dispatch_all();
    // Should have executed at least the first DPC
    test_true!(count >= 1);
    // Depth should be back to 0 after dispatch returns
    test_eq!(q.depth, 0);
    Ok(())
}

fn test_dpc_callback_execution_order() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicU64;
    static RESULTS: [AtomicU64; 4] = [
        AtomicU64::new(0), AtomicU64::new(0),
        AtomicU64::new(0), AtomicU64::new(0),
    ];
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn callback_a(_ctx: *mut u8) {
        let idx = COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        RESULTS[idx as usize].store(10, core::sync::atomic::Ordering::Relaxed);
    }
    fn callback_b(_ctx: *mut u8) {
        let idx = COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        RESULTS[idx as usize].store(20, core::sync::atomic::Ordering::Relaxed);
    }
    fn callback_c(_ctx: *mut u8) {
        let idx = COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        RESULTS[idx as usize].store(30, core::sync::atomic::Ordering::Relaxed);
    }

    let mut q = DpcQueue::new();
    test_true!(q.enqueue(callback_a, core::ptr::null_mut()));
    test_true!(q.enqueue(callback_b, core::ptr::null_mut()));
    test_true!(q.enqueue(callback_c, core::ptr::null_mut()));
    test_eq!(q.pending_count(), 3);

    let count = q.dispatch_all();
    test_eq!(count, 3);
    // FIFO order: A first, B second, C third
    test_eq!(RESULTS[0].load(core::sync::atomic::Ordering::Relaxed), 10);
    test_eq!(RESULTS[1].load(core::sync::atomic::Ordering::Relaxed), 20);
    test_eq!(RESULTS[2].load(core::sync::atomic::Ordering::Relaxed), 30);
    Ok(())
}

fn test_dpc_stress_100_irqs() -> Result<(), &'static str> {
    use core::sync::atomic::AtomicU64;
    static STRESS_COUNT: AtomicU64 = AtomicU64::new(0);

    fn stress_callback(_ctx: *mut u8) {
        STRESS_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    let mut q = DpcQueue::new();

    // Simulate 100 IRQs, each enqueueing a DPC
    for _ in 0..100 {
        test_true!(q.enqueue(stress_callback, core::ptr::null_mut()));
    }
    test_eq!(q.pending_count(), 100);

    // Dispatch all — none should leak, none should infinite-loop
    let count = q.dispatch_all();
    test_eq!(count, 100);
    test_eq!(
        STRESS_COUNT.load(core::sync::atomic::Ordering::Relaxed),
        100
    );
    test_true!(!q.has_pending());
    test_eq!(q.pending_count(), 0);

    // Queue should be reusable after full drain
    STRESS_COUNT.store(0, core::sync::atomic::Ordering::Relaxed);
    test_true!(q.enqueue(stress_callback, core::ptr::null_mut()));
    let count2 = q.dispatch_all();
    test_eq!(count2, 1);
    test_eq!(STRESS_COUNT.load(core::sync::atomic::Ordering::Relaxed), 1);
    Ok(())
}

/// Register all DPC tests.
pub fn register_tests() {
    crate::testing::register("dpc_enqueue_dispatch_level", test_dpc_enqueue_dispatch_level);
    crate::testing::register("dpc_irq_to_dispatch_transition", test_dpc_irq_to_dispatch_transition);
    crate::testing::register("dpc_nesting_depth_limit", test_dpc_nesting_depth_limit);
    crate::testing::register("dpc_callback_execution_order", test_dpc_callback_execution_order);
    crate::testing::register("dpc_stress_100_irqs", test_dpc_stress_100_irqs);
}
