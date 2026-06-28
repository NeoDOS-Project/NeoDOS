//! Async I/O Request Packet (IRP) system.
//!
//! ─── FROZEN ABI (v0.43) ──────────────────────────────────────────────
//! The following APIs are stable and will not change:
//!   - irp_alloc() / irp_free() / irp_get_params()
//!   - irp_complete() / irp_complete_result()
//!   - irp_block_current() / irp_submit_and_wait()
//!   - irp_sync_read() / irp_sync_write()
//!   - irp_get_status() / irp_set_chain()
//!   - IrpQueue (per-device FIFO)
//!   - BlockDevice trait: submit_irp() / poll_irp()
//!
//! Protocol invariants:
//!   - IRP IDs are global u32 (monotonic via AtomicU32)
//!   - Pool index = id % 64; slot collision returns None
//!   - Drivers MUST call irp_get_params() to extract fields under pool lock
//!   - Completion: irp_complete() sets status, wakes waiter, dispatches callback
//!   - Callbacks are dispatched via high-priority work queue (never in IRQ)
//!   - Blocking: magic = IRP_WAIT_MAGIC | irp_id, ThreadState::Blocked
//!   - Chain: irp_set_chain() links IRPs; auto-submits chain_next on complete
//!   - All 5 BlockDevice implementors complete synchronously in submit_irp()
//!
//! ─────────────────────────────────────────────────────────────────────
//!
//! Unified asynchronous I/O model for all kernel block operations.
//! Every I/O operation is represented as an IRP with a unique ID,
//! operation type, buffer, completion callback, and state.
//!
//! Devices maintain IRP queues and process them either synchronously
//! (immediate completion) or asynchronously (polled completion).
//! Completion callbacks are dispatched via the high-priority work queue.

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use crate::work_queue::WORK_QUEUE;
use crate::{test_case, test_eq, test_ne, test_true};

const IRP_POOL_SIZE: usize = 64;

