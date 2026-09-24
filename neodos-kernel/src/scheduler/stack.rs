//! Scheduler stack handling — extracted from mod.rs
use alloc::boxed::Box;
use crate::scheduler::types::{KERNEL_STACK_SIZE, STACK_CANARY, IDLE_TIME_SLICE};
use crate::log::LogSubsys;

pub(crate) const IDLE_STACK_SIZE: usize = 4096;

#[repr(align(16))]
pub struct AlignedKStack(pub [u8; KERNEL_STACK_SIZE]);

impl AlignedKStack {
    pub fn new_boxed() -> Box<Self> {
        let mut stack = Box::new(AlignedKStack([0u8; KERNEL_STACK_SIZE]));
        unsafe {
            (stack.0.as_mut_ptr() as *mut u64).write(STACK_CANARY);
        }
        stack
    }
}

pub fn check_kernel_stack_canary(ks_top: u64, pid: u32, tid: u32, current_rsp: u64) {
    if ks_top == 0 { return; }
    let bottom = ks_top.saturating_sub(KERNEL_STACK_SIZE as u64);
    let canary = unsafe { *(bottom as *const u64) };
    if canary != STACK_CANARY {
        crate::serial_println!(
            "\n!!! CRITICAL KERNEL STACK OVERFLOW DETECTED !!!\n             PID={} TID={} ks_top=0x{:x} current_rsp=0x{:x} bottom=0x{:x} canary=0x{:x} expected=0x{:x}",
            pid, tid, ks_top, current_rsp, bottom, canary, STACK_CANARY
        );
        panic!("KERNEL STACK CANARY CORRUPTED FOR TID={}", tid);
    }
}

pub static mut IDLE_STACK: [u8; IDLE_STACK_SIZE] = [0; IDLE_STACK_SIZE];

pub fn spawn_net_kthread(entry: u64) -> Option<u32> {
    crate::hal::without_interrupts(|| {
        crate::scheduler::current_scheduler()
            .lock()
            .spawn_kthread(entry, crate::scheduler::types::PRIORITY_NORMAL)
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
        crate::hal::without_interrupts(|| {
            crate::work_queue::WORK_QUEUE.process_high();
            crate::work_queue::WORK_QUEUE.process_low();
        });
        crate::eventbus::EVENT_BUS.dispatch_pending();
        crate::hal::hlt_once();
    }
}
