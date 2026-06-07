use crate::arch::x64::gdt;
use crate::arch::x64::gdt::get_selectors;
use crate::scheduler;
use core::sync::atomic::{AtomicU8, Ordering};

#[no_mangle]
static mut EXIT_RSP: u64 = 0;
#[no_mangle]
static mut EXIT_RIP: u64 = 0;
#[no_mangle]
static mut EXIT_RBX: u64 = 0;
#[no_mangle]
static mut EXIT_R12: u64 = 0;
#[no_mangle]
static mut EXIT_R13: u64 = 0;
#[no_mangle]
static mut EXIT_R14: u64 = 0;
#[no_mangle]
static mut EXIT_R15: u64 = 0;
#[no_mangle]
static mut EXIT_RBP: u64 = 0;

#[no_mangle]
static EXIT_NOW: AtomicU8 = AtomicU8::new(0);
static mut WAIT_PID: u32 = 0;

core::arch::global_asm!(
    ".global execute_usermode_asm",
    "execute_usermode_asm:",
    "lea rax, [rip + 1f]",
    "mov [rip + EXIT_RIP], rax",
    "mov [rip + EXIT_RSP], rsp",
    "mov [rip + EXIT_RBX], rbx",
    "mov [rip + EXIT_R12], r12",
    "mov [rip + EXIT_R13], r13",
    "mov [rip + EXIT_R14], r14",
    "mov [rip + EXIT_R15], r15",
    "mov [rip + EXIT_RBP], rbp",
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
    "mov rbx, [rip + EXIT_RBX]",
    "mov r12, [rip + EXIT_R12]",
    "mov r13, [rip + EXIT_R13]",
    "mov r14, [rip + EXIT_R14]",
    "mov r15, [rip + EXIT_R15]",
    "mov rbp, [rip + EXIT_RBP]",
    "push [rip + EXIT_RIP]",
    "ret",
);

#[allow(dead_code)]
extern "C" {
    fn execute_usermode_asm(entry: u64, stack: u64, cs: u64, ss: u64);
    fn exit_to_kernel();
}

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

pub fn spawn_usermode(entry: u64, stack_top: u64, slot_idx: u8, cwd_drive: u8, cwd_path: &str) -> u32 {
    let heap_slot = crate::arch::x64::paging::alloc_heap_slot();
    let heap_base = match heap_slot {
        Some(slot) => slot.base,
        None => {
            crate::serial_println!("[USER] WARNING: no free heap slots, process will have no heap");
            0
        }
    };

    crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        s.add_ring3_process(entry, stack_top, slot_idx, cwd_drive, cwd_path, heap_base)
    })
}

pub fn wait_for_process(pid: u32) {
    unsafe { WAIT_PID = pid; }

    let (entry, user_stack_top, kernel_stack_top) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler().lock();
        // Find the first non-idle thread belonging to this PID
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.pid == pid && k.tid > 0 {
                    let entry_ = k.rip;
                    let ks_top = k.kernel_stack_top;
                    // Find user_stack_top from EPROCESS
                    let sp = if let Some(ep) = s.find_eprocess(pid) {
                        if let Some(slot) = ep.user_slot {
                            let slot_size = 0x20000u64;
                            let max_bin = 0x10000u64;
                            let user_stack = 0x10000u64;
                            crate::arch::x64::paging::USER_BASE
                                + slot as u64 * slot_size
                                + max_bin + user_stack
                        } else {
                            k.rsp
                        }
                    } else {
                        k.rsp
                    };
                    return (entry_, sp, ks_top);
                }
            }
        }
        (0u64, 0u64, 0u64)
    });

    if entry == 0 {
        crate::serial_println!("[USER] wait_for_process: PID {} not found", pid);
        return;
    }

    // Set TSS.RSP0 to thread's kernel stack
    gdt::set_kernel_stack(kernel_stack_top);

    crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        // Set current_tid to the first thread of this PID
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.pid == pid && k.tid > 0 {
                    s.current_tid = k.tid;
                    break;
                }
            }
        }
        // Set thread state to Running
        if let Some(k) = s.current_kthread_mut() {
            k.state = scheduler::ThreadState::Running;
        }
    });

    execute_usermode(entry, user_stack_top);
}

pub fn request_exit_to_kernel() {
    EXIT_NOW.store(1, Ordering::SeqCst);
}

pub fn current_wait_pid() -> u32 {
    unsafe { WAIT_PID }
}

pub fn clear_wait_pid() {
    unsafe { WAIT_PID = 0; }
}
