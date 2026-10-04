//! Kernel slab allocator — per-CPU lookaside lists with global fallback.
//!
//! Architecture:
//! - 9 size classes (8B–2KB) backed by 4 KB slab pages
//! - Per-CPU hot caches (32 objects each) in KPRCB for O(1) lock-free alloc/free
//! - Global pool protected by `spin::Mutex` for cross-CPU replenishment
//! - Fallback to `linked_list_allocator` for objects >2048 bytes or alignment >16
//!
//! Fast path (alloc_local / free_local):
//!   No locks, no atomic ops. Just GS-segment reads/writes to the per-CPU
//!   free_list array. O(1) for both alloc and free.
//!
//! Slow path (refill / drain):
//!   Acquires the global `Mutex`, moves a batch of up to SLAB_BATCH_SIZE
//!   objects between global slab pages and the per-CPU hot cache.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};
use linked_list_allocator::LockedHeap;
use spin::Mutex;
use crate::memory;
use crate::log::LogSubsys;
use crate::arch::x64::cpu_local;

// ── Constants ────────────────────────────────────────────────────────────

const SLAB_PAGE_SIZE: usize = 4096;
const SLAB_MAGIC: u32 = 0x534C_4142; // "SLAB"

/// Maximum object size handled by slab (larger → fallback).
const MAX_SLAB_SIZE: usize = 2048;

/// Minimum alignment guaranteed by slab allocations.
pub const SLAB_ALIGN: usize = 16;

/// Number of per-size caches.
const NUM_CACHES: usize = 9;

/// Size classes — power-of-two from 8 to 2048.
const CACHE_SIZES: [usize; NUM_CACHES] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];

/// Batch size for refill/drain operations (must match SLAB_BATCH_SIZE in cpu_local).
const BATCH_SIZE: usize = 32;

// ── SlabPage: per-page header ────────────────────────────────────────────

/// Header stored at offset 0 of every 4 KB slab page.
///
/// With `#[repr(C, align(16))]` the header is exactly 32 bytes:
///
/// | Offset | Size | Field       |
/// |--------|------|-------------|
/// | 0      | 4    | magic       |
/// | 4      | 2    | slot_size   |
/// | 6      | 2    | capacity    |
/// | 8      | 2    | allocated   |
/// | 10     | 2    | free_head   |
/// | 12     | 4    | _alignment  |
/// | 16     | 8    | next        |
/// | 24     | 8    | _pad        |
#[repr(C, align(16))]
struct SlabPage {
    magic: u32,            // SLAB_MAGIC
    slot_size: u16,        // bytes per slot
    capacity: u16,         // total slots in this page
    allocated: u16,        // slots currently in use
    free_head: u16,        // index of first free slot (0xFFFF = full)
    next: *mut SlabPage,   // next page in the same cache
    _pad: [u8; 8],         // pad to 32 bytes
}

const _: () = {
    assert!(core::mem::size_of::<SlabPage>() == 32);
};

impl SlabPage {
    fn slots_start(&self) -> usize {
        (self as *const Self as usize) + core::mem::size_of::<SlabPage>()
    }

    fn slot_ptr(&self, idx: u16) -> *mut u8 {
        (self.slots_start() + (idx as usize) * (self.slot_size as usize)) as *mut u8
    }

    fn init(&mut self, slot_size: usize) {
        self.magic = SLAB_MAGIC;
        self.slot_size = slot_size as u16;
        let slots_start = core::mem::size_of::<SlabPage>();
        let slots_avail = SLAB_PAGE_SIZE - slots_start;
        self.capacity = (slots_avail / slot_size) as u16;
        self.allocated = 0;
        self.next = ptr::null_mut();

        if self.capacity == 0 {
            self.free_head = 0xFFFF;
            return;
        }

        // Build the free list: each free slot stores the u16 index of the
        // next free slot (0xFFFF = end of list).
        self.free_head = 0;
        for i in 0..self.capacity {
            let next = if i + 1 < self.capacity { i + 1 } else { 0xFFFF };
            unsafe { ptr::write_unaligned(self.slot_ptr(i) as *mut u16, next); }
        }
    }

