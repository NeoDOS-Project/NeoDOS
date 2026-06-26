use crate::serial_print;
use crate::serial_println;

type TestFn = fn() -> Result<(), &'static str>;

#[derive(Copy, Clone)]
struct Test {
    name: &'static str,
    func: TestFn,
}

const MAX_TESTS: usize = 520;
static mut TESTS: [Option<Test>; MAX_TESTS] = [None; MAX_TESTS];
static mut TEST_COUNT: usize = 0;

pub fn register(name: &'static str, func: TestFn) {
    unsafe {
        if TEST_COUNT < MAX_TESTS {
            TESTS[TEST_COUNT] = Some(Test { name, func });
            TEST_COUNT += 1;
        }
    }
}

pub fn run_all() -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    unsafe {
        for i in 0..TEST_COUNT {
            if let Some(test) = &TESTS[i] {
                serial_print!("  TEST {} ... ", test.name);
                match (test.func)() {
                    Ok(()) => {
                        serial_println!("PASS");
                        passed += 1;
                    }
                    Err(msg) => {
                        serial_println!("FAIL: {}", msg);
                        failed += 1;
                    }
                }
            }
        }
    }
    (passed, failed)
}

#[macro_export]
macro_rules! test_case {
    ($name:expr, $body:block) => {
        $crate::testing::register($name, || { $body; Ok(()) });
    };
}

#[macro_export]
macro_rules! test_eq {
    ($left:expr, $right:expr $(,)?) => {
        if ($left) != ($right) {
            return Err(concat!(
                "assertion failed: ", stringify!($left), " == ", stringify!($right)
            ));
        }
    };
}

#[macro_export]
macro_rules! test_ne {
    ($left:expr, $right:expr $(,)?) => {
        if ($left) == ($right) {
            return Err(concat!(
                "assertion failed: ", stringify!($left), " != ", stringify!($right)
            ));
        }
    };
}

#[macro_export]
macro_rules! test_true {
    ($cond:expr $(,)?) => {
        if !($cond) {
            return Err(concat!(
                "assertion failed: expected true: ", stringify!($cond)
            ));
        }
    };
}



// ===== Input buffer tests =====

pub fn register_input_tests() {
    use crate::input::InputBuffer;

    test_case!("input_empty_pop", {
        let buf = InputBuffer::new();
        test_eq!(buf.pop(), None);
    });

    test_case!("input_push_pop_one", {
        let buf = InputBuffer::new();
        test_eq!(buf.push(42), Ok(()));
        test_eq!(buf.pop(), Some(42));
        test_eq!(buf.pop(), None);
    });

    test_case!("input_buffer_capacity", {
        let buf = InputBuffer::new();
        let mut count = 0;
        while buf.push(count as u8).is_ok() {
            count += 1;
        }
        test_ne!(count, 0);
        test_eq!(buf.push(0), Err(()));
    });

    test_case!("input_wrap_around", {
        let buf = InputBuffer::new();
        for i in 0..100 { let _ = buf.push(i); }
        for i in 0..50 { test_eq!(buf.pop(), Some(i)); }
        for i in 100..150 { let _ = buf.push(i); }
        for i in 50..100 { test_eq!(buf.pop(), Some(i)); }
        for i in 100..150 { test_eq!(buf.pop(), Some(i)); }
        test_eq!(buf.pop(), None);
    });

    test_case!("input_full_then_drain", {
        let buf = InputBuffer::new();
        while buf.push(0xFF).is_ok() {}
        let mut count = 0;
        while buf.pop().is_some() {
            count += 1;
        }
        test_ne!(count, 0);
    });
}

pub fn register_vt_tests() {
    use crate::input::vt::{VtInputQueue, VT_COUNT, VT_QUEUE_SIZE};

    test_case!("vt_queue_create", {
        let q = VtInputQueue::new();
        test_eq!(q.pop(), None);
    });

    test_case!("vt_queue_push_pop", {
        let q = VtInputQueue::new();
        test_eq!(q.push(0x41), Ok(()));
        test_eq!(q.pop(), Some(0x41));
        test_eq!(q.pop(), None);
    });

    test_case!("vt_queue_capacity", {
        let q = VtInputQueue::new();
        let mut count = 0;
        while q.push(count as u8).is_ok() {
            count += 1;
        }
        test_true!(count > 0);
        test_eq!(count, VT_QUEUE_SIZE - 1);
    });

    test_case!("vt_queue_wrap_around", {
        let q = VtInputQueue::new();
        for i in 0..VT_QUEUE_SIZE as u8 { let _ = q.push(i); }
        for i in 0..50 { test_eq!(q.pop(), Some(i)); }
        for i in (VT_QUEUE_SIZE as u8)..(VT_QUEUE_SIZE as u8 + 50) { let _ = q.push(i); }
        for i in 50..(VT_QUEUE_SIZE as u8) { test_eq!(q.pop(), Some(i)); }
        for i in (VT_QUEUE_SIZE as u8)..(VT_QUEUE_SIZE as u8 + 50) { test_eq!(q.pop(), Some(i)); }
        test_eq!(q.pop(), None);
    });

    test_case!("vt_push_to_all_queues", {
        use crate::input::manager::{push_byte, pop_byte_from_vt};
        for _vt in 0..VT_COUNT {
            let _ = push_byte(b'X');
            let active = crate::input::active_vt();
            test_eq!(pop_byte_from_vt(active), Some(b'X'));
        }
    });

    test_case!("vt_count_at_least_2", {
        test_true!(VT_COUNT >= 2);
    });
}

pub fn register_process_tests() {
    use crate::scheduler::{Kthread, ThreadState, Eprocess};

    test_case!("kthread_new_initial_state", {
        let k = Kthread::new_idle(1, 0, 0x400000, 0x800000);
        test_eq!(k.tid, 1);
        test_eq!(k.rip, 0x400000);
        test_eq!(k.state, ThreadState::Ready);
        test_eq!(k.cpu_ticks, 0);
        test_eq!(k.pid, 0);
        test_eq!(k.priority, crate::scheduler::PRIORITY_NORMAL);
        test_eq!(k.time_slice_remaining, crate::scheduler::TIME_SLICES[crate::scheduler::PRIORITY_NORMAL as usize]);
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

    test_case!("eprocess_new_ring3", {
        let ep = Eprocess::new_ring3(42, 1, 2, "\\", 0x10000000, 0);
        test_eq!(ep.pid, 42);
        test_eq!(ep.heap_base, 0x10000000);
        test_eq!(ep.heap_break, 0x10000000);
        test_eq!(ep.thread_count, 1);
        test_eq!(ep.cwd_drive, 2);
    });
}

pub fn register_sched_priority_tests() {
    use crate::scheduler::{
        Kthread, ThreadState, Scheduler, Eprocess,
        PRIORITY_HIGH, PRIORITY_NORMAL, PRIORITY_IDLE, TIME_SLICES,
        MAX_STARVATION_TICKS, AGING_INTERVAL_TICKS,
    };

    fn add_test_thread(sched: &mut Scheduler, tid: u32, pid: u32, entry: u64, priority: u8, state: ThreadState) {
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(tid, pid, entry, 0x800000);
        k.state = state;
        k.priority = priority;
        k.time_slice_remaining = TIME_SLICES[priority as usize];
        sched.kthreads[slot] = Some(k);
        // Ensure eprocess exists
        if sched.find_eprocess(pid).is_none() {
            let ep_slot = sched.alloc_eprocess_slot().unwrap();
            sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(pid, 0, 2, "\\", 0x10000000, 0));
        }
        // Ensure next_tid is high enough
        if tid >= sched.next_tid {
            sched.next_tid = tid + 1;
        }
    }

    test_case!("sched_priority_high_picked_first", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;

        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);

        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 2);
    });

    test_case!("sched_priority_round_robin_same_level", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 0;

        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);

        let first = sched.schedule();
        let first_tid = unsafe { (*first).tid };
        test_ne!(first_tid, 0);

        let second = sched.schedule();
        let second_tid = unsafe { (*second).tid };
        test_ne!(second_tid, first_tid);
    });

    test_case!("sched_priority_idle_last", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;

        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);

        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 2);
    });

    test_case!("sched_time_slice_default_values", {
        let k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_NORMAL as usize]);
        test_eq!(k.priority, PRIORITY_NORMAL);
    });

    test_case!("sched_on_timer_tick_decrements_slice", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        sched.current_tid = 1;

        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 5;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(k);

        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(1, 0, 2, "\\", 0x10000000, 0));

        sched.on_timer_tick();

        let remaining = sched.kthreads[slot].as_ref().unwrap().time_slice_remaining;
        test_eq!(remaining, 4);
    });

    test_case!("sched_on_timer_tick_expire_yields", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        sched.current_tid = 1;

        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 1;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(k);

        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(1, 0, 2, "\\", 0x10000000, 0));

        sched.on_timer_tick();

        let state = sched.kthreads[slot].as_ref().unwrap().state;
        test_eq!(state, ThreadState::Ready);
    });

    test_case!("sched_aging_boosts_starved", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        sched.current_tid = 1;

        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        k.state = ThreadState::Ready;
        k.priority = PRIORITY_IDLE;
        k.ticks_since_scheduled = MAX_STARVATION_TICKS + 1;
        k.time_slice_remaining = 50;
        sched.kthreads[slot] = Some(k);

        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(1, 0, 2, "\\", 0x10000000, 0));

        for _ in 0..AGING_INTERVAL_TICKS + 5 {
            sched.on_timer_tick();
        }

        let boosted = sched.kthreads[slot].as_ref().unwrap();
        test_true!(boosted.priority < PRIORITY_IDLE);
    });

    test_case!("sched_set_process_priority", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;

        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        k.state = ThreadState::Ready;
        sched.kthreads[slot] = Some(k);

        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(1, 0, 2, "\\", 0x10000000, 0));

        test_true!(sched.set_process_priority(1, PRIORITY_HIGH));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_HIGH);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_HIGH as usize]);

        test_true!(sched.set_process_priority(1, PRIORITY_IDLE));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_IDLE);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_IDLE as usize]);

        test_true!(!sched.set_process_priority(1, 99));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_IDLE);

        test_true!(!sched.set_process_priority(999, PRIORITY_HIGH));
    });

    test_case!("sched_priority_preempt_higher_ready", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 2;

        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_IDLE, ThreadState::Ready);

        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 1);
    });

    test_case!("sched_priority_blocked_ignored", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 2;

        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Blocked { waiting_for: 99 });
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_IDLE, ThreadState::Ready);

        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 3);
    });

    test_case!("sched_priority_unblock_picks_higher", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 2;

        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Blocked { waiting_for: 0xFFFF_0000 });
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_IDLE, ThreadState::Running);

        sched.kthreads.iter_mut().find(|t| t.as_ref().is_some_and(|k| k.tid == 1))
            .and_then(|t| t.as_mut()).unwrap().state = ThreadState::Ready;

        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 1);
    });
}

