//! Exception dispatcher — routes CPU exceptions to kernel panic or user-mode SEH handlers.

use alloc::boxed::Box;
use crate::serial_println;
use crate::scheduler::current_teb_base;
use crate::panic_classification::PanicClass;

// ── Exception type constants (shared with libneodos user-mode) ──

pub const EXCEPTION_DIVIDE_ERROR: u32 = 0;
pub const EXCEPTION_DEBUG: u32 = 1;
pub const EXCEPTION_NMI: u32 = 2;
pub const EXCEPTION_BREAKPOINT: u32 = 3;
pub const EXCEPTION_OVERFLOW: u32 = 4;
pub const EXCEPTION_BOUND_RANGE: u32 = 5;
pub const EXCEPTION_INVALID_OPCODE: u32 = 6;
pub const EXCEPTION_DEVICE_NOT_AVAILABLE: u32 = 7;
pub const EXCEPTION_DOUBLE_FAULT: u32 = 8;
pub const EXCEPTION_INVALID_TSS: u32 = 10;
pub const EXCEPTION_SEGMENT_NOT_PRESENT: u32 = 11;
pub const EXCEPTION_STACK_SEGMENT_FAULT: u32 = 12;
pub const EXCEPTION_GPF: u32 = 13;
pub const EXCEPTION_PAGE_FAULT: u32 = 14;
pub const EXCEPTION_X87: u32 = 16;
pub const EXCEPTION_ALIGNMENT_CHECK: u32 = 17;
pub const EXCEPTION_MACHINE_CHECK: u32 = 18;
pub const EXCEPTION_SIMD: u32 = 19;
pub const EXCEPTION_VIRTUALIZATION: u32 = 20;

// ── Exception action enum ──

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionAction {
    Continue = 0,
    Terminate = 1,
    ReevaluateFilters = 2,
}

// ── TEB Exception Frame ──

/// Exception handler frame stored in the TEB.
/// Forms a linked list — the head is at `TEB.exception_list`.
#[repr(C)]
pub struct ExceptionFrame {
    pub prev_frame: Option<&'static ExceptionFrame>,
    pub handler_fn: Option<extern "C" fn(u32, u64, u64) -> u32>,
    pub exception_type: u32,
}

// ── TEB structure ──

/// Thread Environment Block layout at 0x7000.
#[repr(C)]
pub struct Teb {
    /// Self-pointer for validation
    pub teb_self: u64,
    /// PID of the owning process
    pub pid: u32,
    /// TID of this thread
    pub tid: u32,
    /// Head of exception handler linked list (NULL if none)
    pub exception_list: Option<&'static ExceptionFrame>,
    /// Reserved for future use (TLS etc.)
    pub _reserved: [u64; 125],
}

impl Teb {
    pub const fn new() -> Self {
        Teb {
            teb_self: 0,
            pid: 0,
            tid: 0,
            exception_list: None,
            _reserved: [0u64; 125],
        }
    }
}

/// TEB virtual address (4 KB page)
pub const TEB_VADDR: u64 = 0x7000;

// ── Initialize TEB paging ──

