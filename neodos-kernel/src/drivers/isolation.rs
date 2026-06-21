// src/drivers/isolation.rs
// X4 — Driver Isolation Layer
//
// Partial isolation for NEM drivers with memory boundaries and access control.
//
// Design:
//   1. Drivers are loaded into a dedicated 16 MB region (0x30000000..0x31000000)
//      where page table permissions are set restrictively:
//        .text  → RX (read + execute, no write)
//        .rodata → R  (read-only)
//        .data/.bss → RW (read + write, no execute)
//   2. The export table (KERNEL_EXPORTS) is the ONLY bridge through which drivers
//      call kernel functions — direct calls to arbitrary kernel memory are blocked
//      by page permissions.
//   3. All hst_* export functions validate driver-provided pointers to ensure they
//      point to the driver's own region or valid user buffers, NOT to kernel internals.
//   4. DEMAND drivers can optionally enable sandbox mode: the page fault handler
//      detects out-of-region access and marks the driver FAULTED automatically.

use crate::arch::x64::paging::{split_2mb_page, HUGE_PAGE_SIZE};
pub use crate::arch::x64::paging::PAGE_4K;
use crate::hal;
use x86_64::structures::paging::PageTableFlags;

// ── Constants ──

/// Base address of the isolated driver memory region.
pub const DRIVER_ISO_BASE: u64 = 0x3000_0000;   // 768 MB
/// Total size of the isolated driver region.
pub const DRIVER_ISO_SIZE: u64 = 0x100_0000;    // 16 MB
pub const DRIVER_ISO_END: u64 = DRIVER_ISO_BASE + DRIVER_ISO_SIZE;

/// Maximum number of drivers that can be loaded in the isolated region.
/// 16 slots fit in 16 MB with 1 MB per slot.
pub const MAX_ISOLATED_DRIVERS: usize = 16;
/// Maximum size per driver (1 MB).
pub const MAX_DRIVER_SIZE: u64 = 0x10_0000;

/// Minimum slot size per driver in the region (align to 1 MB for page table split).
pub const DRIVER_SLOT_SIZE: u64 = 0x10_0000; // 1 MB

// ── Permission flags ──

pub const PERM_RX: u64 = 0x0;   // not writable, not user (we run in ring 0)
pub const PERM_R: u64 = 0x0;
pub const PERM_RW: u64 = 0x2;   // writable

// ── Sandbox status ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IsolationMode {
    None = 0,    // No isolation (legacy, heap-allocated)
    Basic = 1,   // Page-isolated with export table bridge + arg validation
    Sandbox = 2, // Full sandbox: page faults trigger FAULTED automatically
}

// ── Driver region tracking ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriverMemoryRegion {
    pub base: u64,
    pub size: u64,
    pub text_size: u64,
    pub rodata_size: u64,
    pub data_size: u64,
    pub bss_size: u64,
    pub driver_id: u32,
    pub in_use: bool,
}

const EMPTY_REGION: DriverMemoryRegion = DriverMemoryRegion {
    base: 0, size: 0, text_size: 0, rodata_size: 0,
    data_size: 0, bss_size: 0, driver_id: 0, in_use: false,
};

pub static mut ISOLATED_REGIONS: [DriverMemoryRegion; MAX_ISOLATED_DRIVERS] = [EMPTY_REGION; MAX_ISOLATED_DRIVERS];

// ── Region helpers ──

/// Check if a virtual address falls within the isolated driver region.
pub fn is_in_isolated_region(virt: u64) -> bool {
    virt >= DRIVER_ISO_BASE && virt < DRIVER_ISO_END
}

/// Check if a virtual address falls within a specific driver's allocated region.
pub fn is_in_driver_region(virt: u64, driver_id: u32) -> bool {
    unsafe {
        for region in ISOLATED_REGIONS.iter() {
            if region.in_use && region.driver_id == driver_id {
                if virt >= region.base && virt < region.base + region.size {
                    return true;
                }
            }
        }
    }
    false
}

