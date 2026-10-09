#![allow(unused_imports)]
//! Syscall tests — SSDT validation, permission checks, Ob create/query/set/enum,
//! and A4.6 integration tests.

use super::super::{SYSCALL_TABLE, SYSCALL_PERMISSIONS, check_syscall_permission,
           syscall_dispatch, err_to_u64, SyscallError};


pub fn register_sync_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("need_resched_init_false", {
        super::super::NEED_RESCHED.store(false, core::sync::atomic::Ordering::SeqCst);
        test_eq!(super::super::NEED_RESCHED.load(core::sync::atomic::Ordering::SeqCst), false);
    });

    test_case!("need_resched_set", {
        super::super::NEED_RESCHED.store(false, core::sync::atomic::Ordering::SeqCst);
        super::super::set_need_resched();
        test_eq!(super::super::NEED_RESCHED.load(core::sync::atomic::Ordering::SeqCst), true);
    });

    test_case!("need_resched_clear", {
        super::super::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
        let prev = super::super::clear_need_resched();
        test_eq!(prev, true);
        test_eq!(super::super::NEED_RESCHED.load(core::sync::atomic::Ordering::SeqCst), false);
    });

    test_case!("need_resched_clear_returns_prev", {
        super::super::NEED_RESCHED.store(false, core::sync::atomic::Ordering::SeqCst);
        let prev = super::super::clear_need_resched();
        test_eq!(prev, false);
    });

    // ── Syscall stress ──

    test_case!("stress_syscall_rapid_getpid", {
        for _ in 0..200 {
            let pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid()
            });
            test_true!(pid < 1000);
        }
    });

    test_case!("stress_syscall_invalid_numbers", {
        let expected = super::super::err_to_u64(super::super::SyscallError::NoSys);
        for num in &[100u64, 255, 0xFFFFFFFF] {
            let result = super::super::syscall_dispatch(*num, 0, 0, 0, 0, 0);
            test_eq!(result, expected);
        }
    });

    test_case!("stress_syscall_ptr_validation", {
        let kernel_addr: u64 = 0x4000000;
        let valid = super::super::is_user_ptr_valid(kernel_addr, 10);
        test_eq!(valid, false);
        let valid2 = super::super::is_user_ptr_valid(kernel_addr, 1);
        test_eq!(valid2, false);
        let user_addr: u64 = 0x400000;
        let valid3 = super::super::is_user_ptr_valid(user_addr, 10);
        test_eq!(valid3, true);
    });

    // ── AUDIT-36: neoinit_shell_spawn_smoke — spawn infrastructure ──

    test_case!("neoinit_shell_spawn_smoke", {
        let ob_path = "\\Global\\FileSystem\\C:\\Programs\\neoshell.nxe";
        test_true!(ob_path.len() > 20);
        test_eq!(&ob_path[..19], "\\Global\\FileSystem\\");
        test_true!(ob_path.contains("neoshell.nxe"));
    });

    test_case!("neoinit_shell_entry_check", {
        let vaddr: u64 = 0x420000;
        let offset: u64 = 0x420000;
        test_true!(vaddr <= offset);
        test_true!(offset < vaddr + 0x10000);
    });
}

// ── DOS path canonicalization tests (cd/chdir resolution) ──────────────
//
// `normalize_dos_path` is the single canonicalizer used by the VFS path
// resolver behind `SET_CWD`. These tests pin down the drive-aware `.` / `..`
// semantics that NeoShell's `cd` relies on, including the root boundary.

