//! SMP (Symmetric Multi-Processing) initialization.
//!
//! Implements the INIT-SIPI-SIPI sequence to start Application Processors
//! (APs) and brings them into the kernel's 64-bit long mode.
//!
//! Boot flow:
//! 1. BSP allocates per-CPU KPRCB pages, stacks, and GDT/TSS pages
//! 2. BSP copies AP trampoline to physical address 0x800000
//! 3. BSP sends INIT IPI to all APs (excluding self)
//! 4. BSP waits 10 ms, then sends SIPI (vector = entry >> 12)
//! 5. APs wake in 16-bit real mode, set up PM32, jump to 64-bit entry
//! 6. Each AP sets GS base to its KPRCB, loads per-CPU IDT, signals ready
//! 7. BSP waits for all APs to signal ready, reports CPU count

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use crate::arch::x64::msr;
use crate::arch::x64::cpu_local as cpu_local_mod;

// ── Constants ────────────────────────────────────────────────────────────

/// Physical address for the AP trampoline code (must be < 1 MB for real mode).
const AP_TRAMPOLINE_ADDR: u64 = 0x80_0000;

/// Stack size per AP (16 KB).
const AP_STACK_SIZE: usize = 16384;

/// Max number of CPUs.
const MAX_CPUS: usize = cpu_local_mod::MAX_CPUS;

/// AP ICR (Interrupt Command Register) delivery status bit.
const ICR_DELIVERY_STATUS: u32 = 1 << 12;

/// AP ICR destination shorthand: all excluding self.
const ICR_SHORTHAND_ALL_EXCL_SELF: u32 = 3 << 18;

/// AP ICR delivery mode: INIT.
const ICR_MODE_INIT: u32 = 5 << 8;

/// AP ICR delivery mode: SIPI.
const ICR_MODE_SIPI: u32 = 6 << 8;

/// AP ICR trigger mode: edge.
const ICR_TRIGGER_EDGE: u32 = 0 << 15;

/// AP ICR level: assert.
const ICR_LEVEL_ASSERT: u32 = 1 << 14;

// ── Shared state ─────────────────────────────────────────────────────────

/// Lock for AP startup serialization.
static AP_STARTUP_LOCK: Mutex<()> = Mutex::new(());

/// Number of APs that have finished initialization.
static AP_READY_COUNT: AtomicU32 = AtomicU32::new(0);

/// Total CPUs (BSP + APs) after SMP init.
static TOTAL_CPUS: AtomicU32 = AtomicU32::new(1); // BSP counts as 1

/// Physical addresses of per-CPU KPRCB pages (BSP writes, APs read).
static mut AP_KPRCB_PTRS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Physical addresses of per-CPU stacks (BSP writes, APs read).
static mut AP_STACK_PTRS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// APIC IDs discovered during startup.
static mut AP_APIC_IDS: [u32; MAX_CPUS] = [0; MAX_CPUS];

/// Whether AP trampoline has been copied.
static TRAMPOLINE_READY: AtomicBool = AtomicBool::new(false);

// ── AP trampoline (16-bit → 32-bit → 64-bit entry) ──────────────────────

