use x86_64::registers::control::{Cr3, Cr3Flags};
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
    if let Some(ref renderer) = crate::graphics::RENDERER {
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
                "[paging] FB 0x{:x}..0x{:x}: {} page(s) -> UC-",
                fb_start, fb_start + fb_size as u64,
                end_idx - start_idx + 1
            );
        }
    }

    // 4. Map framebuffer if it's above 4 GiB.
    //    This is common on real hardware with discrete GPUs.
    if let Some(ref renderer) = crate::graphics::RENDERER {
        let fb_start = renderer.fb.base_address;
        let fb_size = renderer.fb.size as u64;
        if fb_size > 0 && fb_start + fb_size > 0x1_0000_0000 {
            serial_println!("[paging] Mapping framebuffer at 0x{:x} ({} MB, extends >4 GiB)", fb_start, fb_size / 0x100000);
            map_phys_range_above_4g(fb_start, fb_start + fb_size, kernel_flags | PageTableFlags::HUGE_PAGE);
        }
    }

    // 5. Load our PML4 into CR3.
    let pml4_addr = PhysAddr::new(&PML4 as *const _ as u64);
    match x86_64::structures::paging::PhysFrame::from_start_address(pml4_addr) {
        Ok(frame) => {
            Cr3::write(frame, Cr3Flags::empty());
            serial_println!(
                "[+] Custom Page Tables loaded: 4 GiB identity-mapped, \
                 user window 0x{:x}..0x{:x}",
                USER_BASE, USER_LIMIT
            );
        }
        Err(_) => panic!("PML4 address 0x{:x} not 4 KB-aligned", pml4_addr.as_u64()),
    }
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
    let (frame, flags) = Cr3::read();
    Cr3::write(frame, flags);

    serial_println!(
        "[paging] map_user_range: 0x{:x}..0x{:x} ({} MB) -> USER_ACCESSIBLE",
        base_aligned, end_aligned, mapped / (1024 * 1024)
    );
}
