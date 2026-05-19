use core::sync::atomic::{fence, Ordering};
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::page_table::PageTableEntry;
use x86_64::structures::paging::{PageTable, PageTableFlags};
use x86_64::PhysAddr;

pub const PAGE_SIZE: u64 = 4096;

/// Walk the active x86_64 page tables to find the 4 KB PTE for `virt`.
/// Returns `None` if the page is covered by a huge page (must be split first).
pub fn walk_ptes_4k(virt: u64) -> Option<&'static mut PageTableEntry> {
    let (pml4_frame, _) = Cr3::read();
    let pml4_base = pml4_frame.start_address().as_u64();

    let pml4_idx = ((virt >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((virt >> 30) & 0x1FF) as usize;
    let pd_idx   = ((virt >> 21) & 0x1FF) as usize;
    let pt_idx   = ((virt >> 12) & 0x1FF) as usize;

    unsafe {
        let pml4 = &mut *(pml4_base as *mut PageTable);
        let pdpt = &mut *(pml4[pml4_idx].addr().as_u64() as *mut PageTable);
        let pd   = &mut *(pdpt[pdpt_idx].addr().as_u64() as *mut PageTable);

        let pde = &pd[pd_idx];
        if pde.flags().contains(PageTableFlags::HUGE_PAGE) {
            return None;
        }
        if !pde.flags().contains(PageTableFlags::PRESENT) {
            return None;
        }
        let pt = &mut *(pde.addr().as_u64() as *mut PageTable);
        Some(&mut pt[pt_idx])
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn alloc_page() -> *mut u8 {
    match crate::memory::allocate_frame() {
        Some(phys) => phys as *mut u8,
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn free_page(ptr: *mut u8) {
    let phys = ptr as u64;
    crate::memory::free_frame(phys);
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn map_page(phys: u64, virt: u64, flags: u64) -> i32 {
    let pte = match walk_ptes_4k(virt) {
        Some(pte) => pte,
        None => return -1,
    };
    let mut pte_flags = PageTableFlags::PRESENT;
    if flags & 0x2 != 0 {
        pte_flags |= PageTableFlags::WRITABLE;
    }
    if flags & 0x4 != 0 {
        pte_flags |= PageTableFlags::USER_ACCESSIBLE;
    }
    if flags & 0x8 != 0 {
        pte_flags |= PageTableFlags::WRITE_THROUGH;
    }
    if flags & 0x10 != 0 {
        pte_flags |= PageTableFlags::NO_CACHE;
    }
    pte.set_addr(PhysAddr::new(phys), pte_flags);
    crate::hal::flush_tlb(virt);
    0
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn unmap_page(virt: u64) -> i32 {
    let pte = match walk_ptes_4k(virt) {
        Some(pte) => pte,
        None => return -1,
    };
    if pte.is_unused() {
        return -1;
    }
    pte.set_unused();
    crate::hal::flush_tlb(virt);
    0
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn memory_barrier() {
    fence(Ordering::SeqCst);
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_MEM_ALLOC_PAGE: unsafe extern "C" fn() -> *mut u8 = alloc_page;
#[used]
static KEEP_MEM_FREE_PAGE: unsafe extern "C" fn(*mut u8) = free_page;
#[used]
static KEEP_MEM_MAP_PAGE: unsafe extern "C" fn(u64, u64, u64) -> i32 = map_page;
#[used]
static KEEP_MEM_UNMAP_PAGE: unsafe extern "C" fn(u64) -> i32 = unmap_page;
#[used]
static KEEP_MEM_MEMORY_BARRIER: unsafe extern "C" fn() = memory_barrier;
