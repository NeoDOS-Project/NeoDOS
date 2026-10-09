use crate::log::LogSubsys;
use crate::slab::SlabAllocator;

pub const HEAP_START: u64 = 0x0240_0000; // 36 MB (after expanded user window, v0.40)
pub const HEAP_SIZE: u64 = 0x0100_0000; // 16 MB heap (36-52 MB)

#[global_allocator]
pub static ALLOCATOR: SlabAllocator = SlabAllocator::new();

/// One-line heap statistics for the fallback heap + slab pool. Diagnostic.
pub fn heap_stats_str(tag: &str) {
    let (fb_free, fb_used, fb_size) = ALLOCATOR.fallback_stats();
    let (pages, _cap, alloc, used_bytes) = ALLOCATOR.usage();
    crate::serial_println!(
        "[HEAP] {} fallback_free={} fallback_used={}/{} slab_pages={} slab_objs={} slab_bytes={}",
        tag, fb_free, fb_used, fb_size, pages, alloc, used_bytes);
}

pub fn fallback_free() -> usize {
    ALLOCATOR.fallback_stats().0
}

pub fn init() {
    kinfo!(LogSubsys::Memory, "Initializing heap allocator ({} MB @ 0x{:x})",
                    HEAP_SIZE / 1024 / 1024, HEAP_START);

    ALLOCATOR.init(HEAP_START as *mut u8, HEAP_SIZE as usize);

    kinfo!(LogSubsys::Memory, "Heap allocator ready");
}

#[alloc_error_handler]
pub fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    let (fb_free, fb_used, fb_size) = ALLOCATOR.fallback_stats();
    let (pages, _cap, alloc, used_bytes) = ALLOCATOR.usage();
    crate::raw_serial_println!(
        "[HEAP_OOM] size={} align={} fallback_free={} fallback_used={}/{} slab_pages={} slab_objs={} slab_bytes={}",
        layout.size(), layout.align(), fb_free, fb_used, fb_size, pages, alloc, used_bytes);
    kerror!(LogSubsys::Memory, "\r\n!!! ALLOCATION ERROR !!!");
    kerror!(LogSubsys::Memory, "    size: {}, align: {}", layout.size(), layout.align());
    panic!("allocation error: {:?}", layout)
}