/// Find which driver owns a given virtual address.
pub fn driver_id_for_address(virt: u64) -> Option<u32> {
    unsafe {
        for region in ISOLATED_REGIONS.iter() {
            if region.in_use && virt >= region.base && virt < region.base + region.size {
                return Some(region.driver_id);
            }
        }
    }
    None
}

/// Validate that a pointer + size falls entirely within the driver's region.
/// Returns Ok(()) if valid, Err with description if invalid.
pub fn validate_driver_ptr(
    ptr: *const u8,
    size: usize,
    driver_id: u32,
    writable: bool,
) -> Result<(), &'static str> {
    if ptr.is_null() {
        return Err("Null pointer");
    }
    let addr = ptr as u64;
    let end = addr.wrapping_add(size as u64);

    // Check for overflow
    if end < addr {
        return Err("Pointer range wraps around address space");
    }

    // Allow pointers into the isolated driver region
    if is_in_driver_region(addr, driver_id) && is_in_driver_region(end.wrapping_sub(1), driver_id) {
        if writable {
            // Check the pointer is not in RX/R-only sub-regions
            // .text (base..base+text_size) should not be written to
            // .rodata (base+text_size..base+text_size+rodata_size) should not be written to
            unsafe {
                for region in ISOLATED_REGIONS.iter() {
                    if region.in_use && region.driver_id == driver_id {
                        let text_end = region.base + region.text_size;
                        let rodata_end = text_end + region.rodata_size;
                        // If the range overlaps text or rodata, reject
                        if addr < text_end && end > region.base {
                            return Err("Write to read-only code section denied");
                        }
                        if addr >= text_end && addr < rodata_end {
                            return Err("Write to read-only data section denied");
                        }
                        if addr < rodata_end && end > rodata_end {
                            return Err("Write spans read-only data section");
                        }
                        break;
                    }
                }
            }
        }
        return Ok(());
    }

    // Allow pointers to user memory (heap or mmap regions) for buffer passing
    if addr >= crate::arch::x64::paging::PROCESS_HEAP_BASE
        && end <= crate::arch::x64::paging::PROCESS_HEAP_BASE
            + (crate::arch::x64::paging::MAX_HEAP_SLOTS as u64)
            * crate::arch::x64::paging::PROCESS_HEAP_SIZE
    {
        return Ok(());
    }
    if addr >= crate::arch::x64::paging::MMAP_BASE
        && end <= crate::arch::x64::paging::MMAP_BASE
            + crate::arch::x64::paging::MMAP_TOTAL_SIZE
    {
        return Ok(());
    }

    // Allow pointers to kernel .rodata/.text (string constants, export table)
    // Kernel identity-mapped region: 0x0010_0000..0x0040_0000
    if addr >= 0x0010_0000 && end <= 0x0040_0000 {
        return Ok(());
    }

    // Allow pointers to kernel heap (where parsed NEM data resides)
    // v0.40: moved from 0x0100_0000 to after expanded user window
    if addr >= 0x0240_0000 && end <= 0x0340_0000 {
        return Ok(());
    }

    // Allow pointers to kernel identity-mapped memory between kernel heap and user heap.
    // Covers page tables, boot stack, and kernel data.
    if addr >= 0x0340_0000 && end <= 0x1000_0000 {
        return Ok(());
    }

    // Allow pointers to kernel identity-mapped memory between user heap end and mmap base.
    // Covers the boot stack at ~0x1FFFF000 and any other kernel data.
    if addr >= 0x1200_0000 && end <= 0x2000_0000 {
        return Ok(());
    }

    Err("Pointer outside allowed memory regions")
}

/// Validate that a string pointer (null-terminated) falls within the driver's region.
pub fn validate_driver_str_ptr(ptr: *const u8, driver_id: u32) -> Result<usize, &'static str> {
    if ptr.is_null() {
        return Err("Null string pointer");
    }
    // Scan for null terminator (max 256 bytes to avoid hangs)
    unsafe {
        for i in 0..256usize {
            let b = *ptr.add(i);
            if b == 0 {
                return validate_driver_ptr(ptr, i + 1, driver_id, false)
                    .map(|_| i);
            }
        }
    }
    Err("String too long or not null-terminated")
}

// ── Region initialization ──

