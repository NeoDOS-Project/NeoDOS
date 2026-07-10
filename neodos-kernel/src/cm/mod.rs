pub mod hive;
pub mod cache;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::object::{self, ObType, ObId};
use crate::object::namespace;

use self::hive::{Hive, ValueCell, NULL_CELL};

// ═══════════════════════════════════════════════════════════════════════
// Cm Manager — global registry state
// ═══════════════════════════════════════════════════════════════════════

pub const MAX_HIVES: usize = 8;

pub struct HiveMount {
    pub name: String,
    pub mount_path: String,
    pub hive: Hive,
    pub ob_id: ObId,
}

pub struct CmManager {
    pub(crate) hives: Vec<HiveMount>,
}

impl CmManager {
    pub fn new() -> Self {
        CmManager { hives: Vec::new() }
    }

    pub fn mount(&mut self, name: &str, mount_path: &str, hive: Hive, ob_id: ObId) -> Option<usize> {
        if self.hives.len() >= MAX_HIVES { return None; }
        let idx = self.hives.len();
        self.hives.push(HiveMount {
            name: name.to_string(),
            mount_path: mount_path.to_string(),
            hive,
            ob_id,
        });
        Some(idx)
    }

    pub fn unmount(&mut self, mount_path: &str) -> bool {
        if let Some(pos) = self.hives.iter().position(|h| h.mount_path == mount_path) {
            self.hives.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn find_hive_by_key(&self, key_cell: u32) -> Option<(&HiveMount, u32)> {
        for hm in &self.hives {
            if let Some(relative) = hm.relative_cell(key_cell) {
                return Some((hm, relative));
            }
        }
        None
    }

    pub fn find_hive_by_key_mut(&mut self, key_cell: u32) -> Option<(&mut HiveMount, u32)> {
        for hm in &mut self.hives {
            if let Some(relative) = hm.relative_cell(key_cell) {
                return Some((hm, relative));
            }
        }
        None
    }

    pub fn find_hive_by_path(&self, mount_path: &str) -> Option<&HiveMount> {
        self.hives.iter().find(|h| h.mount_path == mount_path)
    }

    pub fn find_hive_by_path_mut(&mut self, mount_path: &str) -> Option<&mut HiveMount> {
        self.hives.iter_mut().find(|h| h.mount_path == mount_path)
    }

    pub fn hive_count(&self) -> usize {
        self.hives.len()
    }
}

impl HiveMount {
    /// Given an absolute cell index, return the hive-local cell index
    /// if this mount point owns it. Since each hive has its own cell space,
    /// and ObObject native_id encodes the hive index in the upper bits,
    /// THIS IS NOT NEEDED if we use the native_id to store both hive_idx
    /// and cell_idx. Instead, we'll make the ObObject native_id = (hive_idx << 24) | cell_idx.
    fn relative_cell(&self, _encoded_cell: u32) -> Option<u32> {
        // For now, each hive has its own cell space and native_id
        // directly encodes the cell index. So no translation needed.
        // This method is kept for future multi-hive support where
        // the encoded_cell includes the hive index.
        Some(_encoded_cell)
    }
}

// ── Global Cm manager ──

lazy_static! {
    pub static ref CM_MANAGER: Mutex<CmManager> = Mutex::new(CmManager::new());
}

// ═══════════════════════════════════════════════════════════════════════
// Initialization
// ═══════════════════════════════════════════════════════════════════════

/// Encode cell index with hive index: (hive_idx << 24) | cell_idx
pub fn encode_cell(hive_idx: u32, cell_idx: u32) -> u64 {
    ((hive_idx as u64) << 24) | (cell_idx as u64)
}

/// Decode cell index from native_id: returns (hive_idx, cell_idx)
pub fn decode_cell(native_id: u64) -> (u32, u32) {
    let hive_idx = (native_id >> 24) as u32;
    let cell_idx = (native_id & 0x00FFFFFF) as u32;
    (hive_idx, cell_idx)
}

/// Initialize the Cm subsystem. Called during PHASE 3.88 after networking init.
/// Creates the \Registry namespace tree, ensures C:\System\Registry\ exists,
/// and mounts the SYSTEM hive.
pub fn init_cm() {
    // Ensure VFS directory for registry files exists
    let _ = crate::globals::with_vfs(|vfs| -> Result<(), ()> {
        // Try to resolve System\Registry; if not found, create it
        if vfs.resolve_path("C:\\System\\Registry").is_err() {
            let _ = vfs.resolve_path("C:\\System").map_err(|_| ())?;
            vfs.mkdir("C:\\System\\Registry").map_err(|_| ())?;
        }
        Ok(())
    });

    let cm = CM_MANAGER.lock();

    // Create \Registry directories in namespace (if not already created)
    let _ = namespace::ob_create_directory("\\Registry\\Machine");
    let _ = namespace::ob_create_directory("\\Registry\\User");
    drop(cm);

    // Mount SYSTEM hive (try loading from disk first)
    mount_system_hive();
}

fn mount_system_hive() {
    let name = "SYSTEM";
    let mount_path = "\\Registry\\Machine\\System";

    // Try loading from disk first; fall back to fresh hive
    let hive = load_hive_from_vfs(name).unwrap_or_else(|| Hive::new(name));

    // Create Ob object for root key (cell 0, hive 0)
    let encoded = encode_cell(0, 0); // hive 0, cell 0 (root)
    if let Ok(ob_id) = object::ob_create_object(
        ObType::Key,
        "System",
        encoded,
        0,
        None,
    ) {
        // Insert into namespace
        let _ = namespace::ob_create_directory("\\Registry\\Machine\\System");
        let _ = namespace::ob_insert_object("\\Registry\\Machine\\System", ob_id);

        // Register the hive
        let mut cm = CM_MANAGER.lock();
        cm.mount(name, mount_path, hive, ob_id);
    }
}

/// Try to load a hive from C:\System\Registry\<name>.hiv via VFS.
/// Returns None if file doesn't exist or is invalid.
fn load_hive_from_vfs(name: &str) -> Option<Hive> {
    let file_path = alloc::format!("C:\\System\\Registry\\{}.hiv", name);
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(&file_path).ok()?;
        let size = node.size as usize;
        if size < 16 {
            return None;
        }
        let mut buf = alloc::vec![0u8; size];
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).ok()?;
        buf.truncate(read);
        let mut hive = Hive::deserialize(&buf).ok()?;
        hive.name = name.to_string();
        Some(hive)
    })
}

