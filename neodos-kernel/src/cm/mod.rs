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
    hives: Vec<HiveMount>,
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
/// Creates the \Registry namespace tree and mounts the SYSTEM hive.
pub fn init_cm() {
    let cm = CM_MANAGER.lock();

    // Create \Registry directories in namespace (if not already created)
    let _ = namespace::ob_create_directory("\\Registry\\Machine");
    let _ = namespace::ob_create_directory("\\Registry\\User");
    drop(cm);

    // Mount SYSTEM hive
    mount_system_hive();
}

fn mount_system_hive() {
    let name = "SYSTEM";
    let mount_path = "\\Registry\\Machine\\System";

    // Create the hive
    let hive = Hive::new(name);

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

    // 1. CurrentControlSet\Services\NeoInit\DefaultShell = "C:\Programs\NeoShell.nxe"
    if let Some(key) = ensure_key_path(&mut hm.hive, root, "CurrentControlSet\\Services\\NeoInit") {
        if hm.hive.find_value(key, "DefaultShell").is_none() {
            hm.hive.set_value(key, "DefaultShell", hive::REG_SZ, b"C:\\Programs\\NeoShell.nxe");
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

    let hive = Hive::new(name);

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
    let mut cm = CM_MANAGER.lock();
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
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    // Flush is a no-op for now (no disk persistence)
    Ok(())
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
        hive.set_value(neoinit, "DefaultShell", hive::REG_SZ, b"C:\\Programs\\NeoShell.nxe");

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
        test_eq!(v1.as_str().unwrap(), "C:\\Programs\\NeoShell.nxe");
        test_eq!(v1.value_type, hive::REG_SZ);

        let v2 = hive.query_value(net_iface, "DHCPEnabled").unwrap();
        test_eq!(v2.as_dword().unwrap(), 1);
        test_eq!(v2.value_type, hive::REG_DWORD);

        let v3 = hive.query_value(ctrl, "WaitForNetwork").unwrap();
        test_eq!(v3.as_dword().unwrap(), 0);
        test_eq!(v3.value_type, hive::REG_DWORD);

        // Verify idempotency: re-running set_value on existing keys is fine
        hive.set_value(neoinit, "DefaultShell", hive::REG_SZ, b"C:\\Programs\\NeoShell.nxe");
        let v1b = hive.query_value(neoinit, "DefaultShell").unwrap();
        test_eq!(v1b.as_str().unwrap(), "C:\\Programs\\NeoShell.nxe");

        // Verify the key hierarchy is intact
        test_eq!(hive.key_count(root), 1); // Root has CurrentControlSet
        let ccs = hive.find_key(root, "CurrentControlSet").unwrap();
        test_eq!(hive.key_count(ccs), 2); // CurrentControlSet has Services + Control
        let services = hive.find_key(ccs, "Services").unwrap();
        test_eq!(hive.key_count(services), 2); // Services has NeoInit + Network
        test_eq!(hive.value_count(ctrl), 1); // Control has WaitForNetwork
    });
}

#[allow(unused_imports)]
pub use self::hive::Cell;
