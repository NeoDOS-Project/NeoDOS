#![no_std]
#![no_main]

use libneodos::println;

// MathAbiTable must match libmath-nxl/src/main.rs exactly
#[repr(C)]
struct MathAbiTable {
    version: u32,
    add: extern "C" fn(i64, i64) -> i64,
    sub: extern "C" fn(i64, i64) -> i64,
    mul: extern "C" fn(i64, i64) -> i64,
    abs: extern "C" fn(i64) -> i64,
    abs_f64: extern "C" fn(f64) -> f64,
    min: extern "C" fn(i64, i64) -> i64,
    max: extern "C" fn(i64, i64) -> i64,
    clamp: extern "C" fn(i64, i64, i64) -> i64,
    pow: extern "C" fn(i64, u32) -> i64,
    modulo: extern "C" fn(i64, i64) -> i64,
    div: extern "C" fn(i64, i64) -> i64,
    sqrt_int: extern "C" fn(u64) -> u64,
    sqrt_f64: extern "C" fn(f64) -> f64,
    sin: extern "C" fn(f64) -> f64,
    cos: extern "C" fn(f64) -> f64,
    tan: extern "C" fn(f64) -> f64,
    log2: extern "C" fn(f64) -> f64,
    log: extern "C" fn(f64) -> f64,
    exp: extern "C" fn(f64) -> f64,
    _reserved: [u64; 8],
}

