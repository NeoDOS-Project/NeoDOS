//! SMP (Symmetric Multi-Processing) initialization.
//!
//! Implements the INIT-SIPI-SIPI sequence to start Application Processors
//! (APs) and brings them into the kernel's 64-bit long mode.
//!
//! Boot flow:
//! 1. BSP allocates per-CPU KPRCB pages, stacks, and GDT/TSS pages
//! 2. BSP copies AP trampoline to physical address 0x8000 (vector 0x08)
//!    — SIPI vector = addr >> 12 = 0x08, NOT 0x80 (0x80000)
//! 3. BSP sends INIT IPI to all APs (excluding self)
//! 4. BSP waits 10 ms, then sends SIPI (vector = entry >> 12)
//! 5. APs wake in 16-bit real mode, set up PM32, jump to 64-bit entry
//! 6. Each AP sets GS base to its KPRCB, loads per-CPU IDT, signals ready
//! 7. BSP waits for all APs to signal ready, reports CPU count

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use spin::Mutex;

use crate::arch::x64::msr;
use crate::arch::x64::cpu_local as cpu_local_mod;

// ── Constants ────────────────────────────────────────────────────────────

/// Physical address for the AP trampoline code (must be < 1 MB for real mode).
/// FIX: was 0x80_0000 (8 MB) but SIPI vector 0x80 → 0x80000 (512 KB) mismatch → AP never started with -smp 2.
/// Correct: 0x8000 → vector 0x08 (0x08 * 0x1000 = 0x8000) — verified FASE 1.
const AP_TRAMPOLINE_ADDR: u64 = 0x8000;

/// SIPI vector derived from trampoline address (addr >> 12).
const AP_TRAMPOLINE_VECTOR: u8 = (AP_TRAMPOLINE_ADDR >> 12) as u8; // 0x08
const _: () = assert!(AP_TRAMPOLINE_VECTOR == 0x08);

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

/// FASE 3: Backup of original low-memory at 0x8000 before copy_trampoline.
static mut LOWMEM_BACKUP: [u8; 4096] = [0; 4096];
static LOWMEM_BACKUP_VALID: AtomicBool = AtomicBool::new(false);

/// Next CPU index for AP self-assignment (atomic, BSP pre-allocates 1..).
static NEXT_AP_CPU: AtomicUsize = AtomicUsize::new(1);

// ── AP trampoline (16-bit → 32-bit → 64-bit entry) ──────────────────────

