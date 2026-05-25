// src/drivers/boot_loader/mod.rs
// Boot Driver Loader System v3 Only
//
// Orchestrates automatic loading of NEM v3 standalone binary drivers at system startup.
// Loading order: BOOT category → SYSTEM category
//
// A driver becomes ACTIVE only if:
//   - Binary loaded and parsed
//   AND - ABI validated (min/target/max)
//   AND - driver_init() returns success
//   AND - Event Bus binding succeeded
//   AND - certification passed
// Otherwise → remains in previous state (FAULTED if failed)

use alloc::vec::Vec;
use alloc::string::String;
use crate::nem::{self, DriverCategory, ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID};
use crate::drivers::nem::v3loader;
use crate::drivers::driver_runtime::{self, DriverState};
use crate::eventbus::EVENT_KEYBOARD_INPUT;
use crate::eventbus::EVENT_KEYB_LAYOUT;
use crate::fs::vfs::MODE_FILE;
use crate::fs::vfs::MODE_DIR;

pub fn boot_load_all() {
    crate::serial_println!("[BOOT] === Boot Driver Loader v1 ===");
    let mut total_loaded = 0u32;
    let mut total_active = 0u32;
    let mut total_faulted = 0u32;

    for (phase_name, root) in &[
        ("BOOT", "C:\\SYSTEM\\DRIVERS\\BOOT"),
        ("SYSTEM", "C:\\SYSTEM\\DRIVERS\\SYSTEM"),
    ] {
        crate::serial_println!("[BOOT] Scanning {} drivers...", phase_name);
        let files = driver_scan(root);
        for f in &files {
            crate::serial_print!("[BOOT]   Loading {} ... ", f);

            let data = match read_nem_file(f) {
                Ok(d) => d,
                Err(e) => { crate::serial_println!("FAIL ({})", e); continue; }
            };

            // Try NEM v3 standalone binary first
            if let Some(parsed_v3) = nem::parse_nem_v3(&data) {
                // Validate ABI
                if parsed_v3.header.abi_min == 0 || parsed_v3.header.abi_min > ABI_MAX_VALID
                    || parsed_v3.header.abi_max < ABI_MIN_VALID
                    || parsed_v3.header.abi_target < ABI_MIN_VALID
                    || parsed_v3.header.abi_target > ABI_MAX_VALID
                {
                    crate::serial_println!("SKIP (v3 ABI incompatible)");
                    continue;
                }

                let load_result = match v3loader::load_nem_v3(&data) {
                    Ok(r) => r,
                    Err(e) => {
                        crate::serial_println!("FAIL (v3 load: {})", e);
                        continue;
                    }
                };

                let name_str = alloc::string::String::from_utf8_lossy(&load_result.name);
                let name_upper = name_str.to_ascii_uppercase();

                // Register in driver runtime
                let rt_id = driver_runtime::register_driver(
                    &name_upper,
                    nem::NemDriverType::Lifecycle,
                    nem::NEM_API_VERSION,
                    0,
                );

                match rt_id {
                    Ok(id) => {
                        crate::serial_print!("REG OK (id={})", id);
                        total_loaded += 1;

                        // Init
                        let init_ok = match load_result.entry_init {
                            Some(init_fn) => unsafe { init_fn() == 0 },
                            None => true,
                        };

                        if init_ok {
                            let _ = driver_runtime::DRIVER_RUNTIME.lock()
                                .try_transition(id, DriverState::Initialized);
                            crate::serial_print!(" [INIT]");

                            // Registered
                            let _ = driver_runtime::DRIVER_RUNTIME.lock()
                                .try_transition(id, DriverState::Registered);
                            crate::serial_print!(" [REG]");

                            // Event Bus binding (v3 bridge) — register per-driver event types
                            let bind_ok = match name_upper.as_str() {
                                "PS2KBD" => {
                                    let a = v3loader::register_v3_event_bus_handler(
                                        load_result.entry_event, EVENT_KEYBOARD_INPUT
                                    ).is_ok();
                                    let b = v3loader::register_v3_event_bus_handler(
                                        load_result.entry_event, EVENT_KEYB_LAYOUT
                                    ).is_ok();
                                    a && b
                                }
                                "SERIAL" => {
                                    v3loader::register_v3_event_bus_handler(
                                        load_result.entry_event, crate::eventbus::EVENT_SERIAL_DATA
                                    ).is_ok()
                                }
                                _ => {
                                    v3loader::register_v3_event_bus_handler(
                                        load_result.entry_event, EVENT_KEYBOARD_INPUT
                                    ).is_ok()
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

                                // Driver-local activation hook (optional for v3).
                                // ps2kbd.nem requires this to accept events.
                                let activate_ok = match load_result.entry_activate {
                                    Some(activate_fn) => unsafe { activate_fn() == 0 },
                                    None => true,
                                };

                                if !activate_ok {
                                    crate::serial_print!(" [ACT FAIL]");
                                    total_faulted += 1;
                                    driver_runtime::DRIVER_RUNTIME.lock()
                                        .set_error(id, driver_runtime::ERR_INIT_FAILED, true);
                                    crate::serial_println!();
                                    continue;
                                }

                                // Certify & activate
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
                        crate::serial_println!();
                    }
                    Err(e) => {
                        crate::serial_println!("REG FAIL ({})", e);
                        continue;
                    }
                }
                continue; // v3 handled, skip v1/v2 path
            }
        }
    }

    let rt = driver_runtime::DRIVER_RUNTIME.lock();
    let total = rt.count();
    let _active = rt.active_count();
    let _faulted = rt.faulted_count();
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
        let files = driver_scan("C:\\NONEXISTENT");
        test_eq!(files.len(), 0);
    });

    test_case!("boot_abi_constants_valid", {
        test_eq!(ABI_MIN_VALID, 1);
        test_eq!(ABI_TARGET, 1);
        test_eq!(ABI_MAX_VALID, 2);
    });

    test_case!("boot_driver_scan_path_format", {
        let path = "C:\\SYSTEM\\DRIVERS";
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
    });

    test_case!("boot_activation_fails_on_skip", {
        let id = driver_runtime::register_driver(
            "SKIP", nem::NemDriverType::Null, 1, 0
        ).unwrap();
        let mut rt = driver_runtime::DRIVER_RUNTIME.lock();
        rt.try_transition(id, DriverState::Initialized).unwrap();
        let r = rt.certify_and_activate(id);
        test_true!(r.is_err());
    });
}
