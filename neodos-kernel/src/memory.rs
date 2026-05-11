//! Physical memory accounting and a simple frame allocator.
//!
//! This module is used by the `MEM` shell command and provides basic
//! *total/usable/free* statistics derived from the UEFI memory map.
//!
//! ## Current limitations
//! - Only tracks the first 4 GiB (`MAX_PHYS_ADDR`) because the kernel currently
//!   identity-maps 4 GiB in its custom page tables.
//! - Treats `CONVENTIONAL` and `BOOT_SERVICES_*` as usable after ExitBootServices.
//! - Reserves:
//!   - the first 1 MiB,
//!   - the kernel image (`__kernel_start..__kernel_end`),
//!   - the framebuffer range.
//!
//! The bitmap format is: `1 = used`, `0 = free`.

use core::mem::size_of;
use spin::Mutex;
use lazy_static::lazy_static;

const PAGE_SIZE: u64 = 4096;
const MAX_PHYS_ADDR: u64 = 0x1_0000_0000; // 4 GiB (current identity map limit)

// UEFI MemoryType values (UEFI Spec, same as uefi-raw newtype values)
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

struct FrameAllocator {
    // 4GiB / 4KiB = 1,048,576 frames => 1,048,576 bits => 131,072 bytes.
    // Stored as u64 words.
    bitmap: [u64; 16384],
    free_frames: u64,
    usable_frames: u64,
}

impl FrameAllocator {
    const fn new() -> Self {
        FrameAllocator {
            bitmap: [u64::MAX; 16384], // 1 = used, 0 = free; start with everything used.
            free_frames: 0,
            usable_frames: 0,
        }
    }

    fn mark_free_region(&mut self, start: u64, end: u64) {
        let start = start.max(0).min(MAX_PHYS_ADDR);
        let end = end.max(0).min(MAX_PHYS_ADDR);
        if end <= start {
            return;
        }
        let first = start / PAGE_SIZE;
        let last_excl = (end + PAGE_SIZE - 1) / PAGE_SIZE;
        for frame in first..last_excl {
            self.clear_bit(frame as usize);
        }
    }

    fn mark_used_region(&mut self, start: u64, end: u64) {
        let start = start.max(0).min(MAX_PHYS_ADDR);
        let end = end.max(0).min(MAX_PHYS_ADDR);
        if end <= start {
            return;
        }
        let first = start / PAGE_SIZE;
        let last_excl = (end + PAGE_SIZE - 1) / PAGE_SIZE;
        for frame in first..last_excl {
            self.set_bit(frame as usize);
        }
    }

    fn set_bit(&mut self, frame: usize) {
        let word = frame / 64;
        let bit = frame % 64;
        if word >= self.bitmap.len() {
            return;
        }
        self.bitmap[word] |= 1u64 << bit;
    }

    fn clear_bit(&mut self, frame: usize) {
        let word = frame / 64;
        let bit = frame % 64;
        if word >= self.bitmap.len() {
            return;
        }
        self.bitmap[word] &= !(1u64 << bit);
    }

    #[allow(dead_code)]
    fn is_free(&self, frame: usize) -> bool {
        let word = frame / 64;
        let bit = frame % 64;
        if word >= self.bitmap.len() {
            return false;
        }
        (self.bitmap[word] & (1u64 << bit)) == 0
    }

    fn recompute_free_count(&mut self) {
        let mut free = 0u64;
        for (word_idx, &w) in self.bitmap.iter().enumerate() {
            let used_bits = w.count_ones() as u64;
            let bits = 64u64;
            let base = (word_idx as u64) * 64;
            if base >= (MAX_PHYS_ADDR / PAGE_SIZE) {
                break;
            }
            free += bits - used_bits;
        }
        self.free_frames = free;
    }

