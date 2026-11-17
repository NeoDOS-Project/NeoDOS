pub mod types;
pub mod pipe;
pub mod timer;
pub mod semaphore;
pub mod section;
pub mod namespace;
pub mod power;

pub mod table;
pub mod security;
#[path = "enum.rs"] pub mod enum_mod;

pub use types::{ObError, ObId, ObType, OB_NAME_LEN};
pub use types::{ObObjectSnapshot, ObEnumEntry};
pub(crate) use table::{ObObject, ObObjectTable, ObOperations, FileHandleOps, FILE_HANDLE_OPS, OB_TABLE, OB_SECURITY, init_object_manager, ob_create_object, ob_destroy_object, ob_lookup, ob_open_object, ob_close_object, ob_reference, ob_dereference, ob_refcount_stats, ob_set_refcount_for_test, ob_count, ob_enum_snapshot, ob_set_object_name, ob_set_security, ob_create_object_path};
pub use security::ob_open_path;
pub use enum_mod::ob_enum_directory;

// NEODOS-06 (#636): Ob dispatch — handlers consult the object's `ObOperations`
// first; `None` means "not handled by Ob, use the legacy path".
pub fn ob_dispatch_read(object_id: ObId, offset: u64, buf: &mut [u8]) -> Option<usize> {
    let obj = ob_lookup(object_id)?;
    obj.ops?.read(object_id, obj.native_id, offset, buf)
}
pub fn ob_dispatch_write(object_id: ObId, offset: u64, buf: &[u8]) -> Option<usize> {
    let obj = ob_lookup(object_id)?;
    obj.ops?.write(object_id, obj.native_id, offset, buf)
}
pub fn ob_dispatch_ioctl(object_id: ObId, cmd: u32, arg: u64) -> Option<i64> {
    let obj = ob_lookup(object_id)?;
    obj.ops?.ioctl(object_id, obj.native_id, cmd, arg)
}
pub fn ob_dispatch_wait(object_id: ObId) -> Option<bool> {
    let obj = ob_lookup(object_id)?;
    obj.ops?.wait(object_id, obj.native_id)
}