    fn alloc(&mut self) -> *mut u8 {
        if self.free_head == 0xFFFF {
            return ptr::null_mut();
        }
        let idx = self.free_head;
        let slot = self.slot_ptr(idx);
        unsafe { self.free_head = ptr::read_unaligned(slot as *const u16); }
        self.allocated += 1;
        slot
    }

    fn free(&mut self, ptr: *mut u8) -> bool {
        let offset = (ptr as usize).wrapping_sub(self.slots_start());
        let sz = self.slot_size as usize;
        if offset > SLAB_PAGE_SIZE - sz || !offset.is_multiple_of(sz) {
            return false;
        }
        let idx = (offset / sz) as u16;
        if idx >= self.capacity {
            return false;
        }
        // Double-free detection: verify slot not already in free list
        // Safety: the free list is always well-formed (single-linked, 0xFFFF terminated).
        let mut cur = self.free_head;
        let mut iter_count = 0u32;
        while cur != 0xFFFF {
            if cur == idx {
                return false;
            }
            cur = unsafe { ptr::read_unaligned(self.slot_ptr(cur) as *const u16) };
            iter_count += 1;
            if iter_count > self.capacity as u32 {
                return false;
            }
        }
        unsafe { ptr::write_unaligned(ptr as *mut u16, self.free_head); }
        self.free_head = idx;
        self.allocated -= 1;
        true
    }

    fn is_full(&self) -> bool {
        self.free_head == 0xFFFF
    }
}

// ── SlabCache: single size class (global pool) ──────────────────────────

/// Global slab cache for a single size class.
/// Protected by the parent `SlabAllocator` mutex.
struct SlabCache {
    head: *mut SlabPage,
    slot_size: usize,
}

impl SlabCache {
    const fn new(slot_size: usize) -> Self {
        SlabCache { head: ptr::null_mut(), slot_size }
    }

    /// Walk the slab pages and count usage.
    /// Returns (total_pages, total_capacity, total_allocated).
    fn walk_pages(&self) -> (u64, u64, u64) {
        let mut pages = 0u64;
        let mut capacity = 0u64;
        let mut allocated = 0u64;
        let mut curr = self.head;
        while !curr.is_null() {
            let page = unsafe { &*curr };
            pages += 1;
            capacity += page.capacity as u64;
            allocated += page.allocated as u64;
            curr = page.next;
        }
        (pages, capacity, allocated)
    }

    /// Allocate a single object from the global pool.
    fn alloc(&mut self) -> *mut u8 {
        let mut curr = self.head;
        while !curr.is_null() {
            let page = unsafe { &mut *curr };
            if !page.is_full() {
                let slot = page.alloc();
                if !slot.is_null() {
                    return slot;
                }
            }
            curr = page.next;
        }

        // No free slots — allocate a new 4 KB slab page.
        let page_ptr = crate::hal::alloc_page();
        if page_ptr.is_null() {
            return ptr::null_mut();
        }

        let page = page_ptr as *mut SlabPage;
        unsafe {
            (*page).init(self.slot_size);
            (*page).next = self.head;
            self.head = page;
        }
        unsafe { (*page).alloc() }
    }

    /// Free an object back to the global pool.
    fn free(&mut self, ptr: *mut u8) -> bool {
        let page_base = (ptr as usize) & !(SLAB_PAGE_SIZE - 1);
        if page_base == 0 {
            return false;
        }
        let page = page_base as *mut SlabPage;
        let slab = unsafe { &mut *page };
        if slab.magic != SLAB_MAGIC || slab.slot_size as usize != self.slot_size {
            return false;
        }
        slab.free(ptr);
        true
    }

    /// Fill a batch of objects from the global pool into a local buffer.
    /// Returns the number of objects moved.
    fn refill_batch(&mut self, buf: &mut [*mut u8; BATCH_SIZE]) -> usize {
        let mut count = 0;
        while count < BATCH_SIZE {
            let obj = self.alloc();
            if obj.is_null() {
                break;
            }
            buf[count] = obj;
            count += 1;
        }
        count
    }

    /// Drain a batch of objects from a local buffer into the global pool.
    /// Returns the number of objects moved.
    fn drain_batch(&mut self, buf: &[*mut u8], count: usize) -> usize {
        let mut drained = 0;
        for &ptr in buf.iter().take(count) {
            if self.free(ptr) {
                drained += 1;
            }
        }
        drained
    }
}

