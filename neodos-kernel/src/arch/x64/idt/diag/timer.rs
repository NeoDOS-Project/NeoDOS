//! Timer IRQ diagnostic ring.

// ── Lock-free timer diagnostic ring buffer ──
// Written from timer IRQ context (single producer), dumped to serial
// after boot tests pass (single consumer). Avoids serial_println!
// deadlock risk (spin::Mutex in IRQ context).
const TD_RING_SIZE: usize = 256;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct TimerDiagEntry {
    pub(crate) tick: u64,
    pub(crate) cur_tid: u32,
    pub(crate) cur_rsp: u64,
    pub(crate) cur_cs: u64,
    pub(crate) flags: u8,       // bit0=is_user, bit1=should_preempt, bit2=has_non_idle
    pub(crate) path: u8,        // 0=ring3, 1=idle, 2=kernel, 3=no_preempt
    pub(crate) next_tid: u32,
    pub(crate) next_rsp: u64,
    pub(crate) next_ks_top: u64,
    pub(crate) returned_rsp: u64,
    pub(crate) frame_rip: u64,
    pub(crate) frame_cs: u64,
    pub(crate) frame_rflags: u64,
    pub(crate) phase: u8,       // 0=entry, 1=after_sched, 2=pre_return, 3=pre_iretq
}

static mut TD_RING: [TimerDiagEntry; TD_RING_SIZE] = [TimerDiagEntry {
    tick: 0, cur_tid: 0, cur_rsp: 0, cur_cs: 0, flags: 0, path: 0,
    next_tid: 0, next_rsp: 0, next_ks_top: 0, returned_rsp: 0,
    frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 0,
}; TD_RING_SIZE];
static TD_HEAD: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[inline(always)]
pub(crate) fn td_push(entry: TimerDiagEntry) {
    let idx = unsafe {
        TD_HEAD.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % TD_RING_SIZE
    };
    unsafe {
        *TD_RING.get_unchecked_mut(idx) = entry;
    }
}

/// Called by `timer_handler_asm` after it has restored all GPRs and immediately
/// before `iretq`.  `frame_rsp` is therefore the frame the CPU will consume,
/// not a value inferred from a KTHREAD.  This must remain lock-free and
/// allocation-free: it runs while handling the timer IRQ.
#[no_mangle]
pub extern "C" fn timer_trace_iretq_frame(frame_rsp: u64) {
    // #476: no frame audit here — this runs on every tick, including on the
    // 4 KiB idle stack, and the extra call/frame risks overflowing it. The
    // timer dispatch frames are already audited by `timer_preempt` /
    // `timer_k355_idle` at the scheduler return sites.
    let (rip, cs, rflags) = unsafe {
        let frame = frame_rsp as *const u64;
        (frame.read(), frame.add(1).read(), frame.add(2).read())
    };
    td_push(TimerDiagEntry {
        tick: crate::hal::get_ticks(), cur_tid: 0, cur_rsp: frame_rsp,
        cur_cs: cs, flags: 0, path: 3, next_tid: 0, next_rsp: 0,
        next_ks_top: 0, returned_rsp: frame_rsp,
        frame_rip: rip, frame_cs: cs, frame_rflags: rflags, phase: 3,
    });
}

