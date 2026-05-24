// src/drivers/boot_loader/mod.rs
// Boot Driver Loader System v1
//
// Orchestrates automatic loading of .nem drivers at system startup.
// Loading order: BOOT category → SYSTEM category
//
// A Rust .nem driver becomes ACTIVE only if:
//   - Rust binary loaded
//   - AND driver type/policy validated
//   - AND ABI validated (min/target/max)
//   - AND driver_init() returns success
//   - AND Event Bus binding succeeded
//   - AND certification passed
// Otherwise → remains in previous state (FAULTED if failed)

use alloc::vec::Vec;
use alloc::string::String;
use crate::nem::{self, DriverCategory, ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID};
use crate::drivers::nem::{policy, runtime};
use crate::drivers::nem::drivers::ps2kbd;
use crate::drivers::driver_runtime::{self, DriverState};
use crate::eventbus::EVENT_KEYBOARD_INPUT;
use crate::fs::vfs::MODE_FILE;
use crate::fs::vfs::MODE_DIR;

/// Name → entrypoint table for inline Rust drivers.
struct InlineEntry {
    init: Option<runtime::DriverInitFn>,
    event: Option<runtime::DriverEventFn>,
    fini: Option<runtime::DriverFiniFn>,
}

fn inline_drivers() -> [(&'static str, InlineEntry); 1] {
    [(
        "PS2KBD",
        InlineEntry {
            init: Some(ps2kbd::driver_init as runtime::DriverInitFn),
            event: Some(ps2kbd::driver_on_event as runtime::DriverEventFn),
            fini: Some(ps2kbd::driver_fini as runtime::DriverFiniFn),
        },
    )]
}

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

            let parsed = match nem::parse_nem(&data) {
                Some(p) => p,
                None => { crate::serial_println!("FAIL (Invalid NEM header)"); continue; }
            };

            // Validate policy + ABI
            if let Err(e) = policy::validate_driver(&parsed) {
                crate::serial_println!("SKIP ({})", e); continue;
            }
            if let Err(e) = policy::validate_abi(&parsed) {
                crate::serial_println!("SKIP ({})", e); continue;
            }

            // Normalise name
            let name_upper = parsed.name.to_ascii_uppercase();

            // Check for inline implementation
            let inline = inline_drivers().into_iter()
                .find(|(n, _)| *n == name_upper)
                .map(|(_, e)| e);

            if let Some(entry) = inline {
                // ── Inline driver flow — no binary execution ──
                // Register runtime entry (state = Loaded)
                let rt_id = if parsed.is_v2 {
                    driver_runtime::register_driver_ext(
                        parsed.name, parsed.driver_type,
                        nem::NEM_API_VERSION, parsed.compat_flags,
                        parsed.abi_min, parsed.abi_target, parsed.abi_max,
                        parsed.category,
                    )
                } else {
                    driver_runtime::register_driver(
                        parsed.name, parsed.driver_type,
                        nem::NEM_API_VERSION, parsed.compat_flags,
                    )
                };

                match rt_id {
                    Ok(id) => {
                        crate::serial_print!("REG OK (id={})", id);
                        total_loaded += 1;

                        // Register inline handler in NEM runtime
                        runtime::register_inline(id, &name_upper,
                            entry.init, entry.event, entry.fini);
                        crate::serial_print!(" [INLINE]");

                        // Init (calls the extern "C" driver_init with HST)
                        if runtime::call_init(id).is_ok() {
                            let _ = driver_runtime::DRIVER_RUNTIME.lock()
                                .try_transition(id, DriverState::Initialized);
                            crate::serial_print!(" [INIT]");

                            // Registered
                            let _ = driver_runtime::DRIVER_RUNTIME.lock()
                                .try_transition(id, DriverState::Registered);
                            crate::serial_print!(" [REG]");

                            // Event Bus binding
                            if let Err(e) = runtime::register_event_bus_handler(id, EVENT_KEYBOARD_INPUT) {
                                crate::serial_print!(" [BIND FAIL]");
                                total_faulted += 1;
                                driver_runtime::DRIVER_RUNTIME.lock()
                                    .set_error(id, driver_runtime::ERR_BIND_FAILED, true);
                            } else {
                                let _ = driver_runtime::DRIVER_RUNTIME.lock()
                                    .try_transition(id, DriverState::Bound);
                                crate::serial_print!(" [BOUND]");

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
            } else {
                // ── Non-inline driver: fall through to standard loader ──
                // This spawns a user-mode process for the .nem binary
                match crate::drivers::nem::loader::load_nem(f) {
                    Ok(id) => {
                        crate::serial_println!("NEM OK (id={})", id);
                        total_loaded += 1;
                        // loader::load_nem already runs the certification pipeline
                        let cr = driver_runtime::DRIVER_RUNTIME.lock();
                        if cr.get(id).map_or(false, |d| d.state == DriverState::Active) {
                            total_active += 1;
                        } else if cr.get(id).map_or(false, |d| d.state == DriverState::Faulted) {
                            total_faulted += 1;
                        }
                    }
                    Err(e) => {
                        crate::serial_println!("NEM FAIL ({})", e);
                        continue;
                    }
                }
            }
        }
    }

    let rt = driver_runtime::DRIVER_RUNTIME.lock();
    let total = rt.count();
    let active = rt.active_count();
    let faulted = rt.faulted_count();
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