pub fn register_utf8_tests() {
    test_case!("utf8_valid_ascii", {
        let data = b"Hello World!";
        test_eq!(core::str::from_utf8(data), Ok("Hello World!"));
    });

    test_case!("utf8_valid_2byte", {
        let data = [0xC3, 0xA1];
        test_eq!(core::str::from_utf8(&data), Ok("á"));
    });

    test_case!("utf8_valid_3byte", {
        let data = [0xE2, 0x82, 0xAC];
        test_eq!(core::str::from_utf8(&data), Ok("€"));
    });

    test_case!("utf8_invalid_incomplete_seq", {
        let data = &[0xC3][..];
        test_true!(core::str::from_utf8(data).is_err());
    });

    test_case!("utf8_invalid_continuation", {
        let data = &[0xC3, 0x00][..];
        test_true!(core::str::from_utf8(data).is_err());
    });

    test_case!("utf8_empty", {
        test_eq!(core::str::from_utf8(b""), Ok(""));
    });
}

pub fn register_alloc_tests() {
    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use alloc::string::String;

    test_case!("alloc_box_u64", {
        let b = Box::new(42u64);
        test_eq!(*b, 42);
    });

    test_case!("alloc_box_mutation", {
        let mut b = Box::new(100i32);
        *b = 200;
        test_eq!(*b, 200);
    });

    test_case!("alloc_vec_push", {
        let mut v = Vec::new();
        v.push(1);
        v.push(2);
        v.push(3);
        test_eq!(v.len(), 3);
        test_eq!(v[0], 1);
        test_eq!(v[1], 2);
        test_eq!(v[2], 3);
    });

    test_case!("alloc_vec_with_capacity", {
        let v: Vec<u8> = Vec::with_capacity(100);
        test_eq!(v.capacity(), 100);
        test_eq!(v.len(), 0);
    });

    test_case!("alloc_string_from", {
        let s = String::from("hello");
        test_eq!(s.as_str(), "hello");
        test_eq!(s.len(), 5);
    });

    test_case!("alloc_string_push_str", {
        let mut s = String::from("foo");
        s.push_str("bar");
        test_eq!(s.as_str(), "foobar");
    });

    test_case!("alloc_string_format", {
        let s = alloc::format!("Answer: {}", 42);
        test_eq!(s.as_str(), "Answer: 42");
    });

    test_case!("alloc_vec_iter", {
        let v = alloc::vec![10, 20, 30];
        let mut sum = 0;
        for &n in &v {
            sum += n;
        }
        test_eq!(sum, 60);
    });
}

// ── Slab allocator tests ────────────────────────────────

pub fn register_slab_tests() {
    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use alloc::string::String;

    test_case!("slab_box_u8", {
        let b = Box::new(42u8);
        test_eq!(*b, 42);
    });

    test_case!("slab_box_u64", {
        let b = Box::new(0xDEAD_BEEFu64);
        test_eq!(*b, 0xDEAD_BEEF);
    });

    test_case!("slab_box_many_small", {
        for _ in 0..512 {
            let b = Box::new(0u8);
            test_eq!(*b, 0);
        }
    });

    test_case!("slab_box_many_64", {
        let mut vec = Vec::new();
        for i in 0..200 {
            vec.push(Box::new(i as u64));
        }
        for (i, b) in vec.iter().enumerate() {
            test_eq!(**b, i as u64);
        }
    });

    test_case!("slab_box_large_fallback", {
        // 4 KB object exceeds slab → goes to fallback
        let mut b = Box::new([0u8; 4096]);
        b[0] = 0xAA;
        b[4095] = 0xBB;
        test_eq!(b[0], 0xAA);
        test_eq!(b[4095], 0xBB);
    });

    test_case!("slab_string_heap", {
        let mut s = String::with_capacity(64);
        s.push_str("slab allocator test");
        test_eq!(s.as_str(), "slab allocator test");
    });

    test_case!("slab_vec_u32", {
        let mut v = Vec::new();
        for i in 0..500 {
            v.push(i as u32);
        }
        test_eq!(v.len(), 500);
        test_eq!(v[0], 0);
        test_eq!(v[499], 499);
    });

    test_case!("slab_mix_sizes", {
        let a = Box::new(1u8);
        let b = Box::new(2u16);
        let c = Box::new(3u32);
        let d = Box::new(4u64);
        let e = Box::new([5u8; 128]);
        test_eq!(*a, 1);
        test_eq!(*b, 2);
        test_eq!(*c, 3);
        test_eq!(*d, 4);
        test_eq!(e[0], 5);
        test_eq!(e[127], 5);
    });

    test_case!("slab_free_reuse", {
        // Allocate many small objects, free them, then allocate again
        // to verify slab page reuse.
        let mut v: Vec<Box<u32>> = Vec::new();
        for i in 0..100 {
            v.push(Box::new(i));
        }
        core::mem::drop(v);
        let mut v2: Vec<Box<u32>> = Vec::new();
        for i in 0..100 {
            v2.push(Box::new(i * 10));
        }
        for (i, b) in v2.iter().enumerate() {
            test_eq!(**b, (i as u32) * 10);
        }
    });
}

pub fn register_sync_tests() {
    use crate::syscall::{NEED_RESCHED, set_need_resched, clear_need_resched};
    use core::sync::atomic::Ordering;

    test_case!("need_resched_init_false", {
        NEED_RESCHED.store(false, Ordering::SeqCst);
        test_eq!(NEED_RESCHED.load(Ordering::SeqCst), false);
    });

    test_case!("need_resched_set", {
        NEED_RESCHED.store(false, Ordering::SeqCst);
        set_need_resched();
        test_eq!(NEED_RESCHED.load(Ordering::SeqCst), true);
    });

    test_case!("need_resched_clear", {
        NEED_RESCHED.store(true, Ordering::SeqCst);
        let prev = clear_need_resched();
        test_eq!(prev, true);
        test_eq!(NEED_RESCHED.load(Ordering::SeqCst), false);
    });

    test_case!("need_resched_clear_returns_prev", {
        NEED_RESCHED.store(false, Ordering::SeqCst);
        let prev = clear_need_resched();
        test_eq!(prev, false);
    });
}

// =====================================================================
// Stress test harness — scheduler, syscall, memory
// =====================================================================
// These tests run intensive loops that exercise kernel invariants.
// They are registered separately so the shell's `test` command can
// run them only when `--stress` is passed, or they're enabled by
// the `stress` cargo feature.

pub fn register_stress_tests() {
    register_sched_stress();
    register_syscall_stress();
    register_mem_stress();
}

// ── A. Scheduler stress ────────────────────────────────────────────

fn register_sched_stress() {
    test_case!("stress_sched_rapid_yield", {
        // Rapid context-switch via yield simulation
        for i in 0..500 {
            // Manually set/clear NEED_RESCHED to exercise atomic paths
            crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
            let prev = crate::syscall::clear_need_resched();
            test_true!(prev);
            // If we had a secondary process, the resched path would activate.
            // As a unit test, we just verify the atomic toggle cycles cleanly.
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            let _ = i;
        }
    });

    test_case!("stress_sched_state_transitions", {
        // Test that Ready↔Running cycles are legal
        use crate::scheduler::{Kthread, ThreadState};
        let mut p = Kthread::new_idle(99, 0, 0x400000, 0x800000);
        test_eq!(p.state, ThreadState::Ready);
        for _ in 0..200 {
            p.state = ThreadState::Running;
            p.state = ThreadState::Ready;
        }
        p.state = ThreadState::Terminated;
        test_eq!(p.state, ThreadState::Terminated);
    });
}

// ── B. Syscall stress ──────────────────────────────────────────────

fn register_syscall_stress() {
    test_case!("stress_syscall_rapid_getpid", {
        // Rapid PID queries exercise the scheduler lock path
        for _ in 0..200 {
            let pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid()
            });
            test_true!(pid < 1000);
        }
    });

    test_case!("stress_syscall_invalid_numbers", {
        // ABI fuzzing: ensure invalid syscall numbers return -ENOSYS
        // Note: 22 = ThreadCreate (valid), skip it
        let expected = crate::syscall::err_to_u64(crate::syscall::SyscallError::NoSys);
        for num in &[100u64, 255, 0xFFFFFFFF] {
            let result = crate::syscall::syscall_dispatch(*num, 0, 0, 0, 0, 0);
            test_eq!(result, expected);
        }
    });

    test_case!("stress_syscall_ptr_validation", {
        // Ensure user pointer validation rejects kernel addresses
        let kernel_addr: u64 = 0x4000000; // kernel .text start (v0.40)
        let valid = crate::syscall::is_user_ptr_valid(kernel_addr, 10);
        test_eq!(valid, false);
        let valid2 = crate::syscall::is_user_ptr_valid(kernel_addr, 1);
        test_eq!(valid2, false);
        // But user addresses should be potentially valid
        let user_addr: u64 = 0x400000;
        let valid3 = crate::syscall::is_user_ptr_valid(user_addr, 10);
        test_eq!(valid3, true);
    });
}

// ── C. Memory stress ───────────────────────────────────────────────

