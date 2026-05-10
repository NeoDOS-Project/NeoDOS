//! Syscall dispatch table — INT 0x80
//!
//! Calling convention (Ring 3 → kernel):
//!   RAX = syscall number
//!   RBX = arg0
//!   RCX = arg1
//!   RDX = arg2
//!
//! Return value in RAX (passed back from this function to the asm trampolín).
//!
//! Syscall numbers:
//!   0  sys_exit   — terminate current process
//!   1  sys_write  — write bytes to the console
//!   2  sys_yield  — voluntarily give up the CPU
//!   3  sys_getpid — return current PID

use crate::serial_println;
use crate::scheduler::ProcessState;

/// Called from the syscall trampolín in idt.rs with raw register values.
/// Returns the value that will be placed back in RAX when returning to user space.
#[no_mangle]
pub extern "C" fn syscall_dispatch(rax: u64, rbx: u64, rcx: u64, _rdx: u64) -> u64 {
    match rax {
        // ---- sys_exit(code: u64) ----
        0 => {
            let code = rbx;
            serial_println!("[syscall] sys_exit({})", code);

            let s = crate::scheduler::current_scheduler();
            let mut scheduler = s.lock();
            let pid = scheduler.current_pid;

            if pid > 0 {
                if let Some(proc) = scheduler.current_process_mut() {
                    proc.state = ProcessState::Terminated;

                    // Free the user memory slot
                    if let Some(slot) = proc.user_slot.take() {
                        crate::arch::x64::paging::free_user_slot(slot);
                    }
                }
            }

            // Wake any process waiting on this PID
            scheduler.wake_waiters(pid);

            // If the shell is waiting for this PID, signal exit_to_kernel
            if pid > 0 && pid == crate::usermode::current_wait_pid() {
                crate::usermode::request_exit_to_kernel();
            }

            0
        }

        // ---- sys_write(ptr: *const u8, len: usize) ----
        1 => {
            let ptr = rbx as *const u8;
            let len = rcx as usize;

            let addr = rbx;
            if addr < 0x400000 || addr.saturating_add(len as u64) > 0x800000 || len > 4096 {
                serial_println!("[syscall] sys_write: bad address 0x{:x} len {}", addr, len);
                return u64::MAX;
            }

            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            if let Ok(s) = core::str::from_utf8(slice) {
                crate::console::print_str(s);
                serial_println!("[user] {}", s);
            }
            len as u64
        }

        // ---- sys_yield() ----
        2 => {
            serial_println!("[syscall] sys_yield");
            0
        }

        // ---- sys_getpid() ----
        3 => {
            let pid = crate::scheduler::current_scheduler().lock().current_pid;
            serial_println!("[syscall] sys_getpid -> {}", pid);
            pid as u64
        }

        _ => {
            serial_println!("[syscall] unknown syscall RAX={}", rax);
            u64::MAX
        }
    }
}