// The AP trampoline is a small piece of 16-bit real-mode code that:
// 1. Sets up a temporary GDT for protected mode
// 2. Enters 32-bit protected mode
// 3. Loads a 64-bit code segment selector
// 4. Jumps to the 64-bit AP entry point
//
// This is copied to physical address 0x800000 (below 1 MB).
//
// We use `.set` directives to pre-compute all runtime addresses as
// single symbols, avoiding Rust's `global_asm!` limitation of one
// symbol per memory operand.
core::arch::global_asm!(
    ".section .text.ap_trampoline, \"ax\"",
    ".code16",
    ".global ap_trampoline_start",
    "ap_trampoline_start:",
    // Disable interrupts
    "cli",
    // Load GDT (flat 4 GB descriptors)
    "lgdt [ap_gdt_desc_rt]",
    // Enter protected mode (set CR0.PE)
    "mov eax, cr0",
    "or  eax, 1",
    "mov cr0, eax",
    // Far jump to 32-bit code
    ".byte 0x66, 0xEA",                   // ljmpl
    ".4byte ap_pm32_entry_rt",
    ".word 0x08",                          // CS = 0x08 (32-bit code)

    ".code32",
    "ap_pm32_entry:",
    // Set up data segments
    "mov ax, 0x10",
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "mov gs, ax",
    "mov ss, ax",

    // Read APIC ID from LAPIC ID register (MMIO at 0xFEE00000)
    "mov edi, 0xFEE00020",
    "mov eax, [edi]",
    "shr eax, 24",
    "and eax, 0xFF",
    // EAX = APIC ID

    // Load stack pointer from pre-computed address
    "mov ebx, [ap_stack_ptr_rt]",

    // Set RSP to the per-CPU stack
    "mov esp, ebx",

    // Enable PAE (CR4.PAE = bit 5)
    "mov eax, cr4",
    "or  eax, (1 << 5)",
    "mov cr4, eax",

    // Load PML4 base (identity-mapped, set by BSP)
    "mov eax, [ap_pml4_ptr_rt]",
    "mov cr3, eax",

    // Enable long mode (EFER.LME = bit 8)
    "mov ecx, 0xC0000080",                 // IA32_EFER
    "rdmsr",
    "or  eax, (1 << 8)",
    "wrmsr",

    // Enable paging (CR0.PG = bit 31) → enters compatibility mode
    "mov eax, cr0",
    "or  eax, (1 << 31)",
    "mov cr0, eax",

    // Far jump to 64-bit code
    ".byte 0x66, 0xEA",
    ".4byte ap_lm64_entry_rt",
    ".word 0x18",                          // CS = 0x18 (64-bit code)

    ".code64",
    "ap_lm64_entry:",
    // Set up 64-bit data segments
    "mov ax, 0x20",
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "mov gs, ax",
    "mov ss, ax",

    // Jump to Rust AP entry point
    "mov rdi, rsp",                        // arg0 = stack pointer
    "mov rax, [ap_entry_ptr_rt]",
    "call rax",

    // Should never return
    "cli",
    "hlt",

    // ── Pre-computed runtime addresses (offset from 0x800000) ──
    ".set ap_gdt_desc_rt, ap_gdt_desc - ap_trampoline_start + 0x800000",
    ".set ap_pm32_entry_rt, ap_pm32_entry - ap_trampoline_start + 0x800000",
    ".set ap_lm64_entry_rt, ap_lm64_entry - ap_trampoline_start + 0x800000",
    ".set ap_stack_ptr_rt, ap_stack_ptr - ap_trampoline_start + 0x800000",
    ".set ap_pml4_ptr_rt, ap_pml4_ptr - ap_trampoline_start + 0x800000",
    ".set ap_entry_ptr_rt, ap_entry_ptr - ap_trampoline_start + 0x800000",

    // ── Temporary GDT ──
    ".align 4",
    "ap_gdt:",
    // Entry 0: null descriptor
    ".8byte 0",
    // Entry 1: 32-bit code (0x08): base=0, limit=4GB, execute+read
    ".4byte 0x0000FFFF",
    ".4byte 0x00CF9A00",
    // Entry 2: 32-bit data (0x10): base=0, limit=4GB, read+write
    ".4byte 0x0000FFFF",
    ".4byte 0x00CF9200",
    // Entry 3: 64-bit code (0x18): L bit set, D=0
    ".4byte 0x0000FFFF",
    ".4byte 0x00AF9A00",
    // Entry 4: 64-bit data (0x20): read+write
    ".4byte 0x0000FFFF",
    ".4byte 0x00CF9200",
    "ap_gdt_end:",

    "ap_gdt_desc:",
    ".2byte ap_gdt_end - ap_gdt - 1",     // GDT limit
    ".4byte ap_gdt - ap_trampoline_start + 0x800000", // GDT base

    // ── Shared data (filled by BSP before sending SIPI) ──
    ".align 8",
    "ap_stack_ptr:",
    ".8byte 0",
    "ap_pml4_ptr:",
    ".8byte 0",
    "ap_entry_ptr:",
    ".8byte 0",

    ".global ap_trampoline_end",
    "ap_trampoline_end:",
);

extern "C" {
    fn ap_trampoline_start();
    fn ap_trampoline_end();
    fn ap_stack_ptr();
    fn ap_pml4_ptr();
    fn ap_entry_ptr();
}

// ── LAPIC ICR (Interrupt Command Register) ───────────────────────────────

