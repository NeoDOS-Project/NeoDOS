// src/interrupts/ioapic.rs
//! I/O APIC interrupt controller support.
//!
//! Detects the I/O APIC from the ACPI MADT table, configures it as the primary
//! interrupt controller, and provides an API for routing ISA and PCI interrupts.
//!
//! When the I/O APIC is present and successfully initialized, the legacy 8259A
//! PIC is disabled (all IRQ lines masked, no EOI sent to PIC).
//!
//! ╔═══════════════════════════════════════════════════════════════════╗
//! ║  ABI FROZEN at v0.42                                            ║
//! ║  Public API functions (init, is_active, mask/unmask_irq,        ║
//! ║  route_pci_vector, eoi_irq) MUST NOT change signature.          ║
//! ║  Internal register constants MUST NOT be reassigned.            ║
//! ╚═══════════════════════════════════════════════════════════════════╝

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ── I/O APIC MMIO register offsets ─────────────────────────────────
const IOAPIC_IOREGSEL: u64 = 0x00;
const IOAPIC_IOWIN: u64    = 0x10;

// ── I/O APIC register selectors ────────────────────────────────────
const IOAPIC_ID: u32        = 0x00;
const IOAPIC_VER: u32       = 0x01;
const IOAPIC_ARB: u32       = 0x02;
fn ioapic_redir_entry(pin: u8) -> u32 { 0x10 + (pin as u32) * 2 }
fn ioapic_redir_entry_hi(pin: u8) -> u32 { 0x10 + (pin as u32) * 2 + 1 }

// ── Redirection entry flags ────────────────────────────────────────
const IOAPIC_IRQ_MASKED: u64     = 1 << 16;
const IOAPIC_IRQ_TRIGGER_LEVEL: u64 = 1 << 15;
const IOAPIC_IRQ_POLARITY_LOW: u64 = 1 << 13;
const IOAPIC_IRQ_PHYSICAL: u64 = 0 << 11;
const IOAPIC_IRQ_LOGICAL: u64  = 1 << 11;
const IOAPIC_IRQ_DELIVERY_FIXED: u64 = 0;

// ── State ──────────────────────────────────────────────────────────

/// I/O APIC MMIO base address (0 if not found/initialised).
static IOAPIC_ADDR: AtomicU64 = AtomicU64::new(0);

/// Whether the I/O APIC is active and the PIC has been disabled.
static IOAPIC_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Number of redirection entries (max_pin + 1). Determined from IOAPICVER.
static IOAPIC_MAX_PIN: AtomicU64 = AtomicU64::new(0);

// ── Internal helpers ───────────────────────────────────────────────

#[inline]
fn ioapic_read_reg(reg: u32) -> u32 {
    let base = IOAPIC_ADDR.load(Ordering::Relaxed);
    if base == 0 { return 0; }
    unsafe {
        write_volatile((base + IOAPIC_IOREGSEL) as *mut u32, reg);
        read_volatile((base + IOAPIC_IOWIN) as *const u32)
    }
}

#[inline]
fn ioapic_write_reg(reg: u32, value: u32) {
    let base = IOAPIC_ADDR.load(Ordering::Relaxed);
    if base == 0 { return; }
    unsafe {
        write_volatile((base + IOAPIC_IOREGSEL) as *mut u32, reg);
        write_volatile((base + IOAPIC_IOWIN) as *mut u32, value);
    }
}

/// Read a full 64-bit redirection table entry for the given pin.
fn ioapic_read_redir(pin: u8) -> u64 {
    let low  = ioapic_read_reg(ioapic_redir_entry(pin));
    let high = ioapic_read_reg(ioapic_redir_entry_hi(pin));
    (low as u64) | ((high as u64) << 32)
}

/// Write a 64-bit redirection table entry for the given pin.
fn ioapic_write_redir(pin: u8, value: u64) {
    let low  = value as u32;
    let high = (value >> 32) as u32;
    ioapic_write_reg(ioapic_redir_entry(pin), low);
    ioapic_write_reg(ioapic_redir_entry_hi(pin), high);
}

// ── Public API ─────────────────────────────────────────────────────

/// Returns `true` if the I/O APIC was found and initialised.
pub fn is_active() -> bool {
    IOAPIC_ACTIVE.load(Ordering::Relaxed)
}

/// Return the I/O APIC MMIO base address (0 if not initialised).
pub fn ioapic_addr() -> u64 {
    IOAPIC_ADDR.load(Ordering::Relaxed)
}