/// Initialize the isolated driver region by splitting 2 MB pages for 4 KB paging.
/// Called once during boot (PHASE 3.8x).
pub fn init_isolated_region() {
    crate::serial_println!("[ISO] Initializing isolated driver region: 0x{:x}..0x{:x} ({} MB)",
        DRIVER_ISO_BASE, DRIVER_ISO_END, DRIVER_ISO_SIZE / 0x100000);

    let mut count = 0u32;
    let mut addr = DRIVER_ISO_BASE;
    while addr < DRIVER_ISO_END {
        match split_2mb_page(addr) {
            Ok(_) => count += 1,
            Err(_) => {
                crate::serial_println!("[ISO]  split @ 0x{:x} FAILED", addr);
            }
        }
        addr += HUGE_PAGE_SIZE;
    }
    crate::serial_println!("[ISO]  {} huge pages split for 4 KB isolation", count);

    // Unmap the entire region initially (no pre-allocated pages)
    let mut unmapped = 0u32;
    addr = DRIVER_ISO_BASE;
    while addr < DRIVER_ISO_END {
        if let Some(entry) = hal::walk_ptes_4k(addr) {
            if entry.flags().contains(PageTableFlags::PRESENT) {
                let phys = entry.addr().as_u64();
                if phys != addr {
                    let _ = hal::unmap_page(addr);
                    hal::free_page(phys as *mut u8);
                    unmapped += 1;
                }
            }
        }
        addr += PAGE_4K;
    }
    crate::serial_println!("[ISO]  {} identity-mapped pages stripped for isolation", unmapped);
}

// ── Per-driver page allocation in the isolated region ──

/// Allocate an isolated 4 KB page at the given virtual address within the region.
/// Returns the physical address on success.
pub fn alloc_isolated_page(virt: u64, flags: u64) -> Option<u64> {
    if !is_in_isolated_region(virt) || virt & 0xFFF != 0 {
        return None;
    }

    let phys = hal::alloc_page();
    if phys.is_null() {
        return None;
    }
    unsafe {
        core::ptr::write_bytes(phys as *mut u8, 0, PAGE_4K as usize);
    }

    let pte_flags = if flags & 0x2 != 0 {
        0x2 // PRESENT | WRITABLE (kernel-only, no USER_ACCESSIBLE)
    } else {
        0x0 // PRESENT only
    };

    let rc = hal::map_page(phys as u64, virt, pte_flags);
    if rc != 0 {
        hal::free_page(phys);
        return None;
    }
    Some(phys as u64)
}

/// Free an isolated page.
pub fn free_isolated_page(virt: u64) {
    if !is_in_isolated_region(virt) || virt & 0xFFF != 0 {
        return;
    }
    if let Some(entry) = hal::walk_ptes_4k(virt) {
        if entry.flags().contains(PageTableFlags::PRESENT) {
            let phys = entry.addr().as_u64();
            if phys != virt {
                let _ = hal::unmap_page(virt);
                hal::free_page(phys as *mut u8);
            }
        }
    }
}

/// Free a contiguous range of isolated pages [start, end).
pub fn free_isolated_range(start: u64, end: u64) {
    let s = start.max(DRIVER_ISO_BASE);
    let e = end.min(DRIVER_ISO_END);
    let mut addr = s & !(PAGE_4K - 1);
    while addr < e {
        free_isolated_page(addr);
        addr += PAGE_4K;
    }
}

/// Set page table permissions for a single 4 KB page in the isolated region.
pub fn set_page_permissions(virt: u64, writable: bool) -> bool {
    if !is_in_isolated_region(virt) || virt & 0xFFF != 0 {
        return false;
    }
    let entry = match hal::walk_ptes_4k(virt) {
        Some(e) => e,
        None => return false,
    };
    let phys = entry.addr();
    let mut flags = entry.flags();
    if writable {
        flags |= PageTableFlags::WRITABLE;
    } else {
        flags.remove(PageTableFlags::WRITABLE);
    }
    entry.set_addr(phys, flags);
    hal::flush_tlb(virt);
    true
}