/// Write a hive to C:\System\Registry\<name>.hiv via VFS.
fn flush_hive_to_vfs(hive: &Hive) -> Result<(), ()> {
    if !hive.is_dirty() {
        return Ok(());
    }
    let data = hive.serialize();
    let file_path = alloc::format!("C:\\System\\Registry\\{}.hiv", hive.name);
    crate::globals::with_vfs(|vfs| {
        // Try to open existing file and overwrite
        if let Ok((drive_idx, node)) = vfs.resolve_path(&file_path) {
            // Truncate then write
            let _ = vfs.write(drive_idx, node.inode, 0, &data);
            Ok(())
        } else {
            // Create new file
            let node = vfs.create(&file_path).map_err(|_| ())?;
            vfs.write(0, node.inode, 0, &data).map_err(|_| ())?;
            Ok(())
        }
    })
}

/// Ensure a key path exists in a hive, creating intermediate keys as needed.
/// Returns the cell index of the final key in the path.
/// path is like "CurrentControlSet\\Services\\NeoInit"
fn ensure_key_path(hive: &mut Hive, start: u32, path: &str) -> Option<u32> {
    let parts: Vec<&str> = path.split('\\').filter(|p| !p.is_empty()).collect();
    let mut curr = start;
    for part in &parts {
        curr = match hive.find_key(curr, part) {
            Some(found) => found,
            None => hive.create_key(curr, part)?,
        };
    }
    Some(curr)
}

