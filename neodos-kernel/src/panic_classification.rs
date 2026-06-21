use core::fmt;
use alloc::format;
use core::sync::atomic::{AtomicU8, Ordering};

// ── Global classification store ─────────────────────────────────────
// Written by exception handlers before calling panic!(),
// read by the #[panic_handler] to enrich the crash dump.

static PANIC_CLASS: AtomicU8 = AtomicU8::new(0); // PanicClass::Unspecified

#[inline]
pub fn set_panic_class(class: PanicClass) {
    PANIC_CLASS.store(class as u8, Ordering::SeqCst);
}

#[inline]
pub fn current_panic_class() -> PanicClass {
    match PANIC_CLASS.load(Ordering::Relaxed) {
        1 => PanicClass::Gpf,
        2 => PanicClass::PageFault,
        3 => PanicClass::DoubleFault,
        4 => PanicClass::StackCorruption,
        5 => PanicClass::InvalidIretq,
        6 => PanicClass::IrqReentrancy,
        7 => PanicClass::AbiMismatch,
        8 => PanicClass::PageTableCorruption,
        9 => PanicClass::InvalidContextSwitch,
        10 => PanicClass::SchedPanic,
        11 => PanicClass::MemoryCorruption,
        12 => PanicClass::UnknownCpuException,
        13 => PanicClass::AssertionFailed,
        _ => PanicClass::Unspecified,
    }
}

// ── Macro to panic with a classification ────────────────────────────

#[macro_export]
macro_rules! panic_with_class {
    ($class:expr, $($arg:tt)*) => {{
        $crate::panic_classification::set_panic_class($class);
        panic!($($arg)*);
    }};
}

// ── Forensic dump ───────────────────────────────────────────────────
// Dumps trace buffer + scheduler state to serial.  Called from the
// panic handler after disabling interrupts.

pub fn dump_forensic_info() {
    use crate::arch::x64::serial::SERIAL1;
    let port = SERIAL1.lock();
    use core::fmt::Write;
    struct RawWriter(*const crate::arch::x64::serial::SerialPort);
    impl Write for RawWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let port = unsafe { &*self.0 };
            for &b in s.as_bytes() {
                port.send(b);
            }
            Ok(())
        }
    }
    let mut w = RawWriter(&*port as *const _);
    let _ = write!(w, "\n--- Trace buffer (last {} entries) ---\n", crate::trace::TRACE_DUMP_COUNT);
    crate::trace::TRACE.dump(crate::trace::TRACE_DUMP_COUNT, &mut w);

    let _ = write!(w, "--- Scheduler state ---\n");
    let ticks = crate::hal::get_ticks();
    let _ = write!(w, "  Timer ticks: {}\n", ticks);
    if let Some(sched) = crate::scheduler::current_scheduler().try_lock() {
        let _ = write!(w, "  Current TID: {}  Next TID: {}  Next PID: {}\n", sched.current_tid, sched.next_tid, sched.next_pid);
        for (i, t) in sched.kthreads.iter().enumerate() {
            if let Some(k) = t {
                let state = format!("{:?}", k.state);
                let _ = write!(w, "  [{}] TID={} PID={} state={} ticks={}\n",
                    i, k.tid, k.pid, state, k.cpu_ticks);
            }
        }
    } else {
        let _ = write!(w, "  (scheduler lock contended)\n");
    }

    let _ = write!(w, "--- End forensic dump ---\n");
}

