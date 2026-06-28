use crate::arch::x64::gdt;
use crate::arch::x64::gdt::get_selectors;
use crate::scheduler;
use crate::arch::x64::cpu_local::{OFFSET_EXIT_RSP, OFFSET_EXIT_RIP, OFFSET_EXIT_RBX,
    OFFSET_EXIT_R12, OFFSET_EXIT_R13, OFFSET_EXIT_R14, OFFSET_EXIT_R15, OFFSET_EXIT_RBP};
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

// ── Per-CPU exit trampoline ──────────────────────────────────────────────
//
// The EXIT_RSP/EXIT_RIP/etc. context must be per-CPU (each CPU has its
// own IRETQ trampoline). We store these in the KPRCB at known offsets
// and access them via GS segment.
//
// For backward compatibility, we keep the global statics as well (used
// during early boot before GS is set, or for single-CPU mode).

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
static WAIT_PID: AtomicU32 = AtomicU32::new(0);

core::arch::global_asm!(
    ".global execute_usermode_asm",
    "execute_usermode_asm:",
    // Save exit context to per-CPU KPRCB via GS segment.
    // On entry: RDI = entry_point, RSI = stack_pointer, RDX = user_cs, RCX = user_ss
    // We need to save kernel RSP/RIP into KPRCB fields via GS.
    // First, save to global statics (backward compat), then also to KPRCB.

    // Save return address (label 1f) as EXIT_RIP
    "lea rax, [rip + 1f]",
    // Write to global statics (legacy path)
    "mov [rip + EXIT_RIP], rax",
    "mov [rip + EXIT_RSP], rsp",
    "mov [rip + EXIT_RBX], rbx",
    "mov [rip + EXIT_R12], r12",
    "mov [rip + EXIT_R13], r13",
    "mov [rip + EXIT_R14], r14",
    "mov [rip + EXIT_R15], r15",
    "mov [rip + EXIT_RBP], rbp",
    // Also write to per-CPU KPRCB via GS segment
    "mov gs:[{}], rsp",                     // OFFSET_EXIT_RSP
    "mov gs:[{}], rax",                     // OFFSET_EXIT_RIP
    "mov gs:[{}], rbx",                     // OFFSET_EXIT_RBX
    "mov gs:[{}], r12",                     // OFFSET_EXIT_R12
    "mov gs:[{}], r13",                     // OFFSET_EXIT_R13
    "mov gs:[{}], r14",                     // OFFSET_EXIT_R14
    "mov gs:[{}], r15",                     // OFFSET_EXIT_R15
    "mov gs:[{}], rbp",                     // OFFSET_EXIT_RBP
    // IRETQ to Ring 3
    "push rcx",                             // user SS
    "push rsi",                             // user RSP
    "push 0x200",                           // RFLAGS (IF=1)
    "push rdx",                             // user CS
    "push rdi",                             // user RIP
    "iretq",
    "1:",
    "sti",
    "ret",

    ".global exit_to_kernel",
    "exit_to_kernel:",
    // Restore from per-CPU KPRCB via GS segment
    "mov rsp, gs:[{}]",                     // OFFSET_EXIT_RSP
    "mov rbx, gs:[{}]",                     // OFFSET_EXIT_RBX
    "mov r12, gs:[{}]",                     // OFFSET_EXIT_R12
    "mov r13, gs:[{}]",                     // OFFSET_EXIT_R13
    "mov r14, gs:[{}]",                     // OFFSET_EXIT_R14
    "mov r15, gs:[{}]",                     // OFFSET_EXIT_R15
    "mov rbp, gs:[{}]",                     // OFFSET_EXIT_RBP
    "push gs:[{}]",                         // OFFSET_EXIT_RIP
    "ret",
    const OFFSET_EXIT_RSP as u64,
    const OFFSET_EXIT_RIP as u64,
    const OFFSET_EXIT_RBX as u64,
    const OFFSET_EXIT_R12 as u64,
    const OFFSET_EXIT_R13 as u64,
    const OFFSET_EXIT_R14 as u64,
    const OFFSET_EXIT_R15 as u64,
    const OFFSET_EXIT_RBP as u64,
    const OFFSET_EXIT_RSP as u64,
    const OFFSET_EXIT_RBX as u64,
    const OFFSET_EXIT_R12 as u64,
    const OFFSET_EXIT_R13 as u64,
    const OFFSET_EXIT_R14 as u64,
    const OFFSET_EXIT_R15 as u64,
    const OFFSET_EXIT_RBP as u64,
    const OFFSET_EXIT_RIP as u64,
);

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

pub fn spawn_usermode(entry: u64, stack_top: u64, slot_idx: u8, cwd_drive: u8, cwd_path: &str, parent_pid: u32) -> u32 {
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
        s.add_ring3_process(entry, stack_top, slot_idx, cwd_drive, cwd_path, heap_base, parent_pid)
    })
}

pub fn wait_for_process(pid: u32) {
    WAIT_PID.store(pid, Ordering::SeqCst);

    let (entry, user_stack_top, kernel_stack_top) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler().lock();
        for k in s.kthreads.iter().flatten() {
            if k.pid == pid && k.tid > 0 {
                let entry_ = k.rip;
                let ks_top = k.kernel_stack_top;
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
        (0u64, 0u64, 0u64)
    });

    if entry == 0 {
        crate::serial_println!("[USER] wait_for_process: PID {} not found", pid);
        return;
    }

    gdt::set_kernel_stack(kernel_stack_top);

    crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        for k in s.kthreads.iter().flatten() {
            if k.pid == pid && k.tid > 0 {
                s.current_tid = k.tid;
                break;
            }
        }
        if let Some(k) = s.current_kthread_mut() {
            k.state = scheduler::ThreadState::Running;
        }
    });

    execute_usermode(entry, user_stack_top);
}

/// Signal the current CPU to exit to kernel mode on next syscall return.
/// Writes to both the global EXIT_NOW and the per-CPU KPRCB exit_now flag.
pub fn request_exit_to_kernel() {
    EXIT_NOW.store(1, Ordering::SeqCst);
    // Also set per-CPU flag via GS segment (write directly, not via pointer)
    unsafe {
        crate::arch::x64::cpu_local::gs_write_u8(
            crate::arch::x64::cpu_local::OFFSET_EXIT_NOW, 1);
    }
}

pub fn current_wait_pid() -> u32 {
    WAIT_PID.load(Ordering::SeqCst)
}

pub fn set_wait_pid(pid: u32) {
    WAIT_PID.store(pid, Ordering::SeqCst);
}

pub fn clear_wait_pid() {
    WAIT_PID.store(0, Ordering::SeqCst);
}