pub fn timer_diag_dump() {
    // Solo en modo trazas (LOG_TIMERS/SCHED/INTERRUPTS=TRACE)
    if !crate::log::log_enabled(crate::log::LogSubsys::Timers, crate::log::LogLevel::Trace)
        && !crate::log::log_enabled(crate::log::LogSubsys::Sched, crate::log::LogLevel::Trace)
        && !crate::log::log_enabled(crate::log::LogSubsys::Interrupts, crate::log::LogLevel::Trace)
    {
        return;
    }
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[TIMER_DIAG] DUMP ENTERED");
    use core::sync::atomic::Ordering;
    let head = TD_HEAD.load(Ordering::Acquire);
    let count = head.min(TD_RING_SIZE);
    if count == 0 {
        crate::println!("[TIMER_DIAG] No entries recorded (head={})", head);
        unsafe { core::arch::asm!("sti"); }
        return;
    }
    crate::println!("[TIMER_DIAG] === {} entries (head={}) ===", count, head);
    let start = if head < TD_RING_SIZE { 0 } else { head % TD_RING_SIZE };
    for i in 0..count {
        let idx = (start + i) % TD_RING_SIZE;
        let e = unsafe { *TD_RING.get_unchecked(idx) };
        if e.tick == 0 && e.cur_rsp == 0 && e.returned_rsp == 0 { continue; }
        let path_str = match e.path { 0 => "R3", 1 => "IDLE", 2 => "KERN", _ => "NOPRE" };
        let phase_str = match e.phase { 0 => "ENTRY", 1 => "SCHED", 2 => "RET", _ => "IRETQ" };
        crate::println!(
            "[TD][{}] {} {} tick={} tid={} cs=0x{:x} rsp=0x{:x} f={:02x} nxt_tid={} nxt_rsp=0x{:x} ks=0x{:x} ret=0x{:x} frame=[rip=0x{:x} cs=0x{:x} rflags=0x{:x}]",
            i, path_str, phase_str, e.tick, e.cur_tid, e.cur_cs, e.cur_rsp, e.flags,
            e.next_tid, e.next_rsp, e.next_ks_top, e.returned_rsp,
            e.frame_rip, e.frame_cs, e.frame_rflags);
    }
    unsafe { core::arch::asm!("sti"); }
}

pub fn timer_diag_dump_secondary() {
    if !crate::log::log_enabled(crate::log::LogSubsys::Timers, crate::log::LogLevel::Trace)
        && !crate::log::log_enabled(crate::log::LogSubsys::Sched, crate::log::LogLevel::Trace)
        && !crate::log::log_enabled(crate::log::LogSubsys::Interrupts, crate::log::LogLevel::Trace)
    {
        return;
    }
    unsafe { core::arch::asm!("cli"); }
    crate::println!("[TIMER_DIAG_SECONDARY] DUMP ENTERED");
    use core::sync::atomic::Ordering;
    let head = TD_HEAD.load(Ordering::Acquire);
    let count = head.min(TD_RING_SIZE);
    if count == 0 {
        crate::println!("[TIMER_DIAG_SECONDARY] No entries (head={})", head);
        unsafe { core::arch::asm!("sti"); }
        return;
    }
    crate::println!("[TIMER_DIAG_SECONDARY] === {} entries (head={}) ===", count, head);
    let start = if head < TD_RING_SIZE { 0 } else { head % TD_RING_SIZE };
    let mut netd = 0;
    for i in 0..count {
        let idx = (start + i) % TD_RING_SIZE;
        let e = unsafe { *TD_RING.get_unchecked(idx) };
        if e.tick == 0 && e.cur_rsp == 0 && e.returned_rsp == 0 { continue; }
        if e.next_tid == 2 || e.phase == 3 { netd += 1; }
        let path_str = match e.path { 0 => "R3", 1 => "IDLE", 2 => "KERN", _ => "NOPRE" };
        let phase_str = match e.phase { 0 => "ENTRY", 1 => "SCHED", 2 => "RET", _ => "IRETQ" };
        crate::println!(
            "[TD2][{}] {} {} tick={} tid={} cs=0x{:x} rsp=0x{:x} f={:02x} nxt_tid={} nxt_rsp=0x{:x} ks=0x{:x} ret=0x{:x} frame=[rip=0x{:x} cs=0x{:x} rflags=0x{:x}]",
            i, path_str, phase_str, e.tick, e.cur_tid, e.cur_cs, e.cur_rsp, e.flags,
            e.next_tid, e.next_rsp, e.next_ks_top, e.returned_rsp,
            e.frame_rip, e.frame_cs, e.frame_rflags);
    }
    crate::println!("[TD2] netd/iretq relevant: {}", netd);
    unsafe { core::arch::asm!("sti"); }
}