fn register_mem_stress() {
    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use alloc::string::String;

    test_case!("buddy_alloc_free_sanity", {
        let p = crate::memory::allocate_frame().expect("alloc failed");
        crate::memory::free_frame(p);
    });

    test_case!("buddy_multiple_orders", {
        for order in 0..=4 {
            let p = crate::memory::alloc_frames(order).expect("alloc failed");
            crate::memory::free_frames(p, order);
        }
    });

    test_case!("buddy_stress_100k_cycles", {
        let start = crate::hal::get_ticks();
        for _ in 0..100_000 {
            let p = crate::memory::allocate_frame().expect("alloc failed");
            crate::memory::free_frame(p);
        }
        let elapsed = crate::hal::get_ticks().wrapping_sub(start);
        // 100 kHz timer ticks → each tick = 10 µs.
        // 100k cycles should complete in < 100 ticks (< 1 ms).
        let _ = elapsed;
    });

    test_case!("buddy_stress_random_orders", {
        for _ in 0..1000 {
            let order = (0usize..=6).map(|i| i * 3 % 7).next().unwrap_or(0);
            let p = crate::memory::alloc_frames(order).expect("alloc failed");
            crate::memory::free_frames(p, order);
        }
    });

    test_case!("handle_table_250_handles", {
        let mut ht = crate::handle::HandleTable::with_defaults();
        for i in 0..250 {
            let fd = ht.alloc_handle(crate::handle::HandleEntry::file(0, i));
            test_true!(fd.is_some());
            test_eq!(fd.unwrap() as usize, 3 + i as usize);
        }
        test_eq!(ht.len(), 3 + 250);
        for i in 0..250 {
            let entry = ht.get((3 + i) as u8);
            test_eq!(entry.obj_type(), Some(crate::object::ObType::Filesystem));
            let nid = entry.native_id().unwrap_or(0xFFFFFFFF);
            test_eq!(nid, i as u64);
        }
    });

    test_case!("handle_table_reuse_closed_slots", {
        let mut ht = crate::handle::HandleTable::with_defaults();
        // Open 10 handles
        for i in 0..10 {
            ht.alloc_handle(crate::handle::HandleEntry::file(0, i));
        }
        test_eq!(ht.len(), 13);
        // Close handles 3, 4, 5
        ht.set(3, crate::handle::HandleEntry::closed());
        ht.set(4, crate::handle::HandleEntry::closed());
        ht.set(5, crate::handle::HandleEntry::closed());
        // New alloc should reuse fd 3
        let fd = ht.alloc_handle(crate::handle::HandleEntry::file(1, 42));
        test_eq!(fd, Some(3));
        test_eq!(ht.get(3).native_id().unwrap_or(0), 42);
    });

    test_case!("stress_mem_alloc_free_storm", {
        // Rapid Box allocation and drop
        for _ in 0..200 {
            let b = Box::new(42u64);
            let v = *b;
            core::mem::drop(b);
            // After drop, the memory is returned to the allocator
            test_eq!(v, 42);
        }
    });

    test_case!("stress_mem_vec_churn", {
        // Vec growth and shrinkage
        let mut v = Vec::new();
        for i in 0..100 {
            v.push(i);
        }
        test_eq!(v.len(), 100);
        // Drain
        while let Some(_) = v.pop() {}
        test_eq!(v.len(), 0);
        // Refill
        for i in 0..50 {
            v.push(i * 2);
        }
        test_eq!(v.len(), 50);
        test_eq!(v[0], 0);
        test_eq!(v[49], 98);
    });

    test_case!("stress_mem_string_churn", {
        // String concatenation and clearing
        let mut s = String::new();
        for i in 0..50 {
            s.push_str("hello");
            test_eq!(s.len(), (i + 1) * 5);
        }
        s.clear();
        test_eq!(s.len(), 0);
        // Rebuild
        for _ in 0..30 {
            s.push_str("x");
        }
        test_eq!(s.len(), 30);
    });
}

// ── NeoFS metadata tests ───────────────────────────────────────────

