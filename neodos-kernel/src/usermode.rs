use crate::arch::x64::gdt;
use crate::arch::x64::gdt::get_selectors;
use crate::scheduler;
use crate::scheduler::AlignedKStack;
use crate::object;
use crate::object::ObType;
use crate::arch::x64::cpu_local::{OFFSET_EXIT_RSP, OFFSET_EXIT_RIP, OFFSET_EXIT_RBX,
    OFFSET_EXIT_R12, OFFSET_EXIT_R13, OFFSET_EXIT_R14, OFFSET_EXIT_R15, OFFSET_EXIT_RBP};
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use crate::log::LogSubsys;
use alloc::boxed::Box;
use alloc::format;

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
    // Write to per-CPU KPRCB via GS segment (global statics removed for SMP safety — AUDIT-07)
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

pub fn spawn_usermode(entry: u64, stack_top: u64, slot_idx: u8, cwd_drive: u8, cwd_path: &str, parent_pid: u32) -> Result<u32, &'static str> {
    // F-DEV-02: zombie queue backpressure — bounded queue without loss. If storm
    // fills queue, try synchronous reclaim before allocating resources; if still
    // backpressured (all zombies still running), fail spawn with NoMem instead
    // of silently dropping PIDs (which leaked slots forever).
    if crate::scheduler::lifecycle::zombie_queue_len() >= 64 {
        let reclaimed = crate::hal::without_interrupts(|| {
            if let Some(mut s) = crate::scheduler::current_scheduler().try_lock() {
                let cur_pid = s.current_pid();
                let before = crate::scheduler::lifecycle::zombie_queue_len();
                crate::scheduler::lifecycle::reap_pending_zombies(&mut *s, cur_pid);
                crate::scheduler::lifecycle::zombie_queue_len() < before
            } else { false }
        });
        if !reclaimed && crate::scheduler::lifecycle::is_zombie_backpressured() {
            return Err("NoMem: zombie backpressure");
        }
    }
    // F-04 transactional spawn: all pid-dependent allocations now inside the lock.
    // Resources acquired outside (heap_slot, kernel stack) are tracked for rollback
    // if the critical section fails. No pid/tid is reserved before the lock,
    // eliminating the race where two concurrent spawns could guess the same pid
    // and create duplicate Ob names with mismatched native_id.
    let heap_slot = crate::arch::x64::paging::alloc_heap_slot();
    let heap_base = match heap_slot {
        Some(slot) => slot.base,
        None => {
            kwarn!(LogSubsys::User, "no free heap slots, process will have no heap");
            0
        }
    };
    let heap_idx = if heap_base != 0 {
        Some(((heap_base - crate::arch::x64::paging::PROCESS_HEAP_BASE) / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8)
    } else { None };

    // Kernel stack allocation (Box::try_new → no panic on OOM) — pid independent
    let stack = match scheduler::AlignedKStack::try_new_boxed() {
        Some(s) => s,
        None => {
            if let Some(idx) = heap_idx {
                crate::arch::x64::paging::free_heap_slot(idx);
            }
            return Err("NoMem for kernel stack");
        }
    };
    let kernel_stack_top = stack.0.as_ptr() as u64 + scheduler::KERNEL_STACK_SIZE as u64;
    let rsp = scheduler::init_ring3_frame(kernel_stack_top, entry, stack_top);

    // Inherit parent token (quick, outside the main critical section)
    let parent_token = crate::hal::without_interrupts(|| {
        let lock = scheduler::current_scheduler().lock();
        lock.find_eprocess(parent_pid)
            .map(|ep| ep.token.clone())
            .unwrap_or(crate::security::DEFAULT_ADMIN_TOKEN.clone())
    });

    // Single transactional critical section: pid/tid allocation, slot reservation,
    // Eprocess/Kthread creation. Ob objects are now created *inside* the lock
    // with the real pid/tid (no guess), so native_id always matches EPROCESS pid
    // and no duplicate names can occur. If this section fails, we rollback
    // heap_slot and the kernel stack is dropped (Box freed) automatically.
    let result = crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        // Ensure Vecs have capacity — P0.2: try_reserve, return NoMem on OOM
        s.ensure_slots().map_err(|e| {
            // Ensure_slots failed, stack will be dropped by caller, heap slot freed there
            e
        })?;
        // Delegate to the existing helper but with Ob ids = None (created inside if needed).
        // The helper now handles its own Ob creation with the real pid/tid, so we pass None.
        s.add_ring3_process_with_stack(
            entry, slot_idx, cwd_drive, cwd_path,
            heap_base, parent_pid, rsp, kernel_stack_top, stack,
            None, None, None, parent_token,
        )
    });

    match result {
        Ok(pid) => {
            // Success: heap slot is now owned by the new Eprocess, do not free.
            crate::serial_println!("[SPAWN] pid={} heap_base=0x{:x} slot={} OK", pid, heap_base, slot_idx);
            Ok(pid)
        }
        Err(e) => {
            // Rollback heap slot (user slot is caller's responsibility)
            if let Some(idx) = heap_idx {
                crate::arch::x64::paging::free_heap_slot(idx);
                crate::serial_println!("[SPAWN] rollback heap_slot idx={} base=0x{:x} err={}", idx, heap_base, e);
            }
            // Kernel stack Box was moved into add_ring3_process_with_stack and dropped on Err
            // (its Drop frees the allocation). No Ob objects to clean because we passed None
            // and the helper didn't create any on failure (or it would have cleaned).
            Err(e)
        }
    }
}