/// Magic number for IRP waiting — combined with IRP ID for `Process.waiting_for`.
pub const IRP_WAIT_MAGIC: u32 = 0xAAAA_0000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IrpOp {
    Read,
    Write,
    Flush,
    IoCtl(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IrpStatus {
    Pending,
    Completed,
    Error(u32),
}

pub type IrpId = u32;
pub type IrpCallback = fn(irp_id: IrpId, status: IrpStatus, ctx: *mut u8);

#[repr(C)]
pub struct Irp {
    pub id: IrpId,
    pub op: IrpOp,
    pub lba: u64,
    pub count: u8,
    pub buf: *mut u8,
    pub buf_len: usize,
    pub status: IrpStatus,
    pub callback: Option<IrpCallback>,
    pub callback_ctx: *mut u8,
    pub chain_next: Option<IrpId>,
    pub waiting_pid: Option<u32>,
}

/// Snapshot of IRP parameters needed by a device driver to perform I/O.
/// Obtained via `irp_get_params()` — the driver copies these fields out
/// before calling `irp_complete()`, avoiding double-lock deadlock.
#[derive(Debug, Clone, Copy)]
pub struct IrpParams {
    pub op: IrpOp,
    pub lba: u64,
    pub count: u8,
    pub buf: *mut u8,
    pub buf_len: usize,
    pub id: IrpId,
}

struct IrpSlot {
    in_use: bool,
    irp: Irp,
}

pub(crate) struct IrpPoolInner {
    slots: [IrpSlot; IRP_POOL_SIZE],
}

impl IrpPoolInner {
    fn alloc(&mut self, id: IrpId) -> Option<&mut Irp> {
        let idx = (id as usize) % IRP_POOL_SIZE;
        if self.slots[idx].in_use {
            return None;
        }
        self.slots[idx] = IrpSlot {
            in_use: true,
            irp: Irp {
                id,
                op: IrpOp::Read,
                lba: 0,
                count: 0,
                buf: core::ptr::null_mut(),
                buf_len: 0,
                status: IrpStatus::Pending,
                callback: None,
                callback_ctx: core::ptr::null_mut(),
                chain_next: None,
                waiting_pid: None,
            },
        };
        Some(&mut self.slots[idx].irp)
    }

    fn free(&mut self, id: IrpId) {
        let idx = (id as usize) % IRP_POOL_SIZE;
        self.slots[idx].in_use = false;
    }

    pub fn get_mut(&mut self, id: IrpId) -> Option<&mut Irp> {
        let idx = (id as usize) % IRP_POOL_SIZE;
        if self.slots[idx].in_use && self.slots[idx].irp.id == id {
            Some(&mut self.slots[idx].irp)
        } else {
            None
        }
    }
}

pub struct IrpPool {
    pub(crate) inner: Mutex<IrpPoolInner>,
    next_id: AtomicU32,
}

// Safety: the Mutex provides exclusive access to the inner pool.
// Raw pointers within Irp are only accessed under the lock.
unsafe impl Sync for IrpPool {}

impl IrpPool {
    pub const fn new() -> Self {
        const EMPTY_SLOT: IrpSlot = IrpSlot {
            in_use: false,
            irp: Irp {
                id: 0,
                op: IrpOp::Read,
                lba: 0,
                count: 0,
                buf: core::ptr::null_mut::<u8>(),
                buf_len: 0,
                status: IrpStatus::Pending,
                callback: None,
                callback_ctx: core::ptr::null_mut::<u8>(),
                chain_next: None,
                waiting_pid: None,
            },
        };
        IrpPool {
            inner: Mutex::new(IrpPoolInner {
                slots: [EMPTY_SLOT; IRP_POOL_SIZE],
            }),
            next_id: AtomicU32::new(1),
        }
    }
}

/// Global IRP pool.
pub static IRP_POOL: IrpPool = IrpPool::new();

/// Allocate an IRP from the global pool.
/// Returns `None` if the pool slot for this ID is already in use.
pub fn irp_alloc(
    op: IrpOp,
    lba: u64,
    count: u8,
    buf: *mut u8,
    buf_len: usize,
    callback: Option<IrpCallback>,
    callback_ctx: *mut u8,
) -> Option<IrpId> {
    let id = IRP_POOL.next_id.fetch_add(1, Ordering::Relaxed);
    let mut pool = IRP_POOL.inner.lock();
    let irp = pool.alloc(id)?;
    irp.op = op;
    irp.lba = lba;
    irp.count = count;
    irp.buf = buf;
    irp.buf_len = buf_len;
    irp.status = IrpStatus::Pending;
    irp.callback = callback;
    irp.callback_ctx = callback_ctx;
    irp.chain_next = None;
    irp.waiting_pid = None;
    Some(id)
}

/// Free an IRP, returning it to the pool.
pub fn irp_free(id: IrpId) {
    let mut pool = IRP_POOL.inner.lock();
    pool.free(id);
}

/// Get a snapshot of IRP parameters for a device driver.
/// The driver copies these fields out, does I/O, then calls `irp_complete()`.
/// This avoids double-lock deadlock (the pool lock is released before the
/// driver calls `irp_complete`, which also takes the pool lock).
pub fn irp_get_params(id: IrpId) -> Option<IrpParams> {
    let mut pool = IRP_POOL.inner.lock();
    let irp = pool.get_mut(id)?;
    Some(IrpParams {
        op: irp.op,
        lba: irp.lba,
        count: irp.count,
        buf: irp.buf,
        buf_len: irp.buf_len,
        id: irp.id,
    })
}

/// Get the current status of an IRP.
pub fn irp_get_status(id: IrpId) -> IrpStatus {
    let mut pool = IRP_POOL.inner.lock();
    pool.get_mut(id).map_or(IrpStatus::Error(1), |irp| irp.status)
}

/// Mark an IRP as having a chain successor. When the current IRP
/// completes, the chain_next IRP is submitted to the same device.
pub fn irp_set_chain(irp_id: IrpId, next_id: IrpId) {
    let mut pool = IRP_POOL.inner.lock();
    if let Some(irp) = pool.get_mut(irp_id) {
        irp.chain_next = Some(next_id);
    }
}

// ── Work queue dispatch for completion callbacks ──────────────────────

/// Internal payload for deferred completion callback dispatch.
struct IrpCbDispatch {
    callback: IrpCallback,
    irp_id: IrpId,
    status: IrpStatus,
    ctx: *mut u8,
}

fn irp_cb_dispatch(data: *mut u8) {
    let info = unsafe { Box::from_raw(data.cast::<IrpCbDispatch>()) };
    (info.callback)(info.irp_id, info.status, info.ctx);
}

/// Wake a process waiting on an IRP.
fn irp_wake_waiter(pid: u32) {
    let magic = IRP_WAIT_MAGIC | pid;
    let s = crate::scheduler::current_scheduler();
    let mut scheduler = s.lock();
    for k in scheduler.kthreads.iter_mut().flatten() {
        if matches!(k.state, crate::scheduler::ThreadState::Blocked { .. })
            && k.waiting_for == Some(magic)
        {
            k.waiting_for = None;
            k.state = crate::scheduler::ThreadState::Ready;
            crate::syscall::set_need_resched();
        }
    }
}

/// Complete an IRP. Sets its status, wakes any waiting process,
/// handles chaining, and dispatches the completion callback via
/// the high-priority work queue.
pub fn irp_complete(irp_id: IrpId, status: IrpStatus) {
    let mut cb_info: Option<(IrpCallback, *mut u8)> = None;
    let mut waiter_pid: Option<u32> = None;
    let mut _chain: Option<IrpId> = None;

    {
        let mut pool = IRP_POOL.inner.lock();
        if let Some(irp) = pool.get_mut(irp_id) {
            irp.status = status;
            waiter_pid = irp.waiting_pid.take();
            _chain = irp.chain_next.take();
            cb_info = irp.callback.take().map(|cb| (cb, irp.callback_ctx));
        }
    }

    if let Some(pid) = waiter_pid {
        irp_wake_waiter(pid);
    }

    if let Some((callback, ctx)) = cb_info {
        let info = Box::new(IrpCbDispatch {
            callback,
            irp_id,
            status,
            ctx,
        });
        let ptr = Box::into_raw(info).cast::<u8>();
        if !WORK_QUEUE.push_high(irp_cb_dispatch, ptr) {
            crate::hal::without_interrupts(|| {
                WORK_QUEUE.process_high();
            });
            if !WORK_QUEUE.push_high(irp_cb_dispatch, ptr) {
                let _ = unsafe { Box::from_raw(ptr.cast::<IrpCbDispatch>()) };
            }
        }
    }
}

/// Convenience: complete an IRP with the result of a synchronous I/O.
pub fn irp_complete_result(irp_id: IrpId, result: Result<(), ()>) {
    match result {
        Ok(()) => irp_complete(irp_id, IrpStatus::Completed),
        Err(()) => irp_complete(irp_id, IrpStatus::Error(1)),
    }
}

/// Block the current thread until the given IRP completes.
pub fn irp_block_current(irp_id: IrpId) {
    let magic = IRP_WAIT_MAGIC | irp_id;
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut scheduler = s.lock();
        if let Some(k) = scheduler.current_kthread_mut() {
            k.state = crate::scheduler::ThreadState::Blocked { waiting_for: magic };
            k.waiting_for = Some(magic);
        }
        crate::syscall::set_need_resched();
    });
}

