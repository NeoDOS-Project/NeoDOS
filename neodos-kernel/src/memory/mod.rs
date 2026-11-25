pub mod allocator;
pub mod buddy;
pub mod layout;
pub mod slab;

use core::mem::size_of;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;
use crate::log::LogSubsys;

const PAGE_SIZE: u64 = 4096;

const MEM_BOOT_SERVICES_CODE: u32 = 3;
const MEM_BOOT_SERVICES_DATA: u32 = 4;
const MEM_CONVENTIONAL: u32 = 7;

// Page-table entry bits used by the boot-time "is this frame safe to reuse"
// walk (see `reserve_firmware_paging_frames`).
const PTE_PRESENT: u64 = 1 << 0;
const PTE_WRITABLE: u64 = 1 << 1;
const PTE_HUGE: u64 = 1 << 7;
const PTE_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;
const HUGE_PAGE_2M: u64 = 0x20_0000;
const HUGE_PAGE_1G: u64 = 0x4000_0000;

/// Lowest physical address accepted for the buddy bitmap. Leaving the first
/// MiB alone keeps the real-mode/SMP trampoline area untouched.
const BITMAP_MIN_PHYS: u64 = 0x10_0000;

/// Bitmap words needed to track `highest` bytes of *usable* memory.
///
/// The bitmap must only cover memory the allocator actually manages. Sizing it
/// from `phys_max` (which includes the multi-TiB PCIe ECAM window) produced a
/// 32 MB bitmap for a 512 MB machine (#714).
#[inline]
fn bitmap_words_for(highest: u64) -> usize {
    (highest >> 12).div_ceil(64) as usize
}

/// Read the leaf entry that maps `phys` in the active page tables.
///
/// The kernel runs identity-mapped on the firmware page tables until
/// `init_custom_page_tables()`, so virtual == physical here. Returns the leaf
/// entry and its granularity (`HUGE_PAGE_1G`, `HUGE_PAGE_2M` or `PAGE_SIZE`).
unsafe fn walk_leaf(phys: u64) -> Option<(u64, u64)> {
    let cr3 = crate::hal::read_cr3() & !0xFFF;
    let pml4e = *((cr3 + 8 * ((phys >> 39) & 0x1FF)) as *const u64);
    if pml4e & PTE_PRESENT == 0 { return None; }

    let pdpt = (pml4e & PTE_ADDR_MASK) as *const u64;
    let pdpte = *pdpt.add(((phys >> 30) & 0x1FF) as usize);
    if pdpte & PTE_PRESENT == 0 { return None; }
    if pdpte & PTE_HUGE != 0 { return Some((pdpte, HUGE_PAGE_1G)); }

    let pd = (pdpte & PTE_ADDR_MASK) as *const u64;
    let pde = *pd.add(((phys >> 21) & 0x1FF) as usize);
    if pde & PTE_PRESENT == 0 { return None; }
    if pde & PTE_HUGE != 0 { return Some((pde, HUGE_PAGE_2M)); }

    let pt = (pde & PTE_ADDR_MASK) as *const u64;
    let pte = *pt.add(((phys >> 12) & 0x1FF) as usize);
    if pte & PTE_PRESENT == 0 { return None; }
    Some((pte, PAGE_SIZE))
}

/// Whether every 4 KiB page in `[start, start + size)` is mapped writable by
/// the active page tables.
unsafe fn range_writable(start: u64, size: u64) -> bool {
    let mut p = start;
    while p < start + size {
        match walk_leaf(p) {
            Some((entry, _)) if entry & PTE_WRITABLE != 0 => {}
            _ => return false,
        }
        p += PAGE_SIZE;
    }
    true
}