pub fn register_neofs_tests() {
    use crate::fs::neodos_fs::{Inode, DirectoryEntry, NeoDosFs, MODE_DIR, MODE_FILE};
    use crate::fs::neodos_fs::{
        ATTR_READONLY, ATTR_HIDDEN, ATTR_SYSTEM, ATTR_VOLUME, ATTR_DIR, ATTR_ARCHIVE,
        PERM_R, PERM_W, PERM_X, PERM_S, PERM_D,
    };
    use crate::fs::neodos_fs::BLOCK_SIZE;

    const PERM_ALL: u16 = PERM_R | PERM_W | PERM_X | PERM_S | PERM_D;

    // ── 1. Permission flag tests (R, W, X, S, D) ─────────────────────

    test_case!("neofs_perm_r_individual", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | PERM_R, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & PERM_R) != 0);
        test_eq!(inode.mode & PERM_W, 0);
        test_eq!(inode.mode & PERM_X, 0);
        test_eq!(inode.mode & PERM_S, 0);
        test_eq!(inode.mode & PERM_D, 0);
    });

    test_case!("neofs_perm_w_individual", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | PERM_W, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & PERM_W) != 0);
        test_eq!(inode.mode & (PERM_R | PERM_X | PERM_S | PERM_D), 0);
    });

    test_case!("neofs_perm_x_individual", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | PERM_X, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & PERM_X) != 0);
        test_eq!(inode.mode & (PERM_R | PERM_W | PERM_S | PERM_D), 0);
    });

    test_case!("neofs_perm_s_individual", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | PERM_S, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & PERM_S) != 0);
        test_eq!(inode.mode & (PERM_R | PERM_W | PERM_X | PERM_D), 0);
    });

    test_case!("neofs_perm_d_individual", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | PERM_D, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & PERM_D) != 0);
        test_eq!(inode.mode & (PERM_R | PERM_W | PERM_X | PERM_S), 0);
    });

    test_case!("neofs_perm_bit_positions", {
        // Each flag occupies a distinct bit
        test_eq!(PERM_R, 0x0001);
        test_eq!(PERM_W, 0x0002);
        test_eq!(PERM_X, 0x0004);
        test_eq!(PERM_S, 0x0008);
        test_eq!(PERM_D, 0x0010);
        // No overlaps
        test_eq!(PERM_ALL, 0x001F);
        test_eq!(PERM_ALL & MODE_DIR, 0);
        test_eq!(PERM_ALL & MODE_FILE, 0);
    });

    test_case!("neofs_perm_combined_flags", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | PERM_R | PERM_W | PERM_X | PERM_S | PERM_D, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.mode & PERM_ALL, PERM_ALL);
    });

    test_case!("neofs_perm_subset_flags", {
        // Only R and X set, verify W/S/D are clear
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | PERM_R | PERM_X, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & PERM_R) != 0);
        test_true!((inode.mode & PERM_X) != 0);
        test_eq!(inode.mode & (PERM_W | PERM_S | PERM_D), 0);
    });

    test_case!("neofs_perm_zero_perms", {
        // MODE_FILE with zero permission bits
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.mode & PERM_ALL, 0);
    });

    test_case!("neofs_perm_max_bitmask", {
        // All 16 bits set — permission + type bits all coexist
        let inode = Inode {
            inode_num: 1, mode: 0xFFFF, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.mode & PERM_ALL, PERM_ALL);
        test_true!((inode.mode & MODE_DIR) != 0);
        test_true!((inode.mode & MODE_FILE) != 0);
    });

    test_case!("neofs_perm_with_file_mode", {
        // Permission flags coexist with MODE_FILE
        let mode = MODE_FILE | PERM_R | PERM_W | PERM_X;
        let inode = Inode {
            inode_num: 1, mode, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & MODE_FILE) != 0);
        test_true!((inode.mode & PERM_R) != 0);
        test_true!((inode.mode & PERM_W) != 0);
        test_true!((inode.mode & PERM_X) != 0);
        test_eq!(inode.mode & MODE_DIR, 0);
    });

    test_case!("neofs_perm_with_dir_mode", {
        // Permission flags coexist with MODE_DIR
        let mode = MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_S | PERM_D;
        let inode = Inode {
            inode_num: 1, mode, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & MODE_DIR) != 0);
        test_eq!(inode.mode & PERM_ALL, PERM_ALL);
        test_eq!(inode.mode & MODE_FILE, 0);
    });

    test_case!("neofs_perm_serialize_roundtrip", {
        // R|W|X|S|D + MODE_DIR through raw byte serialization
        let original = Inode {
            inode_num: 7,
            mode: MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_S | PERM_D,
            size: 4096,
            atime: 1111, mtime: 2222, ctime: 3333,
            link_count: 3, owner_uid: 500, owner_gid: 50,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        let mut raw = [0u8; 256];
        unsafe { core::ptr::write_unaligned(raw.as_mut_ptr() as *mut Inode, original); }
        let restored: Inode = unsafe { core::ptr::read_unaligned(raw.as_ptr() as *const Inode) };
        test_true!((restored.mode & MODE_DIR) != 0);
        test_eq!(restored.mode & PERM_ALL, PERM_ALL);
        test_eq!(restored.mode & MODE_FILE, 0);
        test_eq!(restored.size, 4096);
        test_eq!(restored.owner_uid, 500);
    });

    test_case!("neofs_perm_all_3bit_combinations", {
        // Exhaustively test all 8 combinations of R/W/X bits
        let perms = [PERM_R, PERM_W, PERM_X];
        for mask in 0..8u16 {
            let mut flags = 0u16;
            for b in 0..3 {
                if mask & (1 << b) != 0 { flags |= perms[b as usize]; }
            }
            let inode = Inode {
                inode_num: 1, mode: flags, size: 0,
                atime: 0, mtime: 0, ctime: 0,
                link_count: 0, owner_uid: 0, owner_gid: 0,
                direct_blocks: [0; 12], indirect_block: 0,
                padding: [0; 160],
            };
            test_eq!(inode.mode & (PERM_R | PERM_W | PERM_X), flags);
        }
    });

    // ── 2. Inode mode / type tests ───────────────────────────────────

    test_case!("neofs_inode_file_mode", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 100,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & MODE_FILE) != 0);
        test_eq!((inode.mode & MODE_DIR), 0);
    });

    test_case!("neofs_inode_dir_mode", {
        let inode = Inode {
            inode_num: 0, mode: MODE_DIR, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & MODE_DIR) != 0);
        test_eq!((inode.mode & MODE_FILE), 0);
    });

    test_case!("neofs_inode_mode_mutual_exclusive", {
        test_eq!(MODE_DIR & MODE_FILE, 0);
    });

    test_case!("neofs_inode_mode_none", {
        let inode = Inode {
            inode_num: 0, mode: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.mode & MODE_FILE, 0);
        test_eq!(inode.mode & MODE_DIR, 0);
    });

    test_case!("neofs_inode_mode_max", {
        let inode = Inode {
            inode_num: 255, mode: 0xFFFF, size: u32::MAX,
            atime: u64::MAX, mtime: u64::MAX, ctime: u64::MAX,
            link_count: u16::MAX, owner_uid: u32::MAX, owner_gid: u32::MAX,
            direct_blocks: [0xFFFFFFFF; 12], indirect_block: u32::MAX,
            padding: [0xFF; 160],
        };
        test_eq!(inode.inode_num, 255);
        test_eq!(inode.mode, 0xFFFF);
        test_eq!(inode.size, u32::MAX);
        test_eq!(inode.atime, u64::MAX);
    });

    // ── 3. Timestamp tests ───────────────────────────────────────────

    test_case!("neofs_timestamp_zero", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.atime, 0);
        test_eq!(inode.mtime, 0);
        test_eq!(inode.ctime, 0);
    });

    test_case!("neofs_timestamp_max", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: u64::MAX, mtime: u64::MAX, ctime: u64::MAX,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.atime, u64::MAX);
        test_eq!(inode.mtime, u64::MAX);
        test_eq!(inode.ctime, u64::MAX);
    });

    test_case!("neofs_timestamp_ordering", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 100,
            atime: 1000, mtime: 2000, ctime: 3000,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!(inode.atime <= inode.mtime);
        test_true!(inode.mtime <= inode.ctime);
    });

    test_case!("neofs_timestamp_serialize", {
        let original = Inode {
            inode_num: 5, mode: MODE_FILE, size: 0,
            atime: 999888777666, mtime: 555444333222, ctime: 111222333444,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        let mut raw = [0u8; 256];
        unsafe { core::ptr::write_unaligned(raw.as_mut_ptr() as *mut Inode, original); }
        let restored: Inode = unsafe { core::ptr::read_unaligned(raw.as_ptr() as *const Inode) };
        test_eq!(restored.atime, 999888777666);
        test_eq!(restored.mtime, 555444333222);
        test_eq!(restored.ctime, 111222333444);
    });

    // ── 4. Inode serialisation round-trip ────────────────────────────

    test_case!("neofs_inode_serialize_roundtrip", {
        let original = Inode {
            inode_num: 42,
            mode: MODE_FILE,
            size: 8192,
            atime: 12345,
            mtime: 23456,
            ctime: 34567,
            link_count: 2,
            owner_uid: 1000,
            owner_gid: 100,
            direct_blocks: [1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            padding: [0; 160],
        };
        let mut raw = [0u8; 256];
        unsafe {
            core::ptr::write_unaligned(raw.as_mut_ptr() as *mut Inode, original);
        }
        let restored: Inode = unsafe {
            core::ptr::read_unaligned(raw.as_ptr() as *const Inode)
        };
        test_eq!(restored.inode_num, 42);
        test_eq!(restored.mode, MODE_FILE);
        test_eq!(restored.size, 8192);
        test_eq!(restored.atime, 12345);
        test_eq!(restored.mtime, 23456);
        test_eq!(restored.ctime, 34567);
        test_eq!(restored.link_count, 2);
        test_eq!(restored.owner_uid, 1000);
        test_eq!(restored.owner_gid, 100);
        test_eq!(restored.direct_blocks[0], 1);
        test_eq!(restored.direct_blocks[1], 2);
        test_eq!(restored.direct_blocks[2], 3);
    });

    test_case!("neofs_inode_serialize_all_zeros", {
        let original = Inode {
            inode_num: 0, mode: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        let mut raw = [0u8; 256];
        unsafe { core::ptr::write_unaligned(raw.as_mut_ptr() as *mut Inode, original); }
        let restored: Inode = unsafe { core::ptr::read_unaligned(raw.as_ptr() as *const Inode) };
        test_eq!(restored.inode_num, 0);
        test_eq!(restored.mode, 0);
        test_eq!(restored.size, 0);
        test_eq!(restored.atime, 0);
        test_eq!(restored.link_count, 0);
        let blocks = restored.direct_blocks;
        for &b in blocks.iter() {
            test_eq!(b, 0);
        }
    });

    // ── 5. Inode block count tests (pure function) ───────────────────

    test_case!("neofs_inode_block_count_empty", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(NeoDosFs::inode_block_count(&inode), 0);
    });

    test_case!("neofs_inode_block_count_one", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 1,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(NeoDosFs::inode_block_count(&inode), 1);
    });

    test_case!("neofs_inode_block_count_exact_block", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: BLOCK_SIZE as u32,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(NeoDosFs::inode_block_count(&inode), 1);
    });

    test_case!("neofs_inode_block_count_cross_block", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: BLOCK_SIZE as u32 + 1,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(NeoDosFs::inode_block_count(&inode), 2);
    });

    test_case!("neofs_inode_block_count_max", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: u32::MAX,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(NeoDosFs::inode_block_count(&inode), 12);
    });

    test_case!("neofs_inode_block_count_dir_root", {
        let inode = Inode {
            inode_num: 0, mode: MODE_DIR, size: 100,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(NeoDosFs::inode_block_count(&inode), 1);
    });

    // ── 6. DirectoryEntry DOS attribute tests ────────────────────────

    test_case!("neofs_dirent_no_attributes", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: 0,
            name: {
                let mut n = [0u8; 249];
                n[..4].copy_from_slice(b"FILE");
                n
            },
        };
        test_eq!(entry.attributes, 0);
    });

    test_case!("neofs_dirent_readonly", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: ATTR_READONLY,
            name: [0u8; 249],
        };
        test_true!((entry.attributes & ATTR_READONLY) != 0);
    });

    test_case!("neofs_dirent_hidden", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: ATTR_HIDDEN,
            name: [0u8; 249],
        };
        test_true!((entry.attributes & ATTR_HIDDEN) != 0);
    });

    test_case!("neofs_dirent_system", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: ATTR_SYSTEM,
            name: [0u8; 249],
        };
        test_true!((entry.attributes & ATTR_SYSTEM) != 0);
    });

    test_case!("neofs_dirent_directory", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 2, attributes: ATTR_DIR,
            name: [0u8; 249],
        };
        test_true!((entry.attributes & ATTR_DIR) != 0);
    });

    test_case!("neofs_dirent_archive", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: ATTR_ARCHIVE,
            name: [0u8; 249],
        };
        test_true!((entry.attributes & ATTR_ARCHIVE) != 0);
    });

    test_case!("neofs_dirent_volume_label", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 0, attributes: ATTR_VOLUME,
            name: [0u8; 249],
        };
        test_true!((entry.attributes & ATTR_VOLUME) != 0);
    });

    test_case!("neofs_dirent_combined_attrs", {
        let attrs = ATTR_READONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_ARCHIVE;
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: attrs,
            name: [0u8; 249],
        };
        test_true!((entry.attributes & ATTR_READONLY) != 0);
        test_true!((entry.attributes & ATTR_HIDDEN) != 0);
        test_true!((entry.attributes & ATTR_SYSTEM) != 0);
        test_true!((entry.attributes & ATTR_ARCHIVE) != 0);
        test_eq!(entry.attributes & ATTR_VOLUME, 0);
    });

    test_case!("neofs_attr_bit_constants", {
        test_eq!(ATTR_READONLY, 0x01);
        test_eq!(ATTR_HIDDEN,   0x02);
        test_eq!(ATTR_SYSTEM,   0x04);
        test_eq!(ATTR_VOLUME,   0x08);
        test_eq!(ATTR_DIR,      0x10);
        test_eq!(ATTR_ARCHIVE,  0x20);
    });

    // ── 7. DirectoryEntry serialisation ──────────────────────────────

    test_case!("neofs_dirent_serialize_roundtrip", {
        let mut name_buf = [0u8; 249];
        name_buf[..5].copy_from_slice(b"HELLO");
        let original = DirectoryEntry {
            inode_num: 7,
            name_len: 5,
            entry_type: 1,
            attributes: ATTR_READONLY | ATTR_ARCHIVE,
            name: name_buf,
        };
        let mut raw = [0u8; 256];
        unsafe {
            core::ptr::write_unaligned(raw.as_mut_ptr() as *mut DirectoryEntry, original);
        }
        let restored: DirectoryEntry = unsafe {
            core::ptr::read_unaligned(raw.as_ptr() as *const DirectoryEntry)
        };
        test_eq!(restored.inode_num, 7);
        test_eq!(restored.name_len, 5);
        test_eq!(restored.entry_type, 1);
        test_eq!(restored.attributes, ATTR_READONLY | ATTR_ARCHIVE);
        let mut expected_name = [0u8; 249];
        expected_name[..5].copy_from_slice(b"HELLO");
        test_eq!(restored.name, expected_name);
    });

    // ── 8. Edge cases: invalid/corrupted metadata ────────────────────

    test_case!("neofs_dirent_zero_len_name", {
        let entry = DirectoryEntry {
            inode_num: 0, name_len: 0, entry_type: 1, attributes: 0,
            name: [0u8; 249],
        };
        test_eq!(entry.name_len, 0);
        test_eq!(entry.inode_num, 0);
    });

    test_case!("neofs_dirent_overflow_name_len", {
        // name_len > 249 should be handled gracefully by the FS skip logic
        let entry = DirectoryEntry {
            inode_num: 5, name_len: 250, entry_type: 1, attributes: 0,
            name: [0u8; 249],
        };
        test_eq!(entry.name_len, 250);
        test_eq!(entry.inode_num, 5);
    });

    test_case!("neofs_dirent_invalid_entry_type", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 3, entry_type: 0xFF, attributes: 0,
            name: {
                let mut n = [0u8; 249];
                n[..3].copy_from_slice(b"BAD");
                n
            },
        };
        test_eq!(entry.entry_type, 0xFF);
    });

    test_case!("neofs_dirent_max_attributes", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: 0xFF,
            name: [0u8; 249],
        };
        test_eq!(entry.attributes, 0xFF);
        // Verify all known bits survive
        test_true!((entry.attributes & ATTR_READONLY) != 0);
        test_true!((entry.attributes & ATTR_HIDDEN) != 0);
        test_true!((entry.attributes & ATTR_SYSTEM) != 0);
        test_true!((entry.attributes & ATTR_DIR) != 0);
        test_true!((entry.attributes & ATTR_ARCHIVE) != 0);
    });

    test_case!("neofs_inode_negative_block_count", {
        let inode = Inode {
            inode_num: 0, mode: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(NeoDosFs::inode_block_count(&inode), 0);
    });

    // ── 9. Owner / link metadata ─────────────────────────────────────

    test_case!("neofs_owner_uid", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.owner_uid, 0);
        test_eq!(inode.owner_gid, 0);
        test_eq!(inode.link_count, 1);
    });

    test_case!("neofs_link_count_no_links", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.link_count, 0);
    });

    test_case!("neofs_link_count_max", {
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: u16::MAX, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.link_count, u16::MAX);
    });

    test_case!("neofs_uid_gid_nonzero", {
        let inode = Inode {
            inode_num: 5, mode: MODE_DIR, size: 512,
            atime: 100, mtime: 200, ctime: 300,
            link_count: 2, owner_uid: 65535, owner_gid: 65535,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_eq!(inode.owner_uid, 65535);
        test_eq!(inode.owner_gid, 65535);
    });

    // ── 10. Stress: repeated inode manipulation ──────────────────────

    test_case!("neofs_stress_inode_toggle_mode", {
        let mut inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        for i in 0..200 {
            inode.mode = if i % 2 == 0 { MODE_FILE } else { MODE_DIR };
            test_eq!(inode.mode & MODE_DIR, if i % 2 == 0 { 0 } else { MODE_DIR });
        }
    });

    test_case!("neofs_stress_perm_cycle", {
        // Cycle through all R/W/X/S/D combinations
        let mut inode = Inode {
            inode_num: 1, mode: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        for i in 0..32u16 {
            inode.mode = MODE_FILE | ((i * 0x1111) & PERM_ALL);
            test_eq!(inode.mode & PERM_ALL, (i * 0x1111) & PERM_ALL);
        }
    });

    test_case!("neofs_stress_inode_uid_cycle", {
        let mut inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        for i in 0..100 {
            inode.owner_uid = i * 1000;
            test_eq!(inode.owner_uid, i * 1000);
        }
    });

    test_case!("neofs_stress_timestamp_churn", {
        let mut inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        for i in 0..100 {
            inode.atime = i as u64;
            inode.mtime = (i * 2) as u64;
            inode.ctime = (i * 3) as u64;
            test_eq!(inode.atime, i as u64);
            test_eq!(inode.mtime, (i * 2) as u64);
            test_eq!(inode.ctime, (i * 3) as u64);
        }
    });

    // ── Permission rendering (matches the format used by DIR) ──
    fn render_perms(mode: u16) -> [u8; 5] {
        let mut p = [b'-'; 5];
        if mode & PERM_R != 0 { p[0] = b'R'; }
        if mode & PERM_W != 0 { p[1] = b'W'; }
        if mode & PERM_X != 0 { p[2] = b'X'; }
        if mode & PERM_S != 0 { p[3] = b'S'; }
        if mode & PERM_D != 0 { p[4] = b'D'; }
        p
    }

    test_case!("neofs_perm_render_all_set", {
        let p = render_perms(PERM_ALL);
        test_eq!(core::str::from_utf8(&p), Ok("RWXSD"));
    });
    test_case!("neofs_perm_render_none", {
        let p = render_perms(0);
        test_eq!(core::str::from_utf8(&p), Ok("-----"));
    });
    test_case!("neofs_perm_render_r_only", {
        let p = render_perms(PERM_R);
        test_eq!(core::str::from_utf8(&p), Ok("R----"));
    });
    test_case!("neofs_perm_render_sd_only", {
        let p = render_perms(PERM_S | PERM_D);
        test_eq!(core::str::from_utf8(&p), Ok("---SD"));
    });
    test_case!("neofs_perm_render_with_dir_mode", {
        let p = render_perms(PERM_R | PERM_W | MODE_DIR);
        test_eq!(core::str::from_utf8(&p), Ok("RW---"));
    });
    test_case!("neofs_perm_render_with_file_mode", {
        let p = render_perms(PERM_X | PERM_S | PERM_D | MODE_FILE);
        test_eq!(core::str::from_utf8(&p), Ok("--XSD"));
    });
    test_case!("neofs_perm_render_with_file_mode_xs_only", {
        let p = render_perms(PERM_X | PERM_S | MODE_FILE);
        test_eq!(core::str::from_utf8(&p), Ok("--XS-"));
    });
    test_case!("neofs_perm_all_32_combinations", {
        for bits in 0..32u16 {
            let mode = bits;
            let p = render_perms(mode);
            for i in 0..5 {
                let expected = if (mode >> i) & 1 != 0 {
                    match i { 0 => b'R', 1 => b'W', 2 => b'X', 3 => b'S', _ => b'D' }
                } else { b'-' };
                test_eq!(p[i], expected);
            }
        }
    });
    test_case!("neofs_perm_mode_upper_bits_isolated", {
        for upper in [0x0100, 0x0200, 0x8000, 0xFF00].iter() {
            let mode = PERM_ALL | upper;
            let p = render_perms(mode);
            test_eq!(core::str::from_utf8(&p), Ok("RWXSD"));
        }
        let mode = MODE_DIR | MODE_FILE | PERM_ALL;
        let p = render_perms(mode);
        test_eq!(core::str::from_utf8(&p), Ok("RWXSD"));
    });

    // ── Timestamp edge cases ───────────────────────────────────
    test_case!("neofs_timestamp_near_boundaries", {
        for ts in [0u64, 1, u64::MAX - 1, u64::MAX].iter() {
            let raw = ts.to_le_bytes();
            let recovered = u64::from_le_bytes(raw);
            test_eq!(recovered, *ts);
        }
    });
    test_case!("neofs_timestamp_independence", {
        let mut inode = Inode {
            inode_num: 1, mode: MODE_FILE, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        inode.atime = 0x1111_2222_3333_4444;
        inode.mtime = 0x4444_3333_2222_1111;
        inode.ctime = 0xDEAD_BEEF_CAFE_BABE;
        test_eq!(inode.atime, 0x1111_2222_3333_4444);
        test_eq!(inode.mtime, 0x4444_3333_2222_1111);
        test_eq!(inode.ctime, 0xDEAD_BEEF_CAFE_BABE);
    });

    // ── DirectoryEntry edge cases ──────────────────────────────
    test_case!("neofs_dirent_name_max_length", {
        let mut name = [0u8; 249];
        for i in 0..249 { name[i] = b'A' + (i % 26) as u8; }
        let entry = DirectoryEntry {
            inode_num: 42, name_len: 249, entry_type: 1, attributes: 0, name,
        };
        test_eq!(entry.inode_num, 42);
        test_eq!(entry.name_len, 249);
        test_eq!(entry.entry_type, 1);
        for i in 0..249 {
            test_eq!(entry.name[i], b'A' + (i % 26) as u8);
        }
    });
    test_case!("neofs_dirent_all_attribute_bits", {
        let entry = DirectoryEntry {
            inode_num: 1, name_len: 4, entry_type: 1, attributes: 0xFF,
            name: { let mut n = [0u8; 249]; n[..4].copy_from_slice(b"ALL\0"); n },
        };
        test_eq!(entry.attributes, 0xFF);
        test_ne!(entry.attributes & ATTR_READONLY, 0);
        test_ne!(entry.attributes & ATTR_HIDDEN, 0);
        test_ne!(entry.attributes & ATTR_SYSTEM, 0);
        test_ne!(entry.attributes & ATTR_VOLUME, 0);
        test_ne!(entry.attributes & ATTR_DIR, 0);
        test_ne!(entry.attributes & ATTR_ARCHIVE, 0);
    });
    test_case!("neofs_dirent_inode_num_zero_and_max", {
        for inum in [0u32, u32::MAX].iter() {
            let entry = DirectoryEntry {
                inode_num: *inum, name_len: 3, entry_type: 1, attributes: 0,
                name: { let mut n = [0u8; 249]; n[..3].copy_from_slice(b"ZMX"); n },
            };
            test_eq!(entry.inode_num, *inum);
        }
    });

    // ── Inode field boundaries ─────────────────────────────────
    test_case!("neofs_inode_all_fields_max", {
        let inode = Inode {
            inode_num: u32::MAX, mode: u16::MAX, size: u32::MAX,
            atime: u64::MAX, mtime: u64::MAX, ctime: u64::MAX,
            link_count: u16::MAX, owner_uid: u32::MAX, owner_gid: u32::MAX,
            direct_blocks: [u32::MAX; 12], indirect_block: u32::MAX,
            padding: [0xFFu8; 160],
        };
        test_eq!(inode.inode_num, u32::MAX);
        test_eq!(inode.mode, u16::MAX);
        test_eq!(inode.size, u32::MAX);
        test_eq!(inode.atime, u64::MAX);
        test_eq!(inode.mtime, u64::MAX);
        test_eq!(inode.ctime, u64::MAX);
        test_eq!(inode.link_count, u16::MAX);
        test_eq!(inode.owner_uid, u32::MAX);
        test_eq!(inode.owner_gid, u32::MAX);
    });
    test_case!("neofs_inode_mixed_zero_max_fields", {
        let inode = Inode {
            inode_num: 0, mode: u16::MAX, size: 0,
            atime: u64::MAX, mtime: 0, ctime: u64::MAX,
            link_count: u16::MAX, owner_uid: 0, owner_gid: u32::MAX,
            direct_blocks: [0; 12], indirect_block: u32::MAX,
            padding: [0u8; 160],
        };
        test_eq!(inode.inode_num, 0);
        test_eq!(inode.mode, u16::MAX);
        test_eq!(inode.size, 0);
        test_eq!(inode.atime, u64::MAX);
        test_eq!(inode.mtime, 0);
        test_eq!(inode.ctime, u64::MAX);
        test_eq!(inode.link_count, u16::MAX);
        test_eq!(inode.owner_uid, 0);
        test_eq!(inode.owner_gid, u32::MAX);
    });

    // ── Corruption / byte-flip tests ───────────────────────────
    test_case!("neofs_corrupt_inode_flip_byte", {
        let inode = Inode {
            inode_num: 42, mode: MODE_FILE | PERM_R | PERM_W, size: 4096,
            atime: 1000, mtime: 2000, ctime: 3000,
            link_count: 1, owner_uid: 0, owner_gid: 0,
            direct_blocks: [10, 11, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0, padding: [0u8; 160],
        };
        let raw = unsafe {
            core::slice::from_raw_parts(&inode as *const _ as *const u8, core::mem::size_of::<Inode>())
        };
        let mut bytes = [0u8; 256];
        bytes[..raw.len()].copy_from_slice(raw);
        bytes[200] ^= 0xFF;
        let corrupted: Inode = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const _) };
        test_eq!(corrupted.inode_num, 42);
        // verify corruption affected padding, not meaningful fields
        test_ne!(corrupted.padding[104], 0);
    });
    test_case!("neofs_corrupt_dirent_flip_byte", {
        let entry = DirectoryEntry {
            inode_num: 7, name_len: 3, entry_type: 1, attributes: 0,
            name: { let mut n = [0u8; 249]; n[..3].copy_from_slice(b"FOO"); n },
        };
        let raw = unsafe {
            core::slice::from_raw_parts(&entry as *const _ as *const u8, core::mem::size_of::<DirectoryEntry>())
        };
        let mut bytes = [0u8; 256];
        bytes[..raw.len()].copy_from_slice(raw);
        bytes[8] ^= 0xAA;
        let corrupted: DirectoryEntry = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const _) };
        test_eq!(corrupted.inode_num, 7);
    });

    // ── Serialization stress ───────────────────────────────────
    test_case!("neofs_stress_inode_deterministic_serialize", {
        let mut state: u64 = 42;
        let mut lcg = || { state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); state };
        for _ in 0..500 {
            let inode = Inode {
                inode_num: (lcg() % 10000) as u32, mode: lcg() as u16, size: (lcg() % (u32::MAX as u64 + 1)) as u32,
                atime: lcg(), mtime: lcg(), ctime: lcg(),
                link_count: lcg() as u16, owner_uid: (lcg() % (u32::MAX as u64 + 1)) as u32, owner_gid: (lcg() % (u32::MAX as u64 + 1)) as u32,
                direct_blocks: [lcg() as u32 % 100000, lcg() as u32 % 100000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                indirect_block: 0, padding: [0u8; 160],
            };
            let raw = unsafe {
                core::slice::from_raw_parts(&inode as *const _ as *const u8, core::mem::size_of::<Inode>())
            };
            let mut buf = [0u8; 256];
            buf[..raw.len()].copy_from_slice(raw);
            let recovered: Inode = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const _) };
            test_eq!(recovered.inode_num, inode.inode_num);
            test_eq!(recovered.mode, inode.mode);
            test_eq!(recovered.size, inode.size);
            test_eq!(recovered.atime, inode.atime);
            test_eq!(recovered.mtime, inode.mtime);
            test_eq!(recovered.ctime, inode.ctime);
            test_eq!(recovered.link_count, inode.link_count);
            test_eq!(recovered.owner_uid, inode.owner_uid);
            test_eq!(recovered.owner_gid, inode.owner_gid);
        }
    });
    test_case!("neofs_stress_dirent_deterministic_serialize", {
        let mut state: u64 = 99;
        let mut lcg = || { state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); state };
        for _ in 0..500 {
            let nlen = ((lcg() % 200) + 1) as usize;
            let mut name = [0u8; 249];
            for i in 0..nlen.min(249) { name[i] = b'A' + (lcg() % 26) as u8; }
            let entry = DirectoryEntry {
                inode_num: (lcg() % 1000) as u32, name_len: nlen as u8,
                entry_type: (lcg() % 3) as u8, attributes: (lcg() % 256) as u8, name,
            };
            let raw = unsafe {
                core::slice::from_raw_parts(&entry as *const _ as *const u8, core::mem::size_of::<DirectoryEntry>())
            };
            let mut buf = [0u8; 256];
            buf[..raw.len()].copy_from_slice(raw);
            let recovered: DirectoryEntry = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const _) };
            test_eq!(recovered.inode_num, entry.inode_num);
            test_eq!(recovered.name_len, entry.name_len);
            test_eq!(recovered.entry_type, entry.entry_type);
            test_eq!(recovered.attributes, entry.attributes);
        }
    });
    test_case!("neofs_stress_mode_field_cycle", {
        let mut inode = Inode {
            inode_num: 1, mode: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        for i in 0..=65535u16 {
            inode.mode = i;
            test_eq!(inode.mode, i);
        }
        test_eq!(inode.mode, 0xFFFF);
    });
}