    #[allow(dead_code)]
    fn allocate_frame(&mut self) -> Option<u64> {
        let max_frames = MAX_PHYS_ADDR / PAGE_SIZE;
        for word_idx in 0..self.bitmap.len() {
            let w = self.bitmap[word_idx];
            if w == u64::MAX {
                continue; // All bits set = all used
            }
            let base_frame = (word_idx as u64) * 64;
            if base_frame >= max_frames {
                break;
            }
            // Find first zero bit
            let free_bit = w.trailing_ones() as usize;
            let frame = base_frame + free_bit as u64;
            if frame >= max_frames {
                break;
            }
            self.set_bit(frame as usize);
            self.free_frames -= 1;
            return Some(frame * PAGE_SIZE);
        }
        None
    }
}

lazy_static! {
    static ref ALLOCATOR: Mutex<FrameAllocator> = Mutex::new(FrameAllocator::new());
    static ref STATS: Mutex<MemoryStats> = Mutex::new(MemoryStats::default());
}

extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

pub fn init(boot_info: &crate::BootInfo) {
    let mut phys_max = 0u64;
    let mut total_frames = 0u64;
    let mut usable_frames = 0u64;

    let mmap_addr = boot_info.memory_map_addr as usize;
    let mmap_size = boot_info.memory_map_size as usize;
    let desc_size = boot_info.memory_map_desc_size as usize;

    // Note: `desc_size` comes from UEFI and must be used instead of
    // `size_of::<MemoryDescriptorV1>()` when stepping the raw buffer.
    if desc_size < size_of::<MemoryDescriptorV1>() || desc_size == 0 || mmap_size == 0 {
        return;
    }

    let mmap_bytes = unsafe { core::slice::from_raw_parts(mmap_addr as *const u8, mmap_size) };
    let entry_count = mmap_size / desc_size;

    {
        let mut alloc = ALLOCATOR.lock();

        // Start: everything used. Free only known-usable regions.
        for i in 0..entry_count {
            let off = i * desc_size;
            let desc_ptr = unsafe { mmap_bytes.as_ptr().add(off) as *const MemoryDescriptorV1 };
            let desc = unsafe { core::ptr::read_unaligned(desc_ptr) };

            let start = desc.phys_start;
            let end = desc.phys_start.saturating_add(desc.page_count.saturating_mul(PAGE_SIZE));
            phys_max = phys_max.max(end);

            let clamped_end = end.min(MAX_PHYS_ADDR);
            if clamped_end > start.min(MAX_PHYS_ADDR) {
                total_frames += (clamped_end - start.min(MAX_PHYS_ADDR) + PAGE_SIZE - 1) / PAGE_SIZE;
            }

            if matches!(desc.ty, MEM_CONVENTIONAL | MEM_BOOT_SERVICES_CODE | MEM_BOOT_SERVICES_DATA) {
                alloc.mark_free_region(start, end);
                usable_frames += (clamped_end - start.min(MAX_PHYS_ADDR) + PAGE_SIZE - 1) / PAGE_SIZE;
            }
        }

        alloc.usable_frames = usable_frames;

        // Reserve low memory and known in-use ranges.
        alloc.mark_used_region(0, 0x10_0000); // first 1 MiB

        let kstart = unsafe { &__kernel_start as *const u8 as u64 };
        let kend = unsafe { &__kernel_end as *const u8 as u64 };
        alloc.mark_used_region(kstart, kend);

        let fb_start = boot_info.fb_info.base_address;
        let fb_end = fb_start.saturating_add(boot_info.fb_info.size as u64);
        alloc.mark_used_region(fb_start, fb_end);

        alloc.recompute_free_count();
    }

    let free_frames = ALLOCATOR.lock().free_frames;
    let stats = MemoryStats {
        phys_max,
        total_kib: total_frames.saturating_mul(4),
        usable_kib: usable_frames.saturating_mul(4),
        free_kib: free_frames.saturating_mul(4),
        used_kib: usable_frames.saturating_mul(4).saturating_sub(free_frames.saturating_mul(4)),
        reserved_kib: total_frames
            .saturating_mul(4)
            .saturating_sub(usable_frames.saturating_mul(4)),
    };
    *STATS.lock() = stats;
}

pub fn stats() -> MemoryStats {
    *STATS.lock()
}

#[allow(dead_code)]
pub fn allocate_frame() -> Option<u64> {
    ALLOCATOR.lock().allocate_frame()
}
