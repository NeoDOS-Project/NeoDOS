use crate::hal::raw;

pub type IrqHandler = extern "C" fn();

#[no_mangle]
#[inline(never)]
pub extern "C" fn register_irq(_vector: u8, _handler: IrqHandler) -> i32 {
    -1
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn ack_irq(vector: u8) {
    unsafe {
        // Always send APIC EOI for all vectors when Local APIC is present.
        if let Some(base) = apic_eoi_base() {
            let eoi_ptr = (base + 0x0B0) as *mut u32;
            core::ptr::write_volatile(eoi_ptr, 0);
        }

        // If I/O APIC is active, the PIC is disabled — no PIO EOI needed.
        if crate::interrupts::ioapic::is_active() {
            return;
        }

        // Legacy PIC EOI (only when IOAPIC is not active)
        if vector >= 0xF0 {
            return;
        }

        if vector >= 32 && vector < 40 {
            raw::raw_outb(0x20u16, 0x20u8);
        } else if vector >= 40 && vector < 48 {
            raw::raw_outb(0xA0u16, 0x20u8);
            raw::raw_outb(0x20u16, 0x20u8);
        }
    }
}

#[inline]
fn apic_eoi_base() -> Option<u64> {
    let base = crate::timers::apic::apic_base();
    if base != 0 { Some(base) } else { None }
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_IRQ_REGISTER_IRQ: unsafe extern "C" fn(u8, IrqHandler) -> i32 = register_irq;
#[used]
static KEEP_IRQ_ACK_IRQ: unsafe extern "C" fn(u8) = ack_irq;