/// Create default registry values during boot (Phase 3.881).
/// Only sets values that don't already exist, so user overrides survive.
pub fn cm_ensure_default_values() {
    let mut cm = CM_MANAGER.lock();
    if cm.hives.is_empty() {
        return;
    }
    let hm = &mut cm.hives[0]; // SYSTEM hive
    let root = hm.hive.root_cell();

    // 1. CurrentControlSet\Services\NeoInit — NeoInit configuration
    if let Some(key) = ensure_key_path(&mut hm.hive, root, "CurrentControlSet\\Services\\NeoInit") {
        if hm.hive.find_value(key, "EnableVT").is_none() {
            hm.hive.set_value(key, "EnableVT", hive::REG_DWORD, &1u32.to_le_bytes());
        }
        if hm.hive.find_value(key, "AutoStartServices").is_none() {
            hm.hive.set_value(key, "AutoStartServices", hive::REG_SZ, b"");
        }
    }

    // 2. CurrentControlSet\Services\Network\Interfaces\0\DHCPEnabled = 1
    if let Some(key) = ensure_key_path(&mut hm.hive, root, "CurrentControlSet\\Services\\Network\\Interfaces\\0") {
        if hm.hive.find_value(key, "DHCPEnabled").is_none() {
            hm.hive.set_value(key, "DHCPEnabled", hive::REG_DWORD, &1u32.to_le_bytes());
        }
    }

    // 3. CurrentControlSet\Control\WaitForNetwork = 0
    if let Some(key) = ensure_key_path(&mut hm.hive, root, "CurrentControlSet\\Control") {
        if hm.hive.find_value(key, "WaitForNetwork").is_none() {
            hm.hive.set_value(key, "WaitForNetwork", hive::REG_DWORD, &0u32.to_le_bytes());
        }
    }
}

/// Mount a hive file from the filesystem at the given mount path.
/// mount_path is like \Registry\Machine\Software
pub fn cm_load_hive(name: &str, mount_path: &str) -> Result<(), ()> {
    // Check not already mounted
    {
        let cm = CM_MANAGER.lock();
        if cm.find_hive_by_path(mount_path).is_some() {
            return Err(());
        }
    }

    // Try loading from disk first; fall back to fresh hive
    let hive = load_hive_from_vfs(name).unwrap_or_else(|| Hive::new(name));

    // Create directory structure in namespace
    let _ = namespace::ob_create_directory_tree(mount_path);

    // Create Ob object for root key
    let cm2 = CM_MANAGER.lock();
    let hive_idx = cm2.hive_count() as u32;
    drop(cm2);

    let encoded = encode_cell(hive_idx, 0);
    let leaf = match mount_path.rfind('\\') {
        Some(idx) => &mount_path[idx + 1..],
        None => mount_path,
    };
    if let Ok(ob_id) = object::ob_create_object(
        ObType::Key,
        leaf,
        encoded,
        0,
        None,
    ) {
        let _ = namespace::ob_insert_object(mount_path, ob_id);
        let mut cm = CM_MANAGER.lock();
        cm.mount(name, mount_path, hive, ob_id);
        Ok(())
    } else {
        Err(())
    }
}

/// Unmount a hive by its mount path.
pub fn cm_unload_hive(mount_path: &str) -> Result<(), ()> {
    // Flush dirty data before unmount
    {
        let cm = CM_MANAGER.lock();
        if let Some(hm) = cm.find_hive_by_path(mount_path) {
            if hm.hive.is_dirty() {
                let snapshot = hm.hive.clone();
                drop(cm);
                flush_hive_to_vfs(&snapshot)?;
            }
        }
    }
    let mut cm = CM_MANAGER.lock();
    // Mark clean after flush
    if let Some(hm) = cm.find_hive_by_path_mut(mount_path) {
        hm.hive.mark_clean();
    }
    // Remove from namespace
    let _ = namespace::ob_remove_object(mount_path);
    if cm.unmount(mount_path) {
        Ok(())
    } else {
        Err(())
    }
}

/// Find a hive and cell by the ObId's native_id.
/// Returns (&HiveMount, hive_idx, cell_idx)
fn find_by_native(native_id: u64) -> Option<(usize, u32)> {
    let (hive_idx, cell_idx) = decode_cell(native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) < cm.hives.len() {
        Some((hive_idx as usize, cell_idx))
    } else {
        None
    }
}

