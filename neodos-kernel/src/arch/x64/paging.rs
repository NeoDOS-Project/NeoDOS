use x86_64::structures::paging::{PageTable, PageTableFlags};
use x86_64::PhysAddr;
use crate::serial_println;

#[repr(align(4096))]
pub struct AlignedPageTable(PageTable);

static mut PML4: AlignedPageTable = AlignedPageTable(PageTable::new());
static mut PDPT: AlignedPageTable = AlignedPageTable(PageTable::new());

// Extra PDPT + PDs for physical memory above 4 GiB (framebuffer, etc.)
static mut PDPT_HIGH: AlignedPageTable = AlignedPageTable(PageTable::new());
static mut PD_HIGH: [AlignedPageTable; 4] = [
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
];

/// Base address of the user-accessible memory window.
/// Must stay inside the 4 GiB identity-mapped range.
pub const USER_BASE:  u64 = 0x0040_0000; // 4 MB
pub const USER_LIMIT: u64 = 0x0080_0000; // 8 MB  (4 MB window)

/// Per-process slot constants
const MAX_BIN_SIZE: u64 = 64 * 1024;      // 64 KB  (mirrors run.rs)
const USER_STACK_SIZE: u64 = 64 * 1024;   // 64 KB
const USER_SLOT_SIZE: u64 = MAX_BIN_SIZE + USER_STACK_SIZE; // 128 KB
pub const USER_SLOT_COUNT: u64 = (USER_LIMIT - USER_BASE) / USER_SLOT_SIZE; // 32

pub struct UserSlot {
    pub code_base: u64,
    pub stack_top: u64,
    pub slot_idx: u8,
}

static mut SLOT_USED: [bool; USER_SLOT_COUNT as usize] = [false; USER_SLOT_COUNT as usize];

/// Per-process heap region: each Ring 3 process gets a 2 MB heap.
/// Region starts at 256 MB to stay clear of kernel heap and image.
pub const PROCESS_HEAP_BASE: u64 = 0x1000_0000;   // 256 MB
pub const PROCESS_HEAP_SIZE: u64 = 0x20_0000;     // 2 MB per process
pub const MAX_HEAP_SLOTS: usize = 16;

static mut HEAP_SLOT_USED: [bool; MAX_HEAP_SLOTS] = [false; MAX_HEAP_SLOTS];

pub struct HeapSlot {
    pub base: u64,
    #[allow(dead_code)]
    pub index: u8,
}

pub fn alloc_heap_slot() -> Option<HeapSlot> {
    for i in 0..MAX_HEAP_SLOTS {
        unsafe {
            if !HEAP_SLOT_USED[i] {
                HEAP_SLOT_USED[i] = true;
                let base = PROCESS_HEAP_BASE + i as u64 * PROCESS_HEAP_SIZE;
                return Some(HeapSlot { base, index: i as u8 });
            }
        }
    }
    None
}

pub fn free_heap_slot(index: u8) {
    let idx = index as usize;
    if idx < MAX_HEAP_SLOTS {
        unsafe { HEAP_SLOT_USED[idx] = false; }
    }
}

/// Allocate a free user slot, returning its base addresses.
/// Returns `None` if all slots are in use.
pub fn alloc_user_slot() -> Option<UserSlot> {
    for i in 0..USER_SLOT_COUNT as usize {
        unsafe {
            if !SLOT_USED[i] {
                SLOT_USED[i] = true;
                let base = USER_BASE + i as u64 * USER_SLOT_SIZE;
                return Some(UserSlot {
                    code_base: base,
                    stack_top: base + MAX_BIN_SIZE + USER_STACK_SIZE,
                    slot_idx: i as u8,
                });
            }
        }
    }
    None
}

/// Free a previously allocated user slot by index.
pub fn free_user_slot(slot_idx: u8) {
    let idx = slot_idx as usize;
    if idx < USER_SLOT_COUNT as usize {
        unsafe { SLOT_USED[idx] = false; }
    }
}

/// Size of one 2 MB huge page used by the PD entries.
pub const HUGE_PAGE_SIZE: u64 = 0x200000;

/// Page Directory tables (for identity-mapped 4 GiB).
/// Public because heap/alloc initialization needs to update flags.
pub static mut PD: [AlignedPageTable; 4] = [
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
    AlignedPageTable(PageTable::new()),
];

