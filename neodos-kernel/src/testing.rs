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
            let pid = crate::hal::without_interrupts(|| {
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

// ── NeoFS metadata tests ───────────────────────────────────────────

pub fn register_neofs_tests() {
    use crate::fs::neodos_fs::{Inode, DirectoryEntry, NeoDosFs, MODE_DIR, MODE_FILE};
    use crate::fs::neodos_fs::{
        ATTR_READONLY, ATTR_HIDDEN, ATTR_SYSTEM, ATTR_VOLUME, ATTR_DIR, ATTR_ARCHIVE,
    };
    use crate::fs::neodos_fs::BLOCK_SIZE;

    // ── 1. Inode mode / type tests ───────────────────────────────────

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
        // MODE_DIR and MODE_FILE are distinct bits
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
        // Max values should not corrupt anything — just verify they're stored
        test_eq!(inode.inode_num, 255);
        test_eq!(inode.mode, 0xFFFF);
        test_eq!(inode.size, u32::MAX);
        test_eq!(inode.atime, u64::MAX);
    });

    // ── 2. Timestamp tests ───────────────────────────────────────────

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

    // ── 3. Inode serialisation round-trip ────────────────────────────

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
        // Serialise to raw bytes (same method as write_inode)
        let mut raw = [0u8; 256];
        unsafe {
            core::ptr::write_unaligned(
                raw.as_mut_ptr() as *mut Inode,
                original
            );
        }
        // Deserialise from raw bytes (same method as load_inode)
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
        // Copy field before iterating (packed struct — avoid misaligned refs)
        let blocks = restored.direct_blocks;
        for &b in blocks.iter() {
            test_eq!(b, 0);
        }
    });

    // ── 4. Inode block count tests (pure function) ───────────────────

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
        // Capped at 12 direct blocks
        test_eq!(NeoDosFs::inode_block_count(&inode), 12);
    });

    test_case!("neofs_inode_block_count_dir_root", {
        // Root dir with first block ptr=0 should still count as 1 block
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

    // ── 5. DirectoryEntry attribute tests ────────────────────────────

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

    // ── 6. DirectoryEntry serialisation ──────────────────────────────

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

    // ── 7. Edge cases: invalid/corrupted metadata ────────────────────

    test_case!("neofs_dirent_zero_len_name", {
        let entry = DirectoryEntry {
            inode_num: 0, name_len: 0, entry_type: 1, attributes: 0,
            name: [0u8; 249],
        };
        // A zero-length name entry should be treated as empty/skip
        // (the FS listing code in list_root continues when name_len==0)
        test_eq!(entry.name_len, 0);
        test_eq!(entry.inode_num, 0);
    });

    test_case!("neofs_inode_negative_block_count", {
        // size=0 with no blocks — block_count must be 0
        let inode = Inode {
            inode_num: 0, mode: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        // Neither file nor dir — block count should be 0
        test_eq!(NeoDosFs::inode_block_count(&inode), 0);
    });

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

    // ── 8. Link count and ownership ──────────────────────────────────

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

    // ── 9. Mode bit manipulation ─────────────────────────────────────

    test_case!("neofs_mode_preserves_extra_bits", {
        // Mode field may have bits beyond 0x40/0x80 set.
        // The FS must not crash on unknown bits.
        let inode = Inode {
            inode_num: 1, mode: MODE_FILE | 0x0F00, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 0, owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12], indirect_block: 0,
            padding: [0; 160],
        };
        test_true!((inode.mode & MODE_FILE) != 0);
        test_true!((inode.mode & 0x0F00) != 0);
    });

    // ── 10. Stress: repeated inode manipulation ──────────────────────

    test_case!("neofs_stress_inode_toggle_mode", {
        // Rapidly toggle between MODE_FILE and MODE_DIR
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

    test_case!("neofs_stress_inode_uid_cycle", {
        // Cycle owner_uid through a range, verify no overflow/truncation
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
        // Write and check timestamps repeatedly
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
    register_neofs_tests();
    // Stress tests are always registered but can be gated by feature
    register_stress_tests();
}