// SAFETY: SlabCache is only ever accessed behind `spin::Mutex`,
// so raw pointer fields are safe.
unsafe impl Send for SlabCache {}
unsafe impl Sync for SlabCache {}
unsafe impl Send for SlabAllocatorInner {}
unsafe impl Sync for SlabAllocatorInner {}

// ── SlabAllocator ────────────────────────────────────────────────────────

pub struct SlabAllocator {
    inner: Mutex<SlabAllocatorInner>,
    fallback: LockedHeap,
}

struct SlabAllocatorInner {
    caches: [SlabCache; NUM_CACHES],
}

const fn new_inner() -> SlabAllocatorInner {
    let c8  = SlabCache::new(8);
    let c16 = SlabCache::new(16);
    let c32 = SlabCache::new(32);
    let c64 = SlabCache::new(64);
    let c128 = SlabCache::new(128);
    let c256 = SlabCache::new(256);
    let c512 = SlabCache::new(512);
    let c1024 = SlabCache::new(1024);
    let c2048 = SlabCache::new(2048);
    SlabAllocatorInner {
        caches: [c8, c16, c32, c64, c128, c256, c512, c1024, c2048],
    }
}

impl SlabAllocator {
    pub const fn new() -> Self {
        SlabAllocator {
            inner: Mutex::new(new_inner()),
            fallback: LockedHeap::empty(),
        }
    }

    /// Return aggregate slab allocator usage.
    /// Returns (total_pages, total_capacity_objects, total_allocated_objects, total_used_bytes).
    pub fn usage(&self) -> (u64, u64, u64, u64) {
        let inner = self.inner.lock();
        let mut pages = 0u64;
        let mut capacity = 0u64;
        let mut allocated = 0u64;
        let mut used_bytes = 0u64;
        for cache in &inner.caches {
            let (p, cap, alloc) = cache.walk_pages();
            pages += p;
            capacity += cap;
            allocated += alloc;
            used_bytes += alloc * cache.slot_size as u64;
        }
        (pages, capacity, allocated, used_bytes)
    }

    /// Fallback-heap statistics: (free_bytes, used_bytes, size_bytes).
    /// Diagnostic only; must not be called while the fallback lock is held.
    pub fn fallback_stats(&self) -> (usize, usize, usize) {
        let g = self.fallback.lock();
        (g.free(), g.used(), g.size())
    }

    pub fn init(&self, heap_start: *mut u8, heap_size: usize) {
        kinfo!(LogSubsys::Slab, "Initializing per-CPU slab allocator ({} caches, batch={})",
                       NUM_CACHES, BATCH_SIZE);

        // Reserve the fallback-heap region in the physical frame allocator
        // so that slab pages (from hal::mem::alloc_page) never collide with
        // the linked-list heap.
        memory::reserve_range(heap_start as u64, heap_size as u64);

        unsafe {
            self.fallback.lock().init(heap_start, heap_size);
        }

        kinfo!(LogSubsys::Slab, "Ready: {}B..{}B slab + {} KB fallback, per-CPU hot cache={} slots",
                       CACHE_SIZES[0], CACHE_SIZES[NUM_CACHES - 1],
                       heap_size / 1024, BATCH_SIZE);
    }

    fn cache_index(size: usize) -> Option<usize> {
        let rounded = size.next_power_of_two().max(8);
        if rounded > MAX_SLAB_SIZE {
            return None;
        }
        Some((rounded.trailing_zeros() - 3) as usize)
    }

    /// Refill the per-CPU hot cache from the global pool.
    /// Called when the local cache is empty.
    #[cold]
    fn refill_from_global(&self, cache_idx: usize) -> usize {
        let mut inner = self.inner.lock();
        let mut batch = [ptr::null_mut::<u8>(); BATCH_SIZE];
        let count = inner.caches[cache_idx].refill_batch(&mut batch);

        // Push objects into per-CPU hot cache (GS-segment writes, no lock needed)
        unsafe {
            for ptr in batch.iter().take(count) {
                let _ = cpu_local::this_cpu_slab_free_local(cache_idx, *ptr);
            }
        }
        count
    }

