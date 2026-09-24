use crate::eventbus::Event;
use core::sync::atomic::{AtomicU64, Ordering};

static KBD_SEQ: AtomicU64 = AtomicU64::new(1);
static KBD_DIRECT_CNT: AtomicU64 = AtomicU64::new(0);
static KBD_DISPATCH_CNT: AtomicU64 = AtomicU64::new(0);

fn kbd_process_internal(scancode: u8, source: &'static str, seq: u64) {
    let released = (scancode & 0x80) != 0;
    let is_make = !released;
    let code = scancode & 0x7F;
    let tid = crate::scheduler::current_tid();
    let pid = crate::scheduler::current_pid();
    // Heuristic for interrupt context: IRQL check or invariant flag if available
    let in_irq = crate::invariants::is_in_timer_irq();
    crate::serial_println!(
        "[KBD] seq={} source={} scancode=0x{:02x} make={} code=0x{:02x} tid={} pid={} in_irq={} direct_cnt={} dispatch_cnt={}",
        seq, source, scancode, is_make, code, tid, pid, in_irq,
        KBD_DIRECT_CNT.load(Ordering::Relaxed),
        KBD_DISPATCH_CNT.load(Ordering::Relaxed)
    );
    // Legacy trace for backwards compat
    crate::serial_println!("[KBD_EVENT] seq={} source={} scancode=0x{:x} make={} code=0x{:x}", seq, source, scancode, is_make, code);

    if let Some(mut kbd) = crate::kbd::KBD.try_lock() {
        kbd.process_scancode(code, is_make);
        crate::serial_println!("[KBD] EXIT seq={} source={} tid={} pid={}", seq, source, tid, pid);
    } else {
        crate::serial_println!("[KBD_EVENT] KBD lock busy! seq={} source={} Skipping", seq, source);
    }
}

/// Direct IRQ path — deprecated after fix (kept for forensics).
/// Previously called synchronously from keyboard_handler (IRQ 33) — now unused.
/// Increments DIRECT counter and uses shared SEQ to correlate with queued copy.
#[allow(dead_code)]
pub fn kbd_event_handler_direct(scancode: u8) -> u64 {
    let seq = KBD_SEQ.fetch_add(1, Ordering::Relaxed);
    KBD_DIRECT_CNT.fetch_add(1, Ordering::Relaxed);
    kbd_process_internal(scancode, "IRQ_DIRECT", seq);
    seq
}

/// Allocate next seq for IRQ without direct processing (single-path fix).
pub fn kbd_next_seq() -> u64 {
    KBD_SEQ.fetch_add(1, Ordering::Relaxed)
}

pub fn kbd_event_handler(event: &Event) {
    if event.event_type == crate::eventbus::EVENT_KEYBOARD_INPUT {
        // Dispatch path — via EVENT_BUS.dispatch_pending() (idle syscall/workqueue)
        // If event.data1 carries seq (set by keyboard_handler), use it for correlation
        let scancode = event.data0 as u8;
        let seq = if event.data1 != 0 { event.data1 } else { KBD_SEQ.fetch_add(1, Ordering::Relaxed) };
        KBD_DISPATCH_CNT.fetch_add(1, Ordering::Relaxed);
        kbd_process_internal(scancode, "DISPATCH", seq);
    }
}

pub fn register_kbd_event_handler() {
    let res = crate::eventbus::EVENT_BUS.register_handler(
        crate::eventbus::EVENT_KEYBOARD_INPUT,
        kbd_event_handler,
        "neokbd",
    );
    crate::serial_println!("[KBD_REG] result={:?} handler=kbd_event_handler", res);
}