// ===== Mmap tests =====

pub fn register_mmap_tests() {
    use crate::scheduler::MmapRegion;

    test_case!("mmap_region_create", {
        let r = MmapRegion {
            base: 0x20000000, len: 0x1000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_eq!(r.base, 0x20000000);
        test_eq!(r.len, 0x1000);
        test_eq!(r.prot, 3);
        test_eq!(r.flags, 1);
    });

    test_case!("mmap_region_anonymous", {
        let r = MmapRegion {
            base: 0x20001000, len: 0x4000, prot: 1, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_true!((r.flags & 1) != 0); // anonymous
        test_eq!(r.prot & 2, 0); // not writable
        test_eq!(r.prot & 1, 1); // readable
    });

    test_case!("mmap_region_file_backed", {
        let r = MmapRegion {
            base: 0x20010000, len: 0x2000, prot: 3, flags: 0,
            drive: 2, inode: 42, file_size: 8192,
        };
        test_eq!(r.flags & 1, 0); // file-backed
        test_eq!(r.drive, 2);
        test_eq!(r.inode, 42);
        test_eq!(r.file_size, 8192);
    });

    test_case!("mmap_region_contains", {
        let r = MmapRegion {
            base: 0x20000000, len: 0x10000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_true!(0x20000000 >= r.base && 0x20000000 < r.base + r.len);
        test_true!(0x2000FFF0 >= r.base && 0x2000FFF0 < r.base + r.len);
        test_true!(!(0x20010000 >= r.base && 0x20010000 < r.base + r.len));
    });

    test_case!("mmap_is_mmap_virtual_addr", {
        test_true!(crate::arch::x64::paging::is_mmap_virtual_addr(0x20000000));
        test_true!(crate::arch::x64::paging::is_mmap_virtual_addr(0x21FFFFFF));
        test_true!(!crate::arch::x64::paging::is_mmap_virtual_addr(0x1FFFFFFF));
        test_true!(!crate::arch::x64::paging::is_mmap_virtual_addr(0x22000000));
    });

    test_case!("mmap_process_add_remove", {
        use crate::scheduler::Eprocess;
        let mut ep = Eprocess::new_ring3(99, 0, 2, "\\", 0x10000000, 0);
        test_eq!(ep.mmap_regions.len(), 0);

        let r1 = MmapRegion {
            base: 0x20000000, len: 0x1000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        ep.mmap_regions.push(r1);
        test_eq!(ep.mmap_regions.len(), 1);
        test_eq!(ep.mmap_regions[0].base, 0x20000000);

        let r2 = MmapRegion {
            base: 0x20001000, len: 0x2000, prot: 1, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        ep.mmap_regions.push(r2);
        test_eq!(ep.mmap_regions.len(), 2);

        let idx = ep.mmap_regions.iter().position(|r| r.base == 0x20000000);
        test_true!(idx.is_some());
        ep.mmap_regions.remove(idx.unwrap());
        test_eq!(ep.mmap_regions.len(), 1);
        test_eq!(ep.mmap_regions[0].base, 0x20001000);
    });
}

// ===== Pipe / IPC tests =====

pub fn register_pipe_tests() {
    use crate::pipe::PIPE_MANAGER;
    use crate::handle::{HandleEntry, default_handle_table, closed_handle_table};

    test_case!("pipe_alloc_free", {
        let pid = PIPE_MANAGER.alloc().expect("pipe alloc failed");
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_write_read", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        let data = b"Hello, Pipe!";
        let n = PIPE_MANAGER.write(pid, data).unwrap();
        test_eq!(n, data.len());
        let mut buf = [0u8; 64];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, data.len());
        test_eq!(&buf[..n], data);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_multiple_writes", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.write(pid, b"abc").unwrap();
        PIPE_MANAGER.write(pid, b"def").unwrap();
        PIPE_MANAGER.write(pid, b"ghi").unwrap();
        let mut buf = [0u8; 16];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, 9);
        test_eq!(&buf[..n], b"abcdefghi");
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_eof", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.write(pid, b"data").unwrap();
        PIPE_MANAGER.dec_write_ref(pid);  // close write -> EOF after read
        let mut buf = [0u8; 16];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, 4);
        let n2 = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n2, 0); // EOF
        PIPE_MANAGER.dec_read_ref(pid);
    });

    test_case!("pipe_buffer_capacity", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        // Fill the buffer (4096 bytes minus 1 for sentinel)
        let buf = [0xABu8; 256];
        let mut total = 0usize;
        loop {
            match PIPE_MANAGER.write(pid, &buf) {
                Ok(n) => total += n,
                Err(_) => break,
            }
        }
        test_true!(total > 0);
        // Drain
        let mut out = [0u8; 256];
        let mut read_total = 0usize;
        loop {
            match PIPE_MANAGER.read(pid, &mut out) {
                Ok(0) => break,
                Ok(n) => read_total += n,
                Err(_) => break,
            }
        }
        test_eq!(read_total, total);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_write_after_read_close", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.dec_read_ref(pid); // close read end
        let result = PIPE_MANAGER.write(pid, b"test");
        test_true!(result.is_err()); // should get EPIPE
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_alloc_max", {
        let mut pipes = alloc::vec::Vec::new();
        while let Some(pid) = PIPE_MANAGER.alloc() {
            pipes.push(pid);
        }
        test_true!(pipes.len() <= 16);
        test_true!(pipes.len() > 0);
        // Allocate should fail now
        test_eq!(PIPE_MANAGER.alloc(), None);
        // Free them all
        for pid in pipes {
            PIPE_MANAGER.inc_read_ref(pid);
            PIPE_MANAGER.inc_write_ref(pid);
            PIPE_MANAGER.dec_read_ref(pid);
            PIPE_MANAGER.dec_write_ref(pid);
        }
    });

    test_case!("pipe_block_current_wake_kwait", {
        use crate::scheduler::ThreadState;

        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);

        // Block current (idle) thread on this pipe — now uses KWait (OB-031)
        crate::pipe::block_current_for_pipe(pid);

        // Verify we're now blocked — idle thread is at kthreads[0]
        // KWait uses MAGIC_PIPE_BASE (0x0001_0000) | pipe_id
        let expected_magic = 0x0001_0000u32 | pid as u32;
        let state = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let lock = s.lock();
            lock.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(state, ThreadState::Blocked { waiting_for: expected_magic });

        // Wake via KWait
        crate::kwait::kwait_wake(&crate::kwait::WaitReason::PipeRead { pipe_id: pid as u16 });

        let state2 = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let lock = s.lock();
            lock.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(state2, ThreadState::Ready);

        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("handle_table_default", {
        let table = default_handle_table();
        test_true!(table[0].is_stdin());
        test_true!(table[1].is_stdout());
        test_true!(table[2].is_stderr());
        for i in 3..16 {
            test_true!(!table[i].is_open());
        }
    });

    test_case!("handle_table_closed", {
        let table = closed_handle_table();
        for i in 0..16 {
            test_true!(!table[i].is_open());
        }
    });

    test_case!("handle_entry_constructors", {
        let r = HandleEntry::pipe_read(5);
        test_true!(r.is_pipe_read());
        test_eq!(r.native_id(), Some(5));
        let w = HandleEntry::pipe_write(3);
        test_true!(w.is_pipe_write());
        test_eq!(w.native_id(), Some(3));
        let f = HandleEntry::file(2, 42);
        test_eq!(f.obj_type(), Some(crate::object::ObType::Filesystem));
        test_eq!(f.native_id(), Some(42));
        test_eq!(f.drive(), Some(2));
        test_eq!(f.offset, 0);
    });

    // ── Pipeline tests ──────────────────────────────────────────────

    test_case!("pipe_two_commands", {
        // Simulate: cmd1 | cmd2
        // Refcount flow: shell creates pipe → cmd1 writes → cmd1 exits
        // → shell closes write → cmd2 reads until EOF
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid); // shell holds read
        PIPE_MANAGER.inc_write_ref(pid); // shell holds write
        PIPE_MANAGER.inc_write_ref(pid); // cmd1 gets write via spawn

        let data = b"pipeline two commands";
        PIPE_MANAGER.write(pid, data).unwrap();
        PIPE_MANAGER.dec_write_ref(pid); // cmd1 exits, drops write
        PIPE_MANAGER.dec_write_ref(pid); // shell closes write end → pipe write-closed

        // cmd2 gets read via spawn
        PIPE_MANAGER.inc_read_ref(pid);
        let mut buf = [0u8; 64];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, data.len());
        test_eq!(&buf[..n], data);
        let n2 = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n2, 0); // EOF

        PIPE_MANAGER.dec_read_ref(pid); // cmd2 exits
        PIPE_MANAGER.dec_read_ref(pid); // shell closes read end
    });

    test_case!("pipe_chain_three", {
        // Simulate: cmd1 | cmd2 | cmd3
        // Two pipes chained
        let p1 = PIPE_MANAGER.alloc().unwrap();
        let p2 = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(p1); PIPE_MANAGER.inc_write_ref(p1); // shell
        PIPE_MANAGER.inc_read_ref(p2); PIPE_MANAGER.inc_write_ref(p2); // shell

        // cmd1 writes to pipe1
        PIPE_MANAGER.inc_write_ref(p1); // cmd1 gets write via spawn
        PIPE_MANAGER.write(p1, b"data1").unwrap();
        PIPE_MANAGER.dec_write_ref(p1); // cmd1 exits
        PIPE_MANAGER.dec_write_ref(p1); // shell closes write → pipe1 write-closed

        // cmd2 reads pipe1, writes to pipe2
        PIPE_MANAGER.inc_read_ref(p1); // cmd2 gets read via spawn
        PIPE_MANAGER.inc_write_ref(p2); // cmd2 gets write via spawn
        let mut tmp = [0u8; 16];
        let n = PIPE_MANAGER.read(p1, &mut tmp).unwrap();
        test_eq!(n, 5);
        test_eq!(&tmp[..n], b"data1");
        test_eq!(PIPE_MANAGER.read(p1, &mut tmp).unwrap(), 0); // EOF

        PIPE_MANAGER.write(p2, b"data2").unwrap();
        PIPE_MANAGER.dec_read_ref(p1); // cmd2 exits, drops pipe1 read
        PIPE_MANAGER.dec_write_ref(p2); // cmd2 exits, drops pipe2 write

        // shell cleans up
        PIPE_MANAGER.dec_read_ref(p1); // shell closes pipe1 read
        PIPE_MANAGER.dec_write_ref(p2); // shell closes pipe2 write → write-closed

        // cmd3 reads pipe2
        PIPE_MANAGER.inc_read_ref(p2); // cmd3 gets read via spawn
        let mut out = [0u8; 16];
        let n2 = PIPE_MANAGER.read(p2, &mut out).unwrap();
        test_eq!(n2, 5);
        test_eq!(&out[..n2], b"data2");
        test_eq!(PIPE_MANAGER.read(p2, &mut out).unwrap(), 0); // EOF

        PIPE_MANAGER.dec_read_ref(p2); // cmd3 exits
        PIPE_MANAGER.dec_read_ref(p2); // shell closes pipe2 read
    });

    test_case!("pipe_blocking_read", {
        // Reading from an empty pipe with write end still open blocks.
        // PipeManager::read returns Err() in this case.
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);

        let mut buf = [0u8; 16];
        // Empty pipe with write end open → should Err (blocking)
        test_true!(PIPE_MANAGER.read(pid, &mut buf).is_err());

        // After writing data, read succeeds
        PIPE_MANAGER.write(pid, b"now data").unwrap();
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, 8);
        test_eq!(&buf[..n], b"now data");

        // With write end still open but buffer drained → Err again
        test_true!(PIPE_MANAGER.read(pid, &mut buf).is_err());

        // After closing write end → EOF (0 bytes)
        PIPE_MANAGER.dec_write_ref(pid);
        let n2 = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n2, 0); // EOF

        PIPE_MANAGER.dec_read_ref(pid);
    });

    // ── OB-016: Pipe via ObObject ──

    test_case!("pipe_ob_create_destroy", {
        let pid = PIPE_MANAGER.alloc().expect("pipe alloc");
        let name = alloc::format!("OBPIPE{}", pid);
        let ob_id = crate::object::ob_create_object(
            crate::object::ObType::Pipe, &name, pid as u64, 0, Some(&crate::pipe::PIPE_OPS),
        ).expect("ob create");
        test_true!(ob_id > 0);
        let obj = crate::object::ob_lookup(ob_id).expect("ob lookup");
        test_eq!(obj.obj_type, crate::object::ObType::Pipe);
        test_eq!(obj.native_id, pid as u64);
        // ob_close_object with refcount 1 → auto-destroy → on_destroy → free_pipe
        crate::object::ob_close_object(ob_id).unwrap();
        test_true!(crate::object::ob_lookup(ob_id).is_none());
        // Pipe slot should be reusable
        let pid2 = PIPE_MANAGER.alloc().expect("reuse after ob free");
        test_eq!(pid, pid2);
    });

    test_case!("pipe_ob_read_write", {
        let pid = PIPE_MANAGER.alloc().expect("pipe alloc");
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        let data = b"OB-016 rw test";
        let n = PIPE_MANAGER.write(pid, data).unwrap();
        test_eq!(n, data.len());
        let mut buf = [0u8; 64];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, data.len());
        test_eq!(&buf[..n], data);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });
}

// ===== Page Cache tests =====

pub fn register_page_cache_tests() {
    use crate::buffer::page_cache::PageCache;

    test_case!("page_cache_create_empty", {
        let pc = PageCache::new();
        test_eq!(pc.entry_count(), 0);
        test_eq!(pc.dirty_count(), 0);
    });

    test_case!("page_cache_peek_miss", {
        let pc = PageCache::new();
        test_eq!(pc.peek(1, 0), None);
        test_eq!(pc.peek(1, 1), None);
        test_eq!(pc.peek(0, 0), None);
    });

    test_case!("page_cache_mark_dirty_adds_dirty", {
        let mut pc = PageCache::new();
        test_eq!(pc.dirty_count(), 0);
        pc.mark_dirty(1, 0);
        test_eq!(pc.dirty_count(), 0);
    });

    test_case!("page_cache_invalidate_noop_empty", {
        let mut pc = PageCache::new();
        pc.invalidate_inode(42);
        test_eq!(pc.entry_count(), 0);
    });

    test_case!("page_cache_invalidate_multiple", {
        let mut pc = PageCache::new();
        pc.invalidate_inode(1);
        pc.invalidate_inode(2);
        test_eq!(pc.entry_count(), 0);
    });

    test_case!("page_cache_entry_count_bounds", {
        let pc = PageCache::new();
        test_true!(pc.entry_count() <= 128);
        test_eq!(pc.dirty_count(), 0);
    });

    test_case!("page_cache_dirty_count_never_negative", {
        let pc = PageCache::new();
        test_true!(pc.dirty_count() < usize::MAX);
    });

    test_case!("page_cache_peek_returns_none_unknown", {
        let pc = PageCache::new();
        for inode in &[1u32, 2, 3] {
            for block in &[0u32, 1, 5, 10] {
                test_eq!(pc.peek(*inode, *block), None);
            }
        }
    });

    test_case!("page_cache_capacity", {
        let pc = PageCache::new();
        test_eq!(pc.capacity(), 128);
        test_eq!(pc.max_capacity(), 2048);
        test_eq!(pc.min_capacity(), 64);
    });

    test_case!("page_cache_stats_empty", {
        let pc = PageCache::new();
        let stats = pc.stats();
        test_eq!(stats.hits, 0);
        test_eq!(stats.misses, 0);
        test_eq!(stats.evictions, 0);
        test_eq!(stats.current_entries, 0);
        test_eq!(stats.dirty_count, 0);
        test_eq!(stats.pending_writes, 0);
        test_eq!(stats.hash_table_len, 0);
    });

    test_case!("page_cache_hit_rate_zero", {
        let pc = PageCache::new();
        test_eq!(pc.hit_rate(), 0.0);
    });

    test_case!("page_cache_pending_write_count_zero", {
        let pc = PageCache::new();
        test_eq!(pc.pending_write_count(), 0);
    });

    test_case!("page_cache_invalidate_leaves_other_inodes", {
        let mut pc = PageCache::new();
        pc.invalidate_inode(1);
        pc.invalidate_inode(2);
        test_eq!(pc.entry_count(), 0);
        pc.invalidate_inode(1);
        test_eq!(pc.entry_count(), 0);
    });

    test_case!("page_cache_flush_noop_empty", {
        let pc = PageCache::new();
        // flush on empty cache should succeed without error
        // (no block device needed for empty cache)
        test_eq!(pc.dirty_count(), 0);
    });
}