    /// Drain the per-CPU hot cache to the global pool.
    /// Called when the local cache is full.
    #[cold]
    fn drain_to_global(&self, cache_idx: usize) {
        let mut inner = self.inner.lock();

        // Read all objects from per-CPU hot cache
        let mut batch = [ptr::null_mut::<u8>(); BATCH_SIZE];
        let mut count = 0usize;
        unsafe {
            while let Some(obj) = cpu_local::this_cpu_slab_alloc_local(cache_idx) {
                if count >= BATCH_SIZE { break; }
                batch[count] = obj;
                count += 1;
            }
        }

        // Push into global pool
        if count > 0 {
            inner.caches[cache_idx].drain_batch(&batch, count);
        }
    }
}

// ── #476 FREE_BAD ownership audit (diagnostic, not a fix) ────────────────
//
// Validates that a `dealloc` actually owns the pointer it is about to free:
// - slab objects live on slab pages *outside* the fallback heap range;
// - fallback objects live *inside* the fallback heap range.
// The range routing is already exact; the audit adds metadata/alignment/
// double-free checks that the fast paths skip.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreeBadKind {
    OutOfRange = 0,
    Misaligned = 1,
    NotOwned = 2,
    AlreadyFree = 3,
    OwnerMismatch = 4,
    InvalidMetadata = 5,
    Unknown = 6,
}

impl FreeBadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FreeBadKind::OutOfRange => "OUT_OF_RANGE",
            FreeBadKind::Misaligned => "MISALIGNED",
            FreeBadKind::NotOwned => "NOT_OWNED",
            FreeBadKind::AlreadyFree => "ALREADY_FREE",
            FreeBadKind::OwnerMismatch => "OWNER_MISMATCH",
            FreeBadKind::InvalidMetadata => "INVALID_METADATA",
            FreeBadKind::Unknown => "UNKNOWN",
        }
    }
}

pub static FREE_BAD_COUNT: AtomicU64 = AtomicU64::new(0);
// per-kind counters (index by kind discriminant)
pub static FREE_BAD_KINDS: [AtomicU64; 7] = [const { AtomicU64::new(0) }; 7];

const FB_RING: usize = 64;
struct FreeBadEv {
    kind: u8, ptr: u64, size: u64, align: u64, cpu: u32, pid: u32, tid: u32, rsp: u64, seq: u64,
}
const FB_ZERO: FreeBadEv = FreeBadEv { kind: 0, ptr: 0, size: 0, align: 0, cpu: 0, pid: 0, tid: 0, rsp: 0, seq: 0 };
static mut FB_RING_BUF: [FreeBadEv; FB_RING] = [FB_ZERO; FB_RING];
static FB_RING_HEAD: AtomicU64 = AtomicU64::new(0);

fn record_free_bad(kind: FreeBadKind, ptr: *mut u8, layout: Layout) {
    let n = FREE_BAD_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    FREE_BAD_KINDS[kind as usize].fetch_add(1, Ordering::Relaxed);
    let cpu = unsafe { cpu_local::this_cpu_id() };
    let (pid, tid) = unsafe {
        let p = cpu_local::this_cpu_current_thread();
        if p.is_null() { (0, 0) } else { ((*p).pid, (*p).tid) }
    };
    let rsp = unsafe { crate::hal::raw::raw_read_rsp() };
    let seq = FB_RING_HEAD.fetch_add(1, Ordering::Relaxed);
    let ev = FreeBadEv {
        kind: kind as u8, ptr: ptr as u64, size: layout.size() as u64, align: layout.align() as u64,
        cpu, pid, tid, rsp, seq,
    };
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(FB_RING_BUF[(seq as usize) % FB_RING]), ev);
    }
    crate::raw_serial_println!(
        "[FREE_BAD] kind={} ptr=0x{:x} size={} align={} cpu={} pid={} tid={} caller_rsp=0x{:x} allocator={}",
        kind.as_str(), ptr as u64, layout.size(), layout.align(), cpu, pid, tid, rsp,
        if (ptr as usize) >= crate::allocator::HEAP_START as usize
            && (ptr as usize) < crate::allocator::HEAP_START as usize + crate::allocator::HEAP_SIZE as usize
        { "fallback" } else { "slab" });
    let _ = n;
}

