pub mod buddy;
pub mod layout;

use core::mem::size_of;
use spin::Mutex;
use lazy_static::lazy_static;

const PAGE_SIZE: u64 = 4096;

const MEM_BOOT_SERVICES_CODE: u32 = 3;
const MEM_BOOT_SERVICES_DATA: u32 = 4;
const MEM_CONVENTIONAL: u32 = 7;

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
    pub phys_max: u64,
    pub total_kib: u64,
    pub usable_kib: u64,
    pub free_kib: u64,
    pub used_kib: u64,
    pub reserved_kib: u64,
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

        let total_for_this = (end - start + PAGE_SIZE - 1) / PAGE_SIZE;
        total_frames += total_for_this;

        if matches!(desc.ty, MEM_CONVENTIONAL | MEM_BOOT_SERVICES_CODE | MEM_BOOT_SERVICES_DATA) {
            usable_frames += total_for_this;
            if free_count < 64 {
                free_regions[free_count] = Some((start, end));
                free_count += 1;
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
    for i in 0..free_count {
        if let Some((start, end)) = free_regions[i] {
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

    // ── Allocate dynamic bitmap for buddy allocator (>4GB support) ──
    // The bitmap must be allocated BEFORE buddy init since the buddy
    // allocator manages all remaining physical memory.
    let mut bitmap_words = (((phys_max >> 12) + 63) / 64) as usize; // ceil(frames / 64)
    let bitmap_bytes = bitmap_words.saturating_mul(8);
    let mut bitmap_pages = ((bitmap_bytes + 4095) / 4096) as usize;
    let mut bitmap_phys = 0u64;

    if bitmap_pages > 0 {
        // Find a clean segment large enough to host the bitmap.
        // Take from the LAST segment (highest address) to avoid
        // fragmenting low-memory free space.
        for seg in clean_segments[..clean_count].iter_mut().rev() {
            let (s, e) = *seg;
            let region_pages = ((e - s) / 4096) as usize;
            if region_pages >= bitmap_pages {
                // Take from the END of the segment
                bitmap_phys = e - (bitmap_pages as u64 * 4096);
                *seg = (s, bitmap_phys);
                break;
            }
        }
        if bitmap_phys == 0 && clean_count > 0 {
            // Fallback: take from the first segment that fits
            for seg in clean_segments[..clean_count].iter_mut() {
                let (s, e) = *seg;
                let region_pages = ((e - s) / 4096) as usize;
                if region_pages >= bitmap_pages {
                    bitmap_phys = s;
                    *seg = (s + bitmap_pages as u64 * 4096, e);
                    break;
                }
            }
        }
        if bitmap_phys == 0 {
            // The bitmap is essential — fallback would mean no memory tracking.
            // This virtually never happens with realistic memory maps having
            // at least a few MB of free conventional memory.
            crate::serial_println!(
                "[MEM] WARNING: Could not allocate {} pages for dynamic bitmap, limiting to 4 GB tracking",
                bitmap_pages
            );
            bitmap_words = crate::memory::buddy::LEGACY_BITMAP_WORDS;
            bitmap_pages = (bitmap_words * 8 + 4095) / 4096;
            // Re-try allocation with smaller size
            for seg in clean_segments[..clean_count].iter_mut().rev() {
                let (s, e) = *seg;
                let region_pages = ((e - s) / 4096) as usize;
                if region_pages >= bitmap_pages {
                    bitmap_phys = e - (bitmap_pages as u64 * 4096);
                    *seg = (s, bitmap_phys);
                    break;
                }
            }
            if bitmap_phys == 0 {
                // Last resort: take from the beginning of first non-empty segment
                for seg in clean_segments[..clean_count].iter_mut() {
                    let (s, e) = *seg;
                    let region_pages = ((e - s) / 4096) as usize;
                    if region_pages >= bitmap_pages {
                        bitmap_phys = s;
                        *seg = (s + bitmap_pages as u64 * 4096, e);
                        break;
                    }
                }
            }
            if bitmap_phys == 0 {
                panic!("Cannot allocate bitmap pages for buddy allocator");
            }
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
    for i in 0..clean_count {
        let (s, e) = clean_segments[i];
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

    let free_pages = buddy::free_pages();
    let stats = MemoryStats {
        phys_max,
        total_kib: total_frames.saturating_mul(4),
        usable_kib: usable_frames.saturating_mul(4),
        free_kib: free_pages.saturating_mul(4),
        used_kib: usable_frames.saturating_mul(4).saturating_sub(free_pages.saturating_mul(4)),
        reserved_kib: total_frames.saturating_mul(4).saturating_sub(usable_frames.saturating_mul(4)),
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
    *STATS.lock()
}

pub fn memory_map() -> MemoryMap {
    *MMAP.lock()
}

pub fn allocate_frame() -> Option<u64> {
    buddy::allocate_frame()
}

pub fn free_frame(phys: u64) {
    buddy::free_frame(phys);
}

pub fn alloc_frames(order: usize) -> Option<u64> {
    buddy::alloc_frames(order)
}

pub fn free_frames(phys: u64, order: usize) {
    buddy::free_frames(phys, order);
}

pub fn reserve_range(start: u64, size: u64) {
    buddy::mark_used_region(start, size);
}

pub fn page_size() -> u64 {
    PAGE_SIZE
}

pub fn max_phys_addr() -> u64 {
    MMAP.lock().highest_addr()
}
