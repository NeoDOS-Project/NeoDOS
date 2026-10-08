#![allow(unused_imports)]
//! Scheduler tests — extracted from mod.rs (mechanical split)
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::scheduler::types::{Kthread, Eprocess, ThreadState, MmapRegion, KernelName, NAME_MAX, PRIORITY_HIGH, PRIORITY_NORMAL, PRIORITY_IDLE, PRIORITY_ABOVE_NORMAL, TIME_SLICES, IDLE_TID, BOOT_TID, MAX_STARVATION_TICKS, AGING_INTERVAL_TICKS, IDLE_TIME_SLICE, KERNEL_STACK_SIZE};
use crate::scheduler::Scheduler;
use crate::log::LogSubsys;


use super::{add_test_thread, prepare_test_schedule, set_test_current};
pub fn register() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;
    test_case!("kthread_new_initial_state", {
        let k = Kthread::new_idle(1, 0, 0x400000, 0x800000);
        test_eq!(k.tid, 1);
        test_eq!(k.rip, 0x400000);
        test_eq!(k.state, ThreadState::Ready);
        test_eq!(k.cpu_ticks, 0);
        test_eq!(k.pid, 0);
        test_eq!(k.priority, PRIORITY_IDLE);
        test_eq!(k.time_slice_remaining, IDLE_TIME_SLICE);
        // Phase 14-A: deterministic default name + read-only accessor.
        test_eq!(k.name(), "idle");
        test_eq!(k.name.len(), 4);
        test_true!(!k.name.is_empty());
        // Phase 14-A: bounded KernelName semantics (basic, custom, boundary,
        // oversized, path derivation). Metadata only; never panics.
        test_eq!(KernelName::from_str("netd").as_str(), "netd");
        let exact = "abcdefghijklmnopqrstuvwxyz012345"; // 32 bytes == NAME_MAX
        test_eq!(exact.len(), NAME_MAX);
        let ne = KernelName::from_str(exact);
        test_eq!(ne.len(), NAME_MAX);
        test_eq!(ne.as_str(), exact);
        let oversized = "abcdefghijklmnopqrstuvwxyz0123456789"; // 36 bytes
        let no = KernelName::from_str(oversized);
        test_eq!(no.len(), NAME_MAX);
        test_eq!(no.as_str(), "abcdefghijklmnopqrstuvwxyz012345");
        test_eq!(
            KernelName::from_path("\\Global\\FileSystem\\C:\\Programs\\neoshell.nxe").as_str(),
            "neoshell"
        );
        test_eq!(KernelName::from_path("netd").as_str(), "netd");
        test_eq!(KernelName::from_str("né").as_str(), "n??");
        test_true!(KernelName::empty().is_empty());
    });
    test_case!("kthread_state_debug", {
        let mut k = Kthread::new_idle(1, 0, 0x400000, 0x800000);
        test_eq!(k.state, ThreadState::Ready);
        k.state = ThreadState::Running;
        test_eq!(k.state, ThreadState::Running);
        k.state = ThreadState::Blocked { waiting_for: 42 };
        test_eq!(k.state, ThreadState::Blocked { waiting_for: 42 });
        k.state = ThreadState::Terminated;
        test_eq!(k.state, ThreadState::Terminated);
    });
    test_case!("kthread_state_partial_eq", {
        let s1 = ThreadState::Ready;
        let s2 = ThreadState::Ready;
        test_eq!(s1, s2);
        test_ne!(ThreadState::Ready, ThreadState::Running);
        test_ne!(ThreadState::Blocked { waiting_for: 1 }, ThreadState::Blocked { waiting_for: 2 });
    });
    test_case!("stack_canary_bounds_model", {
        // #348: the canary lives at `stack_bottom = ks_top - actual_size`.
        // The checker must use the stack's owned size, not the global
        // KERNEL_STACK_SIZE, so the 4 KiB idle stack is inspected at its own
        // bottom rather than 12 KiB below it.
        use crate::scheduler::stack::kernel_stack_canary_addr;

        // Normal 16 KiB kernel stack.
        let top16 = 0x1_0000u64;
        test_eq!(kernel_stack_canary_addr(top16, KERNEL_STACK_SIZE), Some(top16 - 16384));
        // BSP idle 4 KiB stack: bottom is exactly the idle stack base.
        let idle_base = 0x2_0000u64;
        let idle_top = idle_base + crate::scheduler::IDLE_STACK_SIZE as u64;
        test_eq!(
            kernel_stack_canary_addr(idle_top, crate::scheduler::IDLE_STACK_SIZE),
            Some(idle_base)
        );
        // The old (unsound) computation for the idle stack would have read
        // 12 KiB below the owned region; assert the correct address is not that.
        test_ne!(kernel_stack_canary_addr(idle_top, crate::scheduler::IDLE_STACK_SIZE), Some(idle_top - 16384));
        // Guard rails: no address for a zero top or a zero size.
        test_eq!(kernel_stack_canary_addr(0, KERNEL_STACK_SIZE), None);
        test_eq!(kernel_stack_canary_addr(top16, 0), None);
    });
    test_case!("stack_canary_initialized_for_both_sizes", {
        // #348: both stack kinds must actually receive a canary at the address
        // the (sized) checker inspects, otherwise a correct checker would still
        // report corruption.
        use crate::scheduler::stack::{
            kernel_stack_canary_addr, init_raw_stack_canary, IDLE_STACK_SIZE,
        };
        use crate::scheduler::types::STACK_CANARY;

        // 16 KiB heap stack: AlignedKStack writes the canary at its bottom.
        let stack = crate::scheduler::AlignedKStack::new_boxed();
        let base = stack.0.as_ptr() as u64;
        let top = base + KERNEL_STACK_SIZE as u64;
        test_eq!(kernel_stack_canary_addr(top, KERNEL_STACK_SIZE), Some(base));
        let canary = unsafe { *(base as *const u64) };
        test_eq!(canary, STACK_CANARY);

        // 4 KiB idle stack: initialize a stand-in buffer and verify the canary
        // lands at the buffer bottom (the checker's address for the idle size).
        let mut buf = [0u8; IDLE_STACK_SIZE];
        init_raw_stack_canary(buf.as_mut_ptr(), IDLE_STACK_SIZE);
        let btop = buf.as_ptr() as u64 + IDLE_STACK_SIZE as u64;
        test_eq!(kernel_stack_canary_addr(btop, IDLE_STACK_SIZE), Some(buf.as_ptr() as u64));
        test_eq!(unsafe { *(buf.as_ptr() as *const u64) }, STACK_CANARY);
    });
    test_case!("stack_canary_detects_real_corruption", {
        // #348: the sized checker must still detect a genuinely corrupted
        // canary at the correct bottom address.
        use crate::scheduler::stack::{
            check_kernel_stack_canary_sized, kernel_stack_canary_addr, IDLE_STACK_SIZE,
        };
        use crate::scheduler::types::STACK_CANARY;

        let mut buf = [0u8; IDLE_STACK_SIZE];
        let base = buf.as_mut_ptr() as u64;
        let top = base + IDLE_STACK_SIZE as u64;
        // Intact canary -> must not panic (function returns normally).
        crate::scheduler::stack::init_raw_stack_canary(buf.as_mut_ptr(), IDLE_STACK_SIZE);
        test_eq!(kernel_stack_canary_addr(top, IDLE_STACK_SIZE), Some(base));
        check_kernel_stack_canary_sized(top, IDLE_STACK_SIZE, 0, 11, top);

        // Corrupt the canary word; the address the checker reads must change.
        unsafe { (base as *mut u64).write(STACK_CANARY ^ 0x1); }
        test_ne!(unsafe { *(base as *const u64) }, STACK_CANARY);
    });
}