pub fn free_bad_stats() -> (u64, [u64; 7]) {
    let mut kinds = [0u64; 7];
    for (i, k) in FREE_BAD_KINDS.iter().enumerate() { kinds[i] = k.load(Ordering::Relaxed); }
    (FREE_BAD_COUNT.load(Ordering::Relaxed), kinds)
}

/// #476: record a FREE_BAD detected by a non-slab resource allocator (e.g. the
/// paging user/heap slot tables), which have their own ownership metadata.
pub fn record_free_bad_at(kind: FreeBadKind, allocator: &'static str, id: u64, owner: u64) {
    FREE_BAD_COUNT.fetch_add(1, Ordering::Relaxed);
    FREE_BAD_KINDS[kind as usize].fetch_add(1, Ordering::Relaxed);
    let cpu = unsafe { cpu_local::this_cpu_id() };
    let (pid, tid) = unsafe {
        let p = cpu_local::this_cpu_current_thread();
        if p.is_null() { (0, 0) } else { ((*p).pid, (*p).tid) }
    };
    let rsp = unsafe { crate::hal::raw::raw_read_rsp() };
    crate::raw_serial_println!(
        "[FREE_BAD] kind={} allocator={} id={} owner={} cpu={} pid={} tid={} caller_rsp=0x{:x}",
        kind.as_str(), allocator, id, owner, cpu, pid, tid, rsp);
}

pub fn free_bad_dump() {
    let (total, kinds) = free_bad_stats();
    crate::raw_serial_println!(
        "[FREE_BAD_RING] total={} out_of_range={} misaligned={} not_owned={} already_free={} owner_mismatch={} invalid_metadata={} unknown={}",
        total, kinds[0], kinds[1], kinds[2], kinds[3], kinds[4], kinds[5], kinds[6]);
    let head = FB_RING_HEAD.load(Ordering::Relaxed);
    let n = core::cmp::min(head, FB_RING as u64);
    for s in head.saturating_sub(n)..head {
        let e = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(FB_RING_BUF[(s as usize) % FB_RING])) };
        crate::raw_serial_println!(
            "[FREE_BAD_RING] #{} kind={} ptr=0x{:x} size={} align={} cpu={} pid={} tid={} rsp=0x{:x}",
            e.seq, e.kind, e.ptr, e.size, e.align, e.cpu, e.pid, e.tid, e.rsp);
    }
}

// ── Fallback-heap allocation shadow (double-free / not-owned detection) ──
//
// The linked-list fallback heap has no per-block ownership metadata; a second
// free (or a free of a pointer it never handed out) corrupts its free list —
// the #383 kernel #PF class. A fixed open-addressing set of outstanding
// fallback pointers records ownership. Collisions/overflow are allowed to
// under-report, never to falsely accuse.
const FB_SHADOW_CAP: usize = 1 << 13;
const FB_TOMBSTONE: u64 = 1;
static FB_SHADOW: [AtomicU64; FB_SHADOW_CAP] = [const { AtomicU64::new(0) }; FB_SHADOW_CAP];

#[inline]
fn fb_slot(ptr: u64) -> usize {
    (((ptr >> 4) as usize) ^ ((ptr >> 16) as usize)) & (FB_SHADOW_CAP - 1)
}

fn fb_shadow_insert(ptr: u64) {
    if ptr == 0 { return; }
    let mut i = fb_slot(ptr);
    for _ in 0..FB_SHADOW_CAP {
        let v = FB_SHADOW[i].load(Ordering::Relaxed);
        if v == 0 || v == FB_TOMBSTONE {
            if FB_SHADOW[i].compare_exchange(v, ptr, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                return;
            }
            continue;
        }
        if v == ptr { return; } // already outstanding
        i = (i + 1) & (FB_SHADOW_CAP - 1);
    }
}

