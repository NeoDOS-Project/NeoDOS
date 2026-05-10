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
        let mut buf = InputBuffer::new();
        test_eq!(buf.pop(), None);
    });

    test_case!("input_push_pop_one", {
        let mut buf = InputBuffer::new();
        test_eq!(buf.push(42), Ok(()));
        test_eq!(buf.pop(), Some(42));
        test_eq!(buf.pop(), None);
    });

    test_case!("input_buffer_capacity", {
        let mut buf = InputBuffer::new();
        let mut count = 0;
        while buf.push(count as u8).is_ok() {
            count += 1;
        }
        test_ne!(count, 0);
        test_eq!(buf.push(0), Err(()));
    });

    test_case!("input_wrap_around", {
        let mut buf = InputBuffer::new();
        for i in 0..100 { let _ = buf.push(i); }
        for i in 0..50 { test_eq!(buf.pop(), Some(i)); }
        for i in 100..150 { let _ = buf.push(i); }
        for i in 50..100 { test_eq!(buf.pop(), Some(i)); }
        for i in 100..150 { test_eq!(buf.pop(), Some(i)); }
        test_eq!(buf.pop(), None);
    });

    test_case!("input_full_then_drain", {
        let mut buf = InputBuffer::new();
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

// ===== Drive manager tests =====

pub fn register_drive_tests() {
    use crate::fs::drive_manager::{DriveManager, FsInstanceId, DriveManagerError};

    test_case!("drive_mount_get", {
        let mut dm = DriveManager::new();
        test_eq!(dm.mount('C', FsInstanceId::PRIMARY), Ok(()));
        let d = dm.get('C');
        test_ne!(d, None);
        test_eq!(d.unwrap().letter, b'C');
    });

    test_case!("drive_invalid_letter", {
        let mut dm = DriveManager::new();
        test_eq!(dm.mount('1', FsInstanceId::PRIMARY), Err(DriveManagerError::InvalidDriveLetter));
        test_eq!(dm.mount('ç', FsInstanceId::PRIMARY), Err(DriveManagerError::InvalidDriveLetter));
    });

    test_case!("drive_mount_twice", {
        let mut dm = DriveManager::new();
        test_eq!(dm.mount('C', FsInstanceId::PRIMARY), Ok(()));
        test_eq!(dm.mount('C', FsInstanceId::PRIMARY), Err(DriveManagerError::DriveAlreadyMounted));
    });

    test_case!("drive_resolve_basic", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        let (fs_id, path) = dm.resolve_dos_path("C:\\FOO\\BAR").unwrap();
        test_eq!(fs_id, FsInstanceId::PRIMARY);
        test_eq!(path.as_str(), Ok("/FOO/BAR"));
    });

    test_case!("drive_resolve_forward_slash", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        let (_, path) = dm.resolve_dos_path("C:/FOO/BAR").unwrap();
        test_eq!(path.as_str(), Ok("/FOO/BAR"));
    });

    test_case!("drive_resolve_root", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        let (_, path) = dm.resolve_dos_path("C:\\").unwrap();
        test_eq!(path.as_str(), Ok("/"));
    });

    test_case!("drive_resolve_just_letter", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        let (_, path) = dm.resolve_dos_path("C:").unwrap();
        test_eq!(path.as_str(), Ok("/"));
    });

    test_case!("drive_resolve_double_sep", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        let (_, path) = dm.resolve_dos_path("C:\\\\FOO\\\\BAR").unwrap();
        test_eq!(path.as_str(), Ok("/FOO/BAR"));
    });

    test_case!("drive_resolve_trailing_slash", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        let (_, path) = dm.resolve_dos_path("C:\\FOO\\BAR\\").unwrap();
        test_eq!(path.as_str(), Ok("/FOO/BAR"));
    });

    test_case!("drive_resolve_no_drive", {
        let dm = DriveManager::new();
        test_eq!(dm.resolve_dos_path(""), Err(DriveManagerError::InvalidPath));
        test_eq!(dm.resolve_dos_path("C"), Err(DriveManagerError::InvalidPath));
    });

    test_case!("drive_resolve_not_mounted", {
        let dm = DriveManager::new();
        test_eq!(dm.resolve_dos_path("X:\\path"), Err(DriveManagerError::DriveNotMounted));
    });

    test_case!("drive_resolve_numeric_letter", {
        let dm = DriveManager::new();
        test_eq!(dm.resolve_dos_path("1:\\path"), Err(DriveManagerError::InvalidDriveLetter));
    });

    test_case!("drive_resolve_lowercase", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        let (_, path) = dm.resolve_dos_path("c:\\path").unwrap();
        test_eq!(path.as_str(), Ok("/path"));
    });

    test_case!("drive_resolve_invalid_bytes", {
        let mut dm = DriveManager::new();
        dm.mount('C', FsInstanceId::PRIMARY).unwrap();
        test_eq!(dm.resolve_dos_path("C:\\pa\u{80}th"), Err(DriveManagerError::InvalidPath));
        test_eq!(dm.resolve_dos_path("C:\\pa\x1Fth"), Err(DriveManagerError::InvalidPath));
        test_eq!(dm.resolve_dos_path("C:\\pa\x7Fth"), Err(DriveManagerError::InvalidPath));
    });

    test_case!("drive_set_primary", {
        let mut dm = DriveManager::new();
        dm.set_primary('C').unwrap();
        let d = dm.get('C');
        test_ne!(d, None);
    });
}

pub fn register_tests() {
    register_env_tests();
    register_input_tests();
    register_keyboard_tests();
    register_drive_tests();
}