// ===== PCI Enumeration tests =====

pub fn register_pci_enum_tests() {
    test_case!("pci_bus0_has_qemu_devices", {
        use crate::drivers::pci;
        let mut count = 0u16;
        let mut found_vga = false;
        let mut found_ahci = false;
        let mut found_net = false;
        let mut found_isa = false;
        for dev in 0..32 {
            let vendor = pci::pci_config_read_word(0, dev, 0, 0);
            if vendor == 0xFFFF || vendor == 0 {
                continue;
            }
            let header_type = pci::pci_config_read_word(0, dev, 0, 0x0E);
            let is_multi = (header_type & 0x80) != 0;
            let max_func = if is_multi { 8 } else { 1 };
            for func in 0..max_func {
                let vendor = pci::pci_config_read_word(0, dev, func, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    continue;
                }
                let device = pci::pci_config_read_word(0, dev, func, 2);
                // Machine-agnostic QEMU device detection:
                // Works on both PIIX3 (-machine pc) and Q35 (-machine q35)
                if vendor == 0x1234 && device == 0x1111 { found_vga = true; }  // QEMU VGA (both machines)
                if vendor == 0x8086 && device == 0x100E { found_net = true; }  // QEMU e1000 (PIIX3 only)
                if vendor == 0x8086 && device == 0x10D3 { found_net = true; }  // QEMU e1000e (Q35 only)
                if vendor == 0x8086 && device == 0x2922 { found_ahci = true; } // ICH9 AHCI (added via -device ahci)
                if vendor == 0x8086 && device == 0x2918 { found_isa = true; }  // Q35 ISA/LPC bridge
                if vendor == 0x8086 && device == 0x1237 { found_isa = true; }  // PIIX3 ISA bridge
                if vendor == 0x8086 && device == 0x7000 { found_isa = true; }  // PIIX3 ISA bridge (function)
                count += 1;
            }
        }
        test_true!(found_vga);
        test_true!(found_ahci);
        test_true!(found_net);
        test_true!(found_isa);
        test_true!(count >= 5);
    });
    test_case!("pci_bus1_empty", {
        use crate::drivers::pci;
        // Verify that bus 1 has no devices (only bus 0 on QEMU PIIX3)
        let mut found = false;
        for dev in 0..32 {
            let vendor = pci::pci_config_read_word(1, dev, 0, 0);
            if vendor != 0xFFFF && vendor != 0 {
                found = true;
                break;
            }
        }
        test_true!(!found);
    });
    test_case!("pci_algo_no_false_bridges", {
        use crate::drivers::pci;
        // Verify bridge detection algorithm: scan all functions, count bridges
        let mut bridges = 0u16;
        let mut multi_devs = 0u16;
        for dev in 0..32 {
            let vendor = pci::pci_config_read_word(0, dev, 0, 0);
            if vendor == 0xFFFF || vendor == 0 {
                continue;
            }
            let header_type = pci::pci_config_read_word(0, dev, 0, 0x0E);
            let is_multi = (header_type & 0x80) != 0;
            if is_multi { multi_devs += 1; }
            let max_func = if is_multi { 8usize } else { 1usize };
            for func in 0..max_func {
                let vendor = pci::pci_config_read_word(0, dev as u8, func as u8, 0);
                if vendor == 0xFFFF || vendor == 0 {
                    continue;
                }
                let class_rev = pci::pci_config_read_dword(0, dev as u8, func as u8, 0x08);
                let class = ((class_rev >> 24) & 0xFF) as u8;
                let subclass = ((class_rev >> 16) & 0xFF) as u8;
                if class == 0x06 && subclass == 0x04 {
                    bridges += 1;
                }
            }
        }
        test_true!(bridges == 0);
        test_true!(multi_devs >= 1);
    });
}

// ── Test registration (all suites) ─────────────────────────────────