/// Set up the kernel's own page tables (replaces the UEFI-provided ones).
///
/// Layout:
///   PML4[0] → PDPT[0..4] → PD[0..4] → 2 MB huge pages
///   → identity map 0..4 GiB
///
/// USER_BASE..USER_LIMIT is additionally marked USER_ACCESSIBLE so Ring 3 can
/// read and write it.  All other pages stay kernel-only (no USER_ACCESSIBLE).
pub unsafe fn init_custom_page_tables() {
    serial_println!("[+] Initializing custom Page Tables...");

    let kernel_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    let dir_flags    = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    // 1. Link PML4[0] → PDPT
    let pdpt_addr = PhysAddr::new(&PDPT as *const _ as u64);
    PML4.0[0].set_addr(pdpt_addr, dir_flags);

    // 2. Link PDPT[0..4] → PD[0..4]
    for i in 0..4 {
        let pd_addr = PhysAddr::new(&PD[i] as *const _ as u64);
        PDPT.0[i].set_addr(pd_addr, dir_flags);
    }

    // 3. Identity-map 0..4 GiB with 2 MB huge pages.
    for i in 0..4usize {
        for j in 0..512usize {
            let addr = (i * 512 + j) as u64 * HUGE_PAGE_SIZE;

            let mut entry_flags = kernel_flags | PageTableFlags::HUGE_PAGE;
            if is_user_range(addr) {
                entry_flags |= PageTableFlags::USER_ACCESSIBLE;
            }

            PD[i].0[j].set_addr(PhysAddr::new(addr), entry_flags);
        }
    }

    // 3.5. Mark framebuffer page(s) as uncacheable (UC-).
    //      The CPU cache is incoherent with the display controller — writes
    //      to a Write-Back framebuffer stay in the cache while the scanout
    //      reads directly from physical DRAM, causing a "slow sweep" as
    //      dirty lines are gradually evicted.
    if let Some(ref renderer) = *crate::graphics::RENDERER.lock() {
        let fb_start = renderer.fb.base_address;
        let fb_size = renderer.fb.size;
        if fb_size > 0 && fb_start < 0x1_0000_0000 {
            let start_idx = (fb_start / HUGE_PAGE_SIZE) as usize;
            let end_idx = ((fb_start + fb_size as u64 - 1) / HUGE_PAGE_SIZE) as usize;
            for page_idx in start_idx..=end_idx.min(2047) {
                let pd_idx = page_idx / 512;
                let entry_idx = page_idx % 512;
                if pd_idx < 4 {
                    let phys = PD[pd_idx].0[entry_idx].addr();
                    let flags = PD[pd_idx].0[entry_idx].flags() | PageTableFlags::NO_CACHE;
                    PD[pd_idx].0[entry_idx].set_addr(phys, flags);
                }
            }
            serial_println!(
                "[PAG] FB 0x{:x}..0x{:x}: {} page(s) -> UC-",
                fb_start, fb_start + fb_size as u64,
                end_idx - start_idx + 1
            );
        }
    }

    // 4. Map framebuffer if it's above 4 GiB.
    //    This is common on real hardware with discrete GPUs.
    if let Some(ref renderer) = *crate::graphics::RENDERER.lock() {
        let fb_start = renderer.fb.base_address;
        let fb_size = renderer.fb.size as u64;
        if fb_size > 0 && fb_start + fb_size > 0x1_0000_0000 {
            serial_println!("[PAG] Mapping framebuffer at 0x{:x} ({} MB, extends >4 GiB)", fb_start, fb_size / 0x100000);
            map_phys_range_above_4g(fb_start, fb_start + fb_size, kernel_flags | PageTableFlags::HUGE_PAGE);
        }
    }

    // 5. Load our PML4 into CR3.
    let pml4_addr = &PML4 as *const _ as u64;
    if pml4_addr & 0xFFF != 0 {
        panic!("PML4 address 0x{:x} not 4 KB-aligned", pml4_addr);
    }
    crate::hal::write_cr3(pml4_addr);
    serial_println!(
        "[+] Custom Page Tables loaded: 4 GiB identity-mapped, \
         user window 0x{:x}..0x{:x}",
        USER_BASE, USER_LIMIT
    );
}

