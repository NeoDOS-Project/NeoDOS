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

pub fn spawn_usermode(entry: u64, stack_top: u64, slot_idx: u8, cwd_drive: u8, cwd_path: &str, parent_pid: u32) -> Result<u32, &'static str> {
    let heap_slot = crate::arch::x64::paging::alloc_heap_slot();
    let heap_base = match heap_slot {
        Some(slot) => slot.base,
        None => {
            kwarn!(LogSubsys::User, "no free heap slots, process will have no heap");
            0
        }
    };

    // ── Phase 1: ALL allocations OUTSIDE scheduler lock ──
    // 1. Kernel stack allocation (Box::new → heap alloc with stack canary)
    let stack = scheduler::AlignedKStack::new_boxed();
    let kernel_stack_top = stack.0.as_ptr() as u64 + scheduler::KERNEL_STACK_SIZE as u64;
    let rsp = scheduler::init_ring3_frame(kernel_stack_top, entry, stack_top);

    // 2. Reserve PID/TID and pre-allocate Vec slots atomically
    // Do NOT increment next_pid/next_tid here — add_ring3_process_with_stack
    // will allocate them. We only peek and ensure slots, otherwise we
    // double-allocate and Ob native_id (pid/tid) drifts from EPROCESS pid.
    let (pid, tid) = crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        s.ensure_slots();
        let p = s.next_pid;
        let t = s.next_tid;
        (p, t)
    });

    // 3. Create Ob objects (heap allocs, Ob manager locks — all outside scheduler lock)
    let name = alloc::format!("eproc/{}", pid);
    let obj_id = object::ob_create_object(object::ObType::Process, &name, pid as u64, 0, None).ok();

    let ob_name = alloc::format!("proc/{}", pid);
    let ob_id = match object::ob_create_object(object::ObType::Process, &ob_name, pid as u64, 0, None) {
        Ok(id) => {
            let ns_path = alloc::format!("\\Process\\{}", pid);
            let _ = crate::object::namespace::ob_insert_object(&ns_path, id);
            Some(id)
        }
        Err(_) => None,
    };

    crate::serial_println!("[SPAWN] pid={} tid={} obj_id pid={} ob_id pid={}", pid, tid, pid, pid);
    let tname = alloc::format!("kthread/{}", tid);
    let thread_obj_id = object::ob_create_object(object::ObType::Thread, &tname, tid as u64, 0, None).ok();
    if let Some(id) = obj_id { if let Some(o) = object::ob_lookup(id) { crate::serial_println!("[SPAWN] obj_id {} type={:?} native_id={}", id, o.obj_type, o.native_id); } }
    if let Some(id) = ob_id { if let Some(o) = object::ob_lookup(id) { crate::serial_println!("[SPAWN] ob_id {} type={:?} native_id={}", id, o.obj_type, o.native_id); } }
    if let Some(id) = thread_obj_id { if let Some(o) = object::ob_lookup(id) { crate::serial_println!("[SPAWN] thread_obj_id {} type={:?} native_id={}", id, o.obj_type, o.native_id); } }

    // 4. Inherit parent token
    let parent_token = crate::hal::without_interrupts(|| {
        let lock = scheduler::current_scheduler().lock();
        lock.find_eprocess(parent_pid)
            .map(|ep| ep.token.clone())
            .unwrap_or(crate::security::DEFAULT_ADMIN_TOKEN.clone())
    });

    // ── Phase 2: Minimal critical section — only table insertion ──
    crate::hal::without_interrupts(|| {
        let mut s = scheduler::current_scheduler().lock();
        s.add_ring3_process_with_stack(
            entry, slot_idx, cwd_drive, cwd_path,
            heap_base, parent_pid, rsp, kernel_stack_top, stack,
            obj_id, ob_id, thread_obj_id, parent_token,
        )
    })
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
                    waiting_for: pid,
                };
            }
        }
        // Activate the target process
        let mut target_tid = 0;
        for k in s.kthreads.iter().flatten() {
            if k.pid == pid && k.tid > 0 {
                target_tid = k.tid;
                s.current_tid = target_tid;
                break;
            }
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
