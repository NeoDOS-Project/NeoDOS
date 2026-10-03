//! Ob namespace tests.

use super::*;
use crate::{test_case, test_eq, test_true};

pub fn register_namespace_tests() {
    test_case!("ob_directory_create", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        test_eq!(ns.dir_count(), 2);
        test_eq!(ns.object_count(), 0);
    });

    test_case!("ob_directory_hierarchy", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.create_directory("\\Device\\Harddisk0").unwrap();
        test_eq!(ns.dir_count(), 3);

        let id = crate::object::ob_create_object(ObType::BlockDevice, "part", 1, 0, None).unwrap();
        ns.create_directory("\\Device\\Harddisk0\\Partition1").unwrap();
        ns.insert_object("\\Device\\Harddisk0\\Partition1", id).unwrap();

        let found = ns.lookup_path("\\Device\\Harddisk0\\Partition1").unwrap();
        test_eq!(found, id);
        test_eq!(ns.object_count(), 1);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_lookup_path_simple", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\DosDevices").unwrap();
        let id = crate::object::ob_create_object(ObType::Device, "C:", 0, 0, None).unwrap();
        ns.insert_object("\\DosDevices\\C:", id).unwrap();

        let found = ns.lookup_path("\\DosDevices\\C:").unwrap();
        test_eq!(found, id);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_lookup_path_nested", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.create_directory("\\Device\\Harddisk0").unwrap();

        let id = crate::object::ob_create_object(ObType::BlockDevice, "part2", 2, 0, None).unwrap();
        ns.insert_object("\\Device\\Harddisk0\\Partition2", id).unwrap();

        let found = ns.lookup_path("\\Device\\Harddisk0\\Partition2").unwrap();
        test_eq!(found, id);

        test_true!(ns.lookup_path("\\Device\\Harddisk0\\Partition99").is_err());
        test_true!(ns.lookup_path("\\NonExistent").is_err());
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_rename_directory", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        test_true!(ns.rename_directory("\\Device", "Devices").is_ok());

        test_true!(ns.create_directory("\\Device\\Sub").is_err());

        ns.create_directory("\\Devices\\Sub").unwrap();
        test_eq!(ns.dir_count(), 3);

        test_true!(ns.rename_directory("\\Devices", "Devices").is_err());
    });

    test_case!("ob_tree_stress_1000_objects", {
        let mut ns = ObNamespace::new();
        test_true!(ns.create_directory("\\Stress").is_ok());
        let mut ids = alloc::vec::Vec::new();
        for i in 0..1000 {
            let name = alloc::format!("obj_{}", i);
            match crate::object::ob_create_object(ObType::Unknown, &name, i as u64, 0, None) {
                Ok(id) => {
                    let path = alloc::format!("\\Stress\\{}", name);
                    if ns.insert_object(&path, id).is_ok() {
                        ids.push((name, id));
                    }
                }
                Err(_) => break,
            }
        }
        test_eq!(ids.len(), 1000);
        for (name, expected_id) in &ids {
            let path = alloc::format!("\\Stress\\{}", name);
            if let Ok(found) = ns.lookup_path(&path) {
                test_eq!(found, *expected_id);
            }
        }
        test_eq!(ns.object_count(), 1000);
        test_true!(ns.dir_count() >= 2);
        for (_, id) in &ids {
            crate::object::ob_destroy_object(*id).unwrap_or(());
        }
    });

    test_case!("ob_symlink_create_simple", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::object::ob_create_object(ObType::BlockDevice, "hdvol0", 0, 0, None).unwrap();
        ns.insert_object("\\Device\\HarddiskVolume0", id).unwrap();
        ns.create_directory("\\DosDevices").unwrap();
        ns.insert_symlink("\\DosDevices\\C:", "\\Device\\HarddiskVolume0").unwrap();
        test_eq!(ns.symlink_count(), 1);
        let symlink = ns.lookup_symlink("\\DosDevices\\C:").unwrap();
        test_eq!(symlink.target_str(), "\\Device\\HarddiskVolume0");
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_symlink_resolve_one_level", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::object::ob_create_object(ObType::BlockDevice, "hdvol0", 0, 0, None).unwrap();
        ns.insert_object("\\Device\\HarddiskVolume0", id).unwrap();
        ns.create_directory("\\DosDevices").unwrap();
        ns.insert_symlink("\\DosDevices\\C:", "\\Device\\HarddiskVolume0").unwrap();
        let found = ns.lookup_path("\\DosDevices\\C:").unwrap();
        test_eq!(found, id);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_symlink_resolve_chain", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\A").unwrap();
        ns.create_directory("\\A\\B").unwrap();
        let id = crate::object::ob_create_object(ObType::Unknown, "target", 42, 0, None).unwrap();
        ns.insert_object("\\A\\B\\Target", id).unwrap();
        ns.insert_symlink("\\A\\Link1", "B\\Target").unwrap();
        ns.insert_symlink("\\Link2", "A\\Link1").unwrap();
        let found = ns.lookup_path("\\Link2").unwrap();
        test_eq!(found, id);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_symlink_loop_detection", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Loop").unwrap();
        ns.insert_symlink("\\Loop\\A", "\\Loop\\B").unwrap();
        ns.insert_symlink("\\Loop\\B", "\\Loop\\A").unwrap();
        test_true!(ns.lookup_path("\\Loop\\A").is_err());
    });

    test_case!("ob_symlink_invalid_target", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\DosDevices").unwrap();
        ns.create_directory("\\Device").unwrap();
        let id = crate::object::ob_create_object(ObType::BlockDevice, "vol", 0, 0, None).unwrap();
        ns.insert_object("\\Device\\RealVol", id).unwrap();
        ns.insert_symlink("\\DosDevices\\X:", "\\Device\\NonExistent").unwrap();
        test_true!(ns.lookup_path("\\DosDevices\\X:").is_err());
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_case_insensitive_lookup", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::object::ob_create_object(ObType::BlockDevice, "MYDRV", 7, 0, None).unwrap();
        ns.insert_object("\\Device\\MYDRV", id).unwrap();
        let found = ns.lookup_path("\\device\\mydrv").unwrap();
        test_eq!(found, id);
        let found2 = ns.lookup_path("\\Device\\MyDrv").unwrap();
        test_eq!(found2, id);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_normalize_path", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.create_directory("\\Device\\Harddisk0").unwrap();
        let id = crate::object::ob_create_object(ObType::BlockDevice, "part", 5, 0, None).unwrap();
        ns.insert_object("\\Device\\Harddisk0\\Partition1", id).unwrap();
        let found = ns.lookup_by_path("\\Device\\Harddisk0\\Partition1").unwrap();
        test_eq!(found, id);
        let found2 = ns.lookup_by_path("\\Device\\.\\Harddisk0\\..\\Harddisk0\\Partition1").unwrap();
        test_eq!(found2, id);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_lookup_by_path_normalized", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::object::ob_create_object(ObType::BlockDevice, "rootdev", 9, 0, None).unwrap();
        ns.insert_object("\\Device\\RootDev", id).unwrap();
        test_true!(ns.lookup_by_path("\\Device\\RootDev").is_ok());
        test_true!(ns.lookup_by_path("\\Device\\rootdev").is_ok());
        test_true!(ns.lookup_by_path("\\Device\\RootDev\\..\\RootDev").is_ok());
        crate::object::ob_destroy_object(id).unwrap();
    });

    // ── NS-1.1 / NS-1.2: Namespace ownership and protected root dirs ──

    test_case!("ob_set_protected_flag", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.set_protected("\\Device", true).unwrap();
        let key = name_to_key("Device");
        let device_dir = ns.root.child_dirs.get(&key).unwrap();
        test_true!(device_dir.protected);
    });

    test_case!("ob_protected_non_root_subdirs_unprotected", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.set_protected("\\Device", true).unwrap();
        ns.create_directory("\\Device\\Sub").unwrap();
        let device = ns.root.child_dirs.get(&name_to_key("Device")).unwrap();
        test_true!(!device.child_dirs.get(&name_to_key("Sub")).unwrap().protected);
    });

    test_case!("ob_insert_object_internal_bypasses_protection", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.set_protected("\\Device", true).unwrap();
        let id = crate::object::ob_create_object(ObType::Unknown, "hacked", 1, 0, None).unwrap();
        // Internal insert_object bypasses protection (kernel code path)
        test_true!(ns.insert_object("\\Device\\Hacked", id).is_ok());
        crate::object::ob_destroy_object(id).unwrap_or(());
    });

    test_case!("ob_set_protected_unmark", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.set_protected("\\Device", true).unwrap();
        ns.set_protected("\\Device", false).unwrap();
        let key = name_to_key("Device");
        let device_dir = ns.root.child_dirs.get(&key).unwrap();
        test_true!(!device_dir.protected);
    });

    test_case!("ob_protected_deep_nested_insert_allowed", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.set_protected("\\Device", true).unwrap();
        ns.create_directory("\\Device\\Storage").unwrap();
        let id = crate::object::ob_create_object(ObType::Unknown, "disk", 1, 0, None).unwrap();
        // Inserting under non-protected subdir \Device\Storage is allowed
        test_true!(ns.insert_object("\\Device\\Storage\\Disk0", id).is_ok());
        crate::object::ob_destroy_object(id).unwrap_or(());
    });

    test_case!("ob_protected_flag_subdirs_inherit_none", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Root").unwrap();
        ns.set_protected("\\Root", true).unwrap();
        ns.create_directory("\\Root\\Sub").unwrap();
        test_true!(ns.root.child_dirs.get(&name_to_key("Root")).unwrap().protected);
        test_true!(!ns.root.child_dirs.get(&name_to_key("Root")).unwrap()
            .child_dirs.get(&name_to_key("Sub")).unwrap().protected);
    });

    test_case!("ob_set_protected_clears_flag", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Root").unwrap();
        ns.set_protected("\\Root", true).unwrap();
        test_true!(ns.root.child_dirs.get(&name_to_key("Root")).unwrap().protected);
        ns.set_protected("\\Root", false).unwrap();
        test_true!(!ns.root.child_dirs.get(&name_to_key("Root")).unwrap().protected);
    });
}