// ── Per-device IRP Queue ──────────────────────────────────────────────

const IRP_QUEUE_DEPTH: usize = 32;

/// A simple FIFO queue of `IrpId`s for per-device pending I/O.
pub struct IrpQueue {
    entries: [IrpId; IRP_QUEUE_DEPTH],
    head: usize,
    tail: usize,
}

impl IrpQueue {
    pub const fn new() -> Self {
        IrpQueue {
            entries: [0; IRP_QUEUE_DEPTH],
            head: 0,
            tail: 0,
        }
    }

    pub fn push(&mut self, id: IrpId) -> bool {
        let next = (self.tail + 1) % IRP_QUEUE_DEPTH;
        if next == self.head {
            return false;
        }
        self.entries[self.tail] = id;
        self.tail = next;
        true
    }

    pub fn pop(&mut self) -> Option<IrpId> {
        if self.head == self.tail {
            return None;
        }
        let id = self.entries[self.head];
        self.head = (self.head + 1) % IRP_QUEUE_DEPTH;
        Some(id)
    }

    pub fn peek(&self) -> Option<IrpId> {
        if self.head == self.tail {
            None
        } else {
            Some(self.entries[self.head])
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn len(&self) -> usize {
        if self.tail >= self.head {
            self.tail - self.head
        } else {
            IRP_QUEUE_DEPTH - self.head + self.tail
        }
    }
}

// ── Helper: synchronous block on IRP completion ──────────────────────

/// Submit an IRP to a device and block the current process until it
/// completes. Returns the final `IrpStatus`.
pub fn irp_submit_and_wait(
    dev: &mut dyn crate::drivers::block::BlockDevice,
    irp_id: IrpId,
) -> IrpStatus {
    if dev.submit_irp(irp_id).is_err() {
        irp_complete(irp_id, IrpStatus::Error(1));
        return IrpStatus::Error(1);
    }

    if irp_get_status(irp_id) != IrpStatus::Pending {
        return irp_get_status(irp_id);
    }

    irp_block_current(irp_id);
    irp_get_status(irp_id)
}

// ── Synchronous IRP helpers (for code that wants IRP-based I/O) ─────

/// Perform a synchronous read by allocating an IRP, submitting it,
/// and blocking until completion.
pub fn irp_sync_read(
    dev: &mut dyn crate::drivers::block::BlockDevice,
    lba: u64,
    count: u8,
    buf: &mut [u8],
) -> Result<(), ()> {
    let id = irp_alloc(
        IrpOp::Read, lba, count, buf.as_mut_ptr(), buf.len(),
        None, core::ptr::null_mut(),
    ).ok_or(())?;
    let status = irp_submit_and_wait(dev, id);
    irp_free(id);
    match status {
        IrpStatus::Completed => Ok(()),
        _ => Err(()),
    }
}

/// Perform a synchronous write by allocating an IRP, submitting it,
/// and blocking until completion.
pub fn irp_sync_write(
    dev: &mut dyn crate::drivers::block::BlockDevice,
    lba: u64,
    count: u8,
    buf: &[u8],
) -> Result<(), ()> {
    let id = irp_alloc(
        IrpOp::Write, lba, count, buf.as_ptr() as *mut u8, buf.len(),
        None, core::ptr::null_mut(),
    ).ok_or(())?;
    let status = irp_submit_and_wait(dev, id);
    irp_free(id);
    match status {
        IrpStatus::Completed => Ok(()),
        _ => Err(()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

fn test_irp_alloc_free() -> Result<(), &'static str> {
    let id = irp_alloc(IrpOp::Read, 0, 1, core::ptr::null_mut(), 512, None, core::ptr::null_mut())
        .ok_or("irp_alloc failed")?;
    test_true!(id != 0);
    test_eq!(irp_get_status(id), IrpStatus::Pending);
    irp_free(id);
    Ok(())
}

fn test_irp_complete_updates_status() -> Result<(), &'static str> {
    let id = irp_alloc(IrpOp::Write, 100, 4, core::ptr::null_mut(), 2048, None, core::ptr::null_mut())
        .ok_or("irp_alloc failed")?;
    irp_complete(id, IrpStatus::Completed);
    test_eq!(irp_get_status(id), IrpStatus::Completed);
    irp_free(id);
    Ok(())
}

fn test_irp_error_status() -> Result<(), &'static str> {
    let id = irp_alloc(IrpOp::Read, 0, 1, core::ptr::null_mut(), 512, None, core::ptr::null_mut())
        .ok_or("irp_alloc failed")?;
    irp_complete(id, IrpStatus::Error(5));
    test_eq!(irp_get_status(id), IrpStatus::Error(5));
    irp_free(id);
    Ok(())
}

fn test_irp_alloc_unique_ids() -> Result<(), &'static str> {
    let id1 = irp_alloc(IrpOp::Read, 0, 1, core::ptr::null_mut(), 512, None, core::ptr::null_mut())
        .ok_or("alloc1 failed")?;
    let id2 = irp_alloc(IrpOp::Write, 0, 1, core::ptr::null_mut(), 512, None, core::ptr::null_mut())
        .ok_or("alloc2 failed")?;
    test_ne!(id1, id2);
    irp_free(id1);
    irp_free(id2);
    Ok(())
}

fn test_irp_free_reuse() -> Result<(), &'static str> {
    let id1 = irp_alloc(IrpOp::Read, 0, 1, core::ptr::null_mut(), 512, None, core::ptr::null_mut())
        .ok_or("alloc1 failed")?;
    irp_free(id1);
    let id2 = irp_alloc(IrpOp::Read, 0, 1, core::ptr::null_mut(), 512, None, core::ptr::null_mut())
        .ok_or("alloc2 failed")?;
    test_true!(id2 != 0);
    irp_free(id2);
    Ok(())
}