fn fb_shadow_remove(ptr: u64) -> bool {
    if ptr == 0 { return false; }
    let mut i = fb_slot(ptr);
    for _ in 0..FB_SHADOW_CAP {
        let v = FB_SHADOW[i].load(Ordering::Acquire);
        if v == 0 { return false; } // never allocated (or evicted)
        if v == ptr {
            return FB_SHADOW[i]
                .compare_exchange(ptr, FB_TOMBSTONE, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok();
        }
        i = (i + 1) & (FB_SHADOW_CAP - 1);
    }
    false
}

#[inline]
fn in_fallback_heap(addr: usize) -> bool {
    addr >= crate::allocator::HEAP_START as usize
        && addr < crate::allocator::HEAP_START as usize + crate::allocator::HEAP_SIZE as usize
}

/// O(1) ownership validation used before every `dealloc`.
/// Returns `Err(kind)` when the free would violate ownership.
unsafe fn audit_free(ptr: *mut u8, layout: Layout) -> Result<(), FreeBadKind> {
    let addr = ptr as usize;
    if layout.align() > 1 && addr % layout.align() != 0 {
        return Err(FreeBadKind::Misaligned);
    }
    if in_fallback_heap(addr) {
        // Ownership is decided by the range; double-free is handled by the
        // caller via the fallback shadow (not here).
        return Ok(());
    }
    // Outside the fallback range: must be a slab object.
    // Guard the metadata dereference against wild/low pointers.
    if addr < 0x10_0000 || addr > 0xE000_0000 {
        return Err(FreeBadKind::OutOfRange);
    }
    let page_base = addr & !(SLAB_PAGE_SIZE - 1);
    if page_base == 0 {
        return Err(FreeBadKind::OutOfRange);
    }
    // Only dereference a page we can plausibly own: slab pages are outside the
    // fallback heap and above the null/low region.
    let page = page_base as *const SlabPage;
    let magic = core::ptr::read_volatile(core::ptr::addr_of!((*page).magic));
    if magic != SLAB_MAGIC {
        return Err(FreeBadKind::NotOwned);
    }
    let slot_size = core::ptr::read_volatile(core::ptr::addr_of!((*page).slot_size)) as usize;
    let capacity = core::ptr::read_volatile(core::ptr::addr_of!((*page).capacity)) as usize;
    let expected = match (layout.align() <= SLAB_ALIGN).then(|| SlabAllocator::cache_index(layout.size())).flatten() {
        Some(idx) => CACHE_SIZES[idx],
        None => return Err(FreeBadKind::OutOfRange),
    };
    if slot_size != expected {
        return Err(FreeBadKind::OwnerMismatch);
    }
    let slots_start = page_base + core::mem::size_of::<SlabPage>();
    if addr < slots_start {
        return Err(FreeBadKind::InvalidMetadata);
    }
    let offset = addr - slots_start;
    if slot_size == 0 || !offset.is_multiple_of(slot_size) {
        return Err(FreeBadKind::Misaligned);
    }
    if offset / slot_size >= capacity {
        return Err(FreeBadKind::InvalidMetadata);
    }
    // Already sitting in this CPU's hot cache => double free.
    if let Some(idx) = SlabAllocator::cache_index(layout.size()) {
        if cpu_local::this_cpu_slab_cache_contains(idx, ptr) {
            return Err(FreeBadKind::AlreadyFree);
        }
    }
    // Already in the page free list => double free. (The global path checks
    // this in `SlabPage::free`; the per-CPU fast path does not.)
    let sidx = (offset / slot_size) as u16;
    let mut cur = core::ptr::read_volatile(core::ptr::addr_of!((*page).free_head));
    let mut it = 0u32;
    while cur != 0xFFFF {
        if cur == sidx {
            return Err(FreeBadKind::AlreadyFree);
        }
        let slot = slots_start + (cur as usize) * slot_size;
        cur = core::ptr::read_unaligned(slot as *const u16);
        it += 1;
        if it > capacity as u32 {
            return Err(FreeBadKind::InvalidMetadata);
        }
    }
    Ok(())
}

/// Test/diagnostic hook: run the FREE_BAD ownership validation without freeing.
/// Returns `Some(kind)` when the pointer would be an invalid free.
pub unsafe fn audit_free_probe(ptr: *mut u8, layout: Layout) -> Option<FreeBadKind> {
    audit_free(ptr, layout).err()
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // #376: the global pool (`inner`) and the fallback heap are non-IRQ-safe
        // spin locks. A thread descheduled — or merely interrupted — while
        // holding one deadlocks another CPU that holds the scheduler lock and
        // needs the allocator: the timer handler on the holder's CPU then spins
        // on the scheduler lock. Disable interrupts across the allocation so the
        // timer cannot fire while the lock is held.
        crate::hal::without_interrupts(|| self.alloc_inner(layout))
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        crate::hal::without_interrupts(|| self.dealloc_inner(ptr, layout));
    }
}

