use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::serial_print;
use crate::serial_println;

type TestFn = fn() -> Result<(), &'static str>;

struct Test {
    name: &'static str,
    func: TestFn,
}

lazy_static! {
    static ref TESTS: Mutex<Vec<Test>> = Mutex::new(Vec::new());
}

pub fn register(name: &'static str, func: TestFn) {
    TESTS.lock().push(Test { name, func });
}

pub fn run_all() -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    let tests = TESTS.lock();
    for test in tests.iter() {
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
    drop(tests);
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

#[macro_export]
macro_rules! test_false {
    ($cond:expr $(,)?) => {
        if ($cond) {
            return Err(concat!(
                "assertion failed: expected false: ", stringify!($cond)
            ));
        }
    };
}

// ── IRQL tests ────────────────────────────────────────────────────

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

// ── Test registration (all suites) ─────────────────────────────────

pub fn register_tests() {
    crate::crash::register_crash_tests();
    crate::input::register_tests();
    crate::input::vt::register_tests();
    crate::scheduler::register_tests();
    crate::syscall::register_syscall_table_tests();
    crate::syscall::register_sync_tests();
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
    crate::drivers::pci::register_tests();
    crate::fs::btree::register_btree_tests();
    crate::fs::freelist::register_freelist_tests();
    crate::fs::snapshot::register_snapshot_tests();
    crate::fs::neodos_dir::register_dir_tests();
    crate::fs::fsck::register_fsck_tests();
    crate::object::register_object_tests();
    crate::object::pipe::register_tests();
    crate::object::timer::register_timer_tests();
    crate::object::semaphore::register_semaphore_tests();
    crate::object::section::register_section_tests();
    crate::vfs::mount::register_mount_tests();
    crate::vfs::io::register_tests();
    crate::work_queue::register_tests();
    crate::dpc::register_tests();
    crate::irp::register_tests();
    crate::apc::register_tests();
    crate::drivers::hotreload::register_hotreload_tests();
    crate::handle::register_tests();
    crate::buffer::page_cache::register_tests();
    // Per-CPU data structure tests (A1.1)
    crate::arch::x64::cpu_local::register_cpu_local_tests();
    // SMP tests (A1.5)
    crate::arch::x64::smp::register_smp_tests();
    // IPI infrastructure tests (A1.4)
    crate::arch::x64::ipi::register_ipi_tests();
    // HAL v0.4 raw/safe split tests (A2.3)
    crate::hal::tests::register_hal_tests();
    // IRQL framework tests (A2.4)
    register_irql_tests();
    // B4.4 B2 ANSI terminal tests
    crate::console::register_ansi_tests();
    // NT6 Security Reference Monitor tests
    crate::security::register_security_tests();
    // A2.1: PCIe ECAM tests
    crate::hal::pci::register_tests();
    // A2.2: I/O APIC tests
    crate::interrupts::ioapic::register_tests();
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
    // B3.1/B3.2: Networking tests
    crate::net::register_net_tests();
    // A5.2: VirtIO Block driver tests
    crate::drivers::virtio_blk::register_tests();
    // B2.1 Z6: Registry hive database (Cm) tests
    crate::cm::register_cm_tests();
    // SM-001: Service Manager tests
    crate::services::register_service_tests();
    // PM-PHASE1: HAL ACPI reboot/FADT/S5 primitives
    crate::power::acpi::register_pm_tests();
    // Memory stress tests
    crate::memory::register_stress_tests();
    // UTF-8 tests
    register_utf8_tests();
    // Allocator basic tests
    register_alloc_tests();
    // Slab allocator tests
    register_slab_tests();
}

// ── UTF-8 tests ────────────────────────────────────────────────────

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

// ── Allocator basic tests ──────────────────────────────────────────

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
        let v = alloc::vec![1, 2, 3];
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

// ── Slab allocator tests ───────────────────────────────────────────

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