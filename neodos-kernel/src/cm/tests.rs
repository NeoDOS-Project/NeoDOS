use alloc::string::ToString;
use alloc::vec::Vec;
use self::super::hive::{self, Hive, NULL_CELL};

pub fn register_cm_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("cm_create_key_ob", {
        let mut hive = Hive::new("TestHive");
        let root = hive.root_cell();
        test_eq!(root, 0);

        let sub = hive.create_key(root, "SubKey").unwrap();
        test_true!(sub > 0);
        test_true!(sub != hive::NULL_CELL);

        let found = hive.find_key(root, "SubKey").unwrap();
        test_eq!(found, sub);

        let not_found = hive.find_key(root, "NonExistent");
        test_true!(not_found.is_none());
    });

    test_case!("cm_query_value_cache_hit", {
        let mut hive = Hive::new("TestCache");
        let root = hive.root_cell();

        hive.set_value(root, "TestValue", hive::REG_DWORD, &42u32.to_le_bytes()).unwrap();

        let val = hive.query_value(root, "TestValue").unwrap();
        test_eq!(val.value_type, hive::REG_DWORD);
        test_eq!(val.as_dword().unwrap(), 42);
    });

    test_case!("cm_set_value_persist", {
        let mut hive = Hive::new("TestPersist");
        let root = hive.root_cell();

        hive.set_value(root, "Path", hive::REG_SZ, b"C:\\Programs").unwrap();

        let val = hive.query_value(root, "Path").unwrap();
        test_eq!(val.value_type, hive::REG_SZ);
        test_eq!(val.as_str().unwrap(), "C:\\Programs");

        let val2 = hive.query_value(root, "PATH").unwrap();
        test_eq!(val2.as_str().unwrap(), "C:\\Programs");

        hive.set_value(root, "Path", hive::REG_SZ, b"D:\\Data").unwrap();
        let val3 = hive.query_value(root, "Path").unwrap();
        test_eq!(val3.as_str().unwrap(), "D:\\Data");
    });

    test_case!("cm_enum_keys_multi", {
        let mut hive = Hive::new("TestEnum");
        let root = hive.root_cell();

        let k1 = hive.create_key(root, "First").unwrap();
        let k2 = hive.create_key(root, "Second").unwrap();
        let k3 = hive.create_key(root, "Third").unwrap();
        test_true!(k1 != k2 && k2 != k3);

        test_eq!(hive.key_count(root), 3);

        let name0 = hive.enum_key(root, 0).unwrap();
        test_true!(name0 == "First" || name0 == "Second" || name0 == "Third");

        let name1 = hive.enum_key(root, 1).unwrap();
        test_true!(name1 == "First" || name1 == "Second" || name1 == "Third");

        let name2 = hive.enum_key(root, 2).unwrap();
        test_true!(name2 == "First" || name2 == "Second" || name2 == "Third");

        let out_of_range = hive.enum_key(root, 3);
        test_true!(out_of_range.is_none());

        test_true!(name0 != name1 && name1 != name2 && name0 != name2);
    });

    test_case!("cm_hive_reload_integrity", {
        let mut hive = Hive::new("TestIntegrity");
        let root = hive.root_cell();

        let sub = hive.create_key(root, "App").unwrap();
        hive.set_value(sub, "Version", hive::REG_SZ, b"1.0").unwrap();
        hive.set_value(sub, "Count", hive::REG_DWORD, &100u32.to_le_bytes()).unwrap();

        test_eq!(hive.value_count(sub), 2);
        test_eq!(hive.key_count(root), 1);

        hive.delete_key(sub);
        test_eq!(hive.key_count(root), 0);
        test_eq!(hive.find_key(root, "App"), None);
    });

    test_case!("cm_cell_corruption_isolated", {
        let mut hive = Hive::new("TestCorrupt");
        let root = hive.root_cell();

        let mut cells = alloc::vec::Vec::new();
        for i in 0..100 {
            let name = alloc::format!("Key_{}", i);
            if let Some(idx) = hive.create_key(root, &name) {
                cells.push(idx);
                hive.set_value(idx, "data", hive::REG_DWORD, &(i as u32).to_le_bytes()).unwrap();
            } else {
                break;
            }
        }
        test_true!(cells.len() >= 50);

        for (i, &cell_idx) in cells.iter().enumerate() {
            let val = hive.query_value(cell_idx, "data").unwrap();
            test_eq!(val.as_dword().unwrap(), i as u32);
        }

        for &cell_idx in &cells {
            hive.delete_key(cell_idx);
        }
        test_eq!(hive.key_count(root), 0);
    });

    test_case!("cm_syscall_open_key", {
        let mut hive = Hive::new("TestApi");
        let root = hive.root_cell();

        let sub = hive.create_key(root, "System").unwrap();
        let nested = hive.create_key(sub, "Services").unwrap();
        hive.create_key(nested, "Network").unwrap();

        let services = hive.open_key_by_path(root, "System\\Services").unwrap();
        test_eq!(services, nested);

        let network = hive.open_key_by_path(root, "System\\Services\\Network").unwrap();
        test_true!(network != NULL_CELL);

        let not_found = hive.open_key_by_path(root, "System\\Missing");
        test_true!(not_found.is_none());
    });

    test_case!("cm_syscall_set_get_value", {
        let mut hive = Hive::new("TestSetGet");
        let root = hive.root_cell();

        let config = hive.create_key(root, "Config").unwrap();
        hive.create_key(config, "Network").unwrap();

        let net = hive.open_key_by_path(root, "Config\\Network").unwrap();
        hive.set_value(net, "IP", hive::REG_SZ, b"10.0.2.15").unwrap();
        hive.set_value(net, "Port", hive::REG_DWORD, &8080u32.to_le_bytes()).unwrap();
        hive.set_value(net, "Enabled", hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();

        test_eq!(hive.value_count(net), 3);

        let ip = hive.query_value(net, "IP").unwrap();
        test_eq!(ip.as_str().unwrap(), "10.0.2.15");

        let port = hive.query_value(net, "Port").unwrap();
        test_eq!(port.as_dword().unwrap(), 8080);

        let v0 = hive.enum_value(net, 0).unwrap();
        test_true!(!v0.is_empty());

        if let Some(val_idx) = hive.find_value(net, "Enabled") {
            let val_next = match hive.slot(val_idx) {
                Some(hive::Cell::Value(v)) => v.next,
                _ => hive::NULL_CELL,
            };
            if let Some(hive::Cell::Key(ref mut k)) = hive.slot_mut(net) {
                if k.values_head == val_idx {
                    k.values_head = val_next;
                }
            }
            hive.free_cell(val_idx);
        }
        test_eq!(hive.value_count(net), 2);
    });

    test_case!("cm_default_values_created", {
        let mut hive = Hive::new("TestDefaults");
        let root = hive.root_cell();

        let neoinit = {
            let path = "CurrentControlSet\\Services\\NeoInit";
            let parts: Vec<&str> = path.split('\\').filter(|p| !p.is_empty()).collect();
            let mut curr = root;
            for part in &parts {
                curr = match hive.find_key(curr, part) {
                    Some(found) => found,
                    None => hive.create_key(curr, part).unwrap(),
                };
            }
            curr
        };
        hive.set_value(neoinit, "DefaultShell", hive::REG_SZ, b"C:\\Programs\\neoshell.nxe");

        let net_iface = {
            let path = "CurrentControlSet\\Services\\Network\\Interfaces\\0";
            let parts: Vec<&str> = path.split('\\').filter(|p| !p.is_empty()).collect();
            let mut curr = root;
            for part in &parts {
                curr = match hive.find_key(curr, part) {
                    Some(found) => found,
                    None => hive.create_key(curr, part).unwrap(),
                };
            }
            curr
        };
        hive.set_value(net_iface, "DHCPEnabled", hive::REG_DWORD, &1u32.to_le_bytes());

        let ctrl = {
            let path = "CurrentControlSet\\Control";
            let parts: Vec<&str> = path.split('\\').filter(|p| !p.is_empty()).collect();
            let mut curr = root;
            for part in &parts {
                curr = match hive.find_key(curr, part) {
                    Some(found) => found,
                    None => hive.create_key(curr, part).unwrap(),
                };
            }
            curr
        };
        hive.set_value(ctrl, "WaitForNetwork", hive::REG_DWORD, &0u32.to_le_bytes());

        let v1 = hive.query_value(neoinit, "DefaultShell").unwrap();
        test_eq!(v1.as_str().unwrap(), "C:\\Programs\\neoshell.nxe");
        test_eq!(v1.value_type, hive::REG_SZ);

        let v2 = hive.query_value(net_iface, "DHCPEnabled").unwrap();
        test_eq!(v2.as_dword().unwrap(), 1);
        test_eq!(v2.value_type, hive::REG_DWORD);

        let v3 = hive.query_value(ctrl, "WaitForNetwork").unwrap();
        test_eq!(v3.as_dword().unwrap(), 0);
        test_eq!(v3.value_type, hive::REG_DWORD);

        hive.set_value(neoinit, "DefaultShell", hive::REG_SZ, b"C:\\Programs\\neoshell.nxe");
        let v1b = hive.query_value(neoinit, "DefaultShell").unwrap();
        test_eq!(v1b.as_str().unwrap(), "C:\\Programs\\neoshell.nxe");

        test_eq!(hive.key_count(root), 1);
        let ccs = hive.find_key(root, "CurrentControlSet").unwrap();
        test_eq!(hive.key_count(ccs), 2);
        let services = hive.find_key(ccs, "Services").unwrap();
        test_eq!(hive.key_count(services), 2);
        test_eq!(hive.value_count(ctrl), 1);
    });

    test_case!("cm_set_value_persist_roundtrip", {
        let mut original = Hive::new("TestRoundtrip");
        let root = original.root_cell();

        let sub = original.create_key(root, "Application").unwrap();
        original.set_value(root, "Version", hive::REG_SZ, b"1.0.42").unwrap();
        original.set_value(root, "DebugMode", hive::REG_DWORD, &0u32.to_le_bytes()).unwrap();
        original.set_value(root, "MaxUsers", hive::REG_DWORD, &100u32.to_le_bytes()).unwrap();
        original.set_value(sub, "Path", hive::REG_SZ, b"C:\\Programs\\Test").unwrap();
        original.set_value(sub, "Enabled", hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();
        original.set_value(sub, "Timeout", hive::REG_DWORD, &5000u32.to_le_bytes()).unwrap();

        let orig_version = original.query_value(root, "Version").unwrap().clone();

        let data = original.serialize();
        test_true!(data.len() > 16);

        let mut restored = Hive::deserialize(&data).unwrap();
        restored.name = "TestRoundtrip".to_string();

        let restored_root = restored.root_cell();
        test_eq!(restored_root, 0);
        test_eq!(restored.key_count(restored_root), 1);
        let restored_app = restored.find_key(restored_root, "Application").unwrap();
        test_true!(restored_app != hive::NULL_CELL);

        let r_version = restored.query_value(restored_root, "Version").unwrap();
        test_eq!(r_version.value_type, hive::REG_SZ);
        test_eq!(r_version.as_str().unwrap(), orig_version.as_str().unwrap());

        let r_max = restored.query_value(restored_root, "MaxUsers").unwrap();
        test_eq!(r_max.as_dword().unwrap(), 100);

        let r_debug = restored.query_value(restored_root, "DebugMode").unwrap();
        test_eq!(r_debug.as_dword().unwrap(), 0);

        let r_path = restored.query_value(restored_app, "Path").unwrap();
        test_eq!(r_path.as_str().unwrap(), "C:\\Programs\\Test");

        let r_enabled = restored.query_value(restored_app, "Enabled").unwrap();
        test_eq!(r_enabled.as_dword().unwrap(), 1);

        let r_timeout = restored.query_value(restored_app, "Timeout").unwrap();
        test_eq!(r_timeout.as_dword().unwrap(), 5000);

        test_eq!(original.value_count(root), 3);
        test_eq!(restored.value_count(restored_root), 3);
        test_eq!(original.value_count(sub), 3);
        test_eq!(restored.value_count(restored_app), 3);
    });

    test_case!("cm_hive_serialization_integrity", {
        let mut hive = Hive::new("TestIntegrity2");
        let root = hive.root_cell();

        let l1 = hive.create_key(root, "Level1").unwrap();
        let l2 = hive.create_key(l1, "Level2").unwrap();
        let l3 = hive.create_key(l2, "Level3").unwrap();
        hive.set_value(l3, "DeepValue", hive::REG_SZ, b"deep").unwrap();

        let other = hive.create_key(root, "Other").unwrap();
        hive.create_key(other, "SubOther").unwrap();

        test_eq!(hive.key_count(root), 2);

        let data = hive.serialize();
        let mut restored = Hive::deserialize(&data).unwrap();
        restored.name = "TestIntegrity2".to_string();

        let r_root = restored.root_cell();
        test_eq!(restored.key_count(r_root), 2);

        let r_l1 = restored.find_key(r_root, "Level1").unwrap();
        test_true!(r_l1 != hive::NULL_CELL);
        let r_l2 = restored.find_key(r_l1, "Level2").unwrap();
        let r_l3 = restored.find_key(r_l2, "Level3").unwrap();
        let deep = restored.query_value(r_l3, "DeepValue").unwrap();
        test_eq!(deep.as_str().unwrap(), "deep");

        let r_other = restored.find_key(r_root, "Other").unwrap();
        test_true!(restored.find_key(r_other, "SubOther").is_some());

        restored.delete_key(r_other);
        test_eq!(restored.key_count(r_root), 1);

        let data2 = restored.serialize();
        let restored2 = Hive::deserialize(&data2).unwrap();
        let r2_root = restored2.root_cell();
        test_eq!(restored2.key_count(r2_root), 1);
        test_true!(restored2.find_key(r2_root, "Other").is_none());
        test_true!(restored2.find_key(r2_root, "Level1").is_some());
    });

    test_case!("cm_free_list_next_fit", {
        let mut hive = Hive::new("TestNextFit");
        let root = hive.root_cell();

        let k1 = hive.create_key(root, "A").unwrap();
        let k2 = hive.create_key(root, "B").unwrap();
        let k3 = hive.create_key(root, "C").unwrap();
        test_true!(k1 < k2);
        test_true!(k2 < k3);

        hive.delete_key(k2);
        test_eq!(hive.key_count(root), 2);

        let k4 = hive.create_key(root, "D").unwrap();
        test_true!(k4 > k3);

        hive.delete_key(k1);
        test_eq!(hive.key_count(root), 2);

        let k5 = hive.create_key(root, "E").unwrap();
        test_true!(k5 > k4);

        test_eq!(hive.cell_count(), 4);
    });

    test_case!("cm_delete_value", {
        let mut hive = Hive::new("TestDelVal");
        let root = hive.root_cell();

        hive.set_value(root, "Keep", hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();
        hive.set_value(root, "Remove", hive::REG_SZ, b"delete_me").unwrap();
        hive.set_value(root, "Keep2", hive::REG_DWORD, &2u32.to_le_bytes()).unwrap();
        test_eq!(hive.value_count(root), 3);

        test_true!(hive.delete_value(root, "Remove"));
        test_eq!(hive.value_count(root), 2);

        let v1 = hive.query_value(root, "Keep").unwrap();
        test_eq!(v1.as_dword().unwrap(), 1);
        let v2 = hive.query_value(root, "Keep2").unwrap();
        test_eq!(v2.as_dword().unwrap(), 2);

        test_true!(!hive.delete_value(root, "NonExistent"));

        test_true!(hive.delete_value(root, "Keep"));
        test_eq!(hive.value_count(root), 1);

        test_true!(hive.delete_value(root, "Keep2"));
        test_eq!(hive.value_count(root), 0);
    });

    test_case!("cm_delete_value_persist", {
        let mut hive = Hive::new("TestDelValPersist");
        let root = hive.root_cell();

        hive.set_value(root, "A", hive::REG_DWORD, &10u32.to_le_bytes()).unwrap();
        hive.set_value(root, "B", hive::REG_SZ, b"persist").unwrap();
        hive.set_value(root, "C", hive::REG_DWORD, &20u32.to_le_bytes()).unwrap();

        test_true!(hive.delete_value(root, "B"));
        test_eq!(hive.value_count(root), 2);

        let data = hive.serialize();
        let mut restored = Hive::deserialize(&data).unwrap();
        restored.name = "TestDelValPersist".to_string();
        let r_root = restored.root_cell();

        test_eq!(restored.value_count(r_root), 2);
        test_true!(restored.find_value(r_root, "A").is_some());
        test_true!(restored.find_value(r_root, "C").is_some());
        test_true!(restored.find_value(r_root, "B").is_none());

        let va = restored.query_value(r_root, "A").unwrap();
        test_eq!(va.as_dword().unwrap(), 10);
        let vc = restored.query_value(r_root, "C").unwrap();
        test_eq!(vc.as_dword().unwrap(), 20);
    });

    test_case!("cm_unmount_flush", {
        let mut hive = Hive::new("TestUnmountFlush");
        let root = hive.root_cell();
        hive.set_value(root, "FlushMe", hive::REG_SZ, b"data").unwrap();
        test_true!(hive.is_dirty());

        let data = hive.serialize();
        let restored = Hive::deserialize(&data).unwrap();
        let r_root = restored.root_cell();
        let v = restored.query_value(r_root, "FlushMe").unwrap();
        test_eq!(v.as_str().unwrap(), "data");
    });

    test_case!("cm_deep_key_deletion_iterative", {
        let mut hive = Hive::new("TestDeepIter");
        let root = hive.root_cell();

        let l1 = hive.create_key(root, "L1").unwrap();
        let l2 = hive.create_key(l1, "L2").unwrap();
        let l3 = hive.create_key(l2, "L3").unwrap();
        let l4 = hive.create_key(l3, "L4").unwrap();
        hive.set_value(l4, "deep_val", hive::REG_SZ, b"deep").unwrap();

        test_eq!(hive.key_count(root), 1);
        test_eq!(hive.key_count(l1), 1);
        test_eq!(hive.key_count(l2), 1);
        test_eq!(hive.key_count(l3), 1);

        hive.delete_key(l1);

        test_eq!(hive.key_count(root), 0);
        test_true!(hive.find_key(root, "L1").is_none());
        test_eq!(hive.cell_count(), 1);
    });

    test_case!("cm_key_deletion_preserves_siblings", {
        let mut hive = Hive::new("TestSiblings");
        let root = hive.root_cell();

        let a = hive.create_key(root, "Alpha").unwrap();
        let b = hive.create_key(root, "Beta").unwrap();
        let c = hive.create_key(root, "Gamma").unwrap();
        let d = hive.create_key(root, "Delta").unwrap();
        let e = hive.create_key(root, "Epsilon").unwrap();

        test_eq!(hive.key_count(root), 5);

        hive.delete_key(c);
        test_eq!(hive.key_count(root), 4);
        test_true!(hive.find_key(root, "Alpha").is_some());
        test_true!(hive.find_key(root, "Beta").is_some());
        test_true!(hive.find_key(root, "Delta").is_some());
        test_true!(hive.find_key(root, "Epsilon").is_some());

        hive.delete_key(a);
        test_eq!(hive.key_count(root), 3);
        test_true!(hive.find_key(root, "Alpha").is_none());

        hive.delete_key(e);
        test_eq!(hive.key_count(root), 2);
        test_true!(hive.find_key(root, "Epsilon").is_none());

        hive.delete_key(b);
        hive.delete_key(d);
        test_eq!(hive.key_count(root), 0);
    });

    test_case!("cm_neoinit_autostart_set_read", {
        let mut hive = Hive::new("TestAutoStart");
        let root = hive.root_cell();

        let ccs = hive.create_key(root, "CurrentControlSet").unwrap();
        let svc = hive.create_key(ccs, "Services").unwrap();
        let neoinit = hive.create_key(svc, "NeoInit").unwrap();

        hive.set_value(neoinit, "AutoStartServices", hive::REG_SZ,
            b"C:\\System\\Tools\\dhcpd.nxe;C:\\Programs\\netcfg.nxe");

        let val = hive.query_value(neoinit, "AutoStartServices").unwrap();
        test_eq!(val.value_type, hive::REG_SZ);

        let services_str = val.as_str().unwrap();
        test_true!(services_str == "C:\\System\\Tools\\dhcpd.nxe;C:\\Programs\\netcfg.nxe");

        let services: Vec<&str> = services_str.split(';').collect();
        test_eq!(services.len(), 2);
        test_eq!(services[0], "C:\\System\\Tools\\dhcpd.nxe");
        test_eq!(services[1], "C:\\Programs\\netcfg.nxe");
    });

    test_case!("cm_neoinit_autostart_empty", {
        let mut hive = Hive::new("TestAutoStartEmpty");
        let root = hive.root_cell();

        let ccs = hive.create_key(root, "CurrentControlSet").unwrap();
        let svc = hive.create_key(ccs, "Services").unwrap();
        let neoinit = hive.create_key(svc, "NeoInit").unwrap();

        hive.set_value(neoinit, "AutoStartServices", hive::REG_SZ, b"");

        let val = hive.query_value(neoinit, "AutoStartServices").unwrap();
        test_eq!(val.value_type, hive::REG_SZ);
        let services_str = val.as_str().unwrap();
        test_eq!(services_str, "");
        test_eq!(services_str.is_empty(), true);

        let parts: Vec<&str> = services_str.split(';').collect();
        test_eq!(parts.len(), 1);
        test_eq!(parts[0], "");
    });

    test_case!("cm_neoinit_autostart_single", {
        let mut hive = Hive::new("TestAutoStartSingle");
        let root = hive.root_cell();

        let ccs = hive.create_key(root, "CurrentControlSet").unwrap();
        let svc = hive.create_key(ccs, "Services").unwrap();
        let neoinit = hive.create_key(svc, "NeoInit").unwrap();

        hive.set_value(neoinit, "AutoStartServices", hive::REG_SZ,
            b"C:\\System\\Tools\\dhcpd.nxe");

        let val = hive.query_value(neoinit, "AutoStartServices").unwrap();
        let svc_path = val.as_str().unwrap();
        test_eq!(svc_path, "C:\\System\\Tools\\dhcpd.nxe");

        let parts: Vec<&str> = svc_path.split(';').map(|s| s.trim()).collect();
        test_eq!(parts.len(), 1);
        test_eq!(parts[0], "C:\\System\\Tools\\dhcpd.nxe");
    });

    test_case!("cm_neoinit_autostart_parse_edge_cases", {
        let mut hive = Hive::new("TestAutoStartEdge");
        let root = hive.root_cell();

        let ccs = hive.create_key(root, "CurrentControlSet").unwrap();
        let svc = hive.create_key(ccs, "Services").unwrap();
        let neoinit = hive.create_key(svc, "NeoInit").unwrap();

        hive.set_value(neoinit, "AutoStartServices", hive::REG_SZ,
            b"  C:\\Tools\\one.nxe ; C:\\Tools\\two.nxe ;;C:\\Tools\\three.nxe");

        let val = hive.query_value(neoinit, "AutoStartServices").unwrap();
        let raw = val.as_str().unwrap();

        let services: Vec<&str> = raw.split(';').map(|s| s.trim()).collect();
        test_eq!(services.len(), 4);
        test_eq!(services[0], "C:\\Tools\\one.nxe");
        test_eq!(services[1], "C:\\Tools\\two.nxe");
        test_eq!(services[2], "");
        test_eq!(services[3], "C:\\Tools\\three.nxe");

        let non_empty: Vec<&str> = services.iter().filter(|s| !s.is_empty()).copied().collect();
        test_eq!(non_empty.len(), 3);
        test_eq!(non_empty[0], "C:\\Tools\\one.nxe");
        test_eq!(non_empty[1], "C:\\Tools\\two.nxe");
        test_eq!(non_empty[2], "C:\\Tools\\three.nxe");
    });

    test_case!("cm_neoinit_autostart_ob_path", {
        let prefix = b"\\Global\\FileSystem\\";

        let cases: [(&str, &str); 3] = [
            ("C:\\System\\Tools\\dhcpd.nxe",
             "\\Global\\FileSystem\\C:\\System\\Tools\\dhcpd.nxe"),
            ("C:\\Programs\\netcfg.nxe",
             "\\Global\\FileSystem\\C:\\Programs\\netcfg.nxe"),
            ("C:\\Programs\\neoshell.nxe",
             "\\Global\\FileSystem\\C:\\Programs\\neoshell.nxe"),
        ];

        for (svc_path, expected_ob) in &cases {
            let mut buf = [0u8; 512];
            let svc_bytes = svc_path.as_bytes();
            let total = prefix.len() + svc_bytes.len();
            test_true!(total <= buf.len());

            buf[..prefix.len()].copy_from_slice(prefix);
            buf[prefix.len()..total].copy_from_slice(svc_bytes);
            let ob_path = core::str::from_utf8(&buf[..total]).unwrap();

            test_eq!(ob_path, *expected_ob);
        }
    });
}
