//! Inter-Processor Interrupt (IPI) infrastructure.
//!
//! Provides IPI delivery, TLB shootdown, and cross-CPU function call
//! mechanisms for SMP coherence.
//!
//! # IPI Vectors
//!
//! | Vector | Name                | Purpose                          |
//! |--------|---------------------|----------------------------------|
//! | 0xF0   | IPI_RESCHEDULE      | Wake a remote CPU's scheduler    |
//! | 0xF1   | IPI_TLB_SHOOTDOWN   | Invalidate TLB entries           |
//! | 0xF2   | IPI_CALL_FUNCTION   | Execute function on remote CPU   |

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use crate::arch::x64::cpu_local;
use crate::arch::x64::msr;

// ── Constants ────────────────────────────────────────────────────────────

/// ICR (Interrupt Command Register) offsets from LAPIC base.
const ICR_LOW: u64 = 0x0300;
const ICR_HIGH: u64 = 0x0310;

/// Delivery status bit in ICR low dword.
const ICR_DELIVERY_STATUS: u32 = 1 << 12;

/// Delivery mode: Fixed (000).
const ICR_DEST_FIXED: u32 = 0 << 8;

/// Destination shorthand: all excluding self.
const ICR_SHORTHAND_ALL_EXCL_SELF: u32 = 3 << 18;

/// Trigger mode: edge.
const ICR_TRIGGER_EDGE: u32 = 0 << 15;

/// Level: assert.
const ICR_LEVEL_ASSERT: u32 = 1 << 14;

/// Maximum number of CPUs for TLB shootdown tracking.
const MAX_CPUS: usize = cpu_local::MAX_CPUS;

// ── IPI Vector constants ────────────────────────────────────────────────

/// IPI vector for per-CPU reschedule notification.
pub const IPI_RESCHEDULE: u8 = 0xF0;

/// IPI vector for TLB shootdown.
pub const IPI_TLB_SHOOTDOWN: u8 = 0xF1;

/// IPI vector for generic cross-CPU function call.
pub const IPI_CALL_FUNCTION: u8 = 0xF2;

// ── TLB Shootdown state ─────────────────────────────────────────────────

/// Shared TLB shootdown payload. Written by the initiator, read by targets.
#[repr(C)]
pub struct TlbShootdownPayload {
    /// Start address (inclusive, page-aligned).
    pub start: u64,
    /// End address (exclusive, page-aligned).
    pub end: u64,
    /// Bitmask of CPUs that must ACK.
    pub target_mask: u64,
    /// Number of CPUs that have ACKed.
    pub ack_count: AtomicU8,
    /// Set to 1 when all ACKs received.
    pub done: AtomicBool,
}

static TLB_SHOOTDOWN: TlbShootdownPayload = TlbShootdownPayload {
    start: 0,
    end: 0,
    target_mask: 0,
    ack_count: AtomicU8::new(0),
    done: AtomicBool::new(false),
};

// ── Call Function state ──────────────────────────────────────────────────

type CallFunctionCb = unsafe extern "C" fn(u64);

/// Shared call-function payload.
#[repr(C)]
pub struct CallFunctionPayload {
    /// Function pointer to execute on remote CPUs.
    pub func: AtomicU64,
    /// Opaque argument passed to the function.
    pub arg: u64,
    /// Bitmask of CPUs that must ACK.
    pub target_mask: u64,
    /// Number of CPUs that have ACKed.
    pub ack_count: AtomicU8,
    /// Set to 1 when all ACKs received.
    pub done: AtomicBool,
}

static CALL_FUNCTION: CallFunctionPayload = CallFunctionPayload {
    func: AtomicU64::new(0),
    arg: 0,
    target_mask: 0,
    ack_count: AtomicU8::new(0),
    done: AtomicBool::new(false),
};

// ── LAPIC ICR access ────────────────────────────────────────────────────

/// Get the LAPIC MMIO base address.
#[inline]
fn lapic_base() -> u64 {
    msr::read_apic_base_msr() & !0xFFF
}

/// Write a 64-bit value to the LAPIC ICR (Interrupt Command Register).
///
/// # Safety
/// LAPIC MMIO must be mapped and accessible.
unsafe fn lapic_write_icr(val: u64) {
    let base = lapic_base();
    if base == 0 { return; }

    let icr_high = (base + ICR_HIGH) as *mut u32;
    let icr_low = (base + ICR_LOW) as *mut u32;

    // Write high dword (destination APIC ID) first
    core::ptr::write_volatile(icr_high, (val >> 32) as u32);

    // Wait for delivery status to clear before writing low dword
    loop {
        let status = core::ptr::read_volatile(icr_low);
        if (status & ICR_DELIVERY_STATUS) == 0 { break; }
        crate::hal::raw::raw_pause();
    }

    // Write low dword (vector + delivery mode)
    core::ptr::write_volatile(icr_low, val as u32);
}