/// Map a physical range above 4 GiB using PML4[1] → PDPT_HIGH → PD_HIGH.
/// Range must be 2 MiB-aligned and within 4..8 GiB.
unsafe fn map_phys_range_above_4g(start: u64, end: u64, flags: PageTableFlags) {
    let dir_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    let pdpt_addr = PhysAddr::new(&PDPT_HIGH as *const _ as u64);
    PML4.0[1].set_addr(pdpt_addr, dir_flags);

    let start_aligned = start & !(HUGE_PAGE_SIZE - 1);
    let end_aligned = (end + HUGE_PAGE_SIZE - 1) & !(HUGE_PAGE_SIZE - 1);

    for addr in (start_aligned..end_aligned).step_by(HUGE_PAGE_SIZE as usize) {
        // 4..8 GiB → PML4[1], PDPT entry = (addr-4GiB) / 1GiB
        let _pml4_idx = 1;
        let pdpt_idx = ((addr - 0x1_0000_0000) / (512 * HUGE_PAGE_SIZE)) as usize;
        let pd_idx = ((addr - 0x1_0000_0000) / HUGE_PAGE_SIZE as u64 % 512) as usize;

        if pdpt_idx < 4 {
            // Link PDPT_HIGH[pdpt_idx] → PD_HIGH[pdpt_idx]
            let pd_addr = PhysAddr::new(&PD_HIGH[pdpt_idx] as *const _ as u64);
            PDPT_HIGH.0[pdpt_idx].set_addr(pd_addr, dir_flags);
            PD_HIGH[pdpt_idx].0[pd_idx].set_addr(PhysAddr::new(addr), flags);
        }
    }
}

/// Returns true if the 2 MB huge page starting at `addr` overlaps the user window.
///
/// A page overlaps the window if it starts inside [USER_BASE, USER_LIMIT) or
/// its end extends into the window from below.  Because our huge pages are
/// 2 MB-aligned and USER_BASE is 4 MB-aligned, a page either lies entirely
/// inside or entirely outside the window.
#[inline]
fn is_user_range(addr: u64) -> bool {
    addr >= USER_BASE && addr < USER_LIMIT
}

/// Extend the user-accessible window to cover `base..base+size`.
///
/// Both `base` and `base+size` must be 2 MB-aligned and within 0..4 GiB.
/// This function updates the PD entries in-place and flushes the TLB via CR3
/// reload (simple but correct for a single-core system).
///
/// # Safety
/// Must be called while paging is active (after `init_custom_page_tables`).
/// The caller must ensure no other CPU is running.
#[allow(dead_code)]
pub unsafe fn map_user_range(base: u64, size: u64) {
    let end = base.saturating_add(size);
    if end > 0x1_0000_0000 {
        serial_println!("[!] map_user_range: range 0x{:x}..0x{:x} exceeds 4 GiB, clamping", base, end);
    }
    let end = end.min(0x1_0000_0000);

    // Align down/up to 2 MB boundaries.
    let base_aligned = base & !(HUGE_PAGE_SIZE - 1);
    let end_aligned   = (end + HUGE_PAGE_SIZE - 1) & !(HUGE_PAGE_SIZE - 1);

    let mut mapped = 0u64;
    let mut addr = base_aligned;
    while addr < end_aligned {
        let pd_idx   = (addr / HUGE_PAGE_SIZE) as usize / 512; // which PD (0..4)
        let entry_idx = ((addr / HUGE_PAGE_SIZE) as usize) % 512; // entry inside that PD

        if pd_idx < 4 {
            let entry = &mut PD[pd_idx].0[entry_idx];
            // Add USER_ACCESSIBLE without disturbing other flags.
            let flags = entry.flags() | PageTableFlags::USER_ACCESSIBLE;
            let phys  = entry.addr();
            entry.set_addr(phys, flags);
            mapped += HUGE_PAGE_SIZE;
        }
        addr += HUGE_PAGE_SIZE;
    }

    // Flush TLB: reload CR3 with the same value.
    let cr3 = crate::hal::read_cr3();
    crate::hal::write_cr3(cr3);

    serial_println!(
        "[PAG] map_user_range: 0x{:x}..0x{:x} ({} MB) -> USER_ACCESSIBLE",
        base_aligned, end_aligned, mapped / (1024 * 1024)
    );
}

// ─────────────────────────────────────────────────────────
// 4 KB page-level heap management (on-demand paging)
// ─────────────────────────────────────────────────────────
//
// The heap region (PROCESS_HEAP_BASE .. +MAX_HEAP_SLOTS*PROCESS_HEAP_SIZE)
// is initially identity-mapped via 2 MB huge pages.
// We split those huge pages into 4 KB page tables so we can
// grant/revoke USER_ACCESSIBLE on individual pages.

pub const PAGE_4K: u64 = 4096;