impl SlabAllocator {
    unsafe fn alloc_inner(&self, layout: Layout) -> *mut u8 {
        if layout.size() <= MAX_SLAB_SIZE && layout.align() <= SLAB_ALIGN {
            if let Some(idx) = Self::cache_index(layout.size()) {
                // Fast path: per-CPU hot cache (no lock, GS-segment only)
                if let Some(ptr) = cpu_local::this_cpu_slab_alloc_local(idx) {
                    cpu_local::gs_read_u64(
                        cpu_local::OFFSET_SLAB_CACHES + (idx as u32) * 288 + 0x110
                    ); // stats: total_allocated (read to bump, but we skip for perf)
                    return ptr;
                }

                // Slow path: refill from global pool (acquires lock)
                let count = self.refill_from_global(idx);
                if count > 0 {
                    if let Some(ptr) = cpu_local::this_cpu_slab_alloc_local(idx) {
                        return ptr;
                    }
                }
                // Slab OOM — fall through to fallback.
            }
        }
        // Fallback (large or over-aligned) allocation: record ownership so a
        // later double free / not-owned free is detectable (#476 FREE_BAD).
        let p = self.fallback.alloc(layout);
        if !p.is_null() {
            fb_shadow_insert(p as u64);
        }
        p
    }

    unsafe fn dealloc_inner(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        // #476 FREE_BAD: validate ownership before touching any free list.
        if let Err(kind) = audit_free(ptr, layout) {
            record_free_bad(kind, ptr, layout);
            // Diagnostic mitigation: do NOT hand an invalid pointer to a free
            // list (that is what corrupts the heap and destroys the evidence).
            // Leak it and continue.
            return;
        }

        // Route by address range, never by payload magic. The fallback heap is a
        // fixed reserved region (see `allocator::HEAP_START`/`HEAP_SIZE`) and
        // slab pages come from the buddy allocator outside it. The previous
        // heuristic read a `SLAB_MAGIC` header at the page base of *any* pointer;
        // a large fallback buffer (ELF/NXE image, kernel stack) can contain the
        // bytes `SLAB` at a 4 KiB-aligned offset, so its free was misrouted into
        // a slab cache — injecting a foreign pointer into the slab free list and
        // corrupting the kernel heap (#383).
        let addr = ptr as usize;
        let fb_start = crate::allocator::HEAP_START as usize;
        let fb_end = fb_start + crate::allocator::HEAP_SIZE as usize;
        if addr >= fb_start && addr < fb_end {
            // #476: double-free / not-owned detection for the fallback heap.
            if !fb_shadow_remove(ptr as u64) {
                record_free_bad(FreeBadKind::AlreadyFree, ptr, layout);
                return;
            }
            self.fallback.dealloc(ptr, layout);
            return;
        }

        if let Some(idx) = Self::cache_index(layout.size()) {
            // Fast path: return to per-CPU hot cache (no lock)
            if cpu_local::this_cpu_slab_free_local(idx, ptr).is_ok() {
                return;
            }
            // Slow path: drain to global pool (acquires lock)
            self.drain_to_global(idx);
            // Now the local cache has room — retry
            if cpu_local::this_cpu_slab_free_local(idx, ptr).is_ok() {
                return;
            }
        }

        // The pointer is outside the fallback heap range, so it did NOT come
        // from `self.fallback`. Handing it to the fallback free would inject a
        // foreign pointer into the fallback heap free list — the exact
        // corruption class of #383. This branch is not normally reachable
        // (slab-sized frees that reach here always drain successfully), so
        // leak-and-diagnose rather than corrupt the heap.
        crate::raw_serial_println!(
            "[SLAB_DEALLOC_LOST] ptr={:p} size={} not in fallback heap and could not be freed to a slab cache",
            ptr, layout.size()
        );
        debug_assert!(
            false,
            "slab dealloc: ptr {:p} outside fallback heap and could not be freed to a slab cache",
            ptr
        );
    }
}