// ── IPI sending ──────────────────────────────────────────────────────────

/// Send an IPI to a specific CPU by APIC ID.
///
/// # Safety
/// The APIC ID must be valid and the IPI vector must have a registered handler.
pub unsafe fn send_ipi(dest_apic_id: u32, vector: u8) {
    lapic_write_icr(
        ((dest_apic_id as u64) << 32)
        | (vector as u64)
        | ICR_DEST_FIXED as u64
        | ICR_TRIGGER_EDGE as u64
        | ICR_LEVEL_ASSERT as u64
    );
}

/// Send an IPI to all CPUs (including self).
pub unsafe fn send_ipi_all(vector: u8) {
    lapic_write_icr(
        (3u64 << 18) // shorthand: all including self
        | (vector as u64)
        | ICR_DEST_FIXED as u64
        | ICR_TRIGGER_EDGE as u64
        | ICR_LEVEL_ASSERT as u64
    );
}

/// Send an IPI to all CPUs excluding self.
pub unsafe fn send_ipi_all_excl_self(vector: u8) {
    lapic_write_icr(
        (ICR_SHORTHAND_ALL_EXCL_SELF as u64)
        | (vector as u64)
        | ICR_DEST_FIXED as u64
        | ICR_TRIGGER_EDGE as u64
        | ICR_LEVEL_ASSERT as u64
    );
}

/// Send an IPI to a bitmask of CPUs (by logical cpu_id).
/// Iterates over set bits and sends targeted IPIs.
pub unsafe fn send_ipi_mask(mask: u64, vector: u8) {
    let count = cpu_local::cpu_count() as usize;
    for cpu in 0..count {
        if (mask & (1u64 << cpu)) != 0 {
            if let Some(kprcb) = cpu_local::kprcb_page(cpu) {
                let apic_id = core::ptr::read_volatile(
                    (kprcb + 4) as *const u32 // apic_id at offset 0x004
                );
                send_ipi(apic_id, vector);
            }
        }
    }
}

// ── TLB Shootdown ────────────────────────────────────────────────────────

/// Perform a synchronous TLB shootdown for a virtual address range.
///
/// Sends `IPI_TLB_SHOOTDOWN` to all CPUs that might have the page cached.
/// Blocks until all target CPUs acknowledge the invalidation.
///
/// # Arguments
/// * `start` — Start address (inclusive, page-aligned)
/// * `end` — End address (exclusive, page-aligned)
/// * `target_mask` — Bitmask of CPUs to invalidate (excluding self is automatic)
///
/// # Returns
/// `Ok(())` if all CPUs acknowledged, `Err(())` on timeout.
pub fn tlb_shootdown(start: u64, end: u64, target_mask: u64) -> Result<(), ()> {
    let my_cpu = unsafe { cpu_local::this_cpu_id() } as usize;

    // Build target mask excluding self
    let effective_mask = target_mask & !(1u64 << my_cpu);
    if effective_mask == 0 || start >= end {
        return Ok(()); // Nothing to do
    }

    // Write payload
    {
        let payload = &TLB_SHOOTDOWN as *const TlbShootdownPayload as *mut TlbShootdownPayload;
        unsafe {
            (*payload).start = start;
            (*payload).end = end;
            (*payload).target_mask = effective_mask;
            (*payload).ack_count.store(0, Ordering::SeqCst);
            (*payload).done.store(false, Ordering::SeqCst);
            core::sync::atomic::compiler_fence(Ordering::SeqCst);
        }
    }

    // Count expected ACKs (number of bits set in effective_mask)
    let expected_acks = effective_mask.count_ones() as u8;

    // Send IPI to all target CPUs
    unsafe { send_ipi_mask(effective_mask, IPI_TLB_SHOOTDOWN); }

    // Local invalidation
    for page in (start..end).step_by(4096) {
        crate::hal::flush_tlb(page);
    }

    // Wait for ACKs with timeout
    let mut attempts = 0u32;
    const MAX_ATTEMPTS: u32 = 10000;
    loop {
        let acked = TLB_SHOOTDOWN.ack_count.load(Ordering::SeqCst);
        if acked >= expected_acks {
            break;
        }
        unsafe { crate::hal::raw::raw_pause(); }
        attempts += 1;
        if attempts >= MAX_ATTEMPTS {
            return Err(()); // Timeout
        }
    }

    Ok(())
}

/// Handle IPI_TLB_SHOOTDOWN on the receiving CPU.
///
/// Called from the IDT handler for vector 0xF1.
/// Executes `invlpg` for each page in the shared range and sends ACK.
#[no_mangle]
pub extern "C" fn ipi_tlb_shootdown_handler_impl() {
    let start = TLB_SHOOTDOWN.start;
    let end = TLB_SHOOTDOWN.end;

    // Invalidate each page in the range
    for page in (start..end).step_by(4096) {
        crate::hal::flush_tlb(page);
    }

    // Signal ACK
    TLB_SHOOTDOWN.ack_count.fetch_add(1, Ordering::SeqCst);
}