// ── Panic classification ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PanicClass {
    /// Unclassified panic — no matching pattern found.
    Unspecified = 0,
    /// General protection fault — check error code + RIP for sub-classification.
    Gpf = 1,
    /// Page fault — check error code for cause (protection violation, non-present).
    PageFault = 2,
    /// Double fault — exception during exception handling (likely stack or TSS issue).
    DoubleFault = 3,
    /// Stack overflow or misaligned RSP detected.
    StackCorruption = 4,
    /// IRETQ with wrong stack frame layout (e.g., Ring0→Ring0 IRETQ).
    InvalidIretq = 5,
    /// Interrupt handler entered while another IRQ was active.
    IrqReentrancy = 6,
    /// ABI version mismatch between kernel and module/bootloader.
    AbiMismatch = 7,
    /// Page table entry or mapping corrupted.
    PageTableCorruption = 8,
    /// Context switch attempted from illegal scheduler state.
    InvalidContextSwitch = 9,
    /// Scheduler state machine violation.
    SchedPanic = 10,
    /// Heap allocator metadata corruption.
    MemoryCorruption = 11,
    /// Reserved CPU exception (vector 0–19, DPL=0).
    UnknownCpuException = 12,
    /// Explicit assertion failure.
    AssertionFailed = 13,
}

impl PanicClass {
    pub fn to_str(&self) -> &'static str {
        match self {
            PanicClass::Unspecified => "UNSPECIFIED",
            PanicClass::Gpf => "GPF",
            PanicClass::PageFault => "PAGE_FAULT",
            PanicClass::DoubleFault => "DOUBLE_FAULT",
            PanicClass::StackCorruption => "STACK_CORRUPTION",
            PanicClass::InvalidIretq => "INVALID_IRETQ",
            PanicClass::IrqReentrancy => "IRQ_REENTRANCY",
            PanicClass::AbiMismatch => "ABI_MISMATCH",
            PanicClass::PageTableCorruption => "PAGE_TABLE_CORRUPTION",
            PanicClass::InvalidContextSwitch => "INVALID_CONTEXT_SWITCH",
            PanicClass::SchedPanic => "SCHED_PANIC",
            PanicClass::MemoryCorruption => "MEMORY_CORRUPTION",
            PanicClass::UnknownCpuException => "UNKNOWN_CPU_EXCEPTION",
            PanicClass::AssertionFailed => "ASSERTION_FAILED",
        }
    }
}

/// Location of known architectural failure points.
/// Used to match a crash RIP against known dangerous patterns.
#[repr(C)]
pub struct PanicSignature {
    pub rip_lower: u64,
    pub rip_upper: u64,
    pub classification: PanicClass,
    pub description: &'static str,
}

// ── Classification engine ───────────────────────────────────────────

const SIGNATURES: &[PanicSignature] = &[
    // IRETQ instruction in syscall epilogue (~0x20xxxx range)
    PanicSignature { rip_lower: 0x4000000, rip_upper: 0x4010000, classification: PanicClass::InvalidIretq, description: "IRETQ in kernel text range (v0.40)" },
];

/// Classify a panic based on RIP, exception vector, and error code.
/// `rip` — instruction pointer at panic,
/// `vector` — CPU exception vector (0..31) or 0 if not an exception.
/// `error_code` — CPU error code, if any.
/// `is_double_fault` — true if this is a double-fault handler entry.
pub fn classify(rip: u64, vector: u8, error_code: u64, is_double_fault: bool) -> PanicClass {
    if is_double_fault {
        return PanicClass::DoubleFault;
    }

    // Match against known signatures by RIP range
    for sig in SIGNATURES {
        if rip >= sig.rip_lower && rip < sig.rip_upper {
            return sig.classification;
        }
    }

    // Classify by exception vector
    match vector {
        0x0D => {
            // GPF — check if IRETQ instruction
            if error_code == 0x15c {
                PanicClass::InvalidIretq
            } else {
                PanicClass::Gpf
            }
        }
        0x0E => {
            // Page fault — check error code
            let pf_cause = error_code;
            if pf_cause & 0x01 == 0 {
                // Non-present page
                PanicClass::PageFault
            } else {
                // Protection violation
                PanicClass::PageTableCorruption
            }
        }
        0x08 => PanicClass::DoubleFault,
        v if v < 0x20 => PanicClass::UnknownCpuException,
        _ => PanicClass::Unspecified,
    }
}

impl fmt::Display for PanicClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}
