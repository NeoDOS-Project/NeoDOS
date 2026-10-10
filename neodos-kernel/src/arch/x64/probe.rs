//! Recoverable kernel fault probe (NEODOS-09 dependency, #649).
//!
//! A tiny primitives that lets kernel code touch a possibly-inaccessible
//! address and receive an error instead of bugchecking: the page-fault handler
//! redirects a kernel-mode fault to a recovery label when a probe is armed.
//! Used by the SMEP/SMAP/NX tests (and potentially a fixup-based `copy_from_user`).

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};
use x86_64::VirtAddr;

/// Armed while a probe is executing. The page-fault handler only recovers a
/// kernel-mode fault when this is set.
#[no_mangle]
#[used]
pub static PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// RSP captured at probe entry, restored on recovery (the `call` in the probe
/// pushes a return address).
#[no_mangle]
#[used]
pub static PROBE_SAVED_RSP: AtomicU64 = AtomicU64::new(0);

core::arch::global_asm!(
    ".global nem_probe_read_u8",
    "nem_probe_read_u8:",              // rdi = ptr; returns i64 (>=0 ok, -1 fault)
    "  mov [rip + PROBE_SAVED_RSP], rsp",
    "  mov byte ptr [rip + PROBE_ACTIVE], 1",
    "  movzx eax, byte ptr [rdi]",     // may fault
    "  mov byte ptr [rip + PROBE_ACTIVE], 0",
    "  ret",
    ".global nem_probe_read_u8_recover",
    "nem_probe_read_u8_recover:",
    "  mov rsp, [rip + PROBE_SAVED_RSP]",
    "  mov byte ptr [rip + PROBE_ACTIVE], 0",
    "  mov rax, -1",
    "  ret",
);

extern "C" {
    fn nem_probe_read_u8(ptr: u64) -> i64;
    fn nem_probe_read_u8_recover();
}

/// Read one byte from `ptr`, returning `None` if the access faults.
/// Never faults in the caller: an inaccessible address is recovered.
pub fn probe_read_u8(ptr: u64) -> Option<u8> {
    let r = unsafe { nem_probe_read_u8(ptr) };
    if r < 0 { None } else { Some(r as u8) }
}

/// Whether a probe is currently armed (diagnostics/tests).
#[inline]
pub fn probe_armed() -> bool { PROBE_ACTIVE.load(Ordering::Acquire) }

/// Called from `page_fault_handler`. If a probe is armed and the fault came from
/// kernel mode, redirect execution to the probe's recovery label and return
/// `true`. Normal kernel faults (no probe armed) return `false` and bugcheck.
pub fn try_recover(frame: &mut InterruptStackFrame, error: PageFaultErrorCode) -> bool {
    if !PROBE_ACTIVE.load(Ordering::Acquire) {
        return false;
    }
    // Only recover kernel-mode faults; user faults keep their normal path.
    if error.contains(PageFaultErrorCode::USER_MODE) {
        return false;
    }
    PROBE_ACTIVE.store(false, Ordering::Release);
    let recover = nem_probe_read_u8_recover as *const () as u64;
    let saved_rsp = PROBE_SAVED_RSP.load(Ordering::Acquire);
    unsafe {
        frame.as_mut().update(|f| {
            f.instruction_pointer = VirtAddr::new(recover);
            if saved_rsp != 0 {
                f.stack_pointer = VirtAddr::new(saved_rsp);
            }
        });
    }
    true
}
