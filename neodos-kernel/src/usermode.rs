use crate::arch::x64::gdt::get_selectors;
use crate::scheduler;

#[no_mangle]
static mut EXIT_RSP: u64 = 0;
#[no_mangle]
static mut EXIT_RIP: u64 = 0;
use core::sync::atomic::{AtomicU8, Ordering};

#[no_mangle]
static EXIT_NOW: AtomicU8 = AtomicU8::new(0);
static mut WAIT_PID: u32 = 0;

core::arch::global_asm!(
    ".global execute_usermode_asm",
    "execute_usermode_asm:",
    "lea rax, [rip + 1f]",
    "mov [rip + EXIT_RIP], rax",
    "mov [rip + EXIT_RSP], rsp",
    "push rcx",
    "push rsi",
    "push 0x200",
    "push rdx",
    "push rdi",
    "iretq",
    "1:",
    "sti",
    "ret",

    ".global exit_to_kernel",
    "exit_to_kernel:",
    "mov rsp, [rip + EXIT_RSP]",
    "push [rip + EXIT_RIP]",
    "ret",
);

#[allow(dead_code)]
extern "C" {
    fn execute_usermode_asm(entry: u64, stack: u64, cs: u64, ss: u64);
    fn exit_to_kernel();
}

/// IRETQ to Ring 3, saving the kernel stack/return context.
/// Returns only when the process (or another) calls `exit_to_kernel`.
pub fn execute_usermode(entry_point: u64, stack_pointer: u64) {
    let selectors = get_selectors();
    unsafe {
        execute_usermode_asm(
            entry_point,
            stack_pointer,
            selectors.user_code.0 as u64,
            selectors.user_data.0 as u64,
        );
    }
}

/// Add a user-space process to the scheduler and return its PID.
/// The process will get its own user-memory slot, a 2 MB heap region,
/// and inherit the shell's cwd.
pub fn spawn_usermode(entry: u64, stack_top: u64, slot_idx: u8, cwd_drive: u8, cwd_path: &str) -> u32 {
    // Allocate a 2 MB heap slot and mark it USER_ACCESSIBLE.
    let heap_slot = crate::arch::x64::paging::alloc_heap_slot();
    let (heap_base, _heap_idx) = match heap_slot {
        Some(slot) => {
            unsafe {
                crate::arch::x64::paging::map_user_range(slot.base, crate::arch::x64::paging::PROCESS_HEAP_SIZE);
            }
            (slot.base, Some(slot.index))
        }
        None => {
            crate::serial_println!("[spawn_usermode] WARNING: no free heap slots, process will have no heap");
            (0, None)
        }
    };

    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        let pid = s.add_ring3_process(entry, stack_top, slot_idx, cwd_drive, cwd_path);
        // Set heap fields on the just-added process.
        for proc in s.processes.iter_mut() {
            if let Some(p) = proc {
                if p.pid == pid {
                    p.heap_base = heap_base;
                    p.heap_break = heap_base;
                    break;
                }
            }
        }
        pid
    })
}

/// Execute a specific process (by PID) in Ring 3 and block the shell
/// until that process calls sys_exit.
pub fn wait_for_process(pid: u32) {
    unsafe { WAIT_PID = pid; }

    let (entry, user_stack_top) = x86_64::instructions::interrupts::without_interrupts(|| {
        let s = scheduler::current_scheduler().lock();
        let mut entry = 0u64;
        let mut sp = 0u64;
        for proc in s.processes.iter() {
            if let Some(p) = proc {
                if p.pid == pid {
                    entry = p.rip;
                    // Compute user stack top from slot info, not p.rsp (which is
                    // the timer-context-switch frame pointer).
                    sp = if let Some(slot) = p.user_slot {
                        let slot_size = 0x20000u64; // 128 KB per slot
                        let max_bin = 0x10000u64;   // 64 KB code
                        let user_stack = 0x10000u64; // 64 KB stack
                        crate::arch::x64::paging::USER_BASE
                            + slot as u64 * slot_size
                            + max_bin + user_stack
                    } else {
                        p.rsp
                    };
                    break;
                }
            }
        }
        (entry, sp)
    });

    // Tell the scheduler this process is running
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        s.current_pid = pid;
        if let Some(proc) = s.current_process_mut() {
            proc.state = scheduler::ProcessState::Running;
        }
    });

    execute_usermode(entry, user_stack_top);
}

/// Called from syscall_dispatch on RAX=0 — signals the asm trampoline
/// to return to the shell via exit_to_kernel.
pub fn request_exit_to_kernel() {
    EXIT_NOW.store(1, Ordering::SeqCst);
}

/// Returns the PID the shell is currently waiting for.
pub fn current_wait_pid() -> u32 {
    unsafe { WAIT_PID }
}

/// Clear the wait-pid (called after the shell returns from wait_for_process).
pub fn clear_wait_pid() {
    unsafe { WAIT_PID = 0; }
}