/// Reserve the frames that back the live firmware page tables and every RAM
/// range the firmware mapped read-only (W^X code, page-table pages).
///
/// The kernel keeps running on the firmware tables until
/// `init_custom_page_tables()`, so handing these frames to the buddy would
/// either fault on write or corrupt paging. Must run *after*
/// `buddy::init_from_regions()` (which frees the usable regions).
unsafe fn reserve_firmware_paging_frames() {
    let cr3 = crate::hal::read_cr3() & !0xFFF;
    buddy::mark_used_region(cr3, PAGE_SIZE);

    let pml4 = cr3 as *const u64;
    for i in 0..512usize {
        let e1 = *pml4.add(i);
        if e1 & PTE_PRESENT == 0 { continue; }
        let pdpt_frame = e1 & PTE_ADDR_MASK;
        buddy::mark_used_region(pdpt_frame, PAGE_SIZE);

        let pdpt = pdpt_frame as *const u64;
        for j in 0..512usize {
            let e2 = *pdpt.add(j);
            if e2 & PTE_PRESENT == 0 { continue; }
            if e2 & PTE_HUGE != 0 {
                if e2 & PTE_WRITABLE == 0 {
                    buddy::mark_used_region(e2 & PTE_ADDR_MASK, HUGE_PAGE_1G);
                }
                continue;
            }
            let pd_frame = e2 & PTE_ADDR_MASK;
            buddy::mark_used_region(pd_frame, PAGE_SIZE);

            let pd = pd_frame as *const u64;
            for k in 0..512usize {
                let e3 = *pd.add(k);
                if e3 & PTE_PRESENT == 0 { continue; }
                if e3 & PTE_HUGE != 0 {
                    if e3 & PTE_WRITABLE == 0 {
                        buddy::mark_used_region(e3 & PTE_ADDR_MASK, HUGE_PAGE_2M);
                    }
                    continue;
                }
                let pt_frame = e3 & PTE_ADDR_MASK;
                buddy::mark_used_region(pt_frame, PAGE_SIZE);

                // 4 KiB leaves: coalesce adjacent read-only pages.
                let pt = pt_frame as *const u64;
                let mut run_start = 0u64;
                let mut run_end = 0u64;
                for l in 0..512usize {
                    let e4 = *pt.add(l);
                    let ro = e4 & PTE_PRESENT != 0 && e4 & PTE_WRITABLE == 0;
                    if ro {
                        let p = e4 & PTE_ADDR_MASK;
                        if run_end == p {
                            run_end = p + PAGE_SIZE;
                        } else {
                            if run_end > run_start {
                                buddy::mark_used_region(run_start, run_end - run_start);
                            }
                            run_start = p;
                            run_end = p + PAGE_SIZE;
                        }
                    } else if run_end > run_start {
                        buddy::mark_used_region(run_start, run_end - run_start);
                        run_start = 0;
                        run_end = 0;
                    }
                }
                if run_end > run_start {
                    buddy::mark_used_region(run_start, run_end - run_start);
                }
            }
        }
    }
}

/// `[USER_BASE, USER_LIMIT)` is identity-mapped and used directly for Ring 3
/// code and stacks; allocator metadata must never live there.
const USER_WINDOW: (u64, u64) = (0x0040_0000, 0x0240_0000);

#[inline]
fn overlaps_window(start: u64, size: u64) -> bool {
    start < USER_WINDOW.1 && start + size > USER_WINDOW.0
}