pub fn wait_for_process(pid: u32) {
    WAIT_PID.store(pid, Ordering::SeqCst);

    crate::serial_println!("[USERMODE] wait_for_process pid={}", pid);

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
        kdebug!(LogSubsys::User, "wait_for_process: PID {} not found", pid);
        return;
    }

    crate::serial_println!("[USERMODE] entry=0x{:x} stack=0x{:x} kernel_stack_top=0x{:x}",
        entry, user_stack_top, kernel_stack_top);

    unsafe { gdt::prepare_ring3_return(kernel_stack_top, pid as u32, pid); }

    kinfo!(LogSubsys::User, "[THREAD] wait_for_process: entering PID {} user mode (entry=0x{:x})", pid, entry);

    // Transition the boot thread (TID 0, current) to Blocked so the
    // scheduler never picks it again, then activate the target process
    // from Suspended to Running and make it current.  The boot thread's
    // real saved execution context lives in EXIT_RSP/EXIT_RIP (set by
    // execute_usermode_asm) — it will be restored when the Ring 3
    // process exits via exit_to_kernel.
    crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        let tid = s.current_tid;
        crate::serial_println!("[USERMODE] blocking TID 0, current_tid={} activating pid={}", tid, pid);
        // Block the boot thread (TID 0) if current
        if tid == scheduler::BOOT_TID {
            if let Some(k) = s.find_kthread_mut(scheduler::BOOT_TID) {
                scheduler::Scheduler::remove_from_run_queue(k);
                k.state = scheduler::ThreadState::Blocked {
                    waiting_for: pid as u64,
                };
            }
        }
        // Activate the target process
        let mut target_tid = 0;
        let mut target_ptr: *mut scheduler::Kthread = core::ptr::null_mut();
        let mut target_pid: u32 = 0;
        for k in s.kthreads.iter().flatten() {
            if k.pid == pid && k.tid > 0 {
                target_tid = k.tid;
                target_pid = k.pid;
                target_ptr = &**k as *const scheduler::Kthread as *mut scheduler::Kthread;
                s.current_tid = target_tid;
                break;
            }
        }
        // F-01: sync per-CPU KPRCB (BSP)
        if !target_ptr.is_null() {
            unsafe { crate::arch::x64::cpu_local::sync_per_cpu_current(target_ptr, target_pid); }
        }
        crate::serial_println!("[USERMODE] activated TID={}", target_tid);
        if let Some(k) = s.current_kthread_mut() {
            k.state = scheduler::ThreadState::Running;
        }
    });

    // RSP0 must be set atomically with the Ring-3 entry.  Between the
    // without_interrupts block above and the iretq in execute_usermode,
    // a timer can fire, netd can be scheduled, and the context-switch
    // back to TID 0 sets RSP0 to 0 (boot's kernel_stack_top).  The cli
    // here prevents that window, and the subsequent iretq restores IF.
    crate::hal::disable_interrupts();
    unsafe {
        let current_tid = scheduler::current_tid();
        unsafe { gdt::prepare_ring3_return(kernel_stack_top, current_tid, pid); }
    }
    crate::serial_println!("[USERMODE] RSP0=0x{:x} executing execute_usermode entry=0x{:x}",
        kernel_stack_top, entry);
    kdebug!(LogSubsys::User, "[THREAD] RSP0=0x{:x}, entering Ring3", kernel_stack_top);
    execute_usermode(entry, user_stack_top);

    // When the Ring 3 process exits, exit_to_kernel restores the boot
    // context from EXIT_RSP/EXIT_RIP and returns here.  Restore the
    // boot thread's Running state for consistency.
    crate::serial_println!("[USERMODE] EXIT: Ring3 process pid={} terminated, returning to boot", pid);
    crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        s.current_tid = scheduler::BOOT_TID;
        // F-01: sync KPRCB back to boot thread
        let boot_ptr = s.find_kthread(scheduler::BOOT_TID).map(|k| k as *const _ as *mut scheduler::Kthread).unwrap_or(core::ptr::null_mut());
        unsafe { crate::arch::x64::cpu_local::sync_per_cpu_current(boot_ptr, 0); }
        if let Some(k) = s.find_kthread_mut(scheduler::BOOT_TID) {
            k.state = scheduler::ThreadState::Running;
        }
    });
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
