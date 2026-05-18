use crate::serial_print;
use crate::serial_println;

type TestFn = fn() -> Result<(), &'static str>;

#[derive(Copy, Clone)]
struct Test {
    name: &'static str,
    func: TestFn,
}

const MAX_TESTS: usize = 128;
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

// ===== Environment tests =====

pub fn register_env_tests() {
    test_case!("env_set_get", {
        let mut env = crate::shell::environment::Environment::new();
        env.set("PATH", "/bin");
        test_eq!(env.get("PATH"), Some("/bin"));
    });

    test_case!("env_overwrite", {
        let mut env = crate::shell::environment::Environment::new();
        env.set("FOO", "bar");
        env.set("FOO", "baz");
        test_eq!(env.get("FOO"), Some("baz"));
    });

    test_case!("env_missing_key", {
        let env = crate::shell::environment::Environment::new();
        test_eq!(env.get("NONEXIST"), None);
    });

    test_case!("env_empty_value", {
        let mut env = crate::shell::environment::Environment::new();
        env.set("EMPTY", "");
        test_eq!(env.get("EMPTY"), Some(""));
    });

    test_case!("env_case_sensitive", {
        let mut env = crate::shell::environment::Environment::new();
        env.set("Path", "/usr/bin");
        test_eq!(env.get("PATH"), None);
        test_eq!(env.get("Path"), Some("/usr/bin"));
    });

    test_case!("env_multiple_vars", {
        let mut env = crate::shell::environment::Environment::new();
        env.set("A", "1");
        env.set("B", "2");
        env.set("C", "3");
        test_eq!(env.get("A"), Some("1"));
        test_eq!(env.get("B"), Some("2"));
        test_eq!(env.get("C"), Some("3"));
        test_eq!(env.get("D"), None);
    });
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

// ===== Keyboard tests =====

pub fn register_keyboard_tests() {
    use crate::drivers::keyboard::KeyboardDriver;

    test_case!("kbd_codepoint_1byte", {
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x0000), [0x00, 0x00, 0x00]);
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x0041), [0x41, 0x00, 0x00]); // 'A'
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x007F), [0x7F, 0x00, 0x00]);
    });

    test_case!("kbd_codepoint_2byte", {
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x0080), [0xC2, 0x80, 0x00]);
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x00E1), [0xC3, 0xA1, 0x00]); // 'á'
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x07FF), [0xDF, 0xBF, 0x00]);
    });

    test_case!("kbd_codepoint_3byte", {
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x0800), [0xE0, 0xA0, 0x80]);
        test_eq!(KeyboardDriver::codepoint_to_utf8(0x20AC), [0xE2, 0x82, 0xAC]); // '€'
        test_eq!(KeyboardDriver::codepoint_to_utf8(0xFFFF), [0xEF, 0xBF, 0xBF]);
    });

    test_case!("kbd_lookup_compose_us", {
        // US layout has no compose entries
        test_eq!(KeyboardDriver::lookup_compose(0, 0x60, 0x61), None);
        test_eq!(KeyboardDriver::lookup_compose(0, 0xB4, 0x61), None);
    });

    test_case!("kbd_lookup_compose_sp", {
        // Spanish layout (index 1): grave + a = à (0xE0)
        test_eq!(KeyboardDriver::lookup_compose(1, 0x60, 0x61), Some(0xE0));
        // acute + a = á (0xE1)
        test_eq!(KeyboardDriver::lookup_compose(1, 0xB4, 0x61), Some(0xE1));
        // grave + space = standalone grave
        test_eq!(KeyboardDriver::lookup_compose(1, 0x60, 0x20), Some(0x60));
        // unknown pair: grave + z = None
        test_eq!(KeyboardDriver::lookup_compose(1, 0x60, 0x7A), None);
    });
}


pub fn register_process_tests() {
    use crate::scheduler::{Process, ProcessState};

    test_case!("process_new_initial_state", {
        let p = Process::new_ring0(1, 0x400000, 0x800000, None);
        test_eq!(p.pid, 1);
        test_eq!(p.rip, 0x400000);
        test_eq!(p.state, ProcessState::Ready);
        test_eq!(p.cpu_ticks, 0);
        test_eq!(p.user_slot, None);
        test_eq!(p.waiting_for, None);
    });

    test_case!("process_state_debug", {
        let mut p = Process::new_ring0(1, 0x400000, 0x800000, None);
        test_eq!(p.state, ProcessState::Ready);
        p.state = ProcessState::Running;
        test_eq!(p.state, ProcessState::Running);
        p.state = ProcessState::Blocked { waiting_for: 42 };
        test_eq!(p.state, ProcessState::Blocked { waiting_for: 42 });
        p.state = ProcessState::Terminated;
        test_eq!(p.state, ProcessState::Terminated);
    });

    test_case!("process_state_partial_eq", {
        let s1 = ProcessState::Ready;
        let s2 = ProcessState::Ready;
        test_eq!(s1, s2);
        test_ne!(ProcessState::Ready, ProcessState::Running);
        test_ne!(ProcessState::Blocked { waiting_for: 1 }, ProcessState::Blocked { waiting_for: 2 });
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
        use crate::scheduler::{Process, ProcessState};
        let mut p = Process::new_ring0(99, 0x400000, 0x800000, None);
        test_eq!(p.state, ProcessState::Ready);
        for _ in 0..200 {
            // Ready → Running – legal
            p.state = ProcessState::Running;
            // Running → Ready (timer tick) – legal
            p.state = ProcessState::Ready;
        }
        p.state = ProcessState::Terminated;
        // Once terminated, should never go back to Ready (checked elsewhere)
        test_eq!(p.state, ProcessState::Terminated);
    });
}

// ── B. Syscall stress ──────────────────────────────────────────────

fn register_syscall_stress() {
    test_case!("stress_syscall_rapid_getpid", {
        // Rapid PID queries exercise the scheduler lock path
        for _ in 0..200 {
            let pid = x86_64::instructions::interrupts::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid
            });
            test_true!(pid < 1000);
        }
    });

    test_case!("stress_syscall_invalid_numbers", {
        // ABI fuzzing: ensure invalid syscall numbers return u64::MAX
        for num in &[20u64, 100, 255, 0xFFFFFFFF] {
            // Create a dummy dispatch call
            let result = crate::syscall::syscall_dispatch(*num, 0, 0, 0);
            test_eq!(result, u64::MAX);
        }
    });

    test_case!("stress_syscall_ptr_validation", {
        // Ensure user pointer validation rejects kernel addresses
        // We can't call the private `is_user_ptr_valid` directly, but
        // we can test the public behavior via what sys_write would do.
        // If we send a kernel address to sys_write (RAX=1), the dispatch
        // should return u64::MAX without crashing.
        let kernel_addr: u64 = 0x200000; // kernel .text start
        let result = crate::syscall::syscall_dispatch(1, kernel_addr, 10, 0);
        test_eq!(result, u64::MAX);
    });
}

// ── C. Memory stress ───────────────────────────────────────────────

fn register_mem_stress() {
    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use alloc::string::String;

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

// ── Test registration (all suites) ─────────────────────────────────

pub fn register_tests() {
    register_env_tests();
    register_input_tests();
    register_keyboard_tests();
    register_process_tests();
    register_utf8_tests();
    register_alloc_tests();
    register_sync_tests();
    // Stress tests are always registered but can be gated by feature
    register_stress_tests();
}
