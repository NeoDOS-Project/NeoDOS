//! Shell test binary — runs tokenizer + pipeline tests on real NeoDOS hardware.
//! Output format matches kernel test output for auto_test.py parsing:
//!   PASS: test_name
//!   FAIL: test_name [reason]
//!   TESTS: X total, Y passed, Z failed

#![no_std]
#![no_main]

extern crate neoshell_lib;

use libneodos::syscall;
use neoshell_lib::tokenizer::{self, TokenKind, ParseError};
use neoshell_lib::pipeline::{self, MAX_PIPELINE};

// ── Test infrastructure ─────────────────────────

type TestFn = fn() -> Result<(), &'static str>;

struct TestCase {
    name: &'static str,
    func: TestFn,
}

macro_rules! test_case {
    ($name:expr, $func:expr) => {
        TestCase { name: $name, func: $func }
    };
}

fn write(s: &[u8]) { let _ = syscall::sys_write(1, s); }
fn writeln(s: &[u8]) { write(s); write(b"\r\n"); }

fn run_tests(tests: &[TestCase]) -> (usize, usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    for test in tests {
        match (test.func)() {
            Ok(()) => {
                write(b"PASS: ");
                writeln(test.name.as_bytes());
                passed += 1;
            }
            Err(reason) => {
                write(b"FAIL: ");
                write(test.name.as_bytes());
                write(b" [");
                write(reason.as_bytes());
                writeln(b"]");
                failed += 1;
            }
        }
    }
    (tests.len(), passed, failed)
}

// ── Tokenizer tests ─────────────────────────────

fn check_token(
    r: &tokenizer::TokenizeResult,
    index: usize,
    expected_kind: TokenKind,
    expected_val: &[u8],
) -> Result<(), &'static str> {
    let (kind, val) = r.get(index).ok_or("token index out of range")?;
    if kind != expected_kind {
        return Err("token kind mismatch");
    }
    if val != expected_val {
        return Err("token value mismatch");
    }
    Ok(())
}

fn test_pipe() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"cmd1 | cmd2");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 3 { return Err("expected 3 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"cmd1")?;
    check_token(&r, 1, TokenKind::Pipe, b"")?;
    check_token(&r, 2, TokenKind::Word, b"cmd2")?;
    Ok(())
}

fn test_redirect_stdout() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"dir > out.txt");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 2 { return Err("expected 2 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"dir")?;
    check_token(&r, 1, TokenKind::RedirectStdout, b"out.txt")?;
    Ok(())
}

fn test_redirect_append() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"echo hi >> log.txt");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 3 { return Err("expected 3 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"echo")?;
    check_token(&r, 1, TokenKind::Word, b"hi")?;
    check_token(&r, 2, TokenKind::RedirectAppend, b"log.txt")?;
    Ok(())
}

fn test_redirect_stdin() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"sort < input.txt");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 2 { return Err("expected 2 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"sort")?;
    check_token(&r, 1, TokenKind::RedirectStdin, b"input.txt")?;
    Ok(())
}

fn test_redirect_stderr() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"cmd 2> err.txt");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 2 { return Err("expected 2 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"cmd")?;
    check_token(&r, 1, TokenKind::RedirectStderr, b"err.txt")?;
    Ok(())
}

fn test_multiple_redirects() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"cmd < in.txt > out.txt");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 3 { return Err("expected 3 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"cmd")?;
    check_token(&r, 1, TokenKind::RedirectStdin, b"in.txt")?;
    check_token(&r, 2, TokenKind::RedirectStdout, b"out.txt")?;
    Ok(())
}

fn test_quoted_arg() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"echo 'hello %PATH%'");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 2 { return Err("expected 2 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"echo")?;
    check_token(&r, 1, TokenKind::Word, b"hello %PATH%")?;
    Ok(())
}

fn test_double_quotes() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"echo \"hello world\"");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 2 { return Err("expected 2 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"echo")?;
    check_token(&r, 1, TokenKind::Word, b"hello world")?;
    Ok(())
}

fn test_escape_char() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"echo hello^| more");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 3 { return Err("expected 3 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"echo")?;
    check_token(&r, 1, TokenKind::Word, b"hello|")?;
    check_token(&r, 2, TokenKind::Word, b"more")?;
    Ok(())
}

fn test_semicolon() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"cd src; dir");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 4 { return Err("expected 4 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"cd")?;
    check_token(&r, 1, TokenKind::Word, b"src")?;
    check_token(&r, 2, TokenKind::Semicolon, b"")?;
    check_token(&r, 3, TokenKind::Word, b"dir")?;
    Ok(())
}

fn test_unmatched_double_quote() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"echo \"hello");
    if r.error != Some(ParseError::UnmatchedDoubleQuote) {
        return Err("expected UnmatchedDoubleQuote error");
    }
    if r.token_count != 2 { return Err("expected 2 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"echo")?;
    check_token(&r, 1, TokenKind::Word, b"hello")?;
    Ok(())
}

fn test_empty_input() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 0 { return Err("expected 0 tokens"); }
    Ok(())
}

