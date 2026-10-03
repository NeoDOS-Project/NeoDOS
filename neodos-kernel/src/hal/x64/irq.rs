use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::hal::raw;

pub type IrqHandler = extern "C" fn();

/// Address of the LAPIC EOI register (0 = not published).
static LAPIC_EOI_ADDR: AtomicU64 = AtomicU64::new(0);

/// Whether the I/O APIC has taken over from the legacy PIC.
static IOAPIC_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Publish the LAPIC EOI register address. Called by the timer subsystem once
/// the LAPIC MMIO base is known.
pub fn set_lapic_eoi_addr(addr: u64) {
    LAPIC_EOI_ADDR.store(addr, Ordering::Release);
}

/// Record whether the I/O APIC replaced the legacy PIC. Called by the
/// interrupt-controller subsystem.
pub fn set_ioapic_active(active: bool) {
    IOAPIC_ACTIVE.store(active, Ordering::Release);
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn register_irq(_vector: u8, _handler: IrqHandler) -> i32 {
    -1
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn ack_irq(vector: u8) {
    unsafe {
        // Always send APIC EOI for all vectors when the Local APIC is mapped.
        let eoi = LAPIC_EOI_ADDR.load(Ordering::Acquire);
        if eoi != 0 {
            crate::hal::mmio::write32(eoi as usize, 0);
        }

        // If I/O APIC is active, the PIC is disabled — no PIO EOI needed.
        if IOAPIC_ACTIVE.load(Ordering::Acquire) {
            return;
        }

        // Legacy PIC EOI (only when IOAPIC is not active)
        if vector >= 0xF0 {
            return;
        }

        if (32..40).contains(&vector) {
            raw::raw_outb(0x20u16, 0x20u8);
        } else if (40..48).contains(&vector) {
            raw::raw_outb(0xA0u16, 0x20u8);
            raw::raw_outb(0x20u16, 0x20u8);
        }
    }
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_IRQ_REGISTER_IRQ: unsafe extern "C" fn(u8, IrqHandler) -> i32 = register_irq;
#[used]
static KEEP_IRQ_ACK_IRQ: unsafe extern "C" fn(u8) = ack_irq;
