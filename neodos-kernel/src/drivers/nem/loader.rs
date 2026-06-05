// src/drivers/nem/loader.rs
// NEM v3 Loader — loads standalone NEM v3 drivers from NeoFS
//
// This loader reads .nem files from the filesystem and loads them into kernel memory
// using the v3 loader backend. All drivers must be NEM v3 format.

use alloc::vec::Vec;
use crate::nem::NemDriverType;
use crate::drivers::driver_runtime::{self, DriverId};


/// Load a .nem v3 driver from a NeoFS path.
/// 
/// 1. Reads the binary from the filesystem
/// 2. Parses and validates NEM v3 format
/// 3. Allocates memory and applies relocations
/// 4. Returns driver ID on success
pub fn load_nem(path: &str) -> Result<DriverId, &'static str> {
    // Read .nem file from NeoFS
    let data = read_file(path).map_err(|_| "Failed to read .nem file")?;
    
    // Load using v3 backend
    let result = super::v3loader::load_nem_v3(&data)?;
    
    // Register in driver runtime
    let driver_name = core::str::from_utf8(&result.name)
        .map(|s| s.split('\0').next().unwrap_or("UNKNOWN"))
        .unwrap_or("UNKNOWN");
    
    let id = driver_runtime::register_driver(
        driver_name,
        NemDriverType::Null,
        1, // NEM_API_VERSION
        0, // compat_flags
    ).map_err(|_| "Failed to register driver")?;
    
    // Register load result for hot reload
    crate::drivers::hotreload::register_load_result(id, &result);

    crate::serial_println!("[NEM] v3 driver loaded: {} (path={})", driver_name, path);
    Ok(id)
}

/// Read a .nem file from NeoFS into a Vec<u8>.
pub fn read_file(path: &str) -> Result<Vec<u8>, ()> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| ())?;
        if node.mode & crate::fs::vfs::MODE_FILE == 0 {
            return Err(());
        }
        let size = node.size as usize;
        if size == 0 || size > 65536 {
            return Err(());
        }
        let mut buf = alloc::vec::Vec::with_capacity(size);
        buf.resize(size, 0);
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).map_err(|_| ())?;
        buf.truncate(read);
        Ok(buf)
    })
}