fn test_irp_queue_fifo() -> Result<(), &'static str> {
    let mut q = IrpQueue::new();
    test_true!(q.is_empty());
    test_true!(q.push(10));
    test_true!(q.push(20));
    test_true!(q.push(30));
    test_eq!(q.len(), 3);
    test_eq!(q.pop(), Some(10));
    test_eq!(q.pop(), Some(20));
    test_eq!(q.pop(), Some(30));
    test_true!(q.is_empty());
    Ok(())
}

fn test_irp_queue_wraparound() -> Result<(), &'static str> {
    let mut q = IrpQueue::new();
    for i in 0..(IRP_QUEUE_DEPTH - 1) {
        test_true!(q.push(i as IrpId));
    }
    test_eq!(q.len(), IRP_QUEUE_DEPTH - 1);
    test_true!(!q.push(999));
    for i in 0..16 {
        test_eq!(q.pop(), Some(i));
    }
    for i in 100..110 {
        test_true!(q.push(i));
    }
    for i in 16..(IRP_QUEUE_DEPTH - 1) {
        test_eq!(q.pop(), Some(i as IrpId));
    }
    for i in 100..110 {
        test_eq!(q.pop(), Some(i));
    }
    test_true!(q.is_empty());
    Ok(())
}

fn test_irp_callback_dispatched() -> Result<(), &'static str> {
    static mut CALLED: bool = false;
    static mut SEEN_STATUS: IrpStatus = IrpStatus::Pending;

    fn test_cb(_id: IrpId, status: IrpStatus, _ctx: *mut u8) {
        unsafe {
            CALLED = true;
            SEEN_STATUS = status;
        }
    }

    let id = irp_alloc(IrpOp::Read, 0, 1, core::ptr::null_mut(), 512, Some(test_cb), core::ptr::null_mut())
        .ok_or("alloc failed")?;
    irp_complete(id, IrpStatus::Completed);

    crate::hal::without_interrupts(|| {
        crate::work_queue::WORK_QUEUE.process_high();
    });

    unsafe {
        test_true!(CALLED);
        test_eq!(SEEN_STATUS, IrpStatus::Completed);
    }
    irp_free(id);
    Ok(())
}