fn test_escape_in_double_quote() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"echo \"hello ^\"world\"");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 2 { return Err("expected 2 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"echo")?;
    check_token(&r, 1, TokenKind::Word, b"hello \"world")?;
    Ok(())
}

fn test_multiple_spaces() -> Result<(), &'static str> {
    let r = tokenizer::tokenize(b"  cmd1   |   cmd2  ");
    if r.error.is_some() { return Err("unexpected error"); }
    if r.token_count != 3 { return Err("expected 3 tokens"); }
    check_token(&r, 0, TokenKind::Word, b"cmd1")?;
    check_token(&r, 1, TokenKind::Pipe, b"")?;
    check_token(&r, 2, TokenKind::Word, b"cmd2")?;
    Ok(())
}

// ── Pipeline tests ──────────────────────────────

fn test_pipeline_no_pipe() -> Result<(), &'static str> {
    let mut pos = [0usize; MAX_PIPELINE];
    let n = pipeline::parse_pipeline(b"echo hello", &mut pos);
    if n != 0 { return Err("expected 0 pipes"); }
    Ok(())
}

fn test_pipeline_single_pipe() -> Result<(), &'static str> {
    let mut pos = [0usize; MAX_PIPELINE];
    let n = pipeline::parse_pipeline(b"cmd1 | cmd2", &mut pos);
    if n != 1 { return Err("expected 1 pipe"); }
    if pos[0] != 5 { return Err("pipe at wrong position"); }
    Ok(())
}

fn test_pipeline_multiple_pipes() -> Result<(), &'static str> {
    let mut pos = [0usize; MAX_PIPELINE];
    let n = pipeline::parse_pipeline(b"a | b | c | d", &mut pos);
    if n != 3 { return Err("expected 3 pipes"); }
    if pos[0] != 2 { return Err("first pipe at wrong position"); }
    if pos[1] != 6 { return Err("second pipe at wrong position"); }
    if pos[2] != 10 { return Err("third pipe at wrong position"); }
    Ok(())
}

fn test_pipeline_empty_input() -> Result<(), &'static str> {
    let mut pos = [0usize; MAX_PIPELINE];
    let n = pipeline::parse_pipeline(b"", &mut pos);
    if n != 0 { return Err("expected 0 pipes"); }
    Ok(())
}

fn test_pipeline_at_start() -> Result<(), &'static str> {
    let mut pos = [0usize; MAX_PIPELINE];
    let n = pipeline::parse_pipeline(b"| cmd", &mut pos);
    if n != 1 { return Err("expected 1 pipe"); }
    if pos[0] != 0 { return Err("pipe should be at position 0"); }
    Ok(())
}

fn test_pipeline_at_end() -> Result<(), &'static str> {
    let mut pos = [0usize; MAX_PIPELINE];
    let n = pipeline::parse_pipeline(b"cmd |", &mut pos);
    if n != 1 { return Err("expected 1 pipe"); }
    if pos[0] != 4 { return Err("pipe at wrong position"); }
    Ok(())
}

// ── Main ────────────────────────────────────────

static TESTS: &[TestCase] = &[
    // Tokenizer
    test_case!("tokenizer_pipe", test_pipe),
    test_case!("tokenizer_redirect_stdout", test_redirect_stdout),
    test_case!("tokenizer_redirect_append", test_redirect_append),
    test_case!("tokenizer_redirect_stdin", test_redirect_stdin),
    test_case!("tokenizer_redirect_stderr", test_redirect_stderr),
    test_case!("tokenizer_multiple_redirects", test_multiple_redirects),
    test_case!("tokenizer_quoted_arg", test_quoted_arg),
    test_case!("tokenizer_double_quotes", test_double_quotes),
    test_case!("tokenizer_escape_char", test_escape_char),
    test_case!("tokenizer_semicolon", test_semicolon),
    test_case!("tokenizer_unmatched_double_quote", test_unmatched_double_quote),
    test_case!("tokenizer_empty", test_empty_input),
    test_case!("tokenizer_escape_in_double_quote", test_escape_in_double_quote),
    test_case!("tokenizer_multiple_spaces", test_multiple_spaces),
    // Pipeline
    test_case!("pipeline_no_pipe", test_pipeline_no_pipe),
    test_case!("pipeline_single_pipe", test_pipeline_single_pipe),
    test_case!("pipeline_multiple_pipes", test_pipeline_multiple_pipes),
    test_case!("pipeline_empty_input", test_pipeline_empty_input),
    test_case!("pipeline_at_start", test_pipeline_at_start),
    test_case!("pipeline_at_end", test_pipeline_at_end),
];

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (_total, passed, failed) = run_tests(TESTS);

    // Summary in kernel test format for auto_test.py
    write(b"[SHTEST] ");
    write_u64(passed as u64);
    write(b" passed, ");
    write_u64(failed as u64);
    write(b" failed\r\n");

    if failed == 0 {
        write(b"SHELL_TESTS_PASSED\r\n");
    }
    write(b"SHTEST_COMPLETE\r\n");

    if failed > 0 { syscall::sys_exit(1); }
    syscall::sys_exit(0);
}

fn write_u64(v: u64) {
    if v == 0 { write(b"0"); return; }
    let mut b = [0u8; 20];
    let mut i = 19;
    let mut n = v;
    while n > 0 {
        b[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if i == 0 { break; }
        i -= 1;
    }
    write(&b[i..=19]);
}