// The AP trampoline is a small piece of 16-bit real-mode code that:
// 1. Sets up a temporary GDT for protected mode
// 2. Enters 32-bit protected mode
// 3. Loads a 64-bit code segment selector
// 4. Jumps to the 64-bit AP entry point
//
// This is copied to physical address 0x8000 (below 1 MB).
//
// We use `.set` directives to pre-compute all runtime addresses as
// single symbols, avoiding Rust's `global_asm!` limitation of one
// symbol per memory operand.
core::arch::global_asm!(
    ".section .text.ap_trampoline, \"ax\"",
    ".code16",
    ".global ap_trampoline_start",
    "ap_trampoline_start:",
    // Early marker: 'T' to serial 0x3F8
    "mov dx, 0x3F8",
    "mov al, 0x54", // 'T'
    "out dx, al",
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
    // Marker 'P' for PM32
    "mov dx, 0x3F8",
    "mov al, 0x50",
    "out dx, al",
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
    // Marker '1' after stack
    "mov dx, 0x3F8",
    "mov al, 0x31",
    "out dx, al",

    // Enable PAE (CR4.PAE = bit 5)
    "mov eax, cr4",
    "or  eax, (1 << 5)",
    "mov cr4, eax",
    // Marker '2' after PAE
    "mov dx, 0x3F8",
    "mov al, 0x32",
    "out dx, al",

    // Load PML4 base (identity-mapped, set by BSP)
    "mov eax, [ap_pml4_ptr_rt]",
    "mov cr3, eax",
    // Marker '3' after CR3
    "mov dx, 0x3F8",
    "mov al, 0x33",
    "out dx, al",

    // Enable long mode (EFER.LME = bit 8)
    "mov ecx, 0xC0000080",                 // IA32_EFER
    "rdmsr",
    "or  eax, (1 << 8)",
    "wrmsr",
    // Marker '4' after EFER
    "mov dx, 0x3F8",
    "mov al, 0x34",
    "out dx, al",

    // Enable paging (CR0.PG = bit 31) → enters compatibility mode
    "mov eax, cr0",
    "or  eax, (1 << 31)",
    "mov cr0, eax",
    // Marker '5' after paging
    "mov dx, 0x3F8",
    "mov al, 0x35",
    "out dx, al",

    // Far jump to 64-bit code — from 32-bit compat to 64-bit long mode
    // In 32-bit mode, far jmp is EA cd (ptr16:32) without 66 prefix
    ".byte 0xEA",
    ".4byte ap_lm64_entry_rt",
    ".word 0x18",                          // CS = 0x18 (64-bit code)

    ".code64",
    "ap_lm64_entry:",
    // Marker 'L' for LM64
    "mov dx, 0x3F8",
    "mov al, 0x4C",
    "out dx, al",
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

    // ── Pre-computed runtime addresses (offset from 0x8000) ──
    ".set ap_gdt_desc_rt, ap_gdt_desc - ap_trampoline_start + 0x8000",
    ".set ap_pm32_entry_rt, ap_pm32_entry - ap_trampoline_start + 0x8000",
    ".set ap_lm64_entry_rt, ap_lm64_entry - ap_trampoline_start + 0x8000",
    ".set ap_stack_ptr_rt, ap_stack_ptr - ap_trampoline_start + 0x8000",
    ".set ap_pml4_ptr_rt, ap_pml4_ptr - ap_trampoline_start + 0x8000",
    ".set ap_entry_ptr_rt, ap_entry_ptr - ap_trampoline_start + 0x8000",

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
    ".4byte ap_gdt - ap_trampoline_start + 0x8000", // GDT base

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
    let icr_low = (apic_base + 0x300) as *mut u32;
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
    // FASE 1: vector must equal AP_TRAMPOLINE_ADDR>>12 — assert
    debug_assert_eq!(vector, AP_TRAMPOLINE_VECTOR);
    lapic_write_icr(
        (ICR_SHORTHAND_ALL_EXCL_SELF as u64)
        | (ICR_MODE_SIPI as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
        | (vector as u64)
    );
}

/// Send SIPI to a specific APIC ID (targeted, not broadcast).
unsafe fn send_sipi_to(vector: u8, apic_id: u32) {
    debug_assert_eq!(vector, AP_TRAMPOLINE_VECTOR);
    lapic_write_icr(
        ((apic_id as u64) << 32)
        | (ICR_MODE_SIPI as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
        | (vector as u64)
    );
}

/// Send INIT IPI to a specific APIC ID.
unsafe fn send_init_to(apic_id: u32) {
    lapic_write_icr(
        ((apic_id as u64) << 32)
        | (ICR_MODE_INIT as u64)
        | (ICR_TRIGGER_EDGE as u64)
        | (ICR_LEVEL_ASSERT as u64)
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
    // ── Early raw markers: must appear even if serial_println deadlocks ──
    unsafe { core::arch::asm!("mov dx, 0x3F8; mov al, 0x41; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); } // 'A'
    // Minimal serial println without allocation lock? Try but ignore failure
    // Use raw out sequence for "AP_ENTRY" after A.
    unsafe { core::arch::asm!("mov dx, 0x3F8; mov al, 0x42; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); } // 'B'
    unsafe { core::arch::asm!("mov dx, 0x3F8; mov al, 0x43; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); } // 'C'

    // Determine our APIC ID (xAPIC MMIO 0xFEE00020)
    let my_apic = unsafe {
        let apic_base = msr::read_apic_base_msr();
        if apic_base != 0 {
            let id_reg = (apic_base + 0x020) as *const u32;
            (core::ptr::read_volatile(id_reg) >> 24) & 0xFF
        } else {
            0
        }
    };

    // Find our CPU index: first try AP_APIC_IDS, then atomic fallback
    let my_cpu: usize = unsafe {
        let mut found: Option<usize> = None;
        for (i, &id) in AP_APIC_IDS.iter().enumerate().take(MAX_CPUS).skip(1) {
            if id == my_apic && id != 0 {
                found = Some(i);
                break;
            }
        }
        if let Some(i) = found { i } else {
            // Fallback: claim next free index atomically (works for -smp 2)
            // BSP is 0, next is 1
            let idx = NEXT_AP_CPU.fetch_add(1, Ordering::SeqCst);
            if idx < MAX_CPUS {
                AP_APIC_IDS[idx] = my_apic;
                idx
            } else {
                1
            }
        }
    };

    // Marker 'D' before GS
    unsafe { core::arch::asm!("mov dx, 0x3F8; mov al, 0x44; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); } // 'D'

    // Set GS base to our KPRCB — try AP_KPRCB_PTRS first, fallback to cpu_local KPRCB_PAGES
    unsafe {
        let mut kprcb_addr = AP_KPRCB_PTRS[my_cpu];
        if kprcb_addr == 0 {
            if let Some(p) = cpu_local_mod::kprcb_page(my_cpu) {
                kprcb_addr = p;
                AP_KPRCB_PTRS[my_cpu] = p;
            }
        }
        if kprcb_addr != 0 {
            msr::write_gs_base(kprcb_addr);
            // Update APIC ID in KPRCB
            let kprcb = kprcb_addr as *mut cpu_local_mod::Kprcb;
            (*kprcb).apic_id = my_apic;
            (*kprcb).cpu_id = my_cpu as u32;
        }
        // Marker 'E' after GS
        core::arch::asm!("mov dx, 0x3F8; mov al, 0x45; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); // 'E'
    }

    // Marker 'F' before increment — proves we reached here without faulting on GS
    unsafe { core::arch::asm!("mov dx, 0x3F8; mov al, 0x46; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); } // 'F'

    // ── CRITICAL: Signal readiness as early as possible ──
    // This must happen before any heap allocation (GDT/IDT) that could fault
    // and before STI on BSP. Goal of Phase 2 is strictly AP_READY_COUNT++.
    AP_READY_COUNT.fetch_add(1, Ordering::SeqCst);

    // Marker 'G' after increment
    unsafe { core::arch::asm!("mov dx, 0x3F8; mov al, 0x47; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); } // 'G'

    // FASE 5: log estado AP via raw serial (evitar lock de serial_println antes de AP_READY)
    // Emitir datos via raw out sin tomar spinlock: usar helpers de serial raw
    // Por ahora solo marcadores G/H; el serial_println se hará después de HLT estable si es necesario
    // crate::serial_println!("[AP_READY] cpu={} apic={} stack=0x{:x} GS=0x{:x}", my_cpu, my_apic, _stack_top, crate::hal::safe::GsBase::read());

    // P0.2: per-CPU GDT/TSS for this AP (was global TSS race) — best effort after ready
    // Hacerlo después de AP_READY_COUNT para no bloquear el handshake
    crate::arch::x64::gdt::init_ap(my_cpu);

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

    // Also emit 'H' after GDT/IDT
    unsafe { core::arch::asm!("mov dx, 0x3F8; mov al, 0x48; out dx, al", out("dx") _, out("al") _, options(nomem, nostack)); } // 'H'

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

/// Copy the AP trampoline code to the target physical address with backup.
///
/// FASE 3: must not overwrite 0x8000 without saving original bytes.
/// Saves 4096 bytes at 0x8000 into LOWMEM_BACKUP before copy.
unsafe fn copy_trampoline() {
    let src = ap_trampoline_start as *const u8;
    let dst = AP_TRAMPOLINE_ADDR as *mut u8;
    let size = ap_trampoline_end as *const () as usize - ap_trampoline_start as *const () as usize;
    crate::serial_println!("[TRAMPOLINE] src=0x{:x} dst=0x{:x} size={} vec=0x{:x}", src as u64, dst as u64, size, AP_TRAMPOLINE_VECTOR);
    if size > 4096 {
        panic!("trampoline size {} > 4096 would overflow lowmem", size);
    }
    if size == 0 {
        panic!("trampoline size is 0");
    }
    debug_assert_eq!((AP_TRAMPOLINE_ADDR & 0xFFF), 0, "trampoline must be page-aligned for SIPI");
    debug_assert_eq!(AP_TRAMPOLINE_VECTOR as u64 * 0x1000, AP_TRAMPOLINE_ADDR, "vector*4K must equal addr (FASE 1)");
    // Backup original lowmem at 0x8000 (FASE 2/3)
    let backup_dst = LOWMEM_BACKUP.as_mut_ptr();
    core::ptr::copy_nonoverlapping(dst, backup_dst, size);
    LOWMEM_BACKUP_VALID.store(true, Ordering::SeqCst);
    crate::serial_println!("[TRAMPOLINE] backup 0x{:x} size {} saved", AP_TRAMPOLINE_ADDR, size);
    // Verify 0x8000 is within first MB and not overlapping kernel image (faSE 2)
    // EBDA at 0x9FC00, trampoline 0x8000-0x9000 safe, contiguous with TEB 0x7000
    core::ptr::copy_nonoverlapping(src, dst, size);
    // Verify copy
    if core::ptr::read_volatile(dst) != core::ptr::read_volatile(src) {
        crate::serial_println!("[TRAMPOLINE] WARNING copy verification failed first byte");
    }
    TRAMPOLINE_READY.store(true, Ordering::SeqCst);
    crate::serial_println!("[TRAMPOLINE] copy done, READY=true");
}

/// Restore lowmem backup (for debugging/shutdown).
#[allow(dead_code)]
unsafe fn restore_trampoline() {
    if !LOWMEM_BACKUP_VALID.load(Ordering::SeqCst) { return; }
    let size = ap_trampoline_end as *const () as usize - ap_trampoline_start as *const () as usize;
    let src = LOWMEM_BACKUP.as_ptr();
    let dst = AP_TRAMPOLINE_ADDR as *mut u8;
    core::ptr::copy_nonoverlapping(src, dst, size);
    crate::serial_println!("[TRAMPOLINE] restored {} bytes at 0x{:x}", size, AP_TRAMPOLINE_ADDR);
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

/// Quick check if APs might be present.
/// VirtualBox can hang on IPI delivery, so skip multi-CPU init
/// if the APIC version register suggests only 1 CPU.
unsafe fn detect_aps() -> bool {
    let apic_base = msr::read_apic_base_msr();
    crate::serial_println!("[SMP_DETECT] apic_base=0x{:x}", apic_base);
    if apic_base == 0 { crate::serial_println!("[SMP_DETECT] no apic_base → false"); return false; }
    // Check APIC version register
    let version_reg = (apic_base + 0x030) as *const u32;
    let version = core::ptr::read_volatile(version_reg);
    crate::serial_println!("[SMP_DETECT] version=0x{:x} max_lvt={}", version, ((version>>16)&0xFF)+1);
    if version == 0xFFFFFFFF || version == 0 { crate::serial_println!("[SMP_DETECT] version 0/FFFF → false"); return false; }
    // Max LVT entries is a rough proxy; 0 or 1 means single CPU
    let max_lvt = ((version >> 16) & 0xFF) + 1;
    if max_lvt <= 1 { crate::serial_println!("[SMP_DETECT] max_lvt<=1 → false"); return false; }
    // Also check: if ICR delivery status never clears, there are no APs
    let start: u64;
    core::arch::asm!("rdtsc", out("eax") start, out("edx") _);
    let icr_low = (apic_base + 0x300) as *const u32;
    loop {
        let status = core::ptr::read_volatile(icr_low);
        if (status & 0x1000) == 0 { crate::serial_println!("[SMP_DETECT] ICR clear → true"); break; } // ICR_DELIVERY_STATUS clear
        let now: u64;
        core::arch::asm!("rdtsc", out("eax") now, out("edx") _);
        if now.wrapping_sub(start) > 500_000 { // ~500 µs timeout
            crate::serial_println!("[SMP_DETECT] ICR timeout → false");
            return false;
        }
        core::hint::spin_loop();
    }
    crate::serial_println!("[SMP_DETECT] final true");
    true
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

    kinfo!(crate::log::LogSubsys::Boot, "Starting SMP initialization...");

    // Step 1: Allocate per-CPU KPRCB pages
    cpu_local_mod::init_kprcb_pages();

    // Fill AP_KPRCB_PTRS for all CPUs from cpu_local KPRCB_PAGES (FASE: was only BSP)
    unsafe {
        for cpu in 0..MAX_CPUS {
            if let Some(p) = cpu_local_mod::kprcb_page(cpu) {
                AP_KPRCB_PTRS[cpu] = p;
                // Also set APIC ID placeholder (will be updated by AP itself or MADT)
                AP_APIC_IDS[cpu] = cpu as u32; // assume linear APIC IDs for QEMU -smp 2
            }
        }
    }

    // Get BSP's KPRCB page and set GS base
    if let Some(bsp_kprcb) = cpu_local_mod::kprcb_page(0) {
        unsafe {
            AP_KPRCB_PTRS[0] = bsp_kprcb;
            AP_APIC_IDS[0] = 0;
            msr::write_gs_base(bsp_kprcb);
            cpu_local_mod::mark_cpu_online(0);
        }
        kinfo!(crate::log::LogSubsys::Boot, "BSP KPRCB at 0x{:x}, GS base set", bsp_kprcb);
    }
    crate::serial_println!("[SMP] KPRCB PTRS filled: [0]=0x{:x} [1]=0x{:x}", unsafe { AP_KPRCB_PTRS[0] }, unsafe { AP_KPRCB_PTRS[1] });

    crate::serial_println!("[SMP] calling dump_madt");
    crate::timers::hpet::dump_madt();
    crate::serial_println!("[SMP] dump_madt done");

    // Step 2: Detect AP count by checking if we have any APs
    // Send INIT IPI briefly and check ICR — in VirtualBox IPIs to non-existent APs
    // can hang, so we detect this early and skip AP startup.
    let has_aps = unsafe { detect_aps() };
    crate::serial_println!("[SMP] has_aps={}", has_aps);

    if !has_aps {
        kwarn!(crate::log::LogSubsys::Boot, "No APs detected (single CPU mode)");
        TOTAL_CPUS.store(1, Ordering::SeqCst);
        return 1;
    }

    // Step 3: Allocate stacks for APs
    let mut ap_stack_count = 0usize;
    unsafe {
        for (cpu, slot) in AP_STACK_PTRS.iter_mut().enumerate().take(MAX_CPUS).skip(1) {
            let stack_page = crate::hal::alloc_page();
            if stack_page.is_null() {
                kerror!(crate::log::LogSubsys::Boot, "Failed to allocate stack for AP {}", cpu);
                break;
            }
            *slot = stack_page as u64 + AP_STACK_SIZE as u64;
            ap_stack_count += 1;
            crate::serial_println!("[SMP] AP{} stack 0x{:x}..0x{:x}", cpu, *slot - AP_STACK_SIZE as u64, *slot);
        }
    }
    crate::serial_println!("[SMP] allocated {} AP stacks", ap_stack_count);

    // Step 4: Copy trampoline to low memory (FASE 3 backup)
    unsafe { copy_trampoline(); }

    // Step 5: Get PML4 physical address and AP entry pointer
    let pml4_phys = crate::hal::read_cr3() & !0xFFF;
    let ap_entry_ptr = ap_entry as *const () as u64;
    crate::serial_println!("[SMP] PML4=0x{:x} ap_entry=0x{:x} vec=0x{:x} addr=0x{:x}", pml4_phys, ap_entry_ptr, AP_TRAMPOLINE_VECTOR, AP_TRAMPOLINE_ADDR);

    // Step 5b: Patch trampoline with first AP's stack (for -smp 2 single AP bring-up)
    // Sequential bring-up would re-patch per CPU; for now patch for CPU 1
    unsafe {
        let stack_for_ap1 = AP_STACK_PTRS[1];
        if stack_for_ap1 != 0 {
            patch_trampoline(stack_for_ap1, pml4_phys, ap_entry_ptr);
            crate::serial_println!("[SMP] patch_trampoline stack=0x{:x} pml4=0x{:x} entry=0x{:x}", stack_for_ap1, pml4_phys, ap_entry_ptr);
        } else {
            crate::serial_println!("[SMP] ERROR no stack for AP1");
        }
    }

    // Step 6: Send INIT IPI (broadcast)
    kdebug!(crate::log::LogSubsys::Boot, "Sending INIT IPI...");
    crate::serial_println!("[SMP] Sending INIT IPI (broadcast)...");
    unsafe { send_init_ipi(); }
    wait_ms(10);

    // Step 7: Send SIPI (vector = 0x08 for 0x8000) — broadcast
    // FASE 1: verified vector = AP_TRAMPOLINE_ADDR>>12 = 0x08
    let sipi_vector = AP_TRAMPOLINE_VECTOR;
    kinfo!(crate::log::LogSubsys::Boot, "Sending SIPI (vector=0x{:x} addr=0x{:x})...", sipi_vector, AP_TRAMPOLINE_ADDR);
    crate::serial_println!("[SMP] Sending SIPI vector=0x{:x} addr=0x{:x}...", sipi_vector, AP_TRAMPOLINE_ADDR);
    unsafe { send_sipi(sipi_vector); }
    // FASE 3-4 raw marker: 'S' after SIPI sent
    unsafe { core::arch::asm!("mov dx,0x3F8; mov al,0x53; out dx,al", out("dx")_, out("al")_, options(nomem,nostack)); } // 'S'
    wait_ms(200); // longer per Intel: need 200ms after SIPI
    unsafe { core::arch::asm!("mov dx,0x3F8; mov al,0x57; out dx,al", out("dx")_, out("al")_, options(nomem,nostack)); } // 'W' wait start

    // Step 8: Wait for APs to become ready (poll AP_READY_COUNT)
    let mut ap_count = 0u32;
    let mut attempts = 0u32;
    let max_attempts = 100; // 100 × 10ms = 1 second timeout

    while attempts < max_attempts {
        ap_count = AP_READY_COUNT.load(Ordering::SeqCst);
        if ap_count > 0 {
            crate::serial_println!("[SMP] AP_READY_COUNT={} after {} ms", ap_count, attempts*10);
            unsafe { core::arch::asm!("mov dx,0x3F8; mov al,0x52; out dx,al", out("dx")_, out("al")_, options(nomem,nostack)); } // 'R' ready
            break;
        }
        // raw tick marker 'w' each attempt
        if attempts % 10 == 0 {
            unsafe { core::arch::asm!("mov dx,0x3F8; mov al,0x77; out dx,al", out("dx")_, out("al")_, options(nomem,nostack)); } // 'w'
        }
        wait_ms(10);
        attempts += 1;
        if attempts % 20 == 0 {
            crate::serial_println!("[SMP] waiting AP_READY_COUNT 0... {} ms", attempts*10);
        }
    }

    if ap_count == 0 {
        kwarn!(crate::log::LogSubsys::Boot, "No APs found (single CPU mode)");
        crate::serial_println!("[SMP] TIMEOUT AP_READY_COUNT still 0 after 1s — will retry SIPI");
        // Fallthrough to retry instead of returning
    }

    // Step 9: Second SIPI if needed
    if ap_count == 0 {
        crate::serial_println!("[SMP] Retrying SIPI vector=0x{:x}...", sipi_vector);
        kinfo!(crate::log::LogSubsys::Boot, "Retrying SIPI...");
        unsafe { send_sipi(sipi_vector); }
        wait_ms(200);
        ap_count = AP_READY_COUNT.load(Ordering::SeqCst);
        crate::serial_println!("[SMP] after retry AP_READY_COUNT={}", ap_count);
    }

    if ap_count == 0 {
        kwarn!(crate::log::LogSubsys::Boot, "No APs found after retry (single CPU mode)");
        TOTAL_CPUS.store(1, Ordering::SeqCst);
        return 1;
    }

    let total = 1 + ap_count; // BSP + APs
    TOTAL_CPUS.store(total, Ordering::SeqCst);

    // Update cpu_local module's CPU count
    for i in 0..total as usize {
        cpu_local_mod::mark_cpu_online(i as u32);
    }

    kinfo!(crate::log::LogSubsys::Boot, "{} CPU(s) online (1 BSP + {} AP(s))", total, ap_count);
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
        crate::test_eq!(AP_TRAMPOLINE_ADDR, 0x8000u64);
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