/// Set RX permissions for a range (code section).
pub fn set_rx_permissions(base: u64, size: u64) {
    for off in (0..size).step_by(PAGE_4K as usize) {
        set_page_permissions(base + off, false);
    }
}

/// Set RW permissions for a range (data/bss section).
pub fn set_rw_permissions(base: u64, size: u64) {
    for off in (0..size).step_by(PAGE_4K as usize) {
        set_page_permissions(base + off, true);
    }
}

// ── Driver region management ──

/// Allocate a slot in the isolated region for a driver of the given size.
/// Returns the base address or None if no slot available.
pub fn allocate_driver_slot(driver_id: u32, size: u64) -> Option<u64> {
    if size > MAX_DRIVER_SIZE {
        return None;
    }

    unsafe {
        for i in 0..MAX_ISOLATED_DRIVERS {
            if !ISOLATED_REGIONS[i].in_use {
                let base = DRIVER_ISO_BASE + i as u64 * DRIVER_SLOT_SIZE;
                ISOLATED_REGIONS[i] = DriverMemoryRegion {
                    base,
                    size,
                    text_size: 0,
                    rodata_size: 0,
                    data_size: 0,
                    bss_size: 0,
                    driver_id,
                    in_use: true,
                };
                return Some(base);
            }
        }
    }
    None
}

/// Set the sub-region sizes for a driver (after loading).
pub fn set_driver_layout(
    driver_id: u32,
    text_size: u64,
    rodata_size: u64,
    data_size: u64,
    bss_size: u64,
) -> bool {
    unsafe {
        for region in ISOLATED_REGIONS.iter_mut() {
            if region.in_use && region.driver_id == driver_id {
                region.text_size = text_size;
                region.rodata_size = rodata_size;
                region.data_size = data_size;
                region.bss_size = bss_size;
                return true;
            }
        }
    }
    false
}

/// Free a driver slot and release all allocated pages.
pub fn free_driver_slot(driver_id: u32) {
    unsafe {
        for region in ISOLATED_REGIONS.iter_mut() {
            if region.in_use && region.driver_id == driver_id {
                let start = region.base;
                let end = region.base + DRIVER_SLOT_SIZE; // Free whole slot
                free_isolated_range(start, end);
                *region = EMPTY_REGION;
                crate::serial_println!("[ISO] Freed driver slot: id={} @ 0x{:x}", driver_id, start);
                return;
            }
        }
    }
}

/// Get the base address of a driver in the isolated region.
pub fn driver_base(driver_id: u32) -> Option<u64> {
    unsafe {
        for region in ISOLATED_REGIONS.iter() {
            if region.in_use && region.driver_id == driver_id {
                return Some(region.base);
            }
        }
    }
    None
}

/// Get the isolation mode string for a driver category.
pub fn isolation_mode_for_category(cat: crate::nem::DriverCategory) -> IsolationMode {
    match cat {
        crate::nem::DriverCategory::Boot => IsolationMode::Basic,
        crate::nem::DriverCategory::System => IsolationMode::Basic,
        crate::nem::DriverCategory::Demand => IsolationMode::Sandbox,
    }
}

/// Return human-readable isolation mode string.
pub fn isolation_mode_str(mode: IsolationMode) -> &'static str {
    match mode {
        IsolationMode::None => "NONE",
        IsolationMode::Basic => "BASIC",
        IsolationMode::Sandbox => "SANDBOX",
    }
}

/// Iterator over all active isolated regions.
pub fn iter_isolated_regions() -> core::iter::FilterMap<
    core::slice::Iter<'static, DriverMemoryRegion>,
    fn(&DriverMemoryRegion) -> Option<(u32, u64, u64)>,
> {
    unsafe {
        ISOLATED_REGIONS.iter().filter_map(|r| {
            if r.in_use {
                Some((r.driver_id, r.base, r.size))
            } else {
                None
            }
        })
    }
}

// ── Page fault integration for sandbox mode ──

