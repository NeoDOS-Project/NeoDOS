use core::arch::asm;

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
        // IPI vectors (0xF0-0xF2): always use APIC EOI
        if vector >= 0xF0 {
            if let Some(base) = apic_eoi_base() {
                let eoi_ptr = (base + 0x0B0) as *mut u32;
                core::ptr::write_volatile(eoi_ptr, 0);
            }
            return;
        }

        // Timer vector 32: use APIC EOI when APIC timer is active
        if vector == 32 && crate::timers::active() == crate::timers::TimerSource::ApicTimer {
            if let Some(base) = apic_eoi_base() {
                let eoi_ptr = (base + 0x0B0) as *mut u32;
                core::ptr::write_volatile(eoi_ptr, 0);
            }
            return;
        }

        // Legacy PIC EOI for device interrupts (vectors 32-47)
        if vector >= 32 && vector < 40 {
            asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8,
                options(nomem, nostack, preserves_flags));
        } else if vector >= 40 && vector < 48 {
            asm!("out dx, al", in("dx") 0xA0u16, in("al") 0x20u8,
                options(nomem, nostack, preserves_flags));
            asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8,
                options(nomem, nostack, preserves_flags));
        }
    }
}

/// Get the LAPIC MMIO base for EOI writes, or None if APIC not ready.
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