fn check(label: &str, ok: bool) -> bool {
    if ok {
        println!("  {} ... PASS", label);
    } else {
        println!("  {} ... FAIL", label);
    }
    ok
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut all_pass = true;

    // ================================================================
    // HEADER
    // ================================================================
    println!("[LIBMATH TEST SUITE START]");
    println!();

    // ================================================================
    // PHASE 1: LOAD TEST
    // ================================================================
    println!("[TEST] Loading libmath.nxl...");

    let math_base = match libneodos::syscall::sys_loadlib("C:\\SYSTEM\\LIB\\LIBMATH.NXL") {
        Ok(base) => base,
        Err(e) => {
            println!("[FAIL] sys_loadlib error code {}", e);
            println!();
            println!("RESULT: FAILURE");
            println!("FAILED AT: LOAD TEST");
            println!("REASON: Could not load libmath.nxl");
            libneodos::syscall::sys_exit(1)
        }
    };

    println!("[OK] libmath.nxl loaded at 0x{:x}", math_base);
    println!("[TEST] Symbol resolution...");

    let table = unsafe { &*(math_base as *const MathAbiTable) };

    let mut load_ok = true;

    if table.version != 1 {
        println!("  Version mismatch: expected 1, got {}", table.version);
        load_ok = false;
    }

    let add_fn = table.add;
    let sub_fn = table.sub;
    let mul_fn = table.mul;
    let div_fn = table.div;
    let abs_fn = table.abs;
    let min_fn = table.min;
    let max_fn = table.max;
    let pow_fn = table.pow;
    let modulo_fn = table.modulo;
    let sqrt_fn = table.sqrt_int;

    let symbols = [
        ("add", add_fn as usize),
        ("sub", sub_fn as usize),
        ("mul", mul_fn as usize),
        ("div", div_fn as usize),
        ("abs", abs_fn as usize),
        ("min", min_fn as usize),
        ("max", max_fn as usize),
        ("pow", pow_fn as usize),
        ("modulo", modulo_fn as usize),
        ("sqrt_int", sqrt_fn as usize),
    ];

    for (name, ptr) in &symbols {
        if *ptr == 0 {
            println!("  [SYM] {}... FAIL (null pointer)", name);
            load_ok = false;
        }
    }

    if load_ok {
        println!("[LOAD] libmath.nxl........ OK");
        println!("[SYM] add................. OK");
        println!("[SYM] sub................. OK");
        println!("[SYM] mul................. OK");
        println!("[SYM] div................. OK");
    } else {
        println!("[LOAD] libmath.nxl........ FAIL");
        println!("[SYM] add................. FAIL");
        println!("[SYM] sub................. FAIL");
        println!("[SYM] mul................. FAIL");
        println!("[SYM] div................. FAIL");
        all_pass = false;
    }

    // ================================================================
    // PHASE 2: BASIC ARITHMETIC TESTS
    // ================================================================
    println!();
    println!("[BASIC TESTS]");

    let mut basic_pass = true;

    // add(2,3) == 5
    let r = add_fn(2, 3);
    if r != 5 {
        println!("  add(2,3) = {} (expected 5)", r);
        basic_pass = false;
    }

    // sub(10,4) == 6
    let r = sub_fn(10, 4);
    if r != 6 {
        println!("  sub(10,4) = {} (expected 6)", r);
        basic_pass = false;
    }

    // mul(3,3) == 9
    let r = mul_fn(3, 3);
    if r != 9 {
        println!("  mul(3,3) = {} (expected 9)", r);
        basic_pass = false;
    }

    // div(10,2) == 5
    let r = div_fn(10, 2);
    if r != 5 {
        println!("  div(10,2) = {} (expected 5)", r);
        basic_pass = false;
    }

    if basic_pass {
        println!("[BASIC TESTS]............. PASS");
    } else {
        println!("[BASIC TESTS]............. FAIL");
        all_pass = false;
    }

    // ================================================================
    // PHASE 3: EDGE CASE TESTS
    // ================================================================
    println!();
    println!("[EDGE CASES]");

    let mut edge_pass = true;

    edge_pass &= check("add(0, 0) == 0", add_fn(0, 0) == 0);
    edge_pass &= check("add(-1, 1) == 0", add_fn(-1, 1) == 0);
    edge_pass &= check("sub(0, 0) == 0", sub_fn(0, 0) == 0);
    edge_pass &= check("sub(-5, -3) == -2", sub_fn(-5, -3) == -2);
    edge_pass &= check("mul(0, 999999) == 0", mul_fn(0, 999999) == 0);
    edge_pass &= check("mul(-1, 5) == -5", mul_fn(-1, 5) == -5);
    edge_pass &= check("div(1, 1) == 1", div_fn(1, 1) == 1);
    edge_pass &= check("div(-10, 2) == -5", div_fn(-10, 2) == -5);
    edge_pass &= check("div(0, 1) == 0", div_fn(0, 1) == 0);
    edge_pass &= check("abs(-42) == 42", abs_fn(-42) == 42);
    edge_pass &= check("min(3, 7) == 3", min_fn(3, 7) == 3);
    edge_pass &= check("max(3, 7) == 7", max_fn(3, 7) == 7);
    edge_pass &= check("pow(2, 10) == 1024", pow_fn(2, 10) == 1024);
    edge_pass &= check("modulo(10, 3) == 1", modulo_fn(10, 3) == 1);

    if edge_pass {
        println!("[EDGE CASES].............. PASS");
    } else {
        println!("[EDGE CASES].............. FAIL");
        all_pass = false;
    }

    // ================================================================
    // PHASE 4: STRESS TEST (1,000,000 iterations)
    // ================================================================
    println!();
    println!("[STRESS TEST] (1,000,000 iterations)...");

    let mut stress_ok = true;
    for i in 0..1_000_000 {
        let result = add_fn(i, i + 1);
        if result != (2 * i + 1) {
            println!("  MISMATCH at iteration {}: add({}, {}) = {} (expected {})",
                i, i, i + 1, result, 2 * i + 1);
            stress_ok = false;
            break;
        }
    }

    if stress_ok {
        println!("  Completed 1,000,000 iterations without error");
        println!("[STRESS TEST]............. PASS");
    } else {
        println!("[STRESS TEST]............. FAIL");
        all_pass = false;
    }

    // ================================================================
    // PHASE 5: DETERMINISM TEST (1000 iterations)
    // ================================================================
    println!();
    println!("[DETERMINISM] (1000 iterations)...");

    let mut det_ok = true;
    let expected = add_fn(123, 456);
    for _ in 0..1000 {
        let result = add_fn(123, 456);
        if result != expected {
            println!("  MISMATCH: got {} (expected {})", result, expected);
            det_ok = false;
            break;
        }
    }

    if det_ok {
        println!("  All 1000 calls returned {} (identical)", expected);
        println!("[DETERMINISM]............. PASS");
    } else {
        println!("[DETERMINISM]............. FAIL");
        all_pass = false;
    }

    // ================================================================
    // PHASE 6: INTEGRITY CHECKS (ABI stability)
    // ================================================================
    println!();
    println!("[INTEGRITY]");

    let mut abi_ok = true;
    for _ in 0..100 {
        let r1 = add_fn(5, 3);
        let r2 = sub_fn(5, 3);
        let r3 = mul_fn(5, 3);
        let r4 = div_fn(5, 3);
        let r5 = abs_fn(-5);
        let r6 = min_fn(5, 3);
        let r7 = max_fn(5, 3);

        if r1 != 8 || r2 != 2 || r3 != 15 || r4 != 1 || r5 != 5 || r6 != 3 || r7 != 5 {
            println!("  ABI violation at iteration");
            abi_ok = false;
            break;
        }
    }

    if abi_ok {
        println!("  ABI stability: 100 mixed calls OK");
        println!("[INTEGRITY]............... PASS");
    } else {
        println!("[INTEGRITY]............... FAIL");
        all_pass = false;
    }

    // ================================================================
    // FINAL REPORT
    // ================================================================
    println!();
    println!("--------------------------------------------------");
    println!();

    if all_pass {
        println!("RESULT: ALL TESTS PASSED");
        libneodos::syscall::sys_exit(0)
    } else {
        println!("RESULT: FAILURE");
        libneodos::syscall::sys_exit(1)
    }
}
