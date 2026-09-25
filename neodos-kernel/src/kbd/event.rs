use crate::eventbus::Event;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

static KBD_SEQ: AtomicU64 = AtomicU64::new(1);
static KBD_DIRECT_CNT: AtomicU64 = AtomicU64::new(0);
static KBD_DISPATCH_CNT: AtomicU64 = AtomicU64::new(0);

// ── Lock-free SPSC ring buffer for scancodes (IRQ producer → consumer) ──
// Replaces silent discard on KBD.try_lock() contention. IRQ pushes here if
// KBD lock is busy; next successful acquisition drains the queue (FIFO).
// Size 256 is ample for typing bursts (typematic ~30cps).
struct ScancodeQueue {
    buffer: [u8; 256],
    head: AtomicUsize,
    tail: AtomicUsize,
}
impl ScancodeQueue {
    const fn new() -> Self {
        Self { buffer: [0; 256], head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }
    fn push(&self, b: u8) -> Result<(), ()> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let next = (tail + 1) % 256;
        if next == head { return Err(()); }
        unsafe { (self.buffer.as_ptr() as *mut u8).add(tail).write(b); }
        self.tail.store(next, Ordering::Release);
        Ok(())
    }
    fn pop(&self) -> Option<u8> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail { return None; }
        let b = unsafe { self.buffer.as_ptr().add(head).read() };
        self.head.store((head + 1) % 256, Ordering::Release);
        Some(b)
    }
}
static PENDING_SCANCODES: ScancodeQueue = ScancodeQueue::new();

fn kbd_process_internal(scancode: u8, source: &'static str, seq: u64) {
    let released = (scancode & 0x80) != 0;
    let is_make = !released;
    let code = scancode & 0x7F;
    let tid = crate::scheduler::current_tid();
    let pid = crate::scheduler::current_pid();
    // Heuristic for interrupt context: IRQL check or invariant flag if available
    let in_irq = crate::invariants::is_in_timer_irq();
    // Phase 7: per-scancode logging must not flood serial in the hot path.
    // Gate behind LogSubsys::Kbd trace (default level is INFO → silent).
    if crate::log::log_enabled(crate::log::LogSubsys::Kbd, crate::log::LogLevel::Trace) {
        crate::serial_println!(
            "[KBD] seq={} source={} scancode=0x{:02x} make={} code=0x{:02x} tid={} pid={} in_irq={} direct_cnt={} dispatch_cnt={}",
            seq, source, scancode, is_make, code, tid, pid, in_irq,
            KBD_DIRECT_CNT.load(Ordering::Relaxed),
            KBD_DISPATCH_CNT.load(Ordering::Relaxed)
        );
        crate::serial_println!("[KBD_EVENT] seq={} source={} scancode=0x{:x} make={} code=0x{:x}", seq, source, scancode, is_make, code);
    }

    if let Some(mut kbd) = crate::kbd::KBD.try_lock() {
        // Drain any previously queued scancodes first (FIFO) before current
        while let Some(pending) = PENDING_SCANCODES.pop() {
            let p_released = (pending & 0x80) != 0;
            let p_is_make = !p_released;
            // Pass raw scancode so E0 prefix is handled inside process_scancode
            kbd.process_scancode(pending, p_is_make);
        }
        kbd.process_scancode(scancode, is_make);
        if crate::log::log_enabled(crate::log::LogSubsys::Kbd, crate::log::LogLevel::Trace) {
            crate::serial_println!("[KBD] EXIT seq={} source={} tid={} pid={}", seq, source, tid, pid);
        }
    } else {
        // Lock-free queue instead of silent discard (SPSC: IRQ → consumer)
        if PENDING_SCANCODES.push(scancode).is_err() {
            crate::serial_println!("[KBD_EVENT] PENDING overflow! seq={} source={} scancode=0x{:02x} dropped", seq, source, scancode);
        } else {
            crate::serial_println!("[KBD_EVENT] KBD lock busy, queued scancode=0x{:02x} seq={} source={}", scancode, seq, source);
            // Ensure scheduler re-evaluates soon so queued scancode is drained
            crate::syscall::set_need_resched();
        }
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
