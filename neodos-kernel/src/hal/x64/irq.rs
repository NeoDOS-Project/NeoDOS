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

// ── Force ABI symbol retention ──
#[used]
static KEEP_IRQ_REGISTER_IRQ: unsafe extern "C" fn(u8, IrqHandler) -> i32 = register_irq;
#[used]
static KEEP_IRQ_ACK_IRQ: unsafe extern "C" fn(u8) = ack_irq;