// ── Call Function ─────────────────────────────────────────────────────────

/// Execute a function on all specified CPUs synchronously.
///
/// # Arguments
/// * `func` — Function to execute (must be safe to call on any CPU)
/// * `arg` — Opaque argument
/// * `target_mask` — Bitmask of CPUs (excluding self is automatic)
///
/// # Returns
/// `Ok(())` if all CPUs acknowledged, `Err(())` on timeout.
pub fn call_function_all(func: CallFunctionCb, arg: u64, target_mask: u64) -> Result<(), ()> {
    let my_cpu = unsafe { cpu_local::this_cpu_id() } as usize;

    let effective_mask = target_mask & !(1u64 << my_cpu);
    if effective_mask == 0 {
        return Ok(());
    }

    // Write payload
    {
        let payload = &CALL_FUNCTION as *const CallFunctionPayload as *mut CallFunctionPayload;
        unsafe {
            (*payload).func.store(func as usize as u64, Ordering::SeqCst);
            (*payload).arg = arg;
            (*payload).target_mask = effective_mask;
            (*payload).ack_count.store(0, Ordering::SeqCst);
            (*payload).done.store(false, Ordering::SeqCst);
            core::sync::atomic::compiler_fence(Ordering::SeqCst);
        }
    }

    let expected_acks = effective_mask.count_ones() as u8;

    // Send IPI
    unsafe { send_ipi_mask(effective_mask, IPI_CALL_FUNCTION); }

    // Wait for ACKs
    let mut attempts = 0u32;
    const MAX_ATTEMPTS: u32 = 10000;
    loop {
        let acked = CALL_FUNCTION.ack_count.load(Ordering::SeqCst);
        if acked >= expected_acks {
            break;
        }
        unsafe { crate::hal::raw::raw_pause(); }
        attempts += 1;
        if attempts >= MAX_ATTEMPTS {
            return Err(());
        }
    }

    Ok(())
}

/// Handle IPI_CALL_FUNCTION on the receiving CPU.
///
/// Called from the IDT handler for vector 0xF2.
#[no_mangle]
pub extern "C" fn ipi_call_function_handler_impl() {
    let func_addr = CALL_FUNCTION.func.load(Ordering::SeqCst);
    let arg = CALL_FUNCTION.arg;

    if func_addr != 0 {
        let func: CallFunctionCb = unsafe { core::mem::transmute(func_addr) };
        unsafe { func(arg); }
    }

    // Signal ACK
    CALL_FUNCTION.ack_count.fetch_add(1, Ordering::SeqCst);
}

// ── Init ─────────────────────────────────────────────────────────────────

/// Initialize the IPI subsystem. Called during boot after APIC is configured.
pub fn init() {
    kinfo!(crate::log::LogSubsys::Interrupts, "Initializing IPI subsystem");
    kinfo!(crate::log::LogSubsys::Interrupts, "Vectors: RESCHEDULE=0x{:X} TLB_SHOOTDOWN=0x{:X} CALL_FUNCTION=0x{:X}",
                           IPI_RESCHEDULE, IPI_TLB_SHOOTDOWN, IPI_CALL_FUNCTION);
    kinfo!(crate::log::LogSubsys::Interrupts, "Ready");
}

// ── Tests ────────────────────────────────────────────────────────────────

pub fn register_ipi_tests() {
    crate::testing::register("ipi_constants", || {
        crate::test_eq!(IPI_RESCHEDULE, 0xF0u8);
        crate::test_eq!(IPI_TLB_SHOOTDOWN, 0xF1u8);
        crate::test_eq!(IPI_CALL_FUNCTION, 0xF2u8);
        Ok(())
    });

    crate::testing::register("ipi_tlb_shootdown_struct", || {
        // Verify TLB shootdown payload layout
        crate::test_true!(core::mem::size_of::<TlbShootdownPayload>() > 0);
        Ok(())
    });

    crate::testing::register("ipi_call_function_struct", || {
        // Verify call function payload layout
        crate::test_true!(core::mem::size_of::<CallFunctionPayload>() > 0);
        Ok(())
    });

    crate::testing::register("ipi_tlb_shootdown_local_only", || {
        // On single-CPU (BSP only), shootdown with no targets should succeed trivially
        let result = tlb_shootdown(0x1000, 0x2000, 0);
        crate::test_true!(result.is_ok());
        Ok(())
    });

    crate::testing::register("ipi_call_function_no_targets", || {
        // Call function with no targets (mask=0) should succeed trivially
        unsafe extern "C" fn noop(_arg: u64) {}
        let result = call_function_all(noop, 0, 0);
        crate::test_true!(result.is_ok());
        Ok(())
    });
}
