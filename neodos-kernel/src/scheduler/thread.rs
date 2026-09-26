//! Scheduler Kthread impl — extracted from mod.rs
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use crate::scheduler::types::{Kthread, ThreadState, PRIORITY_IDLE, PRIORITY_NORMAL, TIME_SLICES, IDLE_TIME_SLICE};
use crate::scheduler::stack::{AlignedKStack, init_ring0_frame, init_ring3_frame};

impl Kthread {
    pub fn new_idle(tid: u32, pid: u32, entry: u64, stack_top: u64) -> Self {
        let rsp = init_ring0_frame(stack_top, entry);
        Kthread {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp, rip: entry, rflags: 0x202,
            tid, pid,
            state: ThreadState::Ready,
            cpu_ticks: 0,
            waiting_for: None,
            priority: PRIORITY_IDLE,
            time_slice_remaining: IDLE_TIME_SLICE,
            ticks_since_scheduled: 0,
            kernel_stack_top: stack_top,
            kernel_stack: None,
            teb_base: 0,
            cpu: 0,
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
            is_idle: true,
            yield_requested: false,
        }
    }

    /// Idle thread whose `rsp` is captured by the first timer interrupt.
    ///
    /// Unlike [`new_idle`], this writes **no** synthetic iretq frame: the target
    /// CPU is already running on `stack_top`, so fabricating a frame would
    /// corrupt its live stack. `on_timer_tick` stores the real interrupt frame
    /// into `rsp` at the first timeslice expiry, before any switch to it.
    /// `kernel_stack` stays `None` so the pre-allocated AP stack is never freed
    /// by the scheduler.
    pub fn new_idle_bare(tid: u32, pid: u32, stack_top: u64) -> Self {
        Kthread {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp: 0, rip: 0, rflags: 0x202,
            tid, pid,
            state: ThreadState::Running,
            cpu_ticks: 0,
            waiting_for: None,
            priority: PRIORITY_IDLE,
            time_slice_remaining: IDLE_TIME_SLICE,
            ticks_since_scheduled: 0,
            kernel_stack_top: stack_top,
            kernel_stack: None,
            teb_base: 0,
            cpu: 0,
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
            is_idle: true,
            yield_requested: false,
        }
    }

    pub fn new_ring3(tid: u32, pid: u32, entry: u64, user_stack_top: u64) -> Self {
        let stack = AlignedKStack::new_boxed();
        let kernel_stack_top = stack.0.as_ptr() as u64 + crate::scheduler::types::KERNEL_STACK_SIZE as u64;
        let rsp = init_ring3_frame(kernel_stack_top, entry, user_stack_top);
        Self::new_ring3_with_stack(tid, pid, entry, rsp, kernel_stack_top, stack)
    }

    /// Create a Ring 3 Kthread with a pre-allocated kernel stack.
    pub fn new_ring3_with_stack(
        tid: u32, pid: u32, entry: u64,
        rsp: u64, kernel_stack_top: u64, stack: Box<AlignedKStack>,
    ) -> Self {
        if kernel_stack_top == 0 {
            panic!("Kthread::new_ring3_with_stack: kernel_stack_top is 0 for TID={}", tid);
        }
        Kthread {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp, rip: entry, rflags: 0x202,
            tid, pid,
            state: ThreadState::Ready,
            cpu_ticks: 0,
            waiting_for: None,
            priority: PRIORITY_NORMAL,
            time_slice_remaining: TIME_SLICES[PRIORITY_NORMAL as usize],
            ticks_since_scheduled: 0,
            kernel_stack_top,
            kernel_stack: Some(stack),
            teb_base: 0,
            cpu: unsafe { crate::arch::x64::cpu_local::this_cpu_id() },
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
            is_idle: false,
            yield_requested: false,
        }
    }
}
