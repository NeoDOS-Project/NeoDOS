use alloc::vec::Vec;
use alloc::string::String;
use crate::nem::{self, DriverCategory, ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID};
use crate::fs::vfs::MODE_FILE;
use crate::fs::vfs::MODE_DIR;
#[allow(unused_imports)]
use crate::drivers::driver_runtime::{self, DriverState};

#[allow(dead_code)]
fn collect_driver_data(files: &[String], category: DriverCategory) -> Vec<(String, Vec<u8>)> {
    let mut collected = Vec::new();
    for f in files {
        let data = match read_nem_file(f) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let parsed = nem::parse_nem_v3(&data);
        let cat = parsed.as_ref().and_then(|p| nem::DriverCategory::from_u8(p.header.category as u8))
            .unwrap_or(category);
        if cat != category {
            continue;
        }
        let name = parsed.map(|p| p.name.to_ascii_uppercase())
            .unwrap_or_else(|| {
                let base = f.rsplit('\\').next().unwrap_or(f);
                base.trim_end_matches(".nem").trim_end_matches(".NEM").to_ascii_uppercase()
            });
        collected.push((name, data));
    }
    collected
}

#[allow(dead_code)]
fn build_dependency_graph(drivers: &[(String, Vec<u8>)]) -> crate::drivers::dependency::DependencyGraph {
    let mut graph = crate::drivers::dependency::DependencyGraph::new();
    for (name, _) in drivers {
        graph.add_driver(name);
    }
    for (name, data) in drivers {
        if let Some(parsed) = nem::parse_nem_v3(data) {
            let sym_deps = crate::drivers::dependency::resolve_nem_symbol_dependencies(
                parsed.symbols, parsed.strtab
            );
            for dep in &sym_deps {
                let _ = graph.add_dependency(name, dep);
            }
        }
    }
    graph
}

pub fn boot_load_all() {
    let mut mgr = crate::drivers::driver_manager::DriverManager::new();
    mgr.init();
}

pub fn driver_scan(path: &str) -> Vec<String> {
    let mut results = Vec::new();
    crate::globals::with_vfs(|vfs| {
        if let Ok((drive_idx, node)) = vfs.resolve_path(path) {
            if (node.mode & MODE_DIR) == 0 { return; }
            let mut i = 0;
            loop {
                match vfs.readdir(drive_idx, node.inode, i) {
                    Ok(Some(entry)) => {
                        let upper = entry.name.to_ascii_uppercase();
                        if !upper.ends_with(".NEM") || (entry.node.mode & MODE_DIR) != 0 {
                            i += 1; continue;
                        }
                        let full = alloc::format!("{}\\{}", path.trim_end_matches('\\'), entry.name);
                        results.push(full);
                        i += 1;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
    });
    results
}

pub fn read_nem_file(path: &str) -> Result<Vec<u8>, &'static str> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| "VFS resolve failed")?;
        if node.mode & MODE_FILE == 0 {
            return Err("Not a file");
        }
        let size = node.size as usize;
        if size == 0 || size > 65536 {
            return Err("Bad size");
        }
    let mut buf = alloc::vec![0u8; size];
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).map_err(|_| "Read error")?;
        buf.truncate(read);
        Ok(buf)
    })
}

pub fn register_boot_loader_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("boot_scan_empty_dir", {
        let files = driver_scan("C:\\NONEXISTENT\\");
        test_eq!(files.len(), 0);
    });

    test_case!("boot_abi_constants_valid", {
        test_eq!(ABI_MIN_VALID, 1);
        test_eq!(ABI_TARGET, 1);
        test_eq!(ABI_MAX_VALID, 2);
    });

    test_case!("boot_driver_scan_path_format", {
        let path = "C:\\System\\Drivers";
        let files = driver_scan(path);
        test_true!(files.len() < 1000);
        for f in &files {
            test_true!(f.to_ascii_uppercase().ends_with(".NEM"));
        }
    });

    test_case!("boot_category_ordering", {
        test_eq!((DriverCategory::Boot as u8) < (DriverCategory::System as u8), true);
        test_eq!((DriverCategory::System as u8) < (DriverCategory::Demand as u8), true);
    });

    test_case!("boot_inline_activation_flow", {
        let id = driver_runtime::register_driver(
            "TEST", nem::NemDriverType::Null, 1, 0
        ).unwrap();
        let mut rt = driver_runtime::DRIVER_RUNTIME.lock();
        rt.try_transition(id, DriverState::Initialized).unwrap();
        rt.try_transition(id, DriverState::Registered).unwrap();
        rt.try_transition(id, DriverState::Bound).unwrap();
        rt.certify_and_activate(id).unwrap();
        let d = rt.get(id).unwrap();
        test_eq!(d.state, DriverState::Active);
        rt.remove(id);
    });

    test_case!("boot_activation_fails_on_skip", {
        let id = driver_runtime::register_driver(
            "SKIP", nem::NemDriverType::Null, 1, 0
        ).unwrap();
        let mut rt = driver_runtime::DRIVER_RUNTIME.lock();
        rt.try_transition(id, DriverState::Initialized).unwrap();
        let r = rt.certify_and_activate(id);
        test_true!(r.is_err());
        rt.remove(id);
    });

    test_case!("boot_collect_driver_data_empty", {
        let collected = collect_driver_data(&[], DriverCategory::System);
        test_eq!(collected.len(), 0);
    });

    test_case!("boot_build_dep_graph_empty", {
        let graph = build_dependency_graph(&[]);
        test_eq!(graph.driver_count(), 0);
    });
}
