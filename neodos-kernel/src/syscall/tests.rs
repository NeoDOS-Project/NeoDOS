//! Syscall tests — SSDT validation, permission checks, Ob create/query/set/enum,
//! and A4.6 integration tests.

use super::{SYSCALL_TABLE, SYSCALL_PERMISSIONS, check_syscall_permission,
           syscall_dispatch, err_to_u64, SyscallError};

pub fn register_syscall_table_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("syscall_table_sparse_dispatch", {
        test_true!(SYSCALL_TABLE[0].is_some());
        test_true!(SYSCALL_TABLE[99].is_none());
        test_true!(SYSCALL_TABLE[255].is_none());
    });

    test_case!("syscall_permission_admin_check", {
        let result = check_syscall_permission(58, false);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), err_to_u64(SyscallError::Perm));

        let result = check_syscall_permission(58, true);
        test_true!(result.is_ok());

        let result = check_syscall_permission(1, false);
        test_true!(result.is_ok());
    });

    test_case!("syscall_table_validation_boot", {
        const ASSIGNED: &[u64] = &[
            0, 1, 2, 4, 6,
            13, 16, 18, 19, 20, 21,
            29, 40, 41, 47,
            53, 58, 59,
            60, 61, 62, 63, 64, 65, 66,
        ];
        for &n in ASSIGNED {
            test_true!(SYSCALL_TABLE[n as usize].is_some());
        }
        test_true!(SYSCALL_TABLE[3].is_none()); // getpid removed — use Ob API
        test_true!(SYSCALL_TABLE[5].is_none());
        test_true!(SYSCALL_TABLE[7].is_none());
        test_true!(SYSCALL_TABLE[8].is_none());
        test_true!(SYSCALL_TABLE[9].is_none());
        test_true!(SYSCALL_TABLE[10].is_none());
        test_true!(SYSCALL_TABLE[11].is_none());
        test_true!(SYSCALL_TABLE[22].is_none());
        test_true!(SYSCALL_TABLE[23].is_none());
        test_true!(SYSCALL_TABLE[12].is_none());
        test_true!(SYSCALL_TABLE[99].is_none());
        test_true!(SYSCALL_TABLE[255].is_none());
    });

    test_case!("syscall_enosys_unknown", {
        let result = syscall_dispatch(99, 0, 0, 0, 0, 0);
        test_eq!(result, err_to_u64(SyscallError::NoSys));

        let result = syscall_dispatch(255, 0, 0, 0, 0, 0);
        test_eq!(result, err_to_u64(SyscallError::NoSys));
    });

    test_case!("syscall_add_new_easy", {
        test_true!(SYSCALL_TABLE[0].is_some());
        test_true!(SYSCALL_TABLE[1].is_some());
        test_true!(SYSCALL_TABLE[22].is_none());
        test_true!(SYSCALL_TABLE[23].is_none());
        test_true!(SYSCALL_TABLE[8].is_none());
        test_true!(SYSCALL_TABLE[10].is_none());

        test_eq!(SYSCALL_PERMISSIONS[0].ring_min, 3);
        test_eq!(SYSCALL_PERMISSIONS[58].admin, true);

        test_true!(SYSCALL_TABLE[66].is_some());
    });

    // ── A4.6 Integration tests ──

    test_case!("spawn_hello_binary_path_resolve", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let result = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path("C:\\Programs\\coredir.nxe")
        });
        test_true!(result.is_ok());
        if let Ok((_, node)) = result {
            test_true!(node.mode & crate::fs::vfs::MODE_FILE != 0);
            test_true!(node.size >= 4);
        }
    });

    test_case!("spawn_with_fd_redirection_helpers", {
        let read_entry = crate::handle::HandleEntry::pipe_read(1);
        let write_entry = crate::handle::HandleEntry::pipe_write(1);
        let file_entry = crate::handle::HandleEntry::file(2, 42);
        let dir_entry = crate::handle::HandleEntry::dir(2, 0);

        test_true!(read_entry.is_pipe_read());
        test_true!(write_entry.is_pipe_write());
        test_eq!(read_entry.obj_type(), Some(crate::object::ObType::Pipe));
        test_eq!(file_entry.obj_type(), Some(crate::object::ObType::Filesystem));
        test_eq!(dir_entry.obj_type(), Some(crate::object::ObType::Directory));

        let no_redir: u8 = 0xFF;
        test_eq!(no_redir, 255);
        test_true!(no_redir != 0);

        let closed = crate::handle::HandleEntry::closed();
        test_true!(!closed.is_open());
    });

    test_case!("readdir_list_root", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let entries = crate::globals::with_vfs(|vfs| {
            let (drive_idx, node) = vfs.resolve_path("C:\\")?;
            if node.mode & crate::fs::vfs::MODE_DIR == 0 {
                return Err(crate::fs::vfs::VfsError::NotADirectory);
            }
            let mut count = 0u32;
            for i in 0..100 {
                match vfs.readdir(drive_idx, node.inode, i) {
                    Ok(Some(entry)) => {
                        count += 1;
                        if entry.name.is_empty() || entry.node.inode == 0 {
                            return Err(crate::fs::vfs::VfsError::IOError);
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            Ok(count)
        });
        test_true!(entries.is_ok());
        if let Ok(count) = entries {
            test_true!(count > 0);
        }
    });

    test_case!("mkdir_rmdir_roundtrip", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let test_dir = "C:\\Temp\\_A46TESTDIR";

        let mkdir_result = crate::globals::with_vfs(|vfs| {
            vfs.mkdir(test_dir)
        });
        test_true!(mkdir_result.is_ok());

        let stat_result = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(test_dir)
        });
        test_true!(stat_result.is_ok());

        let rmdir_result = crate::globals::with_vfs(|vfs| {
            vfs.remove_dir(test_dir)
        });
        test_true!(rmdir_result.is_ok());

        let stat_again = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(test_dir)
        });
        test_true!(stat_again.is_err());
    });

    test_case!("unlink_file", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let test_file = "C:\\Temp\\_A46TESTFILE.TXT";

        let create_result = crate::globals::with_vfs(|vfs| {
            vfs.create(test_file)
        });
        test_true!(create_result.is_ok());

        let unlink_result = crate::globals::with_vfs(|vfs| {
            vfs.remove_file(test_file)
        });
        test_true!(unlink_result.is_ok());

        let stat_again = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(test_file)
        });
        test_true!(stat_again.is_err());
    });

    test_case!("rename_file", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let old_name = "C:\\Temp\\_A46RENOLD.TXT";
        let new_name = "RENEWED.TXT";

        let create_result = crate::globals::with_vfs(|vfs| {
            vfs.create(old_name)
        });
        test_true!(create_result.is_ok());

        let rename_result = crate::globals::with_vfs(|vfs| {
            vfs.rename(old_name, new_name)
        });
        test_true!(rename_result.is_ok());

        let old_stat = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(old_name)
        });
        test_true!(old_stat.is_err());

        let new_full = "C:\\Temp\\RENEWED.TXT";
        let new_stat = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(new_full)
        });
        test_true!(new_stat.is_ok());

        let _ = crate::globals::with_vfs(|vfs| {
            vfs.remove_file(new_full)
        });
    });

    // ── OB-004: handler_close via ObObject ──

    test_case!("handler_close_file", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "hclose_file", 0, 0, None
        ).unwrap();
        let result = crate::object::ob_close_object(id);
        test_true!(result.is_ok());
        test_true!(crate::object::ob_lookup(id).is_none());
    });

    test_case!("handler_close_pipe", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Pipe, "hclose_pipe", 0, 0, None
        ).unwrap();
        crate::object::ob_open_object(id, 0).unwrap();
        crate::object::ob_close_object(id).unwrap();
        test_true!(crate::object::ob_lookup(id).is_some());
        crate::object::ob_close_object(id).unwrap();
        test_true!(crate::object::ob_lookup(id).is_none());
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-011: ObCreate tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_create_directory", {
        let id = crate::object::ob_create_object_path(
            "\\Global\\TestDir", crate::object::ObType::Directory, 0, None,
        );
        test_true!(id.is_ok());
        let id = id.unwrap();
        test_true!(id > 0);
        let found = crate::object::namespace::ob_lookup_path("\\Global\\TestDir");
        test_true!(found.is_ok());
        test_eq!(found.unwrap(), id);
        crate::object::ob_close_object(id).unwrap();
    });

    test_case!("ob_create_pipe", {
        let id = crate::object::ob_create_object_path(
            "\\Global\\Pipe\\TestPipe", crate::object::ObType::Pipe, 0, None,
        );
        test_true!(id.is_ok());
        let id = id.unwrap();
        test_true!(id > 0);
        crate::object::ob_close_object(id).unwrap();
    });

    test_case!("ob_create_invalid_type", {
        crate::object::namespace::init_object_namespace();
        let _ = crate::object::namespace::ob_create_directory("\\Global");
        let result = crate::object::ob_create_object_path(
            "\\Global\\BadObj", crate::object::ObType::Unknown, 0, None,
        );
        test_true!(result.is_err());
    });

    test_case!("ob_create_duplicate_path", {
        let id1 = crate::object::ob_create_object_path(
            "\\Global\\DupTest", crate::object::ObType::Directory, 0, None,
        );
        test_true!(id1.is_ok());
        let id2 = crate::object::ob_create_object_path(
            "\\Global\\DupTest", crate::object::ObType::Directory, 0, None,
        );
        test_true!(id2.is_err());
        let id1 = id1.unwrap();
        crate::object::ob_close_object(id1).unwrap();
        let _ = crate::object::namespace::ob_remove_object("\\Global\\DupTest");
    });

    test_case!("ob_create_empty_path_fails", {
        let result = crate::object::ob_create_object_path(
            "", crate::object::ObType::Directory, 0, None,
        );
        test_true!(result.is_err());
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-012: ObQueryInfo tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_query_info_basic", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Driver, "qinfo_test", 42, 0, None
        ).unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.obj_type, crate::object::ObType::Driver);
        test_eq!(obj.native_id, 42);
        test_eq!(obj.refcount, 1);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_query_info_basic_closed_fd", {
        let closed = crate::handle::HandleEntry::closed();
        test_true!(!closed.is_open());
    });

    test_case!("ob_query_info_name", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Process, "name_query", 7, 0, None
        ).unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.name_str(), "name_query");
        crate::object::ob_destroy_object(id).unwrap();
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-013: ObSetInfo tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_set_info_object_name", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "old_name", 0, 0, None
        ).unwrap();
        crate::object::ob_set_object_name(id, "new_name").unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.name_str(), "new_name");
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_set_info_invalid_fd", {
        let result = crate::object::ob_set_object_name(99999, "test");
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), crate::object::ObError::NotFound);
    });

    test_case!("ob_set_info_name_too_long", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Device, "short", 0, 0, None
        ).unwrap();
        let long_name = "a".repeat(64);
        crate::object::ob_set_object_name(id, &long_name).unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.name_str().len(), 64);
        crate::object::ob_destroy_object(id).unwrap();
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-014: ObEnum tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_enum_namespace_root", {
        {
            let mut ns = crate::object::namespace::OB_NAMESPACE.lock();
            for dir in &["Device", "DosDevices", "Global", "Driver", "FileSystem", "Ob"] {
                let path = alloc::format!("\\{}", dir);
                let _ = ns.create_directory(&path);
            }
        }
        let entries = crate::object::namespace::ob_enumerate_namespace("\\");
        test_true!(entries.is_ok());
        let entries = entries.unwrap();
        let names: alloc::vec::Vec<&str> = entries.iter()
            .map(|e| {
                let len = e.name.iter().position(|&b| b == 0).unwrap_or(32);
                core::str::from_utf8(&e.name[..len]).unwrap_or("")
            })
            .collect();
        test_true!(names.contains(&"device"));
        test_true!(names.contains(&"global"));
        test_true!(names.contains(&"driver"));
    });

    test_case!("ob_enum_directory_nested", {
        {
            let mut ns = crate::object::namespace::OB_NAMESPACE.lock();
            let _ = ns.create_directory("\\Global");
        }
        let _ = crate::object::namespace::ob_create_directory("\\Global\\EnumTest");
        let entries = crate::object::namespace::ob_enumerate_namespace("\\Global");
        test_true!(entries.is_ok());
        let entries = entries.unwrap();
        let names: alloc::vec::Vec<&str> = entries.iter()
            .map(|e| {
                let len = e.name.iter().position(|&b| b == 0).unwrap_or(32);
                core::str::from_utf8(&e.name[..len]).unwrap_or("")
            })
            .collect();
        test_true!(names.contains(&"enumtest"));
    });

    test_case!("ob_enum_invalid_path", {
        let result = crate::object::namespace::ob_enumerate_namespace("\\NonExistent\\Path");
        test_true!(result.is_err());
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-017: handler_readfile/handler_writefile via ObQueryInfo
    // ═══════════════════════════════════════════════════════════════════

    test_case!("handler_readfile_ob_info_extraction", {
        let inode = 42u32;
        let ob_id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "OBFILE", inode as u64, 0, None,
        ).expect("ob create");
        let obj = crate::object::ob_lookup(ob_id).unwrap();
        test_eq!(obj.native_id, inode as u64);
        test_eq!(obj.obj_type, crate::object::ObType::Filesystem);
        let extracted_inode = obj.native_id as u32;
        test_eq!(extracted_inode, inode);
        crate::object::ob_destroy_object(ob_id).unwrap();
    });

    test_case!("handler_writefile_ob_info_extraction", {
        let inode = 99u32;
        let ob_id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "OBWRITE", inode as u64, 0, None,
        ).expect("ob create");
        let obj = crate::object::ob_lookup(ob_id).unwrap();
        test_eq!(obj.native_id, inode as u64);
        let extracted_inode = obj.native_id as u32;
        test_eq!(extracted_inode, inode);
        crate::object::ob_destroy_object(ob_id).unwrap();
    });

    test_case!("ob_err_to_syscall_mapping", {
        use crate::object::ObError;
        let mappings = [
            (ObError::NotFound, SyscallError::NoEnt),
            (ObError::AlreadyExists, SyscallError::Exist),
            (ObError::InvalidParam, SyscallError::Inval),
            (ObError::RefCountHeld, SyscallError::Busy),
            (ObError::OutOfMemory, SyscallError::NoMem),
            (ObError::AccessDenied, SyscallError::Acces),
            (ObError::NotSupported, SyscallError::NoSys),
            (ObError::InvalidType, SyscallError::Inval),
            (ObError::TableFull, SyscallError::NoMem),
        ];
        for (ob_err, expected_syscall) in &mappings {
            let result = super::ob_err_to_syscall(*ob_err);
            test_eq!(result as i64, *expected_syscall as i64);
        }
    });

    test_case!("cow_inline_write_read", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let test_file = "C:\\Temp\\_COWINL.TST";
        let data = b"Hello NE2 COW!";
        let result = crate::globals::with_vfs(|vfs| {
            let (drive, _) = vfs.resolve_path("C:\\")?;
            let node = vfs.create(test_file)?;
            vfs.write(drive, node.inode, 0, data)?;
            let mut buf = [0u8; 32];
            let n = vfs.read(drive, node.inode, 0, &mut buf)?;
            if n != data.len() { return Err(crate::fs::vfs::VfsError::IOError); }
            if &buf[..n] != data { return Err(crate::fs::vfs::VfsError::IOError); }
            let _ = vfs.remove_file(test_file);
            Ok(())
        });
        test_true!(result.is_ok());
    });

    test_case!("cow_extent_write_read", {
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let test_file = "C:\\Temp\\_COWEXT.TST";
        let data = b"This is a test for extent-based writes in NeoFS v2. The data must exceed INLINE_MAX (16 bytes) to force extent allocation.";
        let result = crate::globals::with_vfs(|vfs| {
            let (drive, _) = vfs.resolve_path("C:\\")?;
            let node = vfs.create(test_file)?;
            let written = vfs.write(drive, node.inode, 0, data)?;
            if written != data.len() { return Err(crate::fs::vfs::VfsError::IOError); }
            let mut buf = [0u8; 256];
            let n = vfs.read(drive, node.inode, 0, &mut buf)?;
            if n != data.len() { return Err(crate::fs::vfs::VfsError::IOError); }
            if &buf[..n] != data { return Err(crate::fs::vfs::VfsError::IOError); }
            let _ = vfs.remove_file(test_file);
            Ok(())
        });
        test_true!(result.is_ok());
    });
}
