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
use linked_list_allocator::LockedHeap;
use spin::Mutex;
use crate::memory;
use crate::serial_println;
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
        if offset > SLAB_PAGE_SIZE - sz || offset % sz != 0 {
            return false;
        }
        let idx = (offset / sz) as u16;
        if idx >= self.capacity {
            return false;
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
        for i in 0..count {
            if self.free(buf[i]) {
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

    pub fn init(&self, heap_start: *mut u8, heap_size: usize) {
        serial_println!("[SLAB] [+] Initializing per-CPU slab allocator ({} caches, batch={})",
                       NUM_CACHES, BATCH_SIZE);

        // Reserve the fallback-heap region in the physical frame allocator
        // so that slab pages (from hal::mem::alloc_page) never collide with
        // the linked-list heap.
        memory::reserve_range(heap_start as u64, heap_size as u64);

        unsafe {
            self.fallback.lock().init(heap_start, heap_size);
        }

        serial_println!("[SLAB] [+] Ready: {}B..{}B slab + {} KB fallback, per-CPU hot cache={} slots",
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
            for i in 0..count {
                let _ = cpu_local::this_cpu_slab_free_local(cache_idx, batch[i]);
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

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
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
        self.fallback.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        // Check if the pointer is from a slab page by inspecting the
        // page-aligned header magic.
        let page_base = (ptr as usize) & !(SLAB_PAGE_SIZE - 1);
        if page_base != 0 {
            let page = page_base as *const SlabPage;
            if (*page).magic == SLAB_MAGIC {
                let sz = (*page).slot_size as usize;
                if let Some(idx) = Self::cache_index(sz) {
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
                    // Should never fail after drain, but fall through just in case
                }
            }
        }

        self.fallback.dealloc(ptr, _layout);
    }
}