/// Initialize the TEB page at 0x7000 by splitting the first 2 MB huge page
/// and mapping the TEB page as USER_ACCESSIBLE.
///
/// Must be called once at boot (Phase 2.5-ish), after the crash dump init
/// but before any Ring 3 process is created.
pub fn init_teb_paging() {
    use crate::arch::x64::paging::PAGE_4K;
    // Split the first 2 MB huge page (covers 0x0..0x200000)
    // so we can map a 4 KB page at 0x7000 separately
    let aligned = TEB_VADDR & !(PAGE_4K - 1);

    // Check if we need to split
    if crate::hal::walk_ptes_4k(aligned).is_none() {
        match crate::arch::x64::paging::split_2mb_page(aligned) {
            Ok(()) => {
                // Flag the PD entry as USER_ACCESSIBLE so the 4 KB PTEs can be user-accessible
                let _ = crate::arch::x64::paging::set_pd_user_accessible(aligned, true);
                serial_println!("[SEH] Split 2MB page @ 0x0 for TEB at 0x{:x}", TEB_VADDR);
            }
            Err(_) => {
                serial_println!("[SEH] Failed to split 2MB page for TEB!");
                return;
            }
        }
    }

    // Map the TEB page as USER_ACCESSIBLE + writable
    let phys = crate::hal::alloc_page();
    if phys.is_null() {
        serial_println!("[SEH] Failed to allocate TEB page frame!");
        return;
    }

    // Initialize the TEB structure
    let teb_ptr = TEB_VADDR as *mut Teb;
    unsafe {
        core::ptr::write_bytes(phys, 0, 4096);
        // Write initial TEB
        core::ptr::write(teb_ptr, Teb::new());
        // Set self-pointer
        (*teb_ptr).teb_self = TEB_VADDR;
    }

    // Map it with USER_ACCESSIBLE | WRITABLE | PRESENT
    let rc = crate::hal::map_page(phys as u64, TEB_VADDR, 0x7);
    if rc != 0 {
        serial_println!("[SEH] Failed to map TEB page!");
        crate::hal::free_page(phys);
        return;
    }

    serial_println!("[SEH] TEB page mapped at 0x{:x}", TEB_VADDR);
}

// ── Invoke TEB exception chain ──

/// Walk the TEB exception handler chain and invoke handlers.
///
/// Returns the ExceptionAction returned by the handler.
/// If no handler is registered or all return ReevaluateFilters,
/// returns ExceptionAction::Terminate.
fn teb_invoke_exception_handler(
    exception_type: u32,
    fault_addr: u64,
    fault_code: u64,
) -> ExceptionAction {
    let teb_base = current_teb_base();
    if teb_base == 0 {
        return ExceptionAction::Terminate;
    }

    let teb = teb_base as *const Teb;
    let exception_list: Option<&'static ExceptionFrame>;
    unsafe {
        exception_list = (*teb).exception_list;
    }

    let mut current = exception_list;
    while let Some(frame) = current {
        if let Some(handler_fn) = frame.handler_fn {
            let action_raw = handler_fn(exception_type, fault_addr, fault_code);
            let action = match action_raw {
                0 => ExceptionAction::Continue,
                1 => ExceptionAction::Terminate,
                2 => ExceptionAction::ReevaluateFilters,
                _ => ExceptionAction::Terminate,
            };
            match action {
                ExceptionAction::ReevaluateFilters => {
                    current = frame.prev_frame;
                    continue;
                }
                _ => return action,
            }
        }
        current = frame.prev_frame;
    }

    ExceptionAction::Terminate
}

/// Set the exception handler for the current thread's TEB.
///
/// Called by sys_set_exception_handler (RAX=29).
/// handler_fn: the callback invoked when an exception occurs.
/// Returns 0 on success, -1 if TEB is not initialized.
pub fn set_thread_exception_handler(
    handler_fn: Option<extern "C" fn(u32, u64, u64) -> u32>,
) -> i64 {
    let teb_base = current_teb_base();
    if teb_base == 0 {
        return -1;
    }

    let teb = teb_base as *mut Teb;
    unsafe {
        let current_list = (*teb).exception_list;
        // Allocate a new ExceptionFrame from the slab allocator
        let frame = alloc::boxed::Box::new(ExceptionFrame {
            prev_frame: current_list,
            handler_fn,
            exception_type: 0,
        });
        let frame_ptr = Box::into_raw(frame);
        (*teb).exception_list = Some(&*frame_ptr);
    }
    0
}

// ── Exception dispatcher ──

/// Result from exception dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchResult {
    Handled,
    Terminated,
    Panic,
}