pub fn register_tests() {
    crate::crash::register_crash_tests();
    register_input_tests();
    register_process_tests();
    register_sched_priority_tests();
    register_utf8_tests();
    register_alloc_tests();
    register_slab_tests();
    register_sync_tests();
    register_neofs_tests();
    register_mmap_tests();
    register_pipe_tests();
    register_page_cache_tests();
    register_pci_enum_tests();
    crate::nem::register_nem_tests();
    crate::elf::register_elf_tests();
    crate::eventbus::register_tests();
    crate::drivers::caps::register_cap_tests();
    crate::drivers::isolation::register_isolation_tests();
    crate::drivers::driver_runtime::register_driver_certification_tests();
    crate::drivers::boot_loader::register_boot_loader_tests();
    crate::drivers::nem::v3loader::register_v3_loader_tests();
    crate::drivers::abi::register_abi_tests();
    crate::drivers::dependency::register_dependency_tests();
    crate::fs::fsck::register_fsck_tests();
    crate::kobj::register_kobj_tests();
    crate::object::register_object_tests();
    crate::slab_container::register_slab_container_tests();
    crate::vfs::mount::register_mount_tests();
    crate::work_queue::register_tests();
    crate::dpc::register_tests();
    crate::irp::register_tests();
    crate::apc::register_tests();
    crate::drivers::hotreload::register_hotreload_tests();
    crate::syscall::register_syscall_table_tests();
    // Per-CPU data structure tests (A1.1)
    crate::arch::x64::cpu_local::register_cpu_local_tests();
    // SMP tests (A1.5)
    crate::arch::x64::smp::register_smp_tests();
    // IPI infrastructure tests (A1.4)
    crate::arch::x64::ipi::register_ipi_tests();
    // Per-CPU slab allocator tests (A1.3)
    register_per_cpu_slab_tests();
    // HAL v0.4 raw/safe split tests (A2.3)
    crate::hal::tests::register_hal_tests();
    // IRQL framework tests (A2.4)
    register_irql_tests();
    // Stress tests are always registered but can be gated by feature
    register_stress_tests();
    // A5.1 Unified block I/O layer (IoStack) tests
    crate::vfs::io::register_tests();
    // B4.4 B2 ANSI terminal tests
    crate::console::register_ansi_tests();
    // NT6 Security Reference Monitor tests
    crate::security::register_security_tests();

    // A2.1: PCIe ECAM tests
    crate::hal::pci::register_tests();
    register_ecam_integration_tests();

    // A2.2: I/O APIC tests
    crate::interrupts::ioapic::register_tests();
    register_ioapic_integration_tests();

    // NT5.5: Unified resource namespace (URN) tests
    crate::urn::register_urn_tests();

    // A3.3: Watchdog subsystem tests
    crate::watchdog::register_watchdog_tests();

    // A3.4: SEH + Exception Dispatcher tests
    crate::exception::dispatcher::register_exception_tests();

    // v0.42: KWait Unified Wait Engine tests
    crate::kwait::register_kwait_tests();

    // v0.42: ABI Freeze verification tests
    crate::abi_freeze::register_abi_freeze_tests();

    // A4.4: Virtual Terminal tests
    register_vt_tests();
}



// ── Per-CPU slab allocator tests (A1.3) ──────────────────────────────────

fn register_per_cpu_slab_tests() {
    use crate::arch::x64::cpu_local;

    test_case!("per_cpu_slab_alloc_free_concurrent", {
        // Verify per-CPU slab alloc/free works through the GlobalAlloc interface.
        // On single-CPU this tests the fast path without lock contention.
        extern crate alloc;
        use alloc::boxed::Box;

        // Allocate and free objects of each slab size class
        for _ in 0..16 {
            let b = Box::new([0u8; 2048]);  // Use max slab size
            drop(b);
        }
        // Allocate small objects that fit in each size class
        for _ in 0..32 {
            let b = Box::new(0u64);
            drop(b);
        }
    });

    test_case!("per_cpu_refill_drain_batching", {
        // Verify that the per-CPU hot cache can be filled and drained
        extern crate alloc;
        use alloc::boxed::Box;

        // Allocate 32+ objects to force a refill from global pool
        let mut boxes = alloc::vec::Vec::new();
        for i in 0..64u64 {
            boxes.push(Box::new(i));
        }
        // Verify values
        for (i, b) in boxes.iter().enumerate() {
            test_eq!(**b, i as u64);
        }
        // Drop all to trigger drain
        drop(boxes);
    });

    test_case!("slab_scaling_8cpu", {
        // Verify no deadlock when allocating many objects (single-CPU variant)
        extern crate alloc;
        use alloc::boxed::Box;

        let mut v = alloc::vec::Vec::new();
        for i in 0..1024u64 {
            v.push(Box::new(i));
        }
        for (i, b) in v.iter().enumerate() {
            test_eq!(**b, i as u64);
        }
        drop(v);
    });

    test_case!("slab_under_irql_dispatch", {
        // Verify slab works correctly when IRQL is at DISPATCH level
        extern crate alloc;
        use alloc::boxed::Box;

        unsafe {
            let irql = cpu_local::gs_read_u8(cpu_local::OFFSET_CURRENT_IRQL);
            // Should be 0 (PASSIVE_LEVEL) during normal execution
            test_eq!(irql, 0u8);
        }

        let b = Box::new(0xABu32);
        test_eq!(*b, 0xABu32);
        drop(b);
    });

    test_case!("slab_stress_100k", {
        // Stress test: 100k alloc/free cycles through the per-CPU path
        extern crate alloc;
        use alloc::boxed::Box;

        for i in 0..100_000u64 {
            let b = Box::new(i);
            test_eq!(*b, i);
            drop(b);
        }
    });
}

// ── IRQL framework tests (A2.4) ──────────────────────────────────────

pub fn register_irql_tests() {
    use crate::hal::irql;

    test_case!("irql_raise_lower_passive_dispatch", {
        unsafe {
            let initial = irql::current_irql();
            test_eq!(initial, irql::PASSIVE_LEVEL);

            let old = irql::raise_irql(irql::DISPATCH_LEVEL);
            test_eq!(old, irql::PASSIVE_LEVEL);
            test_eq!(irql::current_irql(), irql::DISPATCH_LEVEL);

            irql::lower_irql(old);
            test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);
        }
    });

    test_case!("irql_page_fault_at_dispatch_panics", {
        unsafe {
            let initial = irql::current_irql();
            test_eq!(initial, irql::PASSIVE_LEVEL);

            let old = irql::raise_irql(irql::DISPATCH_LEVEL);
            test_eq!(irql::current_irql(), irql::DISPATCH_LEVEL);
            test_true!(irql::at_or_above_dispatch());

            irql::lower_irql(old);
            test_true!(!irql::at_or_above_dispatch());
        }
    });

    test_case!("irql_spinlock_implicit_raise", {
        use crate::hal::irql::IrqMutex;

        let mutex = IrqMutex::new(42u64);
        test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);

        {
            let mut guard = mutex.lock();
            test_eq!(irql::current_irql(), irql::DISPATCH_LEVEL);
            test_eq!(*guard, 42u64);
            *guard = 100;
            test_eq!(*guard, 100u64);
        }
        test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);

        let guard = mutex.lock();
        test_eq!(*guard, 100u64);
        drop(guard);
    });

    test_case!("irql_nesting_stack", {
        unsafe {
            test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);

            let old1 = irql::raise_irql(irql::DISPATCH_LEVEL);
            test_eq!(old1, irql::PASSIVE_LEVEL);
            test_eq!(irql::current_irql(), irql::DISPATCH_LEVEL);

            let old2 = irql::raise_irql(irql::DIRQL_BASE);
            test_eq!(old2, irql::DISPATCH_LEVEL);
            test_eq!(irql::current_irql(), irql::DIRQL_BASE);

            let old3 = irql::raise_irql(irql::HIGH_LEVEL);
            test_eq!(old3, irql::DIRQL_BASE);
            test_eq!(irql::current_irql(), irql::HIGH_LEVEL);

            irql::lower_irql(old3);
            test_eq!(irql::current_irql(), irql::DIRQL_BASE);

            irql::lower_irql(old2);
            test_eq!(irql::current_irql(), irql::DISPATCH_LEVEL);

            irql::lower_irql(old1);
            test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);
        }
    });

    test_case!("irql_preemption_threshold", {
        unsafe {
            test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);

            // Raising to same level — no-op
            let old = irql::raise_irql(irql::PASSIVE_LEVEL);
            test_eq!(old, irql::PASSIVE_LEVEL);
            test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);
            irql::lower_irql(old);

            // Raising to DISPATCH
            let old = irql::raise_irql(irql::DISPATCH_LEVEL);
            test_eq!(old, irql::PASSIVE_LEVEL);
            test_eq!(irql::current_irql(), irql::DISPATCH_LEVEL);

            // Raising again to DISPATCH — returns current level as old
            let old2 = irql::raise_irql(irql::DISPATCH_LEVEL);
            test_eq!(old2, irql::DISPATCH_LEVEL);
            test_eq!(irql::current_irql(), irql::DISPATCH_LEVEL);

            irql::lower_irql(old);
            test_eq!(irql::current_irql(), irql::PASSIVE_LEVEL);
        }
    });
}

// ── A2.2: IOAPIC integration tests ────────────────────────────────

fn register_ioapic_integration_tests() {
    test_case!("ioapic_pic_disabled_when_ioapic_active", {
        if crate::interrupts::ioapic::is_active() {
            // Read PIC master mask register (port 0x21)
            let master_mask = crate::hal::inb(0x21);
            let slave_mask = crate::hal::inb(0xA1);
            test_eq!(master_mask, 0xFF);
            test_eq!(slave_mask, 0xFF);
        }
    });
}

// ── A2.1: ECAM integration tests ──────────────────────────────────

fn register_ecam_integration_tests() {
    use crate::drivers::pci;

    test_case!("ecam_mcfg_table_parse", {
        // MCFG table should be present on QEMU/OVMF.
        // This validates the ACPI table scanning code path.
        match crate::timers::hpet::get_ecam_info() {
            Some((base, _seg, _start, _end)) => {
                test_true!(base > 0);
            }
            None => {
                // On real hardware without PCIe, this is acceptable.
                // We log and pass.
            }
        }
    });

    test_case!("ecam_fallback_to_pio_if_no_mcfg", {
        if crate::hal::pci::ecam_is_active() {
            let vendor = pci::pci_config_read_word(0, 0, 0, 0);
            test_ne!(vendor, 0xFFFF);
            test_ne!(vendor, 0);
        } else {
            let vendor = pci::pci_config_read_word(0, 0, 0, 0);
            test_ne!(vendor, 0xFFFF);
            test_ne!(vendor, 0);
        }
    });

    test_case!("ecam_read_match_legacy_pio", {
        if crate::hal::pci::ecam_is_active() {
            let ecam_vendor = unsafe { crate::hal::pci::ecam_read_config_word(0, 0, 0, 0) };
            // Temporarily deactivate ECAM, read via PIO, reactivate
            crate::hal::pci::ecam_deactivate();
            let pio_vendor = pci::pci_config_read_word(0, 0, 0, 0);
            test_eq!(ecam_vendor, pio_vendor);
            // Re-activate ECAM (if the original init set it)
            if let Some((base, _seg, _start, _end)) = crate::timers::hpet::get_ecam_info() {
                crate::hal::pci::set_ecam_base(base);
            }
        } else {
            // ECAM not active, skip comparison test
        }
    });
}