/// Handle a page fault in the isolated region.
/// For sandboxed drivers, this marks the driver as FAULTED if the access is invalid.
/// Returns true if the fault was handled (page was on-demand allocated).
pub fn handle_isolated_page_fault(virt: u64, _user: bool, _write: bool) -> bool {
    if !is_in_isolated_region(virt) {
        return false;
    }

    // Find which driver this address belongs to
    let driver_id = match driver_id_for_address(virt) {
        Some(id) => id,
        None => {
            crate::serial_println!("[ISO] Page fault in isolated region at 0x{:x} — no owning driver", virt);
            return false;
        }
    };

    let aligned = virt & !(PAGE_4K - 1);

    // If the page is not yet allocated, this is an on-demand allocation
    // for a legitimate driver access (code fetch or data access).
    if hal::walk_ptes_4k(aligned).is_none() ||
        !hal::walk_ptes_4k(aligned).unwrap().flags().contains(PageTableFlags::PRESENT)
    {
        // Check which section this address falls in
        let is_code = unsafe {
            ISOLATED_REGIONS.iter().any(|r| {
                r.in_use && r.driver_id == driver_id
                    && aligned >= r.base
                    && aligned < r.base + r.text_size
            })
        };

        let flags = if is_code { 0x0 } else { 0x2 }; // RX for code, RW for data
        match alloc_isolated_page(aligned, flags) {
            Some(phys) => {
                crate::serial_println!(
                    "[ISO] demand-alloc isolated 4K @ 0x{:x} → phys 0x{:x} (driver {}, {})",
                    aligned, phys, driver_id, if is_code { "code" } else { "data" }
                );
                true
            }
            None => {
                crate::serial_println!("[ISO] OOM allocating isolated page @ 0x{:x}", aligned);
                false
            }
        }
    } else {
        // Page is present but access was denied (e.g., write to RX page)
        crate::serial_println!(
            "[ISO] Access violation in isolated region @ 0x{:x} (driver {})",
            aligned, driver_id
        );

        // In sandbox mode, mark the driver as FAULTED
        let mode = isolation_mode_for_category(crate::nem::DriverCategory::Demand);
        if mode == IsolationMode::Sandbox {
            crate::serial_println!("[ISO]  → sandbox: marking driver {} FAULTED", driver_id);
            crate::drivers::driver_runtime::DRIVER_RUNTIME.lock()
                .set_error(driver_id, crate::drivers::driver_runtime::ERR_POLICY_VIOLATION, true);
        }
        false
    }
}

// ── Export table argument validation wrappers ──

/// Validate that a pointer passed from a driver is safe to dereference.
/// Wraps validate_driver_ptr with automatic driver_id detection.
pub fn validate_export_ptr(ptr: *const u8, size: usize, writable: bool) -> Result<(), &'static str> {
    let driver_id = crate::drivers::nem::driver::current_driver_id();
    if driver_id == 0 {
        // Kernel context — always allowed
        return Ok(());
    }
    validate_driver_ptr(ptr, size, driver_id, writable)
}

/// Validate that a string pointer passed from a driver is safe.
pub fn validate_export_str(ptr: *const u8) -> Result<usize, &'static str> {
    let driver_id = crate::drivers::nem::driver::current_driver_id();
    if driver_id == 0 {
        // Kernel context — scan for null terminator
        if ptr.is_null() {
            return Err("Null string pointer");
        }
        unsafe {
            for i in 0..256usize {
                if *ptr.add(i) == 0 {
                    return Ok(i);
                }
            }
        }
        return Err("String too long");
    }
    validate_driver_str_ptr(ptr, driver_id)
}

/// Validate that a driver pointer + size falls within the driver's allocated
/// data/bss region (for read/write access to driver-local data).
pub fn validate_driver_data_ptr(ptr: *const u8, size: usize) -> Result<(), &'static str> {
    let driver_id = crate::drivers::nem::driver::current_driver_id();
    if driver_id == 0 {
        return Ok(());
    }
    let addr = ptr as u64;
    let end = addr.wrapping_add(size as u64);
    if end < addr {
        return Err("Pointer wraps");
    }

    unsafe {
        for region in ISOLATED_REGIONS.iter() {
            if region.in_use && region.driver_id == driver_id {
                let data_start = region.base + region.text_size + region.rodata_size;
                let data_end = data_start + region.data_size + region.bss_size;
                if addr >= data_start && end <= data_end {
                    return Ok(());
                }
            }
        }
    }
    Err("Pointer outside driver data region")
}