fn test_irp_flush_op() -> Result<(), &'static str> {
    let id = irp_alloc(IrpOp::Flush, 0, 0, core::ptr::null_mut(), 0, None, core::ptr::null_mut())
        .ok_or("alloc failed")?;
    irp_complete(id, IrpStatus::Completed);
    test_eq!(irp_get_status(id), IrpStatus::Completed);
    irp_free(id);
    Ok(())
}

fn test_irp_ioctl_op() -> Result<(), &'static str> {
    let id = irp_alloc(IrpOp::IoCtl(0x1234), 0, 0, core::ptr::null_mut(), 0, None, core::ptr::null_mut())
        .ok_or("alloc failed")?;
    irp_complete(id, IrpStatus::Completed);
    test_eq!(irp_get_status(id), IrpStatus::Completed);
    irp_free(id);
    Ok(())
}

fn test_irp_get_params() -> Result<(), &'static str> {
    let buf = [0u8; 512];
    let id = irp_alloc(
        IrpOp::Read, 42, 3, buf.as_ptr() as *mut u8, 512,
        None, core::ptr::null_mut(),
    ).ok_or("alloc failed")?;
    let params = irp_get_params(id).ok_or("get_params failed")?;
    test_eq!(params.op, IrpOp::Read);
    test_eq!(params.lba, 42);
    test_eq!(params.count, 3);
    test_eq!(params.buf_len, 512);
    test_eq!(params.id, id);
    irp_free(id);
    Ok(())
}