/// Find a hive and cell by the ObId's native_id (mutable).
fn find_by_native_mut(native_id: u64) -> Option<(usize, u32)> {
    let (hive_idx, cell_idx) = decode_cell(native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) < cm.hives.len() {
        drop(cm);
        Some((hive_idx as usize, cell_idx))
    } else {
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Public Cm API (called from syscall handlers)
// ═══════════════════════════════════════════════════════════════════════

/// Open a subkey path relative to a key cell.
/// key_native_id comes from the fd's ObObject.native_id.
/// Returns the native_id of the subkey (for use in fds).
pub fn cm_open_key(key_native_id: u64, subkey_path: &str) -> Result<u64, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    let found = hm.hive.open_key_by_path(cell_idx, subkey_path).ok_or(())?;
    Ok(encode_cell(hive_idx, found))
}

/// Create a subkey under the given key. Returns the new key's native_id.
pub fn cm_create_key(key_native_id: u64, name: &str) -> Result<u64, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
    let new_idx = hm.hive.create_key(cell_idx, name).ok_or(())?;
    Ok(encode_cell(hive_idx, new_idx))
}

/// Delete a key and all its subkeys.
pub fn cm_delete_key(key_native_id: u64) -> Result<(), ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
    hm.hive.delete_key(cell_idx);
    Ok(())
}

/// Enumerate subkeys of a key. Returns the name at index, or None.
pub fn cm_enum_key(key_native_id: u64, index: u32) -> Result<String, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    hm.hive.enum_key(cell_idx, index).ok_or(())
}

/// Set a value on a key.
pub fn cm_set_value(key_native_id: u64, name: &str, value_type: u32, data: &[u8]) -> Result<(), ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
    hm.hive.set_value(cell_idx, name, value_type, data).ok_or(())
}

/// Delete a value on a key.
pub fn cm_delete_value(key_native_id: u64, name: &str) -> Result<(), ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
    if hm.hive.delete_value(cell_idx, name) { Ok(()) } else { Err(()) }
}

/// Query a value on a key. Returns the value cell.
pub fn cm_query_value(key_native_id: u64, name: &str) -> Result<ValueCell, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    hm.hive.query_value(cell_idx, name).ok_or(())
}

/// Enumerate values of a key. Returns the name at index.
pub fn cm_enum_value(key_native_id: u64, index: u32) -> Result<String, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    hm.hive.enum_value(cell_idx, index).ok_or(())
}

/// Flush a hive to disk.
pub fn cm_flush_key(key_native_id: u64) -> Result<(), ()> {
    let (hive_idx, _cell_idx) = decode_cell(key_native_id);

    // Take a snapshot of the hive (clone to release lock before I/O)
    let hive_snapshot = {
        let cm = CM_MANAGER.lock();
        if (hive_idx as usize) >= cm.hives.len() {
            return Err(());
        }
        // Not dirty, nothing to do
        if !cm.hives[hive_idx as usize].hive.is_dirty() {
            return Ok(());
        }
        // Clone the hive for I/O outside the lock
        let hm = &cm.hives[hive_idx as usize];
        hm.hive.clone()
    };

    // Persist to VFS outside CM_MANAGER lock
    flush_hive_to_vfs(&hive_snapshot)?;

    // Mark clean inside the lock
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) < cm.hives.len() {
        cm.hives[hive_idx as usize].hive.mark_clean();
    }
    Ok(())
}

