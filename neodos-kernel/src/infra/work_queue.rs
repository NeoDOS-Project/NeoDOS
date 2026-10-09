//! Two-level deferred work queue system (bottom-half).
//!
//! Provides IRQ-safe work scheduling with two priority levels:
//! - **High-priority**: urgent work processed on syscall return (waking blocked processes, etc.)
//! - **Low-priority**: batch work processed in the idle loop (cache flushing, KOBJ cleanup, etc.)
//!
//! Each work item is a callback (`fn(*mut u8)`) with an opaque data pointer.
//! Queues use a lock-free SPSC ring buffer (IRQ-safe push, consumer must disable interrupts).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering, compiler_fence};
use crate::{test_eq, test_true};

const CAPACITY: usize = 64;

/// Function pointer type for work queue callbacks.
pub type WorkFn = fn(data: *mut u8);

#[derive(Copy, Clone)]
struct WorkEntry {
    func: WorkFn,
    data: *mut u8,
}

/// A single lock-free SPSC work queue.
///
/// # Safety
/// - `push()` is IRQ-safe (lock-free, no interrupts disabled needed).
/// - `pop()` must be called with interrupts disabled to prevent
///   producer/consumer races on single-CPU systems.
pub struct WorkQueue {
    entries: UnsafeCell<[Option<WorkEntry>; CAPACITY]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    pub pending: AtomicBool,
}

unsafe impl Sync for WorkQueue {}

impl WorkQueue {
    pub const fn new() -> Self {
        const NONE: Option<WorkEntry> = None;
        WorkQueue {
            entries: UnsafeCell::new([NONE; CAPACITY]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            pending: AtomicBool::new(false),
        }
    }

    /// Push a work item. IRQ-safe, lock-free.
    /// Returns false if the queue is full.
    pub fn push(&self, func: WorkFn, data: *mut u8) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let next = (tail + 1) % CAPACITY;
        if next == head {
            return false;
        }
        unsafe {
            (*self.entries.get())[tail] = Some(WorkEntry { func, data });
        }
        compiler_fence(Ordering::Release);
        self.tail.store(next, Ordering::Release);
        self.pending.store(true, Ordering::Release);
        true
    }

    /// Pop a work item. Must be called with interrupts disabled.
    fn pop(&self) -> Option<(WorkFn, *mut u8)> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            self.pending.store(false, Ordering::Release);
            return None;
        }
        let entry = unsafe {
            (*self.entries.get())[head]
        };
        compiler_fence(Ordering::Release);
        let new_head = (head + 1) % CAPACITY;
        self.head.store(new_head, Ordering::Release);
        if new_head == tail {
            self.pending.store(false, Ordering::Release);
        }
        entry.map(|e| (e.func, e.data))
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Relaxed) == self.tail.load(Ordering::Relaxed)
    }

    /// Number of pending items in the queue.
    pub fn pending_count(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        if tail >= head {
            tail - head
        } else {
            CAPACITY - head + tail
        }
    }
}

/// Two-level work queue manager.
///
/// - `high`: urgent work processed eagerly (syscall return path)
/// - `low`: batch work processed lazily (idle loop)
pub struct WorkQueueManager {
    pub high: WorkQueue,
    pub low: WorkQueue,
}

unsafe impl Sync for WorkQueueManager {}

impl WorkQueueManager {
    pub const fn new() -> Self {
        WorkQueueManager {
            high: WorkQueue::new(),
            low: WorkQueue::new(),
        }
    }

    /// Push a high-priority work item (IRQ-safe).
    #[inline]
    pub fn push_high(&self, func: WorkFn, data: *mut u8) -> bool {
        self.high.push(func, data)
    }

    /// Push a low-priority work item (IRQ-safe).
    #[inline]
    pub fn push_low(&self, func: WorkFn, data: *mut u8) -> bool {
        self.low.push(func, data)
    }

    /// Drain and process all high-priority items.
    /// Must be called with interrupts disabled.
    pub fn process_high(&self) -> usize {
        let mut count = 0;
        while let Some((func, data)) = self.high.pop() {
            func(data);
            count += 1;
        }
        count
    }

    /// Drain and process all low-priority items.
    /// Must be called with interrupts disabled.
    pub fn process_low(&self) -> usize {
        let mut count = 0;
        while let Some((func, data)) = self.low.pop() {
            func(data);
            count += 1;
        }
        count
    }

    /// Process the high-priority queue at DISPATCH_LEVEL.
    /// Safe to call from any context.
    pub fn process_high_safe(&self) -> usize {
        let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
        let result = self.process_high();
        unsafe { crate::hal::irql::lower_irql(old_irql) };
        result
    }

    /// Process the low-priority queue at DISPATCH_LEVEL.
    /// Safe to call from any context.
    pub fn process_low_safe(&self) -> usize {
        let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
        let result = self.process_low();
        unsafe { crate::hal::irql::lower_irql(old_irql) };
        result
    }

    /// Process both queues (high first, then low) with interrupts disabled.
    pub fn process_all(&self) -> (usize, usize) {
        let high = self.process_high();
        let low = self.process_low();
        (high, low)
    }
}

/// Global work queue manager instance.
pub static WORK_QUEUE: WorkQueueManager = WorkQueueManager::new();

// ── Tests ──