/// Split a 2 MB huge page at `virt` into 512 × 4 KB page table entries.
/// Allocates a physical page for the new PT from the frame allocator.
pub fn split_2mb_page(virt: u64) -> Result<(), ()> {
    let pml4_base = crate::hal::read_cr3() & !0xFFF;

    let pml4_idx = ((virt >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((virt >> 30) & 0x1FF) as usize;
    let pd_idx   = ((virt >> 21) & 0x1FF) as usize;

    unsafe {
        let pml4 = &mut *(pml4_base as *mut PageTable);
        let pdpt = &mut *(pml4[pml4_idx].addr().as_u64() as *mut PageTable);
        let pd   = &mut *(pdpt[pdpt_idx].addr().as_u64() as *mut PageTable);

        let pde = &pd[pd_idx];
        if !pde.flags().contains(PageTableFlags::HUGE_PAGE) {
            return Ok(()); // Already split
        }
        if !pde.flags().contains(PageTableFlags::PRESENT) {
            return Err(()); // Not present
        }

        let huge_base = pde.addr().as_u64();
        let huge_flags = pde.flags();

        // Allocate a 4 KB frame for the new page table
        let pt_phys = crate::hal::alloc_page();
        if pt_phys.is_null() { return Err(()); }
        let pt = &mut *(pt_phys as *mut PageTable);
        *pt = PageTable::new();

        // Fill PT with identity-mapped 4 KB entries
        for i in 0..512u64 {
            let entry_phys = huge_base + i * PAGE_4K;
            let mut entry_flags = huge_flags;
            entry_flags.remove(PageTableFlags::HUGE_PAGE);
            pt[i as usize].set_addr(PhysAddr::new(entry_phys), entry_flags);
        }

        // Replace PD entry: point to PT, clear HUGE_PAGE
        let mut new_flags = huge_flags;
        new_flags.remove(PageTableFlags::HUGE_PAGE);
        if is_heap_virtual_addr(virt) {
            new_flags |= PageTableFlags::USER_ACCESSIBLE;
        }
        pd[pd_idx].set_addr(PhysAddr::new(pt_phys as u64), new_flags);
    }

    crate::hal::flush_tlb(virt);
    serial_println!("[PAG] split 2MB page @ 0x{:x}", virt);
    Ok(())
}

#[allow(dead_code)]
pub fn set_page_user_accessible(virt: u64, user: bool) -> Result<(), ()> {
    let entry = crate::hal::walk_ptes_4k(virt).ok_or(())?;
    let phys = entry.addr();
    let mut flags = entry.flags();
    if user {
        flags |= PageTableFlags::USER_ACCESSIBLE;
    } else {
        flags.remove(PageTableFlags::USER_ACCESSIBLE);
    }
    entry.set_addr(phys, flags);
    crate::hal::flush_tlb(virt);
    Ok(())
}

/// Check if a virtual address falls within any process's heap range.
pub fn is_heap_virtual_addr(virt: u64) -> bool {
    virt >= PROCESS_HEAP_BASE
        && virt < PROCESS_HEAP_BASE + MAX_HEAP_SLOTS as u64 * PROCESS_HEAP_SIZE
}

/// Initialize the heap region for on-demand 4 KB paging:
/// split all 2 MB huge pages covering the heap slots.
pub fn init_heap_demand_paging() {
    for i in 0..MAX_HEAP_SLOTS {
        let virt = PROCESS_HEAP_BASE + i as u64 * PROCESS_HEAP_SIZE;
        if let Err(_) = split_2mb_page(virt) {
            serial_println!("[PAG] split heap slot {} @ 0x{:x} FAILED", i, virt);
        }
    }
    serial_println!("[PAG] heap {} slots split for 4 KB demand paging", MAX_HEAP_SLOTS);
}

/// Allocate a physical 4 KB page and map it as USER_ACCESSIBLE at `virt`.
/// `virt` must be in the heap range and 4 KB-aligned.
/// Returns the physical address mapped, or `None` on OOM.
pub fn heap_alloc_page(virt: u64) -> Option<u64> {
    if !is_heap_virtual_addr(virt) || virt & 0xFFF != 0 {
        return None;
    }
    let phys = crate::hal::alloc_page();
    if phys.is_null() { return None; }
    let rc = crate::hal::map_page(phys as u64, virt, 0x6); // PRESENT | WRITABLE | USER_ACCESSIBLE
    if rc != 0 { return None; }
    Some(phys as u64)
}

/// Free a 4 KB heap page: clear its PTE and release the physical frame.
pub fn heap_free_page(virt: u64) {
    if !is_heap_virtual_addr(virt) || virt & 0xFFF != 0 {
        return;
    }
    let entry = crate::hal::walk_ptes_4k(virt);
    let Some(entry) = entry else { return };
    let phys = entry.addr().as_u64();
    let _ = crate::hal::unmap_page(virt);
    crate::hal::free_page(phys as *mut u8);
}

/// Free all heap pages in the range `[start, end)`.
pub fn heap_free_range(start: u64, end: u64) {
    let s = start.max(PROCESS_HEAP_BASE);
    let e = end.min(PROCESS_HEAP_BASE + MAX_HEAP_SLOTS as u64 * PROCESS_HEAP_SIZE);
    let mut addr = s & !(PAGE_4K - 1);
    while addr < e {
        if let Some(entry) = crate::hal::walk_ptes_4k(addr) {
            if entry.flags().contains(PageTableFlags::PRESENT) {
                let phys = entry.addr().as_u64();
                if phys != addr {
                    let _ = crate::hal::unmap_page(addr);
                    crate::hal::free_page(phys as *mut u8);
                }
            }
        }
        addr += PAGE_4K;
    }
}

/// Handle a page fault for on-demand heap allocation.
/// Called from the page fault handler.
/// Returns true if the fault was handled (instruction will be re-executed).
pub fn handle_heap_page_fault(virt: u64, user: bool, write: bool) -> bool {
    if !is_heap_virtual_addr(virt) {
        return false;
    }
    // Only handle user-mode page-not-present faults in the heap range
    if !user {
        return false;
    }

    let aligned = virt & !(PAGE_4K - 1);

    // Check if the PT entry exists (page table split)
    if crate::hal::walk_ptes_4k(aligned).is_none() {
        // Try to split the 2 MB page on demand
        if split_2mb_page(aligned).is_err() {
            return false;
        }
    }

    // Allocate a physical page and map it
    match heap_alloc_page(aligned) {
        Some(phys) => {
            serial_println!(
                "[PAG] demand-alloc 4K @ 0x{:x} → phys 0x{:x} (write={})",
                aligned, phys, write
            );
            true
        }
        None => false,
    }
}

/// Map a physical MMIO region at a chosen virtual address within 0..4 GiB.
/// The virtual address must be 4 KB-aligned and the range must fit within one
/// 2 MB huge page (the caller must split it first via `split_2mb_page`).
/// Each 4 KB page gets `flags` (e.g. PRESENT|WRITABLE|NO_CACHE for MMIO).
pub fn map_mmio_4k(virt: u64, phys: u64, size: u64, flags: PageTableFlags) -> bool {
    for off in (0..size).step_by(PAGE_4K as usize) {
        let v = virt + off;
        let p = phys + off;
        let pte = match crate::hal::walk_ptes_4k(v) {
            Some(e) => e,
            None => return false,
        };
        pte.set_addr(PhysAddr::new(p), flags);
        crate::hal::flush_tlb(v);
    }
    true
}

/// Remove the `USER_ACCESSIBLE` flag from a 2 MB-aligned range.
/// Both `base` and `base+size` must be 2 MB-aligned and within 0..4 GiB.
#[allow(dead_code)]
pub fn unmap_user_range(base: u64, size: u64) {
    let end = base.saturating_add(size).min(0x1_0000_0000);

    let base_aligned = base & !(HUGE_PAGE_SIZE - 1);
    let end_aligned = (end + HUGE_PAGE_SIZE - 1) & !(HUGE_PAGE_SIZE - 1);

    let mut unmapped = 0u64;
    let mut addr = base_aligned;
    while addr < end_aligned {
        let pd_idx = (addr / HUGE_PAGE_SIZE) as usize / 512;
        let entry_idx = ((addr / HUGE_PAGE_SIZE) as usize) % 512;

        if pd_idx < 4 {
            unsafe {
                let entry = &mut PD[pd_idx].0[entry_idx];
                let flags = entry.flags() & !PageTableFlags::USER_ACCESSIBLE;
                let phys = entry.addr();
                entry.set_addr(phys, flags);
            }
            unmapped += HUGE_PAGE_SIZE;
        }
        addr += HUGE_PAGE_SIZE;
    }

    let cr3 = crate::hal::read_cr3();
    crate::hal::write_cr3(cr3);

    serial_println!(
        "[PAG] unmap_user_range: 0x{:x}..0x{:x} ({} MB) -> kernel-only",
        base_aligned, end_aligned, unmapped / (1024 * 1024)
    );
}