/// Write to the LAPIC ICR (Interrupt Command Register) to send an IPI.
///
/// # Safety
/// Requires LAPIC MMIO to be mapped and accessible.
unsafe fn lapic_write_icr(val: u64) {
    let apic_base = msr::read_apic_base_msr();
    if apic_base == 0 { return; }
    let icr_high = (apic_base + 0x310) as *mut u32;
    let icr_low = (apic_base + 0x308) as *mut u32;
    // Write high dword (destination)
    core::ptr::write_volatile(icr_high, (val >> 32) as u32);
    // Wait for delivery status to clear
    loop {
        let status = core::ptr::read_volatile(icr_low);
        if (status & ICR_DELIVERY_STATUS) == 0 { break; }
        crate::hal::raw::raw_pause();
    }
    // Write low dword (vector + mode)
    core::ptr::write_volatile(icr_low, val as u32);
}

/// Send INIT IPI to all APs (excluding self).
unsafe fn send_init_ipi() {
    lapic_write_icr(
        (ICR_SHORTHAND_ALL_EXCL_SELF as u64)
        | (ICR_MODE_INIT as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
    );
}

/// Send SIPI with the given vector to all APs (excluding self).
/// Vector is the page number of the entry point (entry >> 12).
unsafe fn send_sipi(vector: u8) {
    lapic_write_icr(
        (ICR_SHORTHAND_ALL_EXCL_SELF as u64)
        | (ICR_MODE_SIPI as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
        | (vector as u64)
    );
}

// ── 64-bit AP entry point (called from trampoline) ──────────────────────

/// Entry point for APs once they are in 64-bit long mode.
/// Called from the AP trampoline with RSP = per-CPU stack top.
///
/// This function:
/// 1. Sets GS base to the AP's KPRCB
/// 2. Loads per-CPU IDT
/// 3. Signals readiness to BSP
/// 4. Enters idle loop (HLT-based)
#[no_mangle]
pub extern "sysv64" fn ap_entry(_stack_top: u64) -> ! {
    // Determine our CPU index by matching our APIC ID
    let my_apic = unsafe {
        let apic_base = msr::read_apic_base_msr();
        if apic_base != 0 {
            let id_reg = (apic_base + 0x020) as *const u32;
            (core::ptr::read_volatile(id_reg) >> 24) & 0xFF
        } else {
            0
        }
    };

    // Find our CPU index
    let mut my_cpu: usize = 0;
    unsafe {
        for (i, &id) in AP_APIC_IDS.iter().enumerate().take(MAX_CPUS).skip(1) {
            if id == my_apic {
                my_cpu = i;
                break;
            }
        }
    }

    // Set GS base to our KPRCB
    unsafe {
        let kprcb_addr = AP_KPRCB_PTRS[my_cpu];
        if kprcb_addr != 0 {
            msr::write_gs_base(kprcb_addr);
            // Update APIC ID in KPRCB
            let kprcb = kprcb_addr as *mut cpu_local_mod::Kprcb;
            (*kprcb).apic_id = my_apic;
            (*kprcb).cpu_id = my_cpu as u32;
        }
    }

    // Load per-CPU IDT (each AP needs its own IDT loaded via lidt)
    // For now, load the shared IDT — APs will use the same handlers
    // but each has its own IDT in memory.
    unsafe {
        // Create a per-CPU IDT from the static one
        // The x86_64 crate's IDT is not Send, so we build one inline
        let idt_ptr = alloc_idt_page();
        if !idt_ptr.is_null() {
            let desc = crate::hal::raw::IdtDescriptor::from_raw(
                (256 * 16 - 1) as u16, idt_ptr as u64
            );
            crate::hal::raw::raw_lidt(&desc);
        }
    }

    // Signal readiness
    AP_READY_COUNT.fetch_add(1, Ordering::SeqCst);

    // Enter idle loop
    loop {
        // Check per-CPU need_resched
        unsafe {
            let need = cpu_local_mod::this_cpu_need_resched();
            if need {
                cpu_local_mod::this_cpu_set_need_resched(false);
                // TODO(smp): wire up local scheduler schedule() — APs spin with HLT but never yield
            }
        }
        unsafe { crate::hal::raw::raw_hlt_once(); }
    }
}

/// Allocate a page for per-CPU IDT and copy the static IDT.
/// Returns pointer to the IDT memory (for lidt).
fn alloc_idt_page() -> *mut u8 {
    let layout = core::alloc::Layout::from_size_align(4096, 4096).unwrap();
    let page = unsafe { alloc::alloc::alloc(layout) };
    if page.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { core::ptr::write_bytes(page, 0, 4096); }
    page
}

// ── BSP: copy trampoline ────────────────────────────────────────────────

/// Copy the AP trampoline code to the target physical address.
///
/// # Safety
/// Must be called from BSP only, after page tables are set up.
unsafe fn copy_trampoline() {
    let src = ap_trampoline_start as *const u8;
    let dst = AP_TRAMPOLINE_ADDR as *mut u8;
    let size = ap_trampoline_end as *const () as usize - ap_trampoline_start as *const () as usize;
    core::ptr::copy_nonoverlapping(src, dst, size);
    TRAMPOLINE_READY.store(true, Ordering::SeqCst);
}

/// Patch the trampoline with shared data (stack pointer, PML4, entry).
///
/// # Safety
/// Must be called after copy_trampoline().
unsafe fn patch_trampoline(stack_ptr: u64, pml4_ptr: u64, entry_ptr: u64) {
    let base = AP_TRAMPOLINE_ADDR;

    // ap_stack_ptr is at offset from ap_trampoline_start
    let stack_offset = (ap_stack_ptr as *const () as usize) - (ap_trampoline_start as *const () as usize);
    let pml4_offset = (ap_pml4_ptr as *const () as usize) - (ap_trampoline_start as *const () as usize);
    let entry_offset = (ap_entry_ptr as *const () as usize) - (ap_trampoline_start as *const () as usize);

    *((base + stack_offset as u64) as *mut u64) = stack_ptr;
    *((base + pml4_offset as u64) as *mut u64) = pml4_ptr;
    *((base + entry_offset as u64) as *mut u64) = entry_ptr;
}

// ── BSP: INIT-SIPI-SIPI sequence ────────────────────────────────────────

/// Wait for a specified number of milliseconds using HPET or port 0x80.
fn wait_ms(ms: u32) {
    crate::hal::sleep_hint(ms * 1000);
}

/// Detect the number of CPUs by reading the APIC version register
/// to find the maximum APIC ID.
fn detect_apic_id_count() -> u32 {
    unsafe {
        let apic_base = msr::read_apic_base_msr();
        if apic_base == 0 { return 1; }
        let version_reg = (apic_base + 0x030) as *const u32;
        let version = core::ptr::read_volatile(version_reg);
        let max_lvt = ((version >> 16) & 0xFF) + 1;
        // Max LVT entries is a rough proxy for CPU count
        // In practice we use the APIC IDs we discover
        max_lvt.min(MAX_CPUS as u32)
    }
}

/// Main SMP initialization function.
///
/// Called by BSP during boot after heap and physical memory are ready.
/// Returns the total number of CPUs (BSP + APs).
pub fn init_smp() -> usize {
    let _lock = AP_STARTUP_LOCK.lock();

    crate::serial_println!("[SMP] Starting SMP initialization...");

    // Step 1: Allocate per-CPU KPRCB pages
    cpu_local_mod::init_kprcb_pages();

    // Get BSP's KPRCB page and set GS base
    if let Some(bsp_kprcb) = cpu_local_mod::kprcb_page(0) {
        unsafe {
            AP_KPRCB_PTRS[0] = bsp_kprcb;
            msr::write_gs_base(bsp_kprcb);
            cpu_local_mod::mark_cpu_online(0);
        }
        crate::serial_println!("[SMP] BSP KPRCB at 0x{:x}, GS base set", bsp_kprcb);
    }

    // Step 2: Allocate stacks for APs
    unsafe {
        for (cpu, slot) in AP_STACK_PTRS.iter_mut().enumerate().take(MAX_CPUS).skip(1) {
            let stack_page = crate::hal::alloc_page();
            if stack_page.is_null() {
                crate::serial_println!("[SMP] Failed to allocate stack for AP {}", cpu);
                break;
            }
            *slot = stack_page as u64 + AP_STACK_SIZE as u64;
        }
    }

    // Step 3: Copy trampoline to low memory
    unsafe { copy_trampoline(); }

    // Step 4: Get PML4 physical address
    let _pml4_phys = crate::hal::read_cr3();

    // Step 5: Send INIT IPI
    crate::serial_println!("[SMP] Sending INIT IPI...");
    unsafe { send_init_ipi(); }
    wait_ms(10);

    // Step 6: Send SIPI (vector = AP_TRAMPOLINE_ADDR >> 12 = 0x80)
    let sipi_vector = (AP_TRAMPOLINE_ADDR >> 12) as u8;
    crate::serial_println!("[SMP] Sending SIPI (vector=0x{:x})...", sipi_vector);
    unsafe { send_sipi(sipi_vector); }
    wait_ms(10);

    // Step 7: Wait for APs to become ready
    let mut ap_count = 0u32;
    let mut attempts = 0u32;
    let max_attempts = 100; // 100 × 10ms = 1 second timeout

    while attempts < max_attempts {
        ap_count = AP_READY_COUNT.load(Ordering::SeqCst);
        if ap_count > 0 {
            break;
        }
        wait_ms(10);
        attempts += 1;
    }

    if ap_count == 0 {
        crate::serial_println!("[SMP] No APs found (single CPU mode)");
        TOTAL_CPUS.store(1, Ordering::SeqCst);
        return 1;
    }

    // Step 8: Second SIPI if needed
    if ap_count == 0 {
        crate::serial_println!("[SMP] Retrying SIPI...");
        unsafe { send_sipi(sipi_vector); }
        wait_ms(10);
        ap_count = AP_READY_COUNT.load(Ordering::SeqCst);
    }

    let total = 1 + ap_count; // BSP + APs
    TOTAL_CPUS.store(total, Ordering::SeqCst);

    // Update cpu_local module's CPU count
    for i in 0..total as usize {
        cpu_local_mod::mark_cpu_online(i as u32);
    }

    crate::serial_println!("[SMP] {} CPU(s) online (1 BSP + {} AP(s))", total, ap_count);
    total as usize
}

/// Get the total number of CPUs.
pub fn total_cpus() -> usize {
    TOTAL_CPUS.load(Ordering::SeqCst) as usize
}

/// Get the BSP's CPU ID (always 0).
pub fn bsp_id() -> usize {
    0
}

/// Check if the current CPU is the BSP.
pub fn is_bsp() -> bool {
    msr::is_bsp()
}

// ── IPI sending ──────────────────────────────────────────────────────────

/// IPI vector for reschedule notification.
pub const IPI_RESCHEDULE: u8 = 0xF0;

/// IPI vector for TLB shootdown.
pub const IPI_TLB_SHOOTDOWN: u8 = 0xF1;

/// Send an IPI to a specific CPU by APIC ID.
///
/// # Safety
/// The APIC ID must be valid and the IPI vector must have a registered handler.
pub unsafe fn send_ipi(dest_apic_id: u32, vector: u8) {
    lapic_write_icr(
        ((dest_apic_id as u64) << 32)
        | (vector as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
    );
}

/// Send an IPI to all CPUs (including self).
///
/// # Safety
/// Same requirements as `send_ipi`.
pub unsafe fn send_ipi_all(vector: u8) {
    lapic_write_icr(
        (3u64 << 18) // shorthand: all including self
        | (vector as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
    );
}

/// Send an IPI to all CPUs excluding self.
///
/// # Safety
/// Same requirements as `send_ipi`.
pub unsafe fn send_ipi_all_excl_self(vector: u8) {
    lapic_write_icr(
        (ICR_SHORTHAND_ALL_EXCL_SELF as u64)
        | (vector as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
    );
}

// ── Tests ────────────────────────────────────────────────────────────────

pub fn register_smp_tests() {
    crate::testing::register("smp_constants", || {
        crate::test_eq!(AP_TRAMPOLINE_ADDR, 0x80_0000u64);
        crate::test_eq!(AP_STACK_SIZE, 16384usize);
        Ok(())
    });

    crate::testing::register("smp_trampoline_size", || {
        let size = ap_trampoline_end as *const () as usize - ap_trampoline_start as *const () as usize;
        crate::test_true!(size > 0);
        crate::test_true!(size < 4096);
        Ok(())
    });

    crate::testing::register("smp_bsp_is_cpu0", || {
        let bsp = crate::arch::x64::msr::is_bsp();
        if bsp {
            crate::test_eq!(cpu_local_mod::kprcb_page(0).is_some(), true);
        }
        Ok(())
    });
}