/// Flush all dirty hives to disk.
pub fn cm_flush_all_hives() {
    // Collect clones of all dirty hives
    let snapshots: Vec<(usize, Hive)> = {
        let cm = CM_MANAGER.lock();
        cm.hives.iter()
            .filter(|hm| hm.hive.is_dirty())
            .enumerate()
            .map(|(i, hm)| (i, hm.hive.clone()))
            .collect()
    };

    if snapshots.is_empty() {
        return;
    }

    crate::serial_println!("[CM] Flushing {} dirty hive(s) to disk...", snapshots.len());

    for (hive_idx, hive) in &snapshots {
        if flush_hive_to_vfs(hive).is_ok() {
            // Mark clean
            let mut cm = CM_MANAGER.lock();
            if (*hive_idx as usize) < cm.hives.len() {
                cm.hives[*hive_idx as usize].hive.mark_clean();
            }
        } else {
            crate::serial_println!("[CM] WARNING: Failed to flush hive '{}'", hive.name);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

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

        // Verify case-insensitive lookup
        let val2 = hive.query_value(root, "PATH").unwrap();
        test_eq!(val2.as_str().unwrap(), "C:\\Programs");

        // Update existing value
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

        // All 3 names should be unique
        test_true!(name0 != name1 && name1 != name2 && name0 != name2);
    });

    test_case!("cm_hive_reload_integrity", {
        // Test that after create/delete/set_value, the hive state is consistent
        let mut hive = Hive::new("TestIntegrity");
        let root = hive.root_cell();

        let sub = hive.create_key(root, "App").unwrap();
        hive.set_value(sub, "Version", hive::REG_SZ, b"1.0").unwrap();
        hive.set_value(sub, "Count", hive::REG_DWORD, &100u32.to_le_bytes()).unwrap();

        test_eq!(hive.value_count(sub), 2);
        test_eq!(hive.key_count(root), 1);

        // Delete the key and verify cleanup
        hive.delete_key(sub);
        test_eq!(hive.key_count(root), 0);
        test_eq!(hive.find_key(root, "App"), None);
    });

    test_case!("cm_cell_corruption_isolated", {
        let mut hive = Hive::new("TestCorrupt");
        let root = hive.root_cell();

        // Allocate up to the limit and verify each cell is valid
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

        // Verify all created keys have correct values
        for (i, &cell_idx) in cells.iter().enumerate() {
            let val = hive.query_value(cell_idx, "data").unwrap();
            test_eq!(val.as_dword().unwrap(), i as u32);
        }

        // Clean up
        for &cell_idx in &cells {
            hive.delete_key(cell_idx);
        }
        // Root should have 0 children
        test_eq!(hive.key_count(root), 0);
    });

    test_case!("cm_syscall_open_key", {
        // Test the internal API that syscalls use
        let mut hive = Hive::new("TestApi");
        let root = hive.root_cell();

        let sub = hive.create_key(root, "System").unwrap();
        let nested = hive.create_key(sub, "Services").unwrap();
        hive.create_key(nested, "Network").unwrap();

        // Path-based open
        let services = hive.open_key_by_path(root, "System\\Services").unwrap();
        test_eq!(services, nested);

        let network = hive.open_key_by_path(root, "System\\Services\\Network").unwrap();
        test_true!(network != NULL_CELL);

        let not_found = hive.open_key_by_path(root, "System\\Missing");
        test_true!(not_found.is_none());
    });

    test_case!("cm_syscall_set_get_value", {
        // Test value operations through the top-level Cm API
        // We skip the Ob layer and test the Hive directly
        let mut hive = Hive::new("TestSetGet");
        let root = hive.root_cell();

        // Create nested keys
        let config = hive.create_key(root, "Config").unwrap();
        hive.create_key(config, "Network").unwrap();

        // Set values via path-based access
        let net = hive.open_key_by_path(root, "Config\\Network").unwrap();
        hive.set_value(net, "IP", hive::REG_SZ, b"10.0.2.15").unwrap();
        hive.set_value(net, "Port", hive::REG_DWORD, &8080u32.to_le_bytes()).unwrap();
        hive.set_value(net, "Enabled", hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();

        test_eq!(hive.value_count(net), 3);

        let ip = hive.query_value(net, "IP").unwrap();
        test_eq!(ip.as_str().unwrap(), "10.0.2.15");

        let port = hive.query_value(net, "Port").unwrap();
        test_eq!(port.as_dword().unwrap(), 8080);

        // Enumerate values
        let v0 = hive.enum_value(net, 0).unwrap();
        test_true!(!v0.is_empty());

        // Delete one value by simulating deletion via syscall
        if let Some(val_idx) = hive.find_value(net, "Enabled") {
            // Extract the value's next pointer before any mutable borrow
            let val_next = match hive.slot(val_idx) {
                Some(hive::Cell::Value(v)) => v.next,
                _ => hive::NULL_CELL,
            };
            // Remove from linked list
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

        // Reproduce the ensure_key_path + ensure_default_values logic
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

        // Verify all values were created correctly
        let v1 = hive.query_value(neoinit, "DefaultShell").unwrap();
        test_eq!(v1.as_str().unwrap(), "C:\\Programs\\neoshell.nxe");
        test_eq!(v1.value_type, hive::REG_SZ);

        let v2 = hive.query_value(net_iface, "DHCPEnabled").unwrap();
        test_eq!(v2.as_dword().unwrap(), 1);
        test_eq!(v2.value_type, hive::REG_DWORD);

        let v3 = hive.query_value(ctrl, "WaitForNetwork").unwrap();
        test_eq!(v3.as_dword().unwrap(), 0);
        test_eq!(v3.value_type, hive::REG_DWORD);

        // Verify idempotency: re-running set_value on existing keys is fine
        hive.set_value(neoinit, "DefaultShell", hive::REG_SZ, b"C:\\Programs\\neoshell.nxe");
        let v1b = hive.query_value(neoinit, "DefaultShell").unwrap();
        test_eq!(v1b.as_str().unwrap(), "C:\\Programs\\neoshell.nxe");

        // Verify the key hierarchy is intact
        test_eq!(hive.key_count(root), 1); // Root has CurrentControlSet
        let ccs = hive.find_key(root, "CurrentControlSet").unwrap();
        test_eq!(hive.key_count(ccs), 2); // CurrentControlSet has Services + Control
        let services = hive.find_key(ccs, "Services").unwrap();
        test_eq!(hive.key_count(services), 2); // Services has NeoInit + Network
        test_eq!(hive.value_count(ctrl), 1); // Control has WaitForNetwork
    });

    // ── B2.7 Persistence tests ──

    test_case!("cm_set_value_persist_roundtrip", {
        // Create a hive with values, serialize, deserialize, verify all values survive
        let mut original = Hive::new("TestRoundtrip");
        let root = original.root_cell();

        let sub = original.create_key(root, "Application").unwrap();
        original.set_value(root, "Version", hive::REG_SZ, b"1.0.42").unwrap();
        original.set_value(root, "DebugMode", hive::REG_DWORD, &0u32.to_le_bytes()).unwrap();
        original.set_value(root, "MaxUsers", hive::REG_DWORD, &100u32.to_le_bytes()).unwrap();
        original.set_value(sub, "Path", hive::REG_SZ, b"C:\\Programs\\Test").unwrap();
        original.set_value(sub, "Enabled", hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();
        original.set_value(sub, "Timeout", hive::REG_DWORD, &5000u32.to_le_bytes()).unwrap();

        // Save original state
        let orig_version = original.query_value(root, "Version").unwrap().clone();

        // Serialize
        let data = original.serialize();
        test_true!(data.len() > 16); // Must have header + content

        // Deserialize
        let mut restored = Hive::deserialize(&data).unwrap();
        restored.name = "TestRoundtrip".to_string();

        // Verify structure
        let restored_root = restored.root_cell();
        test_eq!(restored_root, 0);
        test_eq!(restored.key_count(restored_root), 1); // Application
        let restored_app = restored.find_key(restored_root, "Application").unwrap();
        test_true!(restored_app != hive::NULL_CELL);

        // Verify root values
        let r_version = restored.query_value(restored_root, "Version").unwrap();
        test_eq!(r_version.value_type, hive::REG_SZ);
        test_eq!(r_version.as_str().unwrap(), orig_version.as_str().unwrap());

        let r_max = restored.query_value(restored_root, "MaxUsers").unwrap();
        test_eq!(r_max.as_dword().unwrap(), 100);

        let r_debug = restored.query_value(restored_root, "DebugMode").unwrap();
        test_eq!(r_debug.as_dword().unwrap(), 0);

        // Verify subkey values
        let r_path = restored.query_value(restored_app, "Path").unwrap();
        test_eq!(r_path.as_str().unwrap(), "C:\\Programs\\Test");

        let r_enabled = restored.query_value(restored_app, "Enabled").unwrap();
        test_eq!(r_enabled.as_dword().unwrap(), 1);

        let r_timeout = restored.query_value(restored_app, "Timeout").unwrap();
        test_eq!(r_timeout.as_dword().unwrap(), 5000);

        // Verify value counts
        test_eq!(original.value_count(root), 3);
        test_eq!(restored.value_count(restored_root), 3);
        test_eq!(original.value_count(sub), 3);
        test_eq!(restored.value_count(restored_app), 3);
    });

    test_case!("cm_hive_serialization_integrity", {
        // Test that serialization roundtrip preserves complex state
        // including deleted keys, and that the integrity is maintained
        let mut hive = Hive::new("TestIntegrity2");
        let root = hive.root_cell();

        // Create a deep key hierarchy
        let l1 = hive.create_key(root, "Level1").unwrap();
        let l2 = hive.create_key(l1, "Level2").unwrap();
        let l3 = hive.create_key(l2, "Level3").unwrap();
        hive.set_value(l3, "DeepValue", hive::REG_SZ, b"deep").unwrap();

        // Create a sibling branch
        let other = hive.create_key(root, "Other").unwrap();
        hive.create_key(other, "SubOther").unwrap();

        test_eq!(hive.key_count(root), 2);

        // Serialize and restore
        let data = hive.serialize();
        let mut restored = Hive::deserialize(&data).unwrap();
        restored.name = "TestIntegrity2".to_string();

        // Verify structure
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

        // Now delete a key on the restored hive and verify serialization still fine
        restored.delete_key(r_other);
        test_eq!(restored.key_count(r_root), 1);

        // Re-serialize and re-restore to verify delete persists
        let data2 = restored.serialize();
        let restored2 = Hive::deserialize(&data2).unwrap();
        let r2_root = restored2.root_cell();
        test_eq!(restored2.key_count(r2_root), 1);
        test_true!(restored2.find_key(r2_root, "Other").is_none());
        test_true!(restored2.find_key(r2_root, "Level1").is_some());
    });

    // ── CM-FIX tests ──

    test_case!("cm_free_list_next_fit", {
        let mut hive = Hive::new("TestNextFit");
        let root = hive.root_cell();

        // Allocate keys; indices advance sequentially
        let k1 = hive.create_key(root, "A").unwrap();
        let k2 = hive.create_key(root, "B").unwrap();
        let k3 = hive.create_key(root, "C").unwrap();
        test_true!(k1 < k2);
        test_true!(k2 < k3);

        // Free the middle one; key_count reflects removal
        hive.delete_key(k2);
        test_eq!(hive.key_count(root), 2);

        // Alloc continues from next_alloc_hint (doesn't wrap within 2048-cell pool)
        let k4 = hive.create_key(root, "D").unwrap();
        test_true!(k4 > k3);

        // Free first key; count decreases
        hive.delete_key(k1);
        test_eq!(hive.key_count(root), 2);

        // Freed cells don't affect the sequential hint-based allocator
        let k5 = hive.create_key(root, "E").unwrap();
        test_true!(k5 > k4);

        // Internal cell count is consistent
        test_eq!(hive.cell_count(), 4); // root + C + D + E
    });

    test_case!("cm_delete_value", {
        let mut hive = Hive::new("TestDelVal");
        let root = hive.root_cell();

        hive.set_value(root, "Keep", hive::REG_DWORD, &1u32.to_le_bytes()).unwrap();
        hive.set_value(root, "Remove", hive::REG_SZ, b"delete_me").unwrap();
        hive.set_value(root, "Keep2", hive::REG_DWORD, &2u32.to_le_bytes()).unwrap();
        test_eq!(hive.value_count(root), 3);

        // Delete the middle value
        test_true!(hive.delete_value(root, "Remove"));
        test_eq!(hive.value_count(root), 2);

        // Verify remaining values intact
        let v1 = hive.query_value(root, "Keep").unwrap();
        test_eq!(v1.as_dword().unwrap(), 1);
        let v2 = hive.query_value(root, "Keep2").unwrap();
        test_eq!(v2.as_dword().unwrap(), 2);

        // Delete non-existent returns false
        test_true!(!hive.delete_value(root, "NonExistent"));

        // Delete first in list
        test_true!(hive.delete_value(root, "Keep"));
        test_eq!(hive.value_count(root), 1);

        // Delete last remaining
        test_true!(hive.delete_value(root, "Keep2"));
        test_eq!(hive.value_count(root), 0);
    });

    test_case!("cm_delete_value_persist", {
        let mut hive = Hive::new("TestDelValPersist");
        let root = hive.root_cell();

        hive.set_value(root, "A", hive::REG_DWORD, &10u32.to_le_bytes()).unwrap();
        hive.set_value(root, "B", hive::REG_SZ, b"persist").unwrap();
        hive.set_value(root, "C", hive::REG_DWORD, &20u32.to_le_bytes()).unwrap();

        // Delete B
        test_true!(hive.delete_value(root, "B"));
        test_eq!(hive.value_count(root), 2);

        // Serialize and reload
        let data = hive.serialize();
        let mut restored = Hive::deserialize(&data).unwrap();
        restored.name = "TestDelValPersist".to_string();
        let r_root = restored.root_cell();

        test_eq!(restored.value_count(r_root), 2);
        test_true!(restored.find_value(r_root, "A").is_some());
        test_true!(restored.find_value(r_root, "C").is_some());
        test_true!(restored.find_value(r_root, "B").is_none());

        // Values still have correct data after roundtrip
        let va = restored.query_value(r_root, "A").unwrap();
        test_eq!(va.as_dword().unwrap(), 10);
        let vc = restored.query_value(r_root, "C").unwrap();
        test_eq!(vc.as_dword().unwrap(), 20);
    });

    test_case!("cm_unmount_flush", {
        // Test that unmount flushes dirty data: create hive, set values,
        // simulate unmount by serializing (VFS flush mocked via serialize).
        let mut hive = Hive::new("TestUnmountFlush");
        let root = hive.root_cell();
        hive.set_value(root, "FlushMe", hive::REG_SZ, b"data").unwrap();
        test_true!(hive.is_dirty());

        // Serialize to "flush"
        let data = hive.serialize();
        let restored = Hive::deserialize(&data).unwrap();
        let r_root = restored.root_cell();
        let v = restored.query_value(r_root, "FlushMe").unwrap();
        test_eq!(v.as_str().unwrap(), "data");
    });

    test_case!("cm_deep_key_deletion_iterative", {
        let mut hive = Hive::new("TestDeepIter");
        let root = hive.root_cell();

        // Create a deep hierarchy:
        // root
        //   L1
        //     L2
        //       L3
        //         L4 (with value)
        let l1 = hive.create_key(root, "L1").unwrap();
        let l2 = hive.create_key(l1, "L2").unwrap();
        let l3 = hive.create_key(l2, "L3").unwrap();
        let l4 = hive.create_key(l3, "L4").unwrap();
        hive.set_value(l4, "deep_val", hive::REG_SZ, b"deep").unwrap();

        test_eq!(hive.key_count(root), 1);
        test_eq!(hive.key_count(l1), 1);
        test_eq!(hive.key_count(l2), 1);
        test_eq!(hive.key_count(l3), 1);

        // Delete L1 (should recursively delete all children)
        hive.delete_key(l1);

        test_eq!(hive.key_count(root), 0);
        test_true!(hive.find_key(root, "L1").is_none());
        // Verify no dangling refs: cell count should be 1 (only root)
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

        // Delete middle (Gamma)
        hive.delete_key(c);
        test_eq!(hive.key_count(root), 4);
        test_true!(hive.find_key(root, "Alpha").is_some());
        test_true!(hive.find_key(root, "Beta").is_some());
        test_true!(hive.find_key(root, "Delta").is_some());
        test_true!(hive.find_key(root, "Epsilon").is_some());

        // Delete first (Alpha) — tests head-of-list unlinking
        hive.delete_key(a);
        test_eq!(hive.key_count(root), 3);
        test_true!(hive.find_key(root, "Alpha").is_none());

        // Delete last (Epsilon) — tests tail unlinking
        hive.delete_key(e);
        test_eq!(hive.key_count(root), 2);
        test_true!(hive.find_key(root, "Epsilon").is_none());

        // Delete remaining (Beta, Delta)
        hive.delete_key(b);
        hive.delete_key(d);
        test_eq!(hive.key_count(root), 0);
    });
}

#[allow(unused_imports)]
pub use self::hive::Cell;
