use core::fmt;
use core::sync::atomic::{AtomicU16, Ordering};

// ── Trace ring buffer ───────────────────────────────────────────────
// Lock-free, no heap, fixed-size.  Producer = IRQ / syscall / scheduler.
// Consumer = panic handler (interrupts off, single-reader).

pub const TRACE_CAPACITY: usize = 1024;
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
    entries: [TraceEntry; TRACE_CAPACITY],
    head: AtomicU16,
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
        let tick = crate::scheduler::TIMER_TICKS.load(Ordering::Relaxed);
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
        let start = if head >= count { head - count } else { 0 };
        let end = head;
        for i in start..end {
            let idx = i % TRACE_CAPACITY;
            let e = &self.entries[idx];
            let _ = write!(w, "  [{}] {:?} a0={:#x} a1={:#x} a2={:#x} a3={:#x}\n",
                e.tick, e.event, e.arg0, e.arg1, e.arg2, e.arg3);
        }
    }
}

pub static TRACE: TraceBuffer = TraceBuffer::new();

// ── Convenience macros ──────────────────────────────────────────────

#[macro_export]
macro_rules! trace_event {
    ($event:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr) => {
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
