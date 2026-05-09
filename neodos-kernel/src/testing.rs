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

pub fn register_tests() {
    register_env_tests();
}