/// Return the number of I/O APIC redirection entries (max pin + 1).
pub fn ioapic_pin_count() -> u8 {
    (IOAPIC_MAX_PIN.load(Ordering::Relaxed) + 1) as u8
}

/// Initialise the I/O APIC from the MADT table.
///
/// 1. Find the I/O APIC base address from the MADT.
/// 2. Verify the IOAPICID/IOAPICVER registers.
/// 3. Mask all redirection table entries.
/// 4. Disable the legacy PIC (mask all IRQs, stop sending EOI).
/// 5. Route ISA IRQs to vectors 32-47.
///
/// Returns `true` on success.
pub fn init() -> bool {
    let (addr, gsi_base) = match crate::timers::hpet::find_ioapic() {
        Some(a) => a,
        None => {
            crate::serial_println!("[IOAPIC] Not found in MADT");
            return false;
        }
    };

    let addr64 = addr as u64;
    crate::serial_println!(
        "[IOAPIC] Found at MMIO 0x{:x}, GSI base {}",
        addr64, gsi_base
    );

    IOAPIC_ADDR.store(addr64, Ordering::SeqCst);

    // Read version and max redirection entry (bits 23:16 of version register)
    let ver = ioapic_read_reg(IOAPIC_VER);
    let max_redir = (ver >> 16) as u8;
    IOAPIC_MAX_PIN.store(max_redir as u64, Ordering::SeqCst);
    crate::serial_println!(
        "[IOAPIC] Version 0x{:x}, {} redirection entries (pins 0-{})",
        ver & 0xFF, max_redir + 1, max_redir
    );

    // Mask all redirection entries
    for pin in 0..=max_redir {
        let entry = ioapic_read_redir(pin);
        let masked = entry | IOAPIC_IRQ_MASKED;
        ioapic_write_redir(pin, masked);
    }

    // Disable legacy PIC: mask all IRQs
    disable_legacy_pic();

    // Route only ISA IRQs that have known kernel handlers (timer, keyboard,
    // serial). All other IRQs stay masked to prevent spurious interrupts
    // from reaching unconfigured IDT vectors.
    let overrides = crate::timers::hpet::get_isa_overrides();
    for irq in 0..16u8 {
        let has_handler = irq == 0 || irq == 1 || irq == 4 || irq == 12;
        let gsi = resolve_gsi(irq, &overrides);
        let pin = (gsi - gsi_base) as u8;
        if pin > max_redir { continue; }

        if has_handler {
            let vector = 32 + irq;
            let entry: u64 = vector as u64
                | IOAPIC_IRQ_DELIVERY_FIXED
                | IOAPIC_IRQ_PHYSICAL;

            let iso_flags = override_flags(irq, &overrides);
            let entry = if iso_flags & 0x2 != 0 {
                entry | IOAPIC_IRQ_TRIGGER_LEVEL
            } else {
                entry
            };
            let entry = if iso_flags & 0x4 != 0 {
                entry | IOAPIC_IRQ_POLARITY_LOW
            } else {
                entry
            };

            ioapic_write_redir(pin, entry);
            crate::serial_println!(
                "[IOAPIC] IRQ{} → pin{} vector{} (flags={:#x})",
                irq, pin, vector, iso_flags
            );
        } else {
            // Ensure pin stays masked
            let entry = ioapic_read_redir(pin);
            if entry & IOAPIC_IRQ_MASKED == 0 {
                ioapic_write_redir(pin, entry | IOAPIC_IRQ_MASKED);
            }
            crate::serial_println!(
                "[IOAPIC] IRQ{} → GSI{} pin{} (masked, no handler)",
                irq, gsi, pin
            );
        }
    }

    IOAPIC_ACTIVE.store(true, Ordering::SeqCst);
    crate::serial_println!("[IOAPIC] Initialised, PIC disabled");
    true
}

/// Mask a specific IOAPIC pin (disable interrupt).
pub fn mask_irq(irq: u8) {
    let max_pin = IOAPIC_MAX_PIN.load(Ordering::Relaxed) as u8;
    if irq <= max_pin {
        let entry = ioapic_read_redir(irq);
        ioapic_write_redir(irq, entry | IOAPIC_IRQ_MASKED);
    }
}

