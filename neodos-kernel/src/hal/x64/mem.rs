use crate::arch::x64::paging::walk_ptes_4k;
use core::sync::atomic::{fence, Ordering};

pub const PAGE_SIZE: u64 = 4096;

pub extern "C" fn alloc_page() -> *mut u8 {
    match crate::memory::allocate_frame() {
        Some(phys) => phys as *mut u8,
        None => core::ptr::null_mut(),
    }
}

pub extern "C" fn free_page(ptr: *mut u8) {
    let phys = ptr as u64;
    crate::memory::free_frame(phys);
}

pub extern "C" fn map_page(phys: u64, virt: u64, flags: u64) -> i32 {
    let pte = match walk_ptes_4k(virt) {
        Some(pte) => pte,
        None => return -1,
    };
    let mut pte_flags = x86_64::structures::paging::PageTableFlags::PRESENT;
    if flags & 0x2 != 0 {
        pte_flags |= x86_64::structures::paging::PageTableFlags::WRITABLE;
    }
    if flags & 0x4 != 0 {
        pte_flags |= x86_64::structures::paging::PageTableFlags::USER_ACCESSIBLE;
    }
    if flags & 0x8 != 0 {
        pte_flags |= x86_64::structures::paging::PageTableFlags::WRITE_THROUGH;
    }
    if flags & 0x10 != 0 {
        pte_flags |= x86_64::structures::paging::PageTableFlags::NO_CACHE;
    }
    pte.set_addr(x86_64::PhysAddr::new(phys), pte_flags);
    flush_tlb_entry(virt);
    0
}

pub extern "C" fn unmap_page(virt: u64) -> i32 {
    let pte = match walk_ptes_4k(virt) {
        Some(pte) => pte,
        None => return -1,
    };
    if pte.is_unused() {
        return -1;
    }
    pte.set_unused();
    flush_tlb_entry(virt);
    0
}

pub extern "C" fn memory_barrier() {
    fence(Ordering::SeqCst);
}

fn flush_tlb_entry(virt: u64) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, nomem, preserves_flags));
    }
}
