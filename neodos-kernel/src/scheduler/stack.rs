//! Scheduler stack handling — extracted from mod.rs
use alloc::boxed::Box;
use crate::scheduler::types::{KERNEL_STACK_SIZE, STACK_CANARY};

pub(crate) use crate::scheduler::types::IDLE_STACK_SIZE;

#[repr(align(16))]
pub struct AlignedKStack(pub [u8; KERNEL_STACK_SIZE]);

impl AlignedKStack {
    pub fn new_boxed() -> Box<Self> {
        Self::try_new_boxed().expect("AlignedKStack::new_boxed OOM")
    }

    pub fn try_new_boxed() -> Option<Box<Self>> {
        let mut stack = Box::try_new(AlignedKStack([0u8; KERNEL_STACK_SIZE])).ok()?;
        unsafe {
            (stack.0.as_mut_ptr() as *mut u64).write(STACK_CANARY);
        }
        Some(stack)
    }
}

/// #348: address of the canary for a kernel stack described by its top and its
/// actual size. `None` when the top is a sentinel (0) or the size is 0.
///
/// The canary is the first word of the owned stack, i.e. `ks_top - size`. This
/// is a pure function so the bounds arithmetic can be unit-tested without
/// touching real memory.
#[inline]
pub fn kernel_stack_canary_addr(ks_top: u64, stack_size: usize) -> Option<u64> {
    if ks_top == 0 || stack_size == 0 {
        return None;
    }
    Some(ks_top.saturating_sub(stack_size as u64))
}

/// #348: validate the canary of a stack with an explicit `stack_size`.
///
/// `stack_size` is the owned span (`Kernel_stack_size` on a Kthread), not the
/// global `KERNEL_STACK_SIZE`: the idle threads own shorter stacks. The previous
/// implementation always subtracted `KERNEL_STACK_SIZE`, so for the 4 KiB
/// `IDLE_STACK` it read 12 KiB below the owned stack and reported a false
/// overflow.
pub fn check_kernel_stack_canary_sized(
    ks_top: u64,
    stack_size: usize,
    pid: u32,
    tid: u32,
    current_rsp: u64,
) {
    let Some(bottom) = kernel_stack_canary_addr(ks_top, stack_size) else { return };
    let canary = unsafe { *(bottom as *const u64) };
    if canary != STACK_CANARY {
        crate::serial_println!(
            "\n!!! CRITICAL KERNEL STACK OVERFLOW DETECTED !!!\n             PID={} TID={} ks_top=0x{:x} size={} current_rsp=0x{:x} bottom=0x{:x} canary=0x{:x} expected=0x{:x}",
            pid, tid, ks_top, stack_size, current_rsp, bottom, canary, STACK_CANARY
        );
        panic!("KERNEL STACK CANARY CORRUPTED FOR TID={}", tid);
    }
}

/// Backwards-compatible wrapper for callers that own a full `KERNEL_STACK_SIZE`
/// stack (heap-allocated kernel threads). Idle-aware callers use
/// [`check_kernel_stack_canary_sized`].
pub fn check_kernel_stack_canary(ks_top: u64, pid: u32, tid: u32, current_rsp: u64) {
    check_kernel_stack_canary_sized(ks_top, KERNEL_STACK_SIZE, pid, tid, current_rsp);
}

/// #348: initialize the canary at the bottom of a raw stack of the given size.
///
/// Used for the static per-CPU idle stacks, which are not `AlignedKStack` boxes
/// and therefore never received a canary. Must be called before the stack is
/// used.
pub fn init_raw_stack_canary(stack_bottom: *mut u8, stack_size: usize) {
    if stack_bottom.is_null() || stack_size < core::mem::size_of::<u64>() {
        return;
    }
    unsafe {
        (stack_bottom as *mut u64).write(STACK_CANARY);
    }
}

/// #348: initialize the BSP idle stack canary. Idempotent; called once during
/// `Scheduler::new` before the idle Kthread is published.
pub unsafe fn init_idle_stack_canary() {
    init_raw_stack_canary(IDLE_STACK.as_mut_ptr(), IDLE_STACK_SIZE);
}

/// The BSP idle thread's static stack. Shorter than `KERNEL_STACK_SIZE`; its
/// canary lives at [`IDLE_STACK`] itself.
pub static mut IDLE_STACK: [u8; IDLE_STACK_SIZE] = [0; IDLE_STACK_SIZE];

pub fn spawn_net_kthread(entry: u64) -> Option<u32> {
    crate::hal::without_interrupts(|| {
        crate::scheduler::current_scheduler()
            .lock()
            .spawn_kthread_named(entry, crate::scheduler::types::PRIORITY_NORMAL, "netd")
    })
}

// Frame init helpers — preserved exactly (unsafe boundaries, alignment, layout)
pub(crate) fn init_ring0_frame(kernel_stack_top: u64, entry: u64) -> u64 {
    let mut sp = kernel_stack_top & !0xF;
    unsafe {
        let stack = sp as *mut u64;
        stack.offset(-1).write(0x10);
        stack.offset(-2).write(kernel_stack_top);
        stack.offset(-3).write(0x202);
        stack.offset(-4).write(0x08);
        stack.offset(-5).write(entry);
        for j in 6..21 {
            stack.offset(-(j as isize)).write(0);
        }
        sp -= 20 * 8;
    }
    sp
}

pub fn init_ring3_frame(kernel_stack_top: u64, entry: u64, user_stack_top: u64) -> u64 {
    let mut sp = kernel_stack_top & !0xF;
    unsafe {
        let stack = sp as *mut u64;
        stack.offset(-1).write(0x23);
        stack.offset(-2).write(user_stack_top);
        stack.offset(-3).write(0x202);
        stack.offset(-4).write(0x1B);
        stack.offset(-5).write(entry);
        for j in 6..21 {
            stack.offset(-(j as isize)).write(0);
        }
        sp -= 20 * 8;
    }
    sp
}

pub(crate) fn idle_task() -> ! {
    loop {
        // The work queue and event bus are global and single-driver: only the
        // BSP idle thread may drain them. AP idle threads just halt (their
        // timer drives preemption). Running these on both CPUs corrupts the
        // shared queues (Phase 13 SMP).
        let is_bsp = crate::hal::safe::GsBase::read() == 0
            || unsafe { crate::arch::x64::cpu_local::this_cpu_id() == 0 };
        if is_bsp {
            crate::hal::without_interrupts(|| {
                crate::work_queue::WORK_QUEUE.process_high();
                crate::work_queue::WORK_QUEUE.process_low();
            });
            crate::eventbus::EVENT_BUS.dispatch_pending();
        }
        crate::hal::hlt_once();
    }
}