fn test_wq_push_pop() -> Result<(), &'static str> {
    static CALLED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    fn handler(data: *mut u8) {
        CALLED.store(data as u64, core::sync::atomic::Ordering::Relaxed);
    }
    let wq = WorkQueue::new();
    test_true!(wq.push(handler, 42 as *mut u8));
    test_true!(wq.pending.load(core::sync::atomic::Ordering::Relaxed));
    let (func, data) = wq.pop().unwrap();
    test_eq!(data as u64, 42);
    func(data);
    test_eq!(CALLED.load(core::sync::atomic::Ordering::Relaxed), 42);
    test_true!(wq.pop().is_none());
    test_true!(!wq.pending.load(core::sync::atomic::Ordering::Relaxed));
    Ok(())
}

fn test_wq_fifo_order() -> Result<(), &'static str> {
    static RESULTS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    fn handler_a(data: *mut u8) {
        RESULTS.store(data as u64 * 10, core::sync::atomic::Ordering::Relaxed);
    }
    fn handler_b(data: *mut u8) {
        RESULTS.store(data as u64 * 10 + 1, core::sync::atomic::Ordering::Relaxed);
    }
    fn handler_c(data: *mut u8) {
        RESULTS.store(data as u64 * 10 + 2, core::sync::atomic::Ordering::Relaxed);
    }
    let wq = WorkQueue::new();
    test_true!(wq.push(handler_a, core::ptr::without_provenance_mut(1)));
    test_true!(wq.push(handler_b, core::ptr::without_provenance_mut(2)));
    test_true!(wq.push(handler_c, core::ptr::without_provenance_mut(3)));
    test_eq!(wq.pending_count(), 3);

    let (f1, d1) = wq.pop().unwrap();
    f1(d1);
    test_eq!(RESULTS.load(core::sync::atomic::Ordering::Relaxed), 10);

    let (f2, d2) = wq.pop().unwrap();
    f2(d2);
    test_eq!(RESULTS.load(core::sync::atomic::Ordering::Relaxed), 21);

    let (f3, d3) = wq.pop().unwrap();
    f3(d3);
    test_eq!(RESULTS.load(core::sync::atomic::Ordering::Relaxed), 32);

    test_true!(wq.pop().is_none());
    test_eq!(wq.pending_count(), 0);
    Ok(())
}

fn test_wq_empty_queue() -> Result<(), &'static str> {
    let wq = WorkQueue::new();
    test_true!(wq.is_empty());
    test_true!(wq.pop().is_none());
    test_eq!(wq.pending_count(), 0);
    test_true!(!wq.pending.load(core::sync::atomic::Ordering::Relaxed));
    Ok(())
}

fn test_wq_overflow() -> Result<(), &'static str> {
    fn dummy(_: *mut u8) {}
    let wq = WorkQueue::new();
    let mut pushed = 0;
    for i in 0..CAPACITY + 10 {
        if wq.push(dummy, i as *mut u8) {
            pushed += 1;
        } else {
            break;
        }
    }
    // SPSC ring buffer can hold at most CAPACITY - 1 items (head == tail means empty)
    test_eq!(pushed, CAPACITY - 1);
    test_eq!(wq.pending_count(), CAPACITY - 1);
    // Drain all
    for _ in 0..pushed {
        test_true!(wq.pop().is_some());
    }
    test_true!(wq.pop().is_none());
    Ok(())
}

fn test_wq_high_low_isolation() -> Result<(), &'static str> {
    static H_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    static L_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    fn h_work(_: *mut u8) { H_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed); }
    fn l_work(_: *mut u8) { L_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed); }

    let mgr = WorkQueueManager::new();
    mgr.push_high(h_work, core::ptr::null_mut());
    mgr.push_high(h_work, core::ptr::null_mut());
    mgr.push_low(l_work, core::ptr::null_mut());

    test_eq!(mgr.high.pending_count(), 2);
    test_eq!(mgr.low.pending_count(), 1);

    // Process only high
    let high_count = mgr.process_high();
    test_eq!(high_count, 2);
    test_eq!(H_COUNT.load(core::sync::atomic::Ordering::Relaxed), 2);
    test_eq!(L_COUNT.load(core::sync::atomic::Ordering::Relaxed), 0);
    test_eq!(mgr.high.pending_count(), 0);

    // Process only low
    let low_count = mgr.process_low();
    test_eq!(low_count, 1);
    test_eq!(L_COUNT.load(core::sync::atomic::Ordering::Relaxed), 1);
    test_eq!(mgr.low.pending_count(), 0);
    Ok(())
}

fn test_wq_pending_flag() -> Result<(), &'static str> {
    fn dummy(_: *mut u8) {}
    let wq = WorkQueue::new();
    test_true!(!wq.pending.load(core::sync::atomic::Ordering::Relaxed));
    wq.push(dummy, core::ptr::null_mut());
    test_true!(wq.pending.load(core::sync::atomic::Ordering::Relaxed));
    wq.pop();
    test_true!(!wq.pending.load(core::sync::atomic::Ordering::Relaxed));
    Ok(())
}

pub fn register_tests() {
    crate::testing::register("wq_push_pop", test_wq_push_pop);
    crate::testing::register("wq_fifo_order", test_wq_fifo_order);
    crate::testing::register("wq_empty_queue", test_wq_empty_queue);
    crate::testing::register("wq_overflow", test_wq_overflow);
    crate::testing::register("wq_high_low_isolation", test_wq_high_low_isolation);
    crate::testing::register("wq_pending_flag", test_wq_pending_flag);
}
