use core::fmt;
use core::sync::atomic::{AtomicU16, Ordering};

// ── Trace ring buffer ───────────────────────────────────────────────
// Lock-free, no heap, fixed-size.  Producer = IRQ / syscall / scheduler.
// Consumer = panic handler (interrupts off, single-reader).

pub const TRACE_CAPACITY: usize = 1024;
pub const TRACE_EVENT_SIZE: usize = core::mem::size_of::<TraceEntry>();
pub const TRACE_DUMP_COUNT: usize = 32; // entries to dump on panic

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceEvent {
    ContextSwitch = 0x01,
    SyscallEnter  = 0x02,
    SyscallExit   = 0x03,
    IrqEnter      = 0x04,
    IrqExit       = 0x05,
    SchedDecision = 0x06,
    IrqTimerTick  = 0x07,
    /// SCHED_DEBUG: full context switch with state info
    /// arg0=old_tid, arg1=old_state, arg2=new_tid, arg3=new_state
    SchedSwitch   = 0x08,
    /// SCHED_DEBUG: thread state transition
    /// arg0=tid, arg1=state_before, arg2=state_after, arg3=reason
    SchedState    = 0x09,
    /// SCHED_DEBUG: lock event
    /// arg0=lock_id, arg1=owner_tid, arg2=event_type(0=lock,1=unlock,2=contend), arg3=0
    LockEvent     = 0x0A,
    /// SCHED_DEBUG: timer IRQ preemption decision
    /// arg0=decision(0=skip_ring0,1=preempt_ring3,2=preempt_kernel,3=nop),
    /// arg1=current_tid, arg2=interrupted_cs, arg3=has_non_idle
    TimerIrqInfo  = 0x0B,
    Panic         = 0xFF,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TraceEntry {
    pub tick: u64,
    pub event: TraceEvent,
    pub arg0: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
}

pub struct TraceBuffer {
    pub entries: [TraceEntry; TRACE_CAPACITY],
    pub head: AtomicU16,
}

impl Default for TraceBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceBuffer {
    pub const fn new() -> Self {
        const ZERO: TraceEntry = TraceEntry {
            tick: 0, event: TraceEvent::ContextSwitch,
            arg0: 0, arg1: 0, arg2: 0, arg3: 0,
        };
        TraceBuffer {
            entries: [ZERO; TRACE_CAPACITY],
            head: AtomicU16::new(0),
        }
    }

    /// Write one trace entry (lock-free, interrupt-safe).
    pub fn write(&self, event: TraceEvent, arg0: u64, arg1: u64, arg2: u64, arg3: u64) {
        let idx = self.head.fetch_add(1, Ordering::Relaxed) as usize % TRACE_CAPACITY;
        let tick = crate::hal::get_ticks();
        let ptr = &self.entries[idx] as *const TraceEntry as *mut TraceEntry;
        unsafe {
            (*ptr).tick = tick;
            (*ptr).event = event;
            (*ptr).arg0 = arg0;
            (*ptr).arg1 = arg1;
            (*ptr).arg2 = arg2;
            (*ptr).arg3 = arg3;
        }
    }

    /// Dump the most recent N entries to a writer.
    pub fn dump(&self, count: usize, w: &mut dyn fmt::Write) {
        let head = self.head.load(Ordering::Relaxed) as usize;
        let start = head.saturating_sub(count);
        let end = head;
        for i in start..end {
            let idx = i % TRACE_CAPACITY;
            let e = &self.entries[idx];
            let _ = writeln!(w, "  [{}] {:?} a0={:#x} a1={:#x} a2={:#x} a3={:#x}",
                e.tick, e.event, e.arg0, e.arg1, e.arg2, e.arg3);
        }
    }
}

pub static TRACE: TraceBuffer = TraceBuffer::new();

// ── Convenience macros ──────────────────────────────────────────────

#[macro_export]
macro_rules! trace_event {
    ($event:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr $(,)?) => {
        $crate::trace::TRACE.write($event, $a0 as u64, $a1 as u64, $a2 as u64, $a3 as u64);
    };
}

#[macro_export]
macro_rules! trace_cswitch {
    ($from:expr, $to:expr) => {
        $crate::trace_event!(
            $crate::trace::TraceEvent::ContextSwitch,
            $from, $to, 0, 0
        );
    };
}

#[macro_export]
macro_rules! trace_syscall {
    ($num:expr, $arg0:expr, $arg1:expr, $arg2:expr) => {
        $crate::trace_event!(
            $crate::trace::TraceEvent::SyscallEnter,
            $num, $arg0, $arg1, $arg2
        );
    };
}

#[macro_export]
macro_rules! trace_sched {
    ($decision:expr, $pid:expr, $state:expr) => {
        $crate::trace_event!(
            $crate::trace::TraceEvent::SchedDecision,
            $decision, $pid, $state, 0
        );
    };
}

#[macro_export]
macro_rules! trace_irq_enter {
    ($irq:expr) => {
        $crate::trace_event!(
            $crate::trace::TraceEvent::IrqEnter,
            $irq, 0, 0, 0
        );
    };
}

#[macro_export]
macro_rules! trace_irq_exit {
    ($irq:expr) => {
        $crate::trace_event!(
            $crate::trace::TraceEvent::IrqExit,
            $irq, 0, 0, 0
        );
    };
}

/// SCHED_DEBUG: log a context switch with full state info.
#[macro_export]
macro_rules! trace_sched_switch {
    ($old_tid:expr, $old_state:expr, $new_tid:expr, $new_state:expr) => {
        if cfg!(feature = "sched_debug") {
            $crate::trace_event!(
                $crate::trace::TraceEvent::SchedSwitch,
                $old_tid as u64,
                $old_state as u64,
                $new_tid as u64,
                $new_state as u64,
            );
        }
    };
}

/// SCHED_DEBUG: log a thread state transition.
#[macro_export]
macro_rules! trace_sched_state {
    ($tid:expr, $before:expr, $after:expr, $reason:expr) => {
        if cfg!(feature = "sched_debug") {
            $crate::trace_event!(
                $crate::trace::TraceEvent::SchedState,
                $tid as u64,
                $before as u64,
                $after as u64,
                $reason as u64,
            );
        }
    };
}

/// SCHED_DEBUG: log a lock event.
#[macro_export]
macro_rules! trace_lock_event {
    ($lock_id:expr, $owner:expr, $event_type:expr) => {
        if cfg!(feature = "sched_debug") {
            $crate::trace_event!(
                $crate::trace::TraceEvent::LockEvent,
                $lock_id as u64,
                $owner as u64,
                $event_type as u64,
                0,
            );
        }
    };
}

/// SCHED_DEBUG: log timer IRQ preemption decision.
#[macro_export]
macro_rules! trace_timer_irq {
    ($decision:expr, $tid:expr, $cs:expr, $has_non_idle:expr) => {
        if cfg!(feature = "sched_debug") {
            $crate::trace_event!(
                $crate::trace::TraceEvent::TimerIrqInfo,
                $decision as u64,
                $tid as u64,
                $cs as u64,
                $has_non_idle as u64,
            );
        }
    };
}