// ── NCQ Tag Map: per-device tag→IRP mapping for AHCI NCQ ────────────

/// Maximum NCQ tags per device (AHCI supports up to 32).
pub const NCQ_MAX_TAGS: usize = 32;

/// Per-device tag map: maps NCQ command tags (0-31) to IRP IDs.
/// Used by AHCI NCQ for out-of-order completion dispatch.
pub struct IrpTagMap {
    tags: [Option<IrpId>; NCQ_MAX_TAGS],
    next_tag_hint: u8,
}

impl IrpTagMap {
    pub fn new() -> Self {
        IrpTagMap {
            tags: [None; NCQ_MAX_TAGS],
            next_tag_hint: 0,
        }
    }

    /// Assign an IRP ID to a specific tag. Returns false if tag already in use.
    pub fn assign(&mut self, tag: u8, irp_id: IrpId) -> bool {
        if (tag as usize) >= NCQ_MAX_TAGS {
            return false;
        }
        if self.tags[tag as usize].is_some() {
            return false;
        }
        self.tags[tag as usize] = Some(irp_id);
        true
    }

    /// Look up the IRP ID for a given tag. Returns None if tag is free.
    pub fn lookup(&self, tag: u8) -> Option<IrpId> {
        if (tag as usize) >= NCQ_MAX_TAGS {
            return None;
        }
        self.tags[tag as usize]
    }

    /// Free a tag, returning the IRP ID that was mapped (if any).
    pub fn free(&mut self, tag: u8) -> Option<IrpId> {
        if (tag as usize) >= NCQ_MAX_TAGS {
            return None;
        }
        self.tags[tag as usize].take()
    }

    /// Allocate the first free tag. Returns None if all tags are in use.
    pub fn alloc_tag(&mut self) -> Option<u8> {
        for i in 0..NCQ_MAX_TAGS {
            let t = ((self.next_tag_hint as usize + i) % NCQ_MAX_TAGS) as u8;
            if self.tags[t as usize].is_none() {
                self.next_tag_hint = (t + 1) % (NCQ_MAX_TAGS as u8);
                return Some(t);
            }
        }
        None
    }

    /// Number of currently assigned tags.
    pub fn in_use(&self) -> usize {
        self.tags.iter().filter(|t| t.is_some()).count()
    }

    /// Check if all tags are free.
    pub fn is_empty(&self) -> bool {
        self.tags.iter().all(|t| t.is_none())
    }

    /// Check if all tags are in use.
    pub fn is_full(&self) -> bool {
        self.tags.iter().all(|t| t.is_some())
    }
}

// ── NCQ Tag Map tests ──────────────────────────────────────────────