pub fn register_object_tests() {
    use crate::{test_case, test_eq, test_false, test_true};
    namespace::register_namespace_tests();

    test_case!("ob_create_lookup", {
        let id = ob_create_object(ObType::Process, "test_proc", 42, 0, None).unwrap();
        test_true!(id > 0);
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.id, id);
        test_eq!(obj.obj_type, ObType::Process);
        test_eq!(obj.native_id, 42);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_destroy_fails_with_ref", {
        let id = ob_create_object(ObType::Driver, "test_drv", 1, 0, None).unwrap();
        ob_reference(id).unwrap();
        let result = ob_destroy_object(id);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::RefCountHeld);
        ob_dereference(id).unwrap();
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_refcount", {
        let id = ob_create_object(ObType::Device, "ref_test", 0, 0, None).unwrap();
        let r1 = ob_reference(id).unwrap();
        test_eq!(r1, 2);
        let r2 = ob_dereference(id).unwrap();
        test_eq!(r2, 1);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_double_destroy_fails", {
        let id = ob_create_object(ObType::MemoryRegion, "double", 0, 0, None).unwrap();
        ob_destroy_object(id).unwrap();
        let result = ob_destroy_object(id);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::NotFound);
    });

    // #661 probe: an ObOperations whose on_destroy records what it can observe
    // about its own object. After the fix the object must already be unlinked.
    static PROBE_CALLS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static PROBE_REACHABLE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    static PROBE_REF_OK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    struct ProbeOnDestroyOps;
    impl ObOperations for ProbeOnDestroyOps {
        fn on_destroy(&self, id: ObId, _native_id: u64) {
            use core::sync::atomic::Ordering;
            PROBE_CALLS.fetch_add(1, Ordering::SeqCst);
            if ob_lookup(id).is_some() {
                PROBE_REACHABLE.store(true, Ordering::SeqCst);
            }
            if ob_reference(id).is_ok() {
                PROBE_REF_OK.store(true, Ordering::SeqCst);
            }
        }
    }
    static PROBE_OPS: ProbeOnDestroyOps = ProbeOnDestroyOps;

    test_case!("ob_close_finalize_unlinks_before_callback", {
        // #661: the object must be unlinked BEFORE on_destroy runs, so it cannot
        // be resurrected by a concurrent reference/open and on_destroy cannot
        // run twice.
        use core::sync::atomic::Ordering;
        PROBE_CALLS.store(0, Ordering::SeqCst);
        PROBE_REACHABLE.store(false, Ordering::SeqCst);
        PROBE_REF_OK.store(false, Ordering::SeqCst);
        let id = ob_create_object(ObType::Event, "finalize_probe", 0, 0, Some(&PROBE_OPS)).unwrap();
        ob_close_object(id).unwrap();
        test_eq!(PROBE_CALLS.load(Ordering::SeqCst), 1);
        test_false!(PROBE_REACHABLE.load(Ordering::SeqCst));
        test_false!(PROBE_REF_OK.load(Ordering::SeqCst));
        test_true!(ob_lookup(id).is_none());
        // A second close is NotFound and must NOT invoke on_destroy again.
        test_eq!(ob_close_object(id).unwrap_err(), ObError::NotFound);
        test_eq!(PROBE_CALLS.load(Ordering::SeqCst), 1);
    });

    test_case!("ob_destroy_removes_security_descriptor", {
        // #664: the SecurityDescriptor is released on destroy (no leak, no
        // bleed onto a reused ObId).
        use crate::security::acl::SecurityDescriptor;
        let id = ob_create_object(ObType::Event, "sd_probe", 0, 0, None).unwrap();
        ob_set_security(id, SecurityDescriptor::new()).unwrap();
        test_true!(OB_SECURITY.lock().contains_key(&id));
        ob_destroy_object(id).unwrap();
        test_true!(!OB_SECURITY.lock().contains_key(&id));
    });

    test_case!("ob_refcount_overflow_underflow_observable", {
        // #663: refcount saturation and underflow are observable, not silent.
        let (o0, u0) = ob_refcount_stats();
        let id = ob_create_object(ObType::Event, "rc_probe", 0, 0, None).unwrap();
        // Underflow: decrement an already-zero count.
        ob_dereference(id).unwrap(); // 1 -> 0
        ob_dereference(id).unwrap(); // 0 -> underflow counted
        let (_o1, u1) = ob_refcount_stats();
        test_eq!(u1 - u0, 1);
        // Overflow: reference at u32::MAX saturates and is counted.
        ob_set_refcount_for_test(id, u32::MAX).unwrap();
        ob_reference(id).unwrap();
        let (o2, _u2) = ob_refcount_stats();
        test_eq!(o2 - o0, 1);
        // Restore a destroyable count and clean up.
        ob_set_refcount_for_test(id, 1).unwrap();
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_lookup_not_found", {
        let result = ob_lookup(9999);
        test_true!(result.is_none());
    });

    test_case!("neodos05_ob_id_index_consistency", {
        // Create/destroy churn: the O(log n) id index must stay consistent, and
        // freed slots must be reusable under a fresh id (NEODOS-05 / #635).
        let mut ids = alloc::vec::Vec::new();
        for i in 0..32u64 {
            ids.push(ob_create_object(ObType::Event, "idx", i, 0, None).unwrap());
        }
        for (i, id) in ids.iter().enumerate() {
            if i % 2 == 0 { ob_destroy_object(*id).unwrap(); }
        }
        for (i, id) in ids.iter().enumerate() {
            test_eq!(ob_lookup(*id).is_some(), i % 2 == 1);
        }
        // Slot reuse: a fresh object gets a new id that also resolves.
        let fresh = ob_create_object(ObType::Event, "idx_reuse", 999, 0, None).unwrap();
        test_true!(ob_lookup(fresh).is_some());
        test_true!(!ids.contains(&fresh));
        ob_destroy_object(fresh).unwrap();
        for (i, id) in ids.iter().enumerate() {
            if i % 2 == 1 { ob_destroy_object(*id).unwrap(); }
        }
        for id in &ids {
            test_true!(ob_lookup(*id).is_none());
        }
    });

    test_case!("ob_enum_snapshot", {
        let start_count = ob_count();
        let id1 = ob_create_object(ObType::Process, "snap1", 10, 0, None).unwrap();
        let id2 = ob_create_object(ObType::Driver, "snap2", 20, 0, None).unwrap();
        let snap = ob_enum_snapshot();
        test_eq!(snap.len(), start_count + 2);
        let snap1 = snap.iter().find(|s| s.id == id1).unwrap();
        test_eq!(snap1.name, "snap1");
        test_eq!(snap1.obj_type, ObType::Process);
        ob_destroy_object(id1).unwrap();
        ob_destroy_object(id2).unwrap();
    });

    test_case!("ob_open_close", {
        let id = ob_create_object(ObType::Filesystem, "open_close", 99, 0, None).unwrap();
        ob_open_object(id, 0).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.refcount, 2);
        ob_close_object(id).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.refcount, 1);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_type_strings", {
        test_eq!(ObType::Process.to_str(), "PROCESS");
        test_eq!(ObType::Driver.to_str(), "DRIVER");
        test_eq!(ObType::Unknown.to_str(), "UNKNOWN");
        test_eq!(ObType::Key.to_str(), "REGKEY");
        test_eq!(ObType::Semaphore.to_str(), "SEMAPHORE");
        test_eq!(ObType::Timer.to_str(), "TIMER");
        test_eq!(ObType::Section.to_str(), "SECTION");
        test_eq!(ObType::Session.to_str(), "SESSION");
    });

    test_case!("ob_session_object", {
        // USR-P1a: Session is a first-class ObType (=19) usable as an object.
        test_eq!(ObType::Session as u32, 19);
        let id = ob_create_object(ObType::Session, "p1a_session", 0, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.obj_type, ObType::Session);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_error_codes", {
        test_eq!(ObError::NotFound.as_err_code(), -1);
        test_eq!(ObError::RefCountHeld.as_err_code(), -4);
        test_eq!(ObError::Success.as_err_code(), 0);
        test_eq!(ObError::Success.to_str(), "SUCCESS");
        test_eq!(ObError::RefCountHeld.to_str(), "REFCOUNT_HELD");
    });

    // ── OB-004: ob_close_object auto-destroy ──

    test_case!("ob_close_object_auto_destroy", {
        let id = ob_create_object(ObType::Filesystem, "close_file", 0, 0, None).unwrap();
        let before = ob_count();
        ob_close_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());
        test_eq!(ob_count(), before - 1);
    });

    test_case!("ob_close_object_keeps_alive_with_refs", {
        let id = ob_create_object(ObType::Pipe, "close_pipe", 0, 0, None).unwrap();
        ob_open_object(id, 0).unwrap(); // refcount 1→2
        ob_close_object(id).unwrap();   // refcount 2→1 (kept alive)
        test_true!(ob_lookup(id).is_some());
        test_eq!(ob_lookup(id).unwrap().refcount, 1);
        ob_close_object(id).unwrap();   // refcount 1→0 → auto-destroy
        test_true!(ob_lookup(id).is_none());
    });

    // ── OB-005: init_object_manager creates root + base types ──

    test_case!("ob_init_root_directory", {
        let snap = ob_enum_snapshot();
        test_true!(ob_count() >= 11);
        let root = snap.iter().find(|s| s.name == "\\");
        test_true!(root.is_some());
        if let Some(r) = root {
            test_eq!(r.obj_type, ObType::Directory);
        }
    });

    test_case!("ob_init_type_entries", {
        let snap = ob_enum_snapshot();
        let names: alloc::vec::Vec<&str> = snap.iter().map(|s| s.name.as_str()).collect();
        test_true!(names.contains(&"Process"));
        test_true!(names.contains(&"Pipe"));
        test_true!(names.contains(&"Device"));
        test_true!(names.contains(&"Filesystem"));
    });

    // ── OB-010: ob_open_path tests ──

    test_case!("ob_open_path_existing_object", {
        // Register an object and insert it into the namespace
        let id = ob_create_object(ObType::Driver, "test_drv", 42, 0, None).unwrap();
        let _ = crate::object::namespace::ob_create_directory("\\Driver"); // ensure dir exists
        let _ = crate::object::namespace::ob_insert_object("\\Driver\\test_drv", id);

        let admin_token = crate::security::token::Token::new_admin();
        let opened_id = ob_open_path("\\Driver\\test_drv", &admin_token,
            crate::security::acl::ACCESS_READ).unwrap();
        test_eq!(opened_id, id);
        // refcount should be 2 (1 from create + 1 from open)
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.refcount, 2);

        // Cleanup: close releases the open reference
        ob_close_object(id).unwrap();
        ob_destroy_object(id).unwrap();
        let _ = crate::object::namespace::ob_remove_object("\\Driver\\test_drv");
    });

    test_case!("ob_open_path_not_found", {
        let admin_token = crate::security::token::Token::new_admin();
        let result = ob_open_path("\\NonExistent\\Path", &admin_token,
            crate::security::acl::ACCESS_READ);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::NotFound);
    });

    test_case!("ob_open_path_access_denied", {
        // Create an object with a restrictive SD (deny user access)
        let id = ob_create_object(ObType::Driver, "secure_drv", 0, 0, None).unwrap();
        let _ = crate::object::namespace::ob_create_directory("\\Driver");
        let _ = crate::object::namespace::ob_insert_object("\\Driver\\secure_drv", id);

        // Set a SD that denies user tokens ACCESS_READ
        use crate::security::acl::{Acl, Ace, SecurityDescriptor};
        let mut acl = Acl::new();
        let user_sid = crate::security::sid::sid_builtin_user();
        acl.add_ace(Ace::deny(user_sid, crate::security::acl::ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);
        ob_set_security(id, sd).unwrap();

        // Try to open with a user token → should be denied
        let user_token = crate::security::token::Token::new_user();
        let result = ob_open_path("\\Driver\\secure_drv", &user_token,
            crate::security::acl::ACCESS_READ);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::AccessDenied);

        // Admin should still be able to open (admin bypass)
        let admin_token = crate::security::token::Token::new_admin();
        let opened_id = ob_open_path("\\Driver\\secure_drv", &admin_token,
            crate::security::acl::ACCESS_READ).unwrap();
        test_eq!(opened_id, id);

        ob_close_object(id).unwrap();
        ob_destroy_object(id).unwrap();
        let _ = crate::object::namespace::ob_remove_object("\\Driver\\secure_drv");
    });

    test_case!("ob_open_path_non_existent_object_in_namespace", {
        // Path exists in namespace but ObId doesn't match any ObObject
        let admin_token = crate::security::token::Token::new_admin();
        let result = ob_open_path("\\Driver\\nonexistent", &admin_token,
            crate::security::acl::ACCESS_READ);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::NotFound);
    });

    // ── OBF-03: ObType::Thread ──

    test_case!("ob_type_thread_enum", {
        let t = ObType::Thread;
        test_eq!(t as u32, 16);
        test_eq!(t.to_str(), "THREAD");
    });

    // ── OBF-01: ObInfoClass variants ──

    test_case!("ob_info_class_variants", {
        test_eq!(crate::object::types::ObInfoClass::CpuInfo as u32, 7);
        test_eq!(crate::object::types::ObInfoClass::ReadContent as u32, 15);
        test_eq!(crate::object::types::ObInfoClass::VolumeLabel as u32, 16);
        test_eq!(crate::object::types::ObInfoClass::Basic as u32, 0);
        test_eq!(crate::object::types::ObInfoClass::Process as u32, 3);
    });

    // ── OBF-02: ObSetInfoClass variants ──

    test_case!("ob_set_info_class_variants", {
        test_eq!(crate::object::types::ObSetInfoClass::ProcessTerminate as u32, 4);
        test_eq!(crate::object::types::ObSetInfoClass::VfsRename as u32, 6);
        test_eq!(crate::object::types::ObSetInfoClass::WriteContent as u32, 7);
        test_eq!(crate::object::types::ObSetInfoClass::SetCwd as u32, 8);
        test_eq!(crate::object::types::ObSetInfoClass::SetVolumeLabel as u32, 9);
        test_eq!(crate::object::types::ObSetInfoClass::ProcessPriority as u32, 0);
        test_eq!(crate::object::types::ObSetInfoClass::SetProcessVt as u32, 17);
    });

    // ── OBF-04: Thread ObObject lifecycle ──

    test_case!("ob_thread_create_and_destroy", {
        let id = ob_create_object(ObType::Thread, "\\Ob\\Thread\\42", 42, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.obj_type, ObType::Thread);
        test_eq!(obj.native_id, 42);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_thread_type_in_enum_snapshot", {
        let id = ob_create_object(ObType::Thread, "\\Ob\\Thread\\99", 99, 0, None).unwrap();
        let snap = ob_enum_snapshot();
        let found = snap.iter().find(|s| s.id == id).unwrap();
        test_eq!(found.obj_type, ObType::Thread);
        test_eq!(found.native_id, 99);
        ob_destroy_object(id).unwrap();
    });

    // ── Legacy compat tests (migrated from kobj/mod.rs) ──

    test_case!("kobj_register_unregister", {
        let id = ob_create_object(ObType::Process, "test_proc", 42, 0, None).unwrap();
        test_true!(id > 0);
        ob_destroy_object(id).unwrap();
    });

    test_case!("kobj_refcount", {
        let id = ob_create_object(ObType::Driver, "test_drv", 1, 0, None).unwrap();
        let r1 = ob_reference(id).unwrap();
        test_eq!(r1, 2);
        let r2 = ob_dereference(id).unwrap();
        test_eq!(r2, 1);
        ob_destroy_object(id).unwrap();
    });

    test_case!("kobj_type_enum", {
        test_eq!(ObType::Process.to_str(), "PROCESS");
        test_eq!(ObType::Driver.to_str(), "DRIVER");
        test_eq!(ObType::Pipe.to_str(), "PIPE");
        test_eq!(ObType::Symlink.to_str(), "SYMLINK");
        test_eq!(ObType::MountPoint.to_str(), "MOUNTPOINT");
        test_eq!(ObType::Unknown.to_str(), "UNKNOWN");
    });

    test_case!("kobj_entry_name", {
        let id = ob_create_object(ObType::Device, "my_device", 0, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.name_str(), "my_device");
        test_eq!(obj.obj_type, ObType::Device);
        test_eq!(obj.native_id, 0);
        ob_destroy_object(id).unwrap();
    });

    test_case!("kobj_registry_dynamic", {
        let mut ids = alloc::vec::Vec::new();
        for i in 0..128 {
            let name = alloc::format!("fill_{}", i);
            if let Ok(id) = ob_create_object(ObType::Unknown, &name, 0, 0, None) {
                ids.push(id);
            } else {
                break;
            }
        }
        test_eq!(ids.len(), 128);
        let one_more_id = ob_create_object(ObType::Unknown, "one_more", 0, 0, None).unwrap();
        let extra_id = ob_create_object(ObType::Unknown, "extra", 0, 0, None).unwrap();
        test_true!(extra_id > 0);
        for id in ids {
            ob_destroy_object(id).unwrap();
        }
        ob_destroy_object(one_more_id).unwrap();
        ob_destroy_object(extra_id).unwrap();
    });

    test_case!("kobj_lookup", {
        let id = ob_create_object(ObType::Filesystem, "lookup_test", 99, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.native_id, 99);
        test_eq!(obj.obj_type, ObType::Filesystem);
        ob_destroy_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());
    });

    test_case!("kobj_double_unregister", {
        let id = ob_create_object(ObType::MemoryRegion, "double", 0, 0, None).unwrap();
        test_true!(ob_destroy_object(id).is_ok());
        test_true!(ob_destroy_object(id).is_err());
    });

    test_case!("kobj_count", {
        let start = ob_count();
        let id1 = ob_create_object(ObType::Process, "cnt1", 0, 0, None).unwrap();
        let id2 = ob_create_object(ObType::Driver, "cnt2", 0, 0, None).unwrap();
        test_eq!(ob_count(), start + 2);
        ob_destroy_object(id1).unwrap();
        test_eq!(ob_count(), start + 1);
        ob_destroy_object(id2).unwrap();
        test_eq!(ob_count(), start);
    });

    // ── VFS-1.2: ObOpen → VFS ownership ──

    test_case!("vfs_ownership_obid_valid_after_close", {
        // Verify an ObObject created with FileHandleOps can be closed properly
        let id = ob_create_object(ObType::Filesystem, "test_file", 42, 0, Some(&FILE_HANDLE_OPS)).unwrap();
        test_true!(id > 0);
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.native_id, 42);
        test_eq!(obj.obj_type, ObType::Filesystem);
        test_true!(obj.ops.is_some());

        // ob_close_object should deref then auto-destroy (refcount was 1, goes to 0)
        ob_close_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());
    });

    test_case!("vfs_ownership_namespace_entry_cleanup", {
        // Verify file handles do NOT create namespace entries
        // (VFS-1.2: ephemeral ObObject without namespace insertion)
        let id = ob_create_object(ObType::Filesystem, "test_no_ns", 99, 0, Some(&FILE_HANDLE_OPS)).unwrap();
        test_true!(id > 0);

        // Should NOT be findable via namespace lookup
        let ns_result = crate::object::namespace::ob_lookup_path("\\Global\\FileSystem\\C:\\test_no_ns");
        test_true!(ns_result.is_err());

        // Clean up
        ob_close_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());
    });

    // ── VFS-1.3: Stale namespace entry cleanup ──

    test_case!("vfs_namespace_cleanup_on_destroy", {
        // Create an object and insert it into the namespace
        let id = ob_create_object(ObType::Filesystem, "ns_destroy", 42, 0, None).unwrap();
        let _ = crate::object::namespace::ob_create_directory("\\TestDir");
        let _ = crate::object::namespace::ob_insert_object("\\TestDir\\cleanup_destroy", id);

        // Verify it's in the namespace
        let ns_id = crate::object::namespace::ob_lookup_path("\\TestDir\\cleanup_destroy").unwrap();
        test_eq!(ns_id, id);

        // Destroy the object -> should also remove the namespace entry
        ob_destroy_object(id).unwrap();

        // Verify namespace entry is gone
        let ns_result = crate::object::namespace::ob_lookup_path("\\TestDir\\cleanup_destroy");
        test_true!(ns_result.is_err());
    });

    test_case!("vfs_namespace_cleanup_on_close", {
        // Create an object and insert it into the namespace
        let id = ob_create_object(ObType::Filesystem, "ns_close", 99, 0, None).unwrap();
        let _ = crate::object::namespace::ob_insert_object("\\TestDir\\cleanup_close", id);

        // Verify it's in the namespace
        let ns_id = crate::object::namespace::ob_lookup_path("\\TestDir\\cleanup_close").unwrap();
        test_eq!(ns_id, id);

        // Close the object (refcount 1→0) -> should auto-destroy and clean up namespace
        ob_close_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());

        // Verify namespace entry is gone
        let ns_result = crate::object::namespace::ob_lookup_path("\\TestDir\\cleanup_close");
        test_true!(ns_result.is_err());
    });

    // ── NS-1.1 / NS-1.2: Protected root dirs via ob_insert_object_checked ──

    test_case!("ob_insert_object_checked_rejects_protected_parent", {
        let _ = crate::object::namespace::ob_create_directory("\\TestNS");
        let id = ob_create_object(ObType::Filesystem, "test_protected", 1, 0, None).unwrap();
        // Mark \TestNS as protected
        let _ = crate::object::namespace::ob_set_protected("\\TestNS", true);
        let result = crate::object::namespace::ob_insert_object_checked("\\TestNS\\Hacked", id);
        test_true!(result.is_err());
        // Cleanup: remove the protection, then destroy via the regular insert
        let _ = crate::object::namespace::ob_set_protected("\\TestNS", false);
        let _ = crate::object::namespace::ob_insert_object("\\TestNS\\Hacked", id);
        ob_destroy_object(id).unwrap_or(());
    });

    test_case!("ob_insert_object_checked_allows_normal_insert", {
        let _ = crate::object::namespace::ob_create_directory("\\TestNS");
        let id = ob_create_object(ObType::Filesystem, "test_normal", 1, 0, None).unwrap();
        // Mark it protected, but insert under a non-protected subdir
        let _ = crate::object::namespace::ob_set_protected("\\TestNS", true);
        let _ = crate::object::namespace::ob_create_directory("\\TestNS\\Sub");
        let result = crate::object::namespace::ob_insert_object_checked("\\TestNS\\Sub\\Normal", id);
        test_true!(result.is_ok());
        let _ = crate::object::namespace::ob_remove_object("\\TestNS\\Sub\\Normal");
        ob_destroy_object(id).unwrap_or(());
    });

    test_case!("ob_create_directory_checked_rejects_protected_parent", {
        let _ = crate::object::namespace::ob_create_directory("\\TestNS2");
        let _ = crate::object::namespace::ob_set_protected("\\TestNS2", true);
        let result = crate::object::namespace::ob_create_directory_checked("\\TestNS2\\HackedDir");
        test_true!(result.is_err());
    });

    test_case!("ob_create_directory_checked_allows_non_protected", {
        let _ = crate::object::namespace::ob_create_directory("\\TestNS3");
        let result = crate::object::namespace::ob_create_directory_checked("\\TestNS3\\NormalDir");
        test_true!(result.is_ok());
    });

    test_case!("vfs_namespace_no_orphan_on_close_with_refs", {
        // Create object, reference it, insert into namespace
        let id = ob_create_object(ObType::Filesystem, "ns_ref", 7, 0, None).unwrap();
        let _ = crate::object::namespace::ob_insert_object("\\TestDir\\cleanup_ref", id);
        ob_reference(id).unwrap();  // refcount 1→2

        // Close once (refcount 2→1) — object stays alive, namespace entry stays
        ob_close_object(id).unwrap();
        test_true!(ob_lookup(id).is_some());

        // Namespace entry should still be valid
        let ns_id = crate::object::namespace::ob_lookup_path("\\TestDir\\cleanup_ref").unwrap();
        test_eq!(ns_id, id);

        // Close again (refcount 1→0) — auto-destroy, namespace entry removed
        ob_close_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());

        let ns_result = crate::object::namespace::ob_lookup_path("\\TestDir\\cleanup_ref");
        test_true!(ns_result.is_err());
    });

    // ── VFS-3.1: \Global\FileSystem\ enumeration delegates to VFS ──

    test_case!("vfs_namespace_filesystem_isolation", {
        // Insert a legacy namespace entry under \Global\FileSystem\
        let id = ob_create_object(ObType::Filesystem, "legacy_fs", 1, 0, None).unwrap();
        let _ = crate::object::namespace::ob_create_directory("\\Global\\FileSystem");
        let _ = crate::object::namespace::ob_insert_object("\\Global\\FileSystem\\LegacyEntry", id);

        // ob_enum_directory delegates to VFS, not namespace
        let entries = ob_enum_directory("\\Global\\FileSystem\\").unwrap();
        for e in &entries {
            let name_str = core::str::from_utf8(&e.name).unwrap_or("");
            test_true!(name_str != "LegacyEntry");
        }

        // Cleanup
        let _ = crate::object::namespace::ob_remove_object("\\Global\\FileSystem\\LegacyEntry");
        ob_destroy_object(id).unwrap_or(());
    });

    // ── VFS-3.3: ob_create(Directory) under \Global\FileSystem\ is rejected ──

    test_case!("vfs_namespace_protected_paths", {
        let result = ob_create_object_path(
            "\\Global\\FileSystem\\C:\\ProtectedDir",
            ObType::Directory,
            0, None,
        );
        test_true!(result.is_err());
    });

    // ── PowerManager tests ──

    test_case!("pm_create_powermanager_object", {
        let _ = namespace::ob_create_directory("\\System");
        let id = ob_create_object(ObType::PowerManager, "System\\PowerManager", 0, 0, Some(&power::POWER_MANAGER_OPS));
        test_true!(id.is_ok());
        if let Ok(obj_id) = id {
            let obj = ob_lookup(obj_id).unwrap();
            test_eq!(obj.obj_type, ObType::PowerManager);
            test_eq!(obj.native_id, 0);
            ob_close_object(obj_id).unwrap();
        }
    });

    test_case!("pm_open_powermanager_via_namespace", {
        let _ = namespace::ob_create_directory("\\System");
        let id = ob_create_object(ObType::PowerManager, "System\\PowerManager", 0, 0, Some(&power::POWER_MANAGER_OPS));
        test_true!(id.is_ok());
        if let Ok(obj_id) = id {
            let _ = namespace::ob_insert_object("\\System\\PowerManager", obj_id);
            let token = crate::security::token::Token::new_admin();
            let opened = ob_open_path("\\System\\PowerManager", &token, 3);
            test_true!(opened.is_ok());
            if let Ok(opened_id) = opened {
                let obj = ob_lookup(opened_id).unwrap();
                test_eq!(obj.obj_type, ObType::PowerManager);
                ob_close_object(opened_id).unwrap();
            }
            ob_close_object(obj_id).unwrap();
        }
    });

    test_case!("pm_powermanager_wrong_type_rejected", {
        let id = ob_create_object(ObType::Process, "not_power", 0, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_true!(obj.obj_type != ObType::PowerManager);
        ob_destroy_object(id).unwrap();
    });
}