/// Unmask a specific IOAPIC pin (enable interrupt).
pub fn unmask_irq(irq: u8) {
    let max_pin = IOAPIC_MAX_PIN.load(Ordering::Relaxed) as u8;
    if irq <= max_pin {
        let entry = ioapic_read_redir(irq);
        ioapic_write_redir(irq, entry & !IOAPIC_IRQ_MASKED);
    }
}

/// Route a PCIe MSI/MSI-X vector directly through the I/O APIC.
/// This is used when a device does not support MSI/MSI-X.
/// Returns the allocated vector number.
pub fn route_pci_vector(vector: u8, pin: u8, apic_id: u8) {
    let max_pin = IOAPIC_MAX_PIN.load(Ordering::Relaxed) as u8;
    if pin > max_pin { return; }
    let entry: u64 = vector as u64
        | IOAPIC_IRQ_DELIVERY_FIXED
        | IOAPIC_IRQ_PHYSICAL
        | IOAPIC_IRQ_TRIGGER_LEVEL  // PCI interrupts are level-triggered
        | ((apic_id as u64) << 56);
    ioapic_write_redir(pin, entry);
}

/// Send an EOI to the I/O APIC.
/// For edge-triggered interrupts, this is a no-op (the Local APIC EOI
/// handled by ack_irq is sufficient). For level-triggered, the I/O APIC
/// needs an EOI after the device deasserts INTx.
/// Currently this is a no-op because we use APIC EOI for everything.
pub fn eoi_irq(_vector: u8) {
    // The Local APIC handles EOI for edge-triggered interrupts.
    // For level-triggered, the device driver must ensure the interrupt
    // condition is cleared before EOI. The I/O APIC's IRR is cleared
    // when the device deasserts the INTx line.
}

// ── Private helpers ────────────────────────────────────────────────

/// Resolve the GSI (Global System Interrupt) for an ISA IRQ.
/// Checks MADT ISA interrupt source overrides first.
fn resolve_gsi(irq: u8, overrides: &[(u8, u32, u16)]) -> u32 {
    for &(source, gsi, _flags) in overrides {
        if source == irq {
            return gsi;
        }
    }
    irq as u32
}

/// Return the override flags (polarity, trigger mode) for an ISA IRQ.
fn override_flags(irq: u8, overrides: &[(u8, u32, u16)]) -> u16 {
    for &(source, _gsi, flags) in overrides {
        if source == irq {
            return flags;
        }
    }
    0 // default: active-high, edge-triggered
}

/// Disable the legacy 8259A PIC by masking all IRQs.
/// This prevents the PIC from asserting INTR after I/O APIC takes over.
fn disable_legacy_pic() {
    // Mask all IRQs on master and slave PICs.
    // OCW1: write mask to data ports.
    crate::hal::outb(0x21, 0xFF);  // Master PIC: mask all
    crate::hal::outb(0xA1, 0xFF);  // Slave PIC: mask all
    crate::serial_println!("[IOAPIC] Legacy PIC masked (all IRQs disabled)");
}

// ── Tests ──────────────────────────────────────────────────────────

/// Register IOAPIC tests. Called from the kernel test harness.
pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    test_case!("ioapic_has_valid_pin_count", {
        // After init, IOAPIC should have at least 24 pins (or 0 if not present)
        let count = ioapic_pin_count();
        if is_active() {
            test_true!(count >= 24);
        } else {
            test_eq!(count, 0);
        }
    });

    test_case!("ioapic_resolve_gsi_no_override", {
        let overrides = alloc::vec![];
        test_eq!(resolve_gsi(4, &overrides), 4);
        test_eq!(resolve_gsi(0, &overrides), 0);
        test_eq!(resolve_gsi(15, &overrides), 15);
    });

    test_case!("ioapic_resolve_gsi_with_override", {
        let overrides = alloc::vec![(0u8, 2u32, 0u16), (4u8, 4u32, 0x0Du16)];
        test_eq!(resolve_gsi(0, &overrides), 2);
        test_eq!(resolve_gsi(4, &overrides), 4);
        test_eq!(resolve_gsi(1, &overrides), 1);
    });

    test_case!("ioapic_mask_unmask_safe", {
        mask_irq(0);
        unmask_irq(0);
    });

    test_case!("ioapic_pic_disabled_when_ioapic_active", {
        if is_active() {
            let master_mask = crate::hal::inb(0x21);
            let slave_mask = crate::hal::inb(0xA1);
            test_eq!(master_mask, 0xFF);
            test_eq!(slave_mask, 0xFF);
        }
    });
}