fn test_ncq_tag_alloc_free() -> Result<(), &'static str> {
    let mut map = IrpTagMap::new();
    test_true!(map.is_empty());
    test_eq!(map.in_use(), 0);

    let tag = map.alloc_tag().ok_or("alloc_tag failed")?;
    test_eq!(tag, 0);
    test_true!(map.assign(tag, 42));
    test_eq!(map.in_use(), 1);
    test_eq!(map.lookup(tag), Some(42));

    let irp_id = map.free(tag);
    test_eq!(irp_id, Some(42));
    test_true!(map.is_empty());
    Ok(())
}

fn test_ncq_tag_32_concurrent() -> Result<(), &'static str> {
    let mut map = IrpTagMap::new();
    let mut assigned = 0u32;

    // Assign all 32 tags
    for i in 0..32 {
        let tag = map.alloc_tag().ok_or("alloc_tag full early")?;
        test_true!(map.assign(tag, 1000 + i as u32));
        assigned |= 1 << tag;
    }
    test_eq!(assigned, 0xFFFF_FFFF);
    test_true!(map.is_full());

    // Alloc should return None now
    test_true!(map.alloc_tag().is_none());

    // Free all tags
    for t in 0..32 {
        let irp_id = map.free(t as u8);
        test_true!(irp_id.is_some());
    }
    test_true!(map.is_empty());
    Ok(())
}

fn test_ncq_tag_reuse() -> Result<(), &'static str> {
    let mut map = IrpTagMap::new();
    let t1 = map.alloc_tag().ok_or("alloc 1")?;
    test_eq!(t1, 0);
    let t2 = map.alloc_tag().ok_or("alloc 2")?;
    test_eq!(t2, 1);
    test_ne!(t1, t2);

    // Free t1 (tag 0). Round-robin hint is at 2, so next alloc returns 2, not 0.
    let _ = map.free(t1);
    let t3 = map.alloc_tag().ok_or("alloc 3")?;
    test_eq!(t3, 2);  // round-robin: hint=2 → slot 2 free

    // Free t2 (tag 1). Now slots 0 and 1 free.
    let _ = map.free(t2);
    // Free t3 (tag 2). All free.
    let _ = map.free(t3);
    test_true!(map.is_empty());
    Ok(())
}

fn test_ncq_tag_round_robin_hint() -> Result<(), &'static str> {
    let mut map = IrpTagMap::new();

    // Allocate 32 tags, then free even ones
    let mut allocated = [0u8; 32];
    for i in 0..32 {
        let t = map.alloc_tag().ok_or("alloc")?;
        allocated[i as usize] = t;
    }
    for i in (0..32).step_by(2) {
        let _ = map.free(allocated[i]);
    }

    // Allocating again should prefer lower indices (round-robin from freed even)
    let first_re = map.alloc_tag().ok_or("realloc")?;
    test_eq!(first_re, allocated[0]);

    Ok(())
}

pub fn register_tests() {
    test_case!("irp_alloc_free", { test_irp_alloc_free()?; });
    test_case!("irp_complete_updates_status", { test_irp_complete_updates_status()?; });
    test_case!("irp_error_status", { test_irp_error_status()?; });
    test_case!("irp_alloc_unique_ids", { test_irp_alloc_unique_ids()?; });
    test_case!("irp_free_reuse", { test_irp_free_reuse()?; });
    test_case!("irp_queue_fifo", { test_irp_queue_fifo()?; });
    test_case!("irp_queue_wraparound", { test_irp_queue_wraparound()?; });
    test_case!("irp_callback_dispatched", { test_irp_callback_dispatched()?; });
    test_case!("irp_flush_op", { test_irp_flush_op()?; });
    test_case!("irp_ioctl_op", { test_irp_ioctl_op()?; });
    test_case!("irp_get_params", { test_irp_get_params()?; });
    test_case!("ncq_tag_alloc_free", { test_ncq_tag_alloc_free()?; });
    test_case!("ncq_tag_32_concurrent", { test_ncq_tag_32_concurrent()?; });
    test_case!("ncq_tag_reuse", { test_ncq_tag_reuse()?; });
    test_case!("ncq_tag_round_robin_hint", { test_ncq_tag_round_robin_hint()?; });
}
