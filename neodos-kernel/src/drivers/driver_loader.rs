// Driver Loader — reads .nem from NeoFS, validates, registers with runtime

use alloc::vec::Vec;
use crate::nem::{self, NemDriverType};
use crate::drivers::driver_runtime::{self, DriverId};
use crate::eventbus::{self, EVENT_DRIVER_LOADED, SOURCE_KERNEL};

// ── Load a .nem driver from a NeoFS path ──

pub fn load_nem(path: &str) -> Result<(DriverId, NemDriverType), &'static str> {
    // 1. Read file from NeoFS
    let data = read_file(path).map_err(|_| "Cannot read file")?;

    // 2. Parse NEM header
    let parsed = nem::parse_nem(&data).ok_or("Invalid NEM format")?;

    // (parse_nem already validates magic, version, header_size, api_version)

    // 3. Register with runtime
    let id = driver_runtime::register_driver(
        parsed.name,
        parsed.driver_type,
        nem::NEM_API_VERSION,
        parsed.compat_flags,
    )?;
    // 4. Set state to Registered
    driver_runtime::DRIVER_RUNTIME.lock().set_state(id, driver_runtime::DriverState::Registered);

    // 5. Push DRIVER_LOADED event
    let _ = eventbus::EVENT_BUS.push_event(
        EVENT_DRIVER_LOADED,
        SOURCE_KERNEL,
        0,
        id as u64,
        0,
        0,
    );

    // 6. Log
    crate::serial_println!(
        "[NEM] Loaded: {} (type={}, id={})",
        parsed.name,
        parsed.driver_type.to_str(),
        id,
    );

    Ok((id, parsed.driver_type))
}

// ── Unload a driver ──

pub fn unload_driver(id: DriverId) -> bool {
    let mut runtime = driver_runtime::DRIVER_RUNTIME.lock();
    let entry = runtime.remove(id);
    if let Some(drv) = entry {
        crate::serial_println!("[NEM] Unloaded: {} (id={})", drv.name_str(), id);
        true
    } else {
        false
    }
}

// ── Read a complete file from NeoFS into a Vec<u8> ──

fn read_file(path: &str) -> Result<Vec<u8>, ()> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| ())?;
        if node.mode & crate::fs::vfs::MODE_FILE == 0 {
            return Err(());
        }
        let size = node.size as usize;
        if size == 0 || size > 65536 {
            return Err(()); // sanity limit
        }
        let mut buf = alloc::vec::Vec::with_capacity(size);
        buf.resize(size, 0u8);
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).map_err(|_| ())?;
        buf.truncate(read);
        Ok(buf)
    })
}

// ── Shell integration ──

pub fn cmd_loadnem(path: &str) {
    crate::serial_print!("[NEM] Loading {} ... ", path);
    match load_nem(path) {
        Ok((id, dt)) => {
            crate::serial_println!("OK (id={}, type={})", id, dt.to_str());
            crate::println!("[NEM] Loaded: {} (id={})", path, id);
        }
        Err(e) => {
            crate::serial_println!("FAIL: {}", e);
            crate::println!("[NEM] FAIL: {} — {}", path, e);
        }
    }
}

pub fn cmd_unloadnem(id_str: &str) {
    let id: DriverId = match id_str.parse() {
        Ok(n) => n,
        Err(_) => {
            crate::println!("[NEM] Invalid driver ID: {}", id_str);
            return;
        }
    };
    if unload_driver(id) {
        crate::println!("[NEM] Driver {} unloaded", id);
    } else {
        crate::println!("[NEM] Driver {} not found", id);
    }
}

// ── List loaded drivers ──

pub fn cmd_nemlist() {
    let names = driver_runtime::driver_names();
    if names.is_empty() {
        crate::println!("[NEM] No drivers loaded");
        return;
    }
    crate::println!("[NEM] Loaded drivers:");
    for (name, id, state) in &names {
        crate::println!("  {:>3}  {:20}  {:10}", id, name, state.to_str());
    }
}