/// Place the bitmap in writable memory, preferring the highest clean segment
/// (away from low memory and the user window). Returns 0 if no candidate is
/// usable, in which case the caller retries with the legacy bitmap size.
fn place_bitmap(segments: &mut [(u64, u64)], count: usize, pages: usize) -> u64 {
    let bytes = pages as u64 * PAGE_SIZE;
    if bytes == 0 { return 0; }

    // Highest first: keeps low memory contiguous and avoids the user window.
    for seg in segments[..count].iter_mut().rev() {
        let (s, e) = *seg;
        if e - s < bytes { continue; }
        let candidate = e - bytes;
        if candidate < BITMAP_MIN_PHYS || overlaps_window(candidate, bytes) { continue; }
        if !unsafe { range_writable(candidate, bytes) } { continue; }
        *seg = (s, candidate);
        return candidate;
    }
    // Fallback: first writable segment that fits, taken from its start.
    for seg in segments[..count].iter_mut() {
        let (s, e) = *seg;
        if e - s < bytes { continue; }
        let candidate = s;
        if candidate < BITMAP_MIN_PHYS || overlaps_window(candidate, bytes) { continue; }
        if !unsafe { range_writable(candidate, bytes) } { continue; }
        *seg = (candidate + bytes, e);
        return candidate;
    }
    0
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryDescriptorV1 {
    ty: u32,
    _pad: u32,
    phys_start: u64,
    virt_start: u64,
    page_count: u64,
    att: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryStats {
    // Physical memory (6 fields, backward compatible with v0.44)
    pub phys_max: u64,
    pub total_kib: u64,
    pub usable_kib: u64,
    pub free_kib: u64,
    pub used_kib: u64,
    pub reserved_kib: u64,

    // Kernel heap (added in v0.46 / NeoMem v0.1)
    pub kernel_heap_total_kib: u64,
    pub kernel_heap_used_kib: u64,
    pub kernel_heap_free_kib: u64,

    // User memory pools
    pub user_memory_total_kib: u64,
    pub user_memory_used_kib: u64,
    pub user_memory_free_kib: u64,

    // Paging
    pub total_pages: u64,
    pub free_pages: u64,
    pub used_pages: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryMap {
    pub total_phys: u64,
    pub highest_page: u64,
}

impl MemoryMap {
    pub fn highest_addr(&self) -> u64 {
        self.highest_page * PAGE_SIZE
    }
}

lazy_static! {
    static ref STATS: Mutex<MemoryStats> = Mutex::new(MemoryStats::default());
    static ref MMAP: Mutex<MemoryMap> = Mutex::new(MemoryMap { total_phys: 0, highest_page: 0 });
}

extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

pub fn init(boot_info: &crate::BootInfo) {
    let mut phys_max = 0u64;
    // Highest address that the allocator can actually hand out. Drives the
    // bitmap size; deliberately excludes reserved/MMIO space (#714).
    let mut usable_max = 0u64;
    let mut total_frames = 0u64;
    let mut usable_frames = 0u64;
    let mut free_regions: [Option<(u64, u64)>; 64] = [None; 64];
    let mut free_count = 0usize;

    let mmap_addr = boot_info.memory_map_addr as usize;
    let mmap_size = boot_info.memory_map_size as usize;
    let desc_size = boot_info.memory_map_desc_size as usize;

    if desc_size < size_of::<MemoryDescriptorV1>() || desc_size == 0 || mmap_size == 0 {
        return;
    }

    let mmap_bytes = unsafe { core::slice::from_raw_parts(mmap_addr as *const u8, mmap_size) };
    let entry_count = mmap_size / desc_size;

    for i in 0..entry_count {
        let off = i * desc_size;
        let desc_ptr = unsafe { mmap_bytes.as_ptr().add(off) as *const MemoryDescriptorV1 };
        let desc = unsafe { core::ptr::read_unaligned(desc_ptr) };

        let start = desc.phys_start;
        let end = desc.phys_start.saturating_add(desc.page_count.saturating_mul(PAGE_SIZE));
        phys_max = phys_max.max(end);

        let total_for_this = (end - start).div_ceil(PAGE_SIZE);
        total_frames += total_for_this;

        if matches!(desc.ty, MEM_CONVENTIONAL | MEM_BOOT_SERVICES_CODE | MEM_BOOT_SERVICES_DATA) {
            usable_frames += total_for_this;
            if free_count < 64 {
                usable_max = usable_max.max(end);
                free_regions[free_count] = Some((start, end));
                free_count += 1;
            } else {
                // Region array full: still account for the address space so
                // the bitmap covers every frame the allocator may see.
                usable_max = usable_max.max(end);
            }
        }
    }

    let reserved: &[(u64, u64)] = &[
        (0, 0x10_0000),
        (unsafe { &__kernel_start as *const u8 as u64 },
         unsafe { &__kernel_end as *const u8 as u64 }),
        (boot_info.fb_info.base_address,
         boot_info.fb_info.base_address.saturating_add(boot_info.fb_info.size as u64)),
    ];

    let mut clean_segments: [(u64, u64); 64] = [(0, 0); 64];
    let mut clean_count = 0usize;
    for &region in free_regions.iter().take(free_count) {
        if let Some((start, end)) = region {
            clean_segments[clean_count] = (start, end);
            clean_count += 1;
        }
    }
    for &(rs, re) in reserved {
        let mut n = 0usize;
        for k in 0..clean_count {
            let (s, e) = clean_segments[k];
            if e <= rs || s >= re {
                clean_segments[n] = (s, e);
                n += 1;
            } else {
                if s < rs {
                    clean_segments[n] = (s, rs);
                    n += 1;
                }
                if e > re {
                    clean_segments[n] = (re, e);
                    n += 1;
                }
            }
        }
        clean_count = n;
    }

    // ── Allocate dynamic bitmap for buddy allocator ──
    // The bitmap must be allocated BEFORE buddy init since the buddy
    // allocator manages all remaining physical memory.
    //
    // Size it from the highest *usable* address, never from `phys_max`: the
    // UEFI map contains a multi-TiB PCIe ECAM window, which used to inflate
    // the bitmap to 32 MB and push it over firmware page tables (#714).
    let mut bitmap_words = bitmap_words_for(usable_max);
    let mut bitmap_pages = bitmap_words.saturating_mul(8).div_ceil(4096);
    let mut bitmap_phys = 0u64;

    if bitmap_pages > 0 {
        bitmap_phys = place_bitmap(&mut clean_segments, clean_count, bitmap_pages);
        if bitmap_phys == 0 {
            // Retry with the legacy 4 GB tracking bitmap (128 KB). This is a
            // last resort for maps where no writable segment fits the full
            // bitmap; it must never place the bitmap over read-only frames.
            kwarn!(LogSubsys::Memory,
                "Could not place {} bitmap pages in writable memory, \
                 limiting to 4 GB tracking",
                bitmap_pages
            );
            bitmap_words = crate::memory::buddy::LEGACY_BITMAP_WORDS;
            bitmap_pages = bitmap_words.saturating_mul(8).div_ceil(4096);
            bitmap_phys = place_bitmap(&mut clean_segments, clean_count, bitmap_pages);
        }
        if bitmap_phys == 0 {
            panic!("Cannot allocate bitmap pages for buddy allocator");
        }
        // Zero-fill the bitmap pages
        for i in 0..bitmap_pages {
            unsafe { core::ptr::write_bytes((bitmap_phys + i as u64 * 4096) as *mut u8, 0, 4096); }
        }
    }

    // Initialise the buddy bitmap
    buddy::init_bitmap(bitmap_phys as *mut u64, bitmap_words);

    let mut buddy_regions: [(u64, u64); 64] = [(0, 0); 64];
    let mut buddy_count = 0usize;
    for &(s, e) in clean_segments[..clean_count].iter() {
        if e > s && buddy_count < 64 {
            buddy_regions[buddy_count] = (s, e);
            buddy_count += 1;
        }
    }

    buddy::init_from_regions(&buddy_regions[..buddy_count], phys_max);

    // Mark bitmap pages as used in the buddy allocator
    if bitmap_phys > 0 && bitmap_pages > 0 {
        buddy::mark_used_region(bitmap_phys, bitmap_pages as u64 * 4096);
    }

    // #761: the user window (4-36 MB) is identity-mapped with USER_ACCESSIBLE
    // in the shared page tables. Those physical frames belong to Ring 3 code
    // and stacks, so the buddy allocator must never hand them to the kernel:
    // a page table or heap object placed there would be writable from Ring 3.
    buddy::mark_used_region(USER_WINDOW.0, USER_WINDOW.1 - USER_WINDOW.0);

    // Firmware page tables and read-only (W^X) RAM are still live: keep them
    // out of the buddy allocator until the kernel installs its own tables.
    unsafe { reserve_firmware_paging_frames(); }
    // The reservations above only set bitmap bits; rebuild the order lists so
    // those frames are not handed out from stale free runs.
    buddy::rebuild_free_lists();

    kinfo!(LogSubsys::Memory,
        "Buddy bitmap: {} KB at 0x{:x} for {} MiB usable",
        bitmap_words.saturating_mul(8) / 1024,
        bitmap_phys,
        usable_max / 0x10_0000);

    let free_pages = buddy::free_pages();
    let _total_frames = total_frames;
    let _usable_frames = usable_frames;
    // Boot-time snapshot: dynamic fields (kernel_heap, user, paging) are
    // recomputed in stats() on each call. Set them to 0 here.
    let stats = MemoryStats {
        phys_max,
        total_kib: _total_frames.saturating_mul(4),
        usable_kib: _usable_frames.saturating_mul(4),
        free_kib: free_pages.saturating_mul(4),
        used_kib: _usable_frames.saturating_mul(4).saturating_sub(free_pages.saturating_mul(4)),
        reserved_kib: _total_frames.saturating_mul(4).saturating_sub(_usable_frames.saturating_mul(4)),
        kernel_heap_total_kib: 0,
        kernel_heap_used_kib: 0,
        kernel_heap_free_kib: 0,
        user_memory_total_kib: 0,
        user_memory_used_kib: 0,
        user_memory_free_kib: 0,
        total_pages: 0,
        free_pages: 0,
        used_pages: 0,
    };
    *STATS.lock() = stats;

    let highest_page = if phys_max > 0 { (phys_max - 1) / PAGE_SIZE + 1 } else { 0 };
    *MMAP.lock() = MemoryMap { total_phys: phys_max, highest_page };

    layout::init_default();
    validate_layout_consistency();
}

pub fn validate_layout_consistency() {
    let l = layout::layout().lock();
    if let Some(r) = l.find_region(b"user_window\0") {
        assert_eq!(r.base, crate::arch::x64::paging::USER_BASE,
            "layout USER_BASE mismatch: layout=0x{:x}, const=0x{:x}", r.base, crate::arch::x64::paging::USER_BASE);
    }
    if let Some(r) = l.find_region(b"kernel_heap\0") {
        assert_eq!(r.base, 0x0240_0000,
            "layout kernel_heap mismatch");
    }
    if let Some(r) = l.find_region(b"driver_iso\0") {
        assert_eq!(r.base, crate::drivers::isolation::DRIVER_ISO_BASE,
            "layout DRIVER_ISO_BASE mismatch");
    }
}

pub fn stats() -> MemoryStats {
    let base = *STATS.lock();

    // Recompute free/used from buddy allocator (changes dynamically)
    let free_pages_now = buddy::free_pages();
    let free_kib = free_pages_now.saturating_mul(4);
    let used_kib = base.usable_kib.saturating_sub(free_kib);

    // Kernel heap: total from allocator, used from slab allocator
    let kernel_heap_total = crate::allocator::HEAP_SIZE / 1024;
    let (_, _, _, slab_used_bytes) = crate::allocator::ALLOCATOR.usage();
    let kernel_used = slab_used_bytes / 1024;
    let kernel_free = kernel_heap_total.saturating_sub(kernel_used);

    // User memory: total pool sizes (heap slots + mmap region)
    let user_heap_total = crate::arch::x64::paging::MAX_HEAP_SLOTS as u64
        * crate::arch::x64::paging::PROCESS_HEAP_SIZE / 1024;
    let mmap_total = crate::arch::x64::paging::MMAP_TOTAL_SIZE / 1024;
    let user_total = user_heap_total + mmap_total;
    let used_slots = crate::arch::x64::paging::used_heap_slots() as u64;
    let user_used = used_slots * crate::arch::x64::paging::PROCESS_HEAP_SIZE / 1024;
    let user_free = user_total.saturating_sub(user_used);

    // Paging stats from buddy allocator
    let total_frames = buddy::total_frames();

    MemoryStats {
        phys_max: base.phys_max,
        total_kib: base.total_kib,
        usable_kib: base.usable_kib,
        free_kib,
        used_kib,
        reserved_kib: base.reserved_kib,
        kernel_heap_total_kib: kernel_heap_total,
        kernel_heap_used_kib: kernel_used,
        kernel_heap_free_kib: kernel_free,
        user_memory_total_kib: user_total,
        user_memory_used_kib: user_used,
        user_memory_free_kib: user_free,
        total_pages: total_frames,
        free_pages: free_pages_now,
        used_pages: total_frames.saturating_sub(free_pages_now),
    }
}

pub fn memory_map() -> MemoryMap {
    *MMAP.lock()
}

/// Frames handed out inside the user window (must stay zero). Diagnostic
/// counter for the #761 invariant guard.
static USER_WINDOW_ALLOCS: AtomicU64 = AtomicU64::new(0);

pub fn user_window_alloc_count() -> u64 {
    USER_WINDOW_ALLOCS.load(Ordering::Relaxed)
}

#[inline]
fn overlaps_user_window(addr: u64, size: u64) -> bool {
    let (start, end) = USER_WINDOW;
    addr < end && addr.saturating_add(size) > start
}

pub fn allocate_frame() -> Option<u64> {
    let addr = buddy::allocate_frame()?;
    if overlaps_user_window(addr, PAGE_SIZE) {
        USER_WINDOW_ALLOCS.fetch_add(1, Ordering::Relaxed);
        panic!(
            "buddy allocator returned frame 0x{:x} inside USER_WINDOW",
            addr
        );
    }
    Some(addr)
}

pub fn free_frame(phys: u64) {
    buddy::free_frame(phys);
}

pub fn alloc_frames(order: usize) -> Option<u64> {
    let addr = buddy::alloc_frames(order)?;
    let size = PAGE_SIZE << order;
    if overlaps_user_window(addr, size) {
        USER_WINDOW_ALLOCS.fetch_add(1, Ordering::Relaxed);
        panic!(
            "buddy allocator returned {}-frame block 0x{:x} inside USER_WINDOW",
            1usize << order,
            addr
        );
    }
    Some(addr)
}

pub fn free_frames(phys: u64, order: usize) {
    buddy::free_frames(phys, order);
}

pub fn reserve_range(start: u64, size: u64) {
    buddy::mark_used_region(start, size);
    // `mark_used_region` only updates the bitmap, so the frame could still sit
    // inside an already-published free run. Rebuild the free lists.
    buddy::rebuild_free_lists();
}

pub fn page_size() -> u64 {
    PAGE_SIZE
}

pub fn max_phys_addr() -> u64 {
    MMAP.lock().highest_addr()
}

// ── Stress tests ───────────────────────────────────────────────────

pub fn register_stress_tests() {
    extern crate alloc;
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use alloc::string::String;

    // Regression for #611: paging totals must reflect managed frames, not
    // phys_max, so they stay consistent with free/used.
    test_case!("memory_paging_stats_consistent", {
        let s = stats();
        test_eq!(s.used_pages, s.total_pages.saturating_sub(s.free_pages));
        test_true!(s.free_pages <= s.total_pages);
        test_true!(s.total_pages.saturating_mul(4) <= s.total_kib);
    });

    // Regression for #714: the buddy bitmap must be sized from usable memory,
    // not from `phys_max` (which includes the multi-TiB PCIe ECAM window), and
    // must never be placed over firmware read-only frames.
    test_case!("memory_bitmap_sized_by_usable", {
        // 512 MB of usable RAM -> 131072 frames -> 2048 words -> 16 KiB.
        test_eq!(bitmap_words_for(0x2000_0000), 2048);
        // Sizing from the 1 TiB ECAM window used to request 32 MB.
        test_true!(bitmap_words_for(0x1000_0000_0000) > 4_000_000);
        test_true!(bitmap_words_for(0x2000_0000) < bitmap_words_for(0x1000_0000_0000));
    });

    // Regression for #761: the identity-mapped user window must be reserved in
    // the buddy allocator, so no kernel allocation can alias Ring 3 memory.
    test_case!("memory_user_window_reserved", {
        let base = crate::arch::x64::paging::USER_BASE;
        let limit = crate::arch::x64::paging::USER_LIMIT;
        test_true!(!buddy::is_free_frame(base));
        test_true!(!buddy::is_free_frame(limit - PAGE_SIZE));

        let mut frames = Vec::new();
        for _ in 0..128 {
            match allocate_frame() {
                Some(p) => {
                    test_true!(p < base || p >= limit);
                    frames.push(p);
                }
                None => break,
            }
        }
        for p in frames {
            free_frame(p);
        }

        let block = alloc_frames(2).expect("alloc_frames failed");
        test_true!(block + 4 * PAGE_SIZE <= base || block >= limit);
        free_frames(block, 2);
        test_eq!(user_window_alloc_count(), 0);
    });

    // Every page-table frame must live outside the user window; otherwise a
    // Ring 3 write could rewrite the tables that map the kernel (#761).
    test_case!("memory_page_tables_outside_user_window", {
        let base = crate::arch::x64::paging::USER_BASE;
        let limit = crate::arch::x64::paging::USER_LIMIT;
        let outside = |p: u64| !(p < limit && p + PAGE_SIZE > base);

        let cr3 = crate::hal::read_cr3() & !0xFFF;
        test_true!(outside(cr3));
        let pml4 = cr3 as *const u64;
        for i in 0..512usize {
            let e1 = unsafe { *pml4.add(i) };
            if e1 & PTE_PRESENT == 0 || e1 & PTE_HUGE != 0 {
                continue;
            }
            let pdpt = (e1 & PTE_ADDR_MASK) as *const u64;
            test_true!(outside(pdpt as u64));
            for j in 0..512usize {
                let e2 = unsafe { *pdpt.add(j) };
                if e2 & PTE_PRESENT == 0 || e2 & PTE_HUGE != 0 {
                    continue;
                }
                let pd = (e2 & PTE_ADDR_MASK) as *const u64;
                test_true!(outside(pd as u64));
                for k in 0..512usize {
                    let e3 = unsafe { *pd.add(k) };
                    if e3 & PTE_PRESENT == 0 || e3 & PTE_HUGE != 0 {
                        continue;
                    }
                    let pt = (e3 & PTE_ADDR_MASK) as *const u64;
                    test_true!(outside(pt as u64));
                }
            }
        }
    });

    test_case!("buddy_alloc_free_sanity", {
        let p = allocate_frame().expect("alloc failed");
        free_frame(p);
    });

    test_case!("buddy_free_clears_bitmap", {
        // Frees must clear the used bit; `rebuild_free_lists` relies on the
        // bitmap being authoritative.
        let p = allocate_frame().expect("alloc failed");
        test_true!(!buddy::is_free_frame(p));
        free_frame(p);
        test_true!(buddy::is_free_frame(p));
    });

    test_case!("buddy_reserve_after_init_takes_effect", {
        // Regression for #714: reserving a range after `init_from_regions`
        // must remove it from the published free lists, not just flip bitmap
        // bits. Otherwise the allocator keeps handing the frame out.
        let p = allocate_frame().expect("alloc failed");
        free_frame(p);
        reserve_range(p, PAGE_SIZE);
        for _ in 0..64 {
            let q = allocate_frame().expect("alloc failed");
            test_true!(q != p);
            free_frame(q);
        }
    });

    test_case!("buddy_multiple_orders", {
        for order in 0..=4 {
            let p = alloc_frames(order).expect("alloc failed");
            free_frames(p, order);
        }
    });

    test_case!("buddy_stress_100k_cycles", {
        let start = crate::hal::get_ticks();
        for _ in 0..100_000 {
            let p = allocate_frame().expect("alloc failed");
            free_frame(p);
        }
        let elapsed = crate::hal::get_ticks().wrapping_sub(start);
        let _ = elapsed;
    });

    test_case!("buddy_stress_random_orders", {
        for _ in 0..1000 {
            let order = (0usize..=6).map(|i| i * 3 % 7).next().unwrap_or(0);
            let p = alloc_frames(order).expect("alloc failed");
            free_frames(p, order);
        }
    });

    test_case!("stress_mem_alloc_free_storm", {
        for _ in 0..200 {
            let b = Box::new(42u64);
            let v = *b;
            core::mem::drop(b);
            test_eq!(v, 42);
        }
    });

    test_case!("stress_mem_vec_churn", {
        let mut v = Vec::new();
        for i in 0..100 { v.push(i); }
        test_eq!(v.len(), 100);
        while v.pop().is_some() {}
        test_eq!(v.len(), 0);
        for i in 0..50 { v.push(i * 2); }
        test_eq!(v.len(), 50);
        test_eq!(v[0], 0);
        test_eq!(v[49], 98);
    });

    test_case!("stress_mem_string_churn", {
        let mut s = String::new();
        for i in 0..50 {
            s.push_str("hello");
            test_eq!(s.len(), (i + 1) * 5);
        }
        s.clear();
        test_eq!(s.len(), 0);
        for _ in 0..30 { s.push('x'); }
        test_eq!(s.len(), 30);
    });
}

// ── #476 FREE_BAD allocator ownership audit tests ──────────────────────

pub fn register_free_bad_tests() {
    extern crate alloc;
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    use core::alloc::Layout;

    test_case!("n476_free_bad_ownership_detector", {
        use crate::slab::{audit_free_probe, FreeBadKind};

        let l8 = Layout::from_size_align(8, 8).unwrap();
        let l16 = Layout::from_size_align(16, 8).unwrap();

        // Valid slab allocation: ownership must be accepted.
        let p = unsafe { alloc::alloc::alloc(l8) };
        test_true!(!p.is_null());
        test_eq!(unsafe { audit_free_probe(p, l8) }, None);

        // Misaligned pointer into the same slab page.
        test_eq!(unsafe { audit_free_probe(p.add(1), l8) }, Some(FreeBadKind::Misaligned));

        // Wrong size class => owner mismatch.
        test_eq!(unsafe { audit_free_probe(p, l16) }, Some(FreeBadKind::OwnerMismatch));

        // Out of range (page base 0).
        test_eq!(unsafe { audit_free_probe(0x8 as *mut u8, l8) }, Some(FreeBadKind::OutOfRange));

        // Not owned: a kernel image address, outside the fallback heap and with
        // no slab page magic.
        static SENTINEL: u64 = 0;
        let s = (&SENTINEL) as *const u64 as *mut u8;
        test_eq!(unsafe { audit_free_probe(s, l8) }, Some(FreeBadKind::NotOwned));

        // Free it, then re-probe: a second free must be detected.
        unsafe { alloc::alloc::dealloc(p, l8) };
        test_eq!(unsafe { audit_free_probe(p, l8) }, Some(FreeBadKind::AlreadyFree));
    });
}


