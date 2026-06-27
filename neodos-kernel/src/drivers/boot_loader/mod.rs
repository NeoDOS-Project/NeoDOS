use alloc::vec::Vec;
use alloc::string::String;
use alloc::collections::BTreeMap;
use crate::nem::{self, DriverCategory, ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID};
use crate::drivers::nem::v3loader;
use crate::drivers::driver_runtime::{self, DriverState};
use crate::eventbus::EVENT_KEYBOARD_INPUT;
use crate::eventbus::EVENT_KEYB_LAYOUT;
use crate::eventbus::EVENT_RTC_READ;
use crate::eventbus::EVENT_SHUTDOWN;
use crate::fs::vfs::MODE_FILE;
use crate::fs::vfs::MODE_DIR;

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
                base.trim_end_matches(".NEM").to_ascii_uppercase()
            });
        collected.push((name, data));
    }
    collected
}

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
    crate::serial_println!("[BOOT] === Boot Driver Loader v2 (with dep resolver) ===");
    let mut total_loaded = 0u32;
    let mut total_active = 0u32;
    let mut total_faulted = 0u32;

    for (phase_name, root, cat) in &[
        ("BOOT", "C:\\System\\Drivers", DriverCategory::Boot),
        ("SYSTEM", "C:\\System\\Drivers", DriverCategory::System),
    ] {
        crate::serial_println!("[BOOT] Scanning {} drivers...", phase_name);
        let files = driver_scan(root);
        if files.is_empty() {
            continue;
        }

        let collected = collect_driver_data(&files, *cat);

        let graph = build_dependency_graph(&collected);
        let sorted = match graph.resolve_order() {
            Ok(order) => order,
            Err(e) => {
                crate::serial_println!("[BOOT]   Dependency resolution failed: {:?}", e);
                crate::serial_println!("[BOOT]   Falling back to filesystem order");
                collected.iter().map(|(n, _)| n.clone()).collect()
            }
        };

        let name_to_data: BTreeMap<String, &(String, Vec<u8>)> =
            collected.iter().map(|entry| (entry.0.clone(), entry)).collect();

        for name in &sorted {
            let entry = match name_to_data.get(name) {
                Some(e) => e,
                None => continue,
            };
            let data = &entry.1;
            let parsed_v3 = match nem::parse_nem_v3(data) {
                Some(p) => p,
                None => continue,
            };

            if name == "AHCI" {
                if crate::boot_benchmark::AHCI_COMMANDS.load(core::sync::atomic::Ordering::Relaxed) == 0 {
                    crate::serial_println!("[BOOT]   Skipping {} ... (AHCI controller not active)", name);
                    continue;
                }
            }

            let abi_result = crate::drivers::abi::negotiate_default(
                parsed_v3.header.abi_min,
                parsed_v3.header.abi_target,
                parsed_v3.header.abi_max,
            );
            if !abi_result.is_compatible() {
                crate::serial_println!("SKIP (v3 ABI: {})", abi_result.to_str());
                continue;
            }

            crate::serial_print!("[BOOT]   Loading {} ... ", name);
            let load_result = match v3loader::load_nem_v3(data) {
                Ok(r) => r,
                Err(e) => {
                    crate::serial_println!("FAIL (v3 load: {})", e);
                    continue;
                }
            };

            let name_str = String::from_utf8_lossy(&load_result.name);
            let name_upper = name_str.to_ascii_uppercase();

            let rt_id = driver_runtime::register_driver_ext(
                &name_upper,
                nem::NemDriverType::Lifecycle,
                nem::NEM_API_VERSION,
                0,
                parsed_v3.header.abi_min,
                parsed_v3.header.abi_target,
                parsed_v3.header.abi_max,
                load_result.category,
            );

            match rt_id {
                Ok(id) => {
                    crate::serial_print!("REG OK (id={})", id);
                    total_loaded += 1;

                    // X4: Bind the isolated region slot to this driver
                    v3loader::bind_isolated_driver(id, &load_result);

                    // Track load result for hot reload
                    crate::drivers::hotreload::register_load_result(id, &load_result);

                    // Set current driver context for capability checks
                    unsafe { crate::drivers::nem::driver::set_current_driver(id); }

                    let init_ok = match load_result.entry_init {
                        Some(init_fn) => unsafe { init_fn() == 0 },
                        None => true,
                    };

                    if init_ok {
                        let _ = driver_runtime::DRIVER_RUNTIME.lock()
                            .try_transition(id, DriverState::Initialized);
                        crate::serial_print!(" [INIT]");

                        let _ = driver_runtime::DRIVER_RUNTIME.lock()
                            .try_transition(id, DriverState::Registered);
                        crate::serial_print!(" [REG]");

                        let bind_ok = match name_upper.as_str() {
                            "PS2KBD" => {
                                let a = v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_KEYBOARD_INPUT, id
                                ).is_ok();
                                let b = v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_KEYB_LAYOUT, id
                                ).is_ok();
                                a && b
                            }
                            "PS2MOUSE" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, crate::eventbus::EVENT_MOUSE_INPUT, id
                                ).is_ok()
                            }
                            "SERIAL" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, crate::eventbus::EVENT_SERIAL_DATA, id
                                ).is_ok()
                            }
                            "RTC" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_RTC_READ, id
                                ).is_ok()
                            }
                            "ACPI" => {
                                v3loader::register_v3_event_bus_handler(
                                    load_result.entry_event, EVENT_SHUTDOWN, id
                                ).is_ok()
                            }
                            _ => {
                                // Unknown driver type – bind successfully without
                                // registering any event bus handler.
                                // DO NOT register for KEYBOARD_INPUT here: that would
                                // create a duplicate handler that calls ps2kbd, causing
                                // every keyboard event to be dispatched twice.
                                true
                            }
                        };
                        if !bind_ok {
                            crate::serial_print!(" [BIND FAIL]");
                            total_faulted += 1;
                            driver_runtime::DRIVER_RUNTIME.lock()
                                .set_error(id, driver_runtime::ERR_BIND_FAILED, true);
                        } else {
                            let _ = driver_runtime::DRIVER_RUNTIME.lock()
                                .try_transition(id, DriverState::Bound);
                            crate::serial_print!(" [BOUND]");

                            let activate_ok = match load_result.entry_activate {
                                Some(activate_fn) => unsafe { activate_fn() == 0 },
                                None => true,
                            };

                            if !activate_ok {
                                crate::serial_print!(" [ACT FAIL]");
                                total_faulted += 1;
                                driver_runtime::DRIVER_RUNTIME.lock()
                                    .set_error(id, driver_runtime::ERR_INIT_FAILED, true);
                                // Clear driver context before continuing
                                unsafe { crate::drivers::nem::driver::clear_current_driver(); }
                                crate::serial_println!();
                                continue;
                            }

                            if driver_runtime::DRIVER_RUNTIME.lock()
                                .certify_and_activate(id).is_ok()
                            {
                                crate::serial_print!(" [ACTIVE]");
                                total_active += 1;
                            } else {
                                crate::serial_print!(" [CERT FAIL]");
                                total_faulted += 1;
                                driver_runtime::DRIVER_RUNTIME.lock()
                                    .set_error(id, driver_runtime::ERR_CERTIFICATION_FAILED, true);
                            }
                        }
                    } else {
                        crate::serial_print!(" [INIT FAIL]");
                        total_faulted += 1;
                        driver_runtime::DRIVER_RUNTIME.lock()
                            .set_error(id, driver_runtime::ERR_INIT_FAILED, true);
                    }

                    // Clear current driver context after entry point returns
                    unsafe { crate::drivers::nem::driver::clear_current_driver(); }

                    crate::serial_println!();
                }
                Err(e) => {
                    crate::serial_println!("REG FAIL ({})", e);
                }
            }
        }
    }

    let rt = driver_runtime::DRIVER_RUNTIME.lock();
    let total = rt.count();
    drop(rt);

    crate::serial_println!("[BOOT] === Summary: {} total, {} loaded, {} active, {} faulted ===",
        total, total_loaded, total_active, total_faulted);
}

pub fn driver_scan(path: &str) -> Vec<String> {
    let mut results = Vec::new();
    crate::globals::with_vfs(|vfs| {
        match vfs.resolve_path(path) {
            Ok((drive_idx, node)) => {
                if (node.mode & MODE_DIR) == 0 { return; }
                let mut i = 0;
                loop {
                    match vfs.readdir(drive_idx, node.inode, i) {
                        Ok(Some(entry)) => {
                            let name = entry.name.to_ascii_uppercase();
                            if !name.ends_with(".NEM") || (entry.node.mode & MODE_DIR) != 0 {
                                i += 1; continue;
                            }
                            let full = alloc::format!("{}\\{}", path.trim_end_matches('\\'), name);
                            results.push(full);
                            i += 1;
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
            Err(_) => {}
        }
    });
    results
}

fn read_nem_file(path: &str) -> Result<Vec<u8>, &'static str> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| "VFS resolve failed")?;
        if node.mode & MODE_FILE == 0 {
            return Err("Not a file");
        }
        let size = node.size as usize;
        if size == 0 || size > 65536 {
            return Err("Bad size");
        }
        let mut buf = alloc::vec::Vec::with_capacity(size);
        buf.resize(size, 0);
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
