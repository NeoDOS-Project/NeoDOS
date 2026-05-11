use crate::arch::x64::gdt::get_selectors;
use crate::scheduler;

#[no_mangle]
static mut EXIT_RSP: u64 = 0;
#[no_mangle]
static mut EXIT_RIP: u64 = 0;
#[no_mangle]
static mut EXIT_NOW: u8 = 0;
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
/// The process will get its own user-memory slot.
pub fn spawn_usermode(entry: u64, stack_top: u64, slot_idx: u8) -> u32 {
    let mut s = scheduler::current_scheduler().lock();
    s.add_ring3_process(entry, stack_top, slot_idx)
}

/// Execute a specific process (by PID) in Ring 3 and block the shell
/// until that process calls sys_exit.
pub fn wait_for_process(pid: u32) {
    unsafe { WAIT_PID = pid; }

    let (entry, user_stack_top) = {
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
    };

    // Tell the scheduler this process is running
    {
        let mut s = scheduler::current_scheduler().lock();
        s.current_pid = pid;
        if let Some(proc) = s.current_process_mut() {
            proc.state = scheduler::ProcessState::Running;
        }
    }

    execute_usermode(entry, user_stack_top);
}

/// Called from syscall_dispatch on RAX=0 — signals the asm trampoline
/// to return to the shell via exit_to_kernel.
pub fn request_exit_to_kernel() {
    unsafe { EXIT_NOW = 1; }
}

/// Returns the PID the shell is currently waiting for.
pub fn current_wait_pid() -> u32 {
    unsafe { WAIT_PID }
}

/// Clear the wait-pid (called after the shell returns from wait_for_process).
pub fn clear_wait_pid() {
    unsafe { WAIT_PID = 0; }
}