/// Main exception dispatch entry point.
///
/// Called from the IDT exception handlers.
///
/// * `exception_type` — One of the EXCEPTION_* constants.
/// * `rip` — Instruction pointer at the time of the exception.
/// * `rsp` — Stack pointer at the time of the exception.
/// * `error_code` — CPU error code (valid for GPF, page fault, etc.).
/// * `is_user` — True if the exception occurred in Ring 3.
/// * `fault_addr` — Faulting address (CR2 for page faults).
/// * `fault_code` — Additional fault code info.
///
/// Returns `DispatchResult` indicating what happened.
pub fn exception_dispatch(
    exception_type: u32,
    rip: u64,
    rsp: u64,
    error_code: u64,
    is_user: bool,
    fault_addr: u64,
    fault_code: u64,
) -> DispatchResult {
    if !is_user {
        // Ring 0: kernel exception — always crash dump + panic
        serial_println!("[EXC] Kernel exception: type={} rip={:#x} rsp={:#x} err={:#x}",
            exception_type, rip, rsp, error_code);

        let panic_class = match exception_type {
            EXCEPTION_PAGE_FAULT => PanicClass::PageFault,
            EXCEPTION_GPF => PanicClass::Gpf,
            EXCEPTION_DOUBLE_FAULT => PanicClass::DoubleFault,
            EXCEPTION_STACK_SEGMENT_FAULT => PanicClass::StackCorruption,
            EXCEPTION_INVALID_TSS => PanicClass::InvalidContextSwitch,
            _ => PanicClass::UnknownCpuException,
        };

        crate::panic_classification::set_panic_class(panic_class);
        // Call crash dump
        let param = [rip, rsp, error_code, exception_type as u64];
        crate::crash::dump_crash(crate::crash::CAUSE_PANIC, &param, rip, rsp);

        DispatchResult::Panic
    } else {
        // Ring 3: user exception — attempt to deliver via TEB chain
        let action = teb_invoke_exception_handler(exception_type, fault_addr, fault_code);

        match action {
            ExceptionAction::Continue => {
                serial_println!("[EXC] User exception handled (Continue): type={} rip={:#x}",
                    exception_type, rip);
                DispatchResult::Handled
            }
            ExceptionAction::Terminate => {
                serial_println!("[EXC] User exception unhandled (Terminate): type={} rip={:#x}",
                    exception_type, rip);
                DispatchResult::Terminated
            }
            ExceptionAction::ReevaluateFilters => {
                // Should not reach here — terminate as fallback
                serial_println!("[EXC] User exception filters exhausted: type={} rip={:#x}",
                    exception_type, rip);
                DispatchResult::Terminated
            }
        }
    }
}

// ── Tests ──

pub fn register_exception_tests() {
    use crate::test_case;
    use crate::test_true;
    use crate::test_eq;

    test_case!("seh_teb_frame_alloc", {
        let frame = alloc::boxed::Box::new(ExceptionFrame {
            prev_frame: None,
            handler_fn: None,
            exception_type: 0,
        });
        let ptr = core::ptr::addr_of!(*frame) as u64;
        test_true!(ptr != 0);
        drop(frame);
    });

    test_case!("seh_exception_action_values", {
        test_eq!(ExceptionAction::Continue as u32, 0);
        test_eq!(ExceptionAction::Terminate as u32, 1);
        test_eq!(ExceptionAction::ReevaluateFilters as u32, 2);
    });

    test_case!("seh_teb_layout", {
        let teb = Teb::new();
        test_eq!(teb.teb_self, 0);
        test_eq!(teb.pid, 0);
        test_eq!(teb.tid, 0);
        test_true!(teb.exception_list.is_none());
    });

    test_case!("seh_exception_type_constants", {
        test_eq!(EXCEPTION_DIVIDE_ERROR, 0);
        test_eq!(EXCEPTION_GPF, 13);
        test_eq!(EXCEPTION_PAGE_FAULT, 14);
        test_eq!(EXCEPTION_BREAKPOINT, 3);
    });

    test_case!("seh_dispatch_kernel_classification", {
        let result = exception_dispatch(
            EXCEPTION_DIVIDE_ERROR, 0x200042, 0x1F0000, 0, false, 0, 0,
        );
        test_eq!(result, DispatchResult::Panic);

        let result = exception_dispatch(
            EXCEPTION_GPF, 0x200042, 0x1F0000, 0, false, 0, 0,
        );
        test_eq!(result, DispatchResult::Panic);
    });
}

// ── NXL AbiTable entry for sys_set_exception_handler ──
// The user-mode library calls through the NXL export table.
// The kernel's syscall handler (RAX=29) invokes set_thread_exception_handler.
