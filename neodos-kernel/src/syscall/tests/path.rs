#![allow(unused_imports)]
//! Syscall tests — SSDT validation, permission checks, Ob create/query/set/enum,
//! and A4.6 integration tests.

use super::super::{SYSCALL_TABLE, SYSCALL_PERMISSIONS, check_syscall_permission,
           syscall_dispatch, err_to_u64, SyscallError};


pub fn register_path_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;
    use alloc::string::String;

    test_case!("path_norm_root", {
        test_eq!(super::super::normalize_dos_path("C:\\"), String::from("C:\\"));
        test_eq!(super::super::normalize_dos_path("C:"), String::from("C:\\"));
        test_eq!(super::super::normalize_dos_path("C:\\\\"), String::from("C:\\"));
    });

    test_case!("path_norm_child_and_current", {
        test_eq!(super::super::normalize_dos_path("C:\\System"), String::from("C:\\System"));
        test_eq!(super::super::normalize_dos_path("C:\\System\\Tools"), String::from("C:\\System\\Tools"));
        test_eq!(super::super::normalize_dos_path("C:\\System\\."), String::from("C:\\System"));
        test_eq!(super::super::normalize_dos_path("C:\\System\\.\\Tools"), String::from("C:\\System\\Tools"));
        test_eq!(super::super::normalize_dos_path("C:\\System\\Tools\\"), String::from("C:\\System\\Tools"));
    });

    test_case!("path_norm_parent", {
        test_eq!(super::super::normalize_dos_path("C:\\System\\Tools\\.."), String::from("C:\\System"));
        test_eq!(super::super::normalize_dos_path("C:\\System\\Tools\\..\\.."), String::from("C:\\"));
        test_eq!(super::super::normalize_dos_path("C:\\System\\..\\System"), String::from("C:\\System"));
    });

    test_case!("path_norm_root_boundary", {
        // `..` above the drive root must clamp to the root, never `C:\..` or `C:`.
        test_eq!(super::super::normalize_dos_path("C:\\.."), String::from("C:\\"));
        test_eq!(super::super::normalize_dos_path("C:\\..\\.."), String::from("C:\\"));
        test_eq!(super::super::normalize_dos_path("C:\\System\\Tools\\..\\..\\.."), String::from("C:\\"));
        test_eq!(super::super::normalize_dos_path(".."), String::from("\\"));
        test_eq!(super::super::normalize_dos_path("\\.."), String::from("\\"));
    });

    test_case!("path_norm_drive_and_separators", {
        // Lowercase drive is upper-cased, forward slashes accepted, and a
        // missing separator after `:` is normalized to `\`.
        test_eq!(super::super::normalize_dos_path("c:\\System"), String::from("C:\\System"));
        test_eq!(super::super::normalize_dos_path("C:/System/Tools"), String::from("C:\\System\\Tools"));
        test_eq!(super::super::normalize_dos_path("C:System\\Tools"), String::from("C:\\System\\Tools"));
        test_eq!(super::super::normalize_dos_path("/System"), String::from("\\System"));
    });

    // ── End-to-end resolution against the live VFS ─────────────────────
    // Absolute paths are independent of the caller's cwd, so these are
    // deterministic regardless of where the test runner lives.

    test_case!("chdir_resolve_absolute", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let root = super::super::resolve_chdir_target(String::from("C:\\"));
        test_true!(root.is_ok());
        if let Ok((drive, path)) = root {
            test_eq!(drive, 2u8);
            test_eq!(path, String::from("\\"));
        }
        let system = super::super::resolve_chdir_target(String::from("C:\\System"));
        test_true!(system.is_ok());
        if let Ok((_, path)) = system {
            test_eq!(path, String::from("\\System"));
        }
    });

    test_case!("chdir_resolve_canonical", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let up = super::super::resolve_chdir_target(String::from("C:\\System\\Tools\\..\\.."));
        test_true!(up.is_ok());
        if let Ok((_, path)) = up {
            test_eq!(path, String::from("\\"));
        }
        let dot = super::super::resolve_chdir_target(String::from("C:\\System\\.\\Tools"));
        test_true!(dot.is_ok());
        if let Ok((_, path)) = dot {
            test_eq!(path, String::from("\\System\\Tools"));
        }
        let clamp = super::super::resolve_chdir_target(String::from("C:\\System\\Tools\\..\\..\\.."));
        test_true!(clamp.is_ok());
        if let Ok((_, path)) = clamp {
            test_eq!(path, String::from("\\"));
        }
    });

    test_case!("chdir_resolve_nonexistent_does_not_commit", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let missing = super::super::resolve_chdir_target(String::from("C:\\NoSuchDirXYZ_42"));
        test_true!(missing.is_err());
    });

    test_case!("chdir_resolve_file_is_not_a_directory", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        // A regular file must be rejected even though it exists.
        let file = super::super::resolve_chdir_target(String::from("C:\\Programs\\neoinit.nxe"));
        test_true!(file.is_err());
    });
}
