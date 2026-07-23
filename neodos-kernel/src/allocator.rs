use crate::log::LogSubsys;
use crate::slab::SlabAllocator;

pub const HEAP_START: u64 = 0x0240_0000; // 36 MB (after expanded user window, v0.40)
pub const HEAP_SIZE: u64 = 0x0100_0000; // 16 MB heap (36-52 MB)

#[global_allocator]
pub static ALLOCATOR: SlabAllocator = SlabAllocator::new();

pub fn init() {
    kinfo!(LogSubsys::Memory, "Initializing heap allocator ({} MB @ 0x{:x})",
                    HEAP_SIZE / 1024 / 1024, HEAP_START);

    ALLOCATOR.init(HEAP_START as *mut u8, HEAP_SIZE as usize);

    kinfo!(LogSubsys::Memory, "Heap allocator ready");
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    kerror!(LogSubsys::Memory, "\r\n!!! ALLOCATION ERROR !!!");
    kerror!(LogSubsys::Memory, "    size: {}, align: {}", layout.size(), layout.align());
    panic!("allocation error: {:?}", layout)
}
