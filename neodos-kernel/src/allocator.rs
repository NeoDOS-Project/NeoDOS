use linked_list_allocator::LockedHeap;
use crate::serial_println;

pub const HEAP_START: u64 = 0x0100_0000; // 16 MB
pub const HEAP_SIZE: u64 = 0x0100_0000; // 16 MB heap (16-32 MB)

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn init() {
    serial_println!("[+] Initializing heap allocator ({} MB @ 0x{:x})", 
                    HEAP_SIZE / 1024 / 1024, HEAP_START);

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE as usize);
    }

    serial_println!("[+] Heap allocator ready");
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    serial_println!("\r\n!!! ALLOCATION ERROR !!!");
    serial_println!("    size: {}, align: {}", layout.size(), layout.align());
    panic!("allocation error: {:?}", layout)
}
