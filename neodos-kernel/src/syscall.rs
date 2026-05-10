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

/// Called from the syscall trampolín in idt.rs with raw register values.
/// Returns the value that will be placed back in RAX when returning to user space.
#[no_mangle]
pub extern "C" fn syscall_dispatch(rax: u64, rbx: u64, rcx: u64, _rdx: u64) -> u64 {
    match rax {
        // ---- sys_exit(code: u64) ----
        // Marks the calling process as Terminated so the scheduler skips it.
        0 => {
            serial_println!("[syscall] sys_exit({})", rbx);
            let scheduler_mutex = crate::scheduler::current_scheduler();
            let mut scheduler = scheduler_mutex.lock();
            let pid = scheduler.current_pid;
            if let Some(proc) = scheduler.current_process_mut() {
                proc.state = crate::scheduler::ProcessState::Terminated;
                serial_println!("[syscall] PID {} terminated", pid);
            }
            0
        }

        // ---- sys_write(ptr: *const u8, len: usize) ----
        // Writes `len` bytes from user-space pointer `ptr` to the kernel console.
        // We trust the pointer is within the USER_ACCESSIBLE range (0x400000-0x800000).
        1 => {
            let ptr = rbx as *const u8;
            let len = rcx as usize;

            // Bounds-check: only allow reads from the user memory area.
            // The identity-mapped window exposed to Ring 3 is 0x400000..0x800000.
            let addr = rbx;
            if addr < 0x400000 || addr.saturating_add(len as u64) > 0x800000 || len > 4096 {
                serial_println!("[syscall] sys_write: bad address 0x{:x} len {}", addr, len);
                return u64::MAX; // -1
            }

            // SAFETY: Validated above; user space is identity-mapped.
            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            if let Ok(s) = core::str::from_utf8(slice) {
                crate::console::print_str(s);
                serial_println!("[user] {}", s);
            }
            len as u64
        }

        // ---- sys_yield() ----
        // Forces an immediate preemption by temporarily marking the current process
        // as Ready, letting the next tick pick someone else.
        2 => {
            serial_println!("[syscall] sys_yield");
            // The scheduler will pick the next process on the next timer tick.
            // Nothing to do here — returning from the syscall already restores the
            // user context; the timer IRQ will preempt normally.
            0
        }

        // ---- sys_getpid() ----
        // Returns the PID of the currently running process.
        3 => {
            let pid = crate::scheduler::current_scheduler().lock().current_pid;
            serial_println!("[syscall] sys_getpid -> {}", pid);
            pid as u64
        }

        _ => {
            serial_println!("[syscall] unknown syscall RAX={}", rax);
            u64::MAX // -1 / ENOSYS
        }
    }
}