// ── Debug / diagnostics ──

/// Format isolated region info for display (used by NDREG).
pub fn format_isolation_info(driver_id: u32) -> alloc::string::String {
    unsafe {
        for region in ISOLATED_REGIONS.iter() {
            if region.in_use && region.driver_id == driver_id {
                return alloc::format!(
                    "ISO @ 0x{:x} ({} KB): .text={}, .rodata={}, .data={}, .bss={}",
                    region.base, region.size / 1024,
                    region.text_size, region.rodata_size,
                    region.data_size, region.bss_size,
                );
            }
        }
    }
    alloc::string::String::from("Not isolated")
}

// ── Tests ──

pub fn register_isolation_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    use crate::test_ne;

    test_case!("iso_constants_sanity", {
        test_eq!(DRIVER_ISO_BASE, 0x3000_0000);
        test_eq!(DRIVER_ISO_SIZE, 0x100_0000);
        test_eq!(DRIVER_ISO_END, 0x3100_0000);
        test_eq!(MAX_ISOLATED_DRIVERS, 16);
        test_eq!(DRIVER_SLOT_SIZE, 0x10_0000);
        test_eq!(MAX_DRIVER_SIZE, 0x10_0000);
    });

    test_case!("iso_region_bounds", {
        test_true!(is_in_isolated_region(DRIVER_ISO_BASE));
        test_true!(is_in_isolated_region(DRIVER_ISO_BASE + 0xFFF));
        test_true!(is_in_isolated_region(DRIVER_ISO_END - 1));
        test_eq!(is_in_isolated_region(DRIVER_ISO_BASE - 1), false);
        test_eq!(is_in_isolated_region(DRIVER_ISO_END), false);
        test_eq!(is_in_isolated_region(0x2000_0000), false);
        test_eq!(is_in_isolated_region(0x4000_0000), false);
    });

    test_case!("iso_allocate_free_slot", {
        test_true!(allocate_driver_slot(2042, 0x80000).is_some());
        let base = driver_base(2042).unwrap();
        test_true!(base >= DRIVER_ISO_BASE && base < DRIVER_ISO_END);
        test_true!(is_in_driver_region(base, 2042));
        test_true!(is_in_driver_region(base + 0x7FFFF, 2042));
        test_eq!(is_in_driver_region(base + 0x80000, 2042), false);

        // Allocate another slot
        test_true!(allocate_driver_slot(2043, 0x40000).is_some());
        let base2 = driver_base(2043).unwrap();
        test_true!(base2 >= DRIVER_ISO_BASE && base2 < DRIVER_ISO_END);
        test_ne!(base, base2);
        test_true!(is_in_driver_region(base2, 2043));

        // Free first slot
        free_driver_slot(2042);
        test_eq!(is_in_driver_region(base, 2042), false);
        test_true!(is_in_driver_region(base2, 2043));

        // Clean up
        free_driver_slot(2043);
    });

    test_case!("iso_driver_id_for_address", {
        let slot = allocate_driver_slot(2010, 0x10000);
        test_ne!(slot, None);
        let base = slot.unwrap();
        let r1 = driver_id_for_address(base);
        test_eq!(r1, Some(2010));
        let r2 = driver_id_for_address(base + 0xFFFF);
        test_eq!(r2, Some(2010));
        let r3 = driver_id_for_address(base + DRIVER_SLOT_SIZE);
        // Should be the next driver or none
        test_true!(r3 != Some(2010)); // not our driver
        free_driver_slot(2010);
    });

    test_case!("iso_set_driver_layout", {
        test_true!(allocate_driver_slot(2005, 0x100000).is_some());
        test_true!(set_driver_layout(2005, 0x4000, 0x2000, 0x8000, 0x1000));

        unsafe {
            let region = ISOLATED_REGIONS.iter().find(|r| r.in_use && r.driver_id == 2005);
            test_ne!(region, None);
            let r = region.unwrap();
            test_eq!(r.text_size, 0x4000);
            test_eq!(r.rodata_size, 0x2000);
            test_eq!(r.data_size, 0x8000);
            test_eq!(r.bss_size, 0x1000);
        }

        free_driver_slot(2005);
    });

    test_case!("iso_validate_ptr_driver_data", {
        test_true!(allocate_driver_slot(2007, 0x100000).is_some());
        set_driver_layout(2007, 0x4000, 0x2000, 0x8000, 0x1000);

        let base = driver_base(2007).unwrap();
        let data_start = base + 0x4000 + 0x2000; // text + rodata

        // Valid data pointer
        let data_ptr = data_start as *const u8;
        test_true!(validate_driver_ptr(data_ptr, 0x100, 2007, true).is_ok());

        // Read-only section (text) — writable check fails
        let text_ptr = base as *const u8;
        test_true!(validate_driver_ptr(text_ptr, 0x100, 2007, false).is_ok());
        test_true!(validate_driver_ptr(text_ptr, 0x100, 2007, true).is_err());

        // Null pointer
        test_true!(validate_driver_ptr(core::ptr::null(), 0x100, 2007, false).is_err());

        // Outside region (0x4000_0000 is beyond the identity-mapped 4 GB range and not allowed)
        let outside_ptr = 0x4000_0000 as *const u8;
        test_true!(validate_driver_ptr(outside_ptr, 0x100, 2007, false).is_err());

        free_driver_slot(2007);
    });

    test_case!("iso_validate_ptr_overflow", {
        test_true!(allocate_driver_slot(2008, 0x100000).is_some());
        let base = driver_base(2008).unwrap();
        // Pointer + size wraps around
        test_true!(validate_driver_ptr(
            (base + 0x100) as *const u8,
            usize::MAX,
            2008,
            false,
        ).is_err());
        free_driver_slot(2008);
    });

    test_case!("iso_driver_max_slots", {
        // Count already-used slots (boot drivers may occupy some slots)
        let used_before = unsafe {
            ISOLATED_REGIONS.iter().filter(|r| r.in_use).count()
        };
        let available = MAX_ISOLATED_DRIVERS - used_before;
        let mut ids = alloc::vec::Vec::new();

        // Allocate all available slots
        for i in 0..available {
            let slot = allocate_driver_slot((i + 100) as u32, 0x10000);
            test_ne!(slot, None);
            ids.push((i + 100) as u32);
        }
        // No more slots available
        let full = allocate_driver_slot(200, 0x10000);
        test_eq!(full, None);

        // Free all allocated
        for &id in ids.iter() {
            free_driver_slot(id);
        }
        // Should be able to allocate again
        let slot = allocate_driver_slot(201, 0x10000);
        test_ne!(slot, None);
        free_driver_slot(201);
    });

    test_case!("iso_validate_str_ptr", {
        test_true!(allocate_driver_slot(2009, 0x100000).is_some());
        let base = driver_base(2009).unwrap();

        // Allocate a page in the data region and write a string
        let data_page = base + 0x6000;
        test_true!(alloc_isolated_page(data_page, 0x2).is_some()); // RW

        let s = b"Hello\0";
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), data_page as *mut u8, s.len());
        }
        let result = validate_driver_str_ptr(data_page as *const u8, 2009);
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 5);

        // Null pointer
        test_true!(validate_driver_str_ptr(core::ptr::null(), 2009).is_err());

        free_driver_slot(2009);
    });

    test_case!("iso_mode_for_category", {
        test_eq!(isolation_mode_for_category(crate::nem::DriverCategory::Boot), IsolationMode::Basic);
        test_eq!(isolation_mode_for_category(crate::nem::DriverCategory::System), IsolationMode::Basic);
        test_eq!(isolation_mode_for_category(crate::nem::DriverCategory::Demand), IsolationMode::Sandbox);
    });

    test_case!("iso_mode_str", {
        test_eq!(isolation_mode_str(IsolationMode::None), "NONE");
        test_eq!(isolation_mode_str(IsolationMode::Basic), "BASIC");
        test_eq!(isolation_mode_str(IsolationMode::Sandbox), "SANDBOX");
    });
}
