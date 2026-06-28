pub mod types;
pub mod pipe;
pub mod timer;
pub mod semaphore;
pub mod section;
pub mod namespace;

pub use types::{ObError, ObId, ObType, OB_NAME_LEN};
pub use types::{ObObjectSnapshot, ObEnumEntry};

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;


/// Operations trait — each object type can provide callbacks.
pub trait ObOperations: Send + Sync {
    fn on_destroy(&self, _id: ObId, _native_id: u64) {}
}

// ── Per-object metadata ──

#[derive(Clone, Copy)]
pub struct ObObject {
    pub id: ObId,
    pub obj_type: ObType,
    pub name: [u8; OB_NAME_LEN],
    pub refcount: u32,
    pub flags: u32,
    pub native_id: u64,
    pub ops: Option<&'static dyn ObOperations>,
}

impl ObObject {
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(OB_NAME_LEN);
        core::str::from_utf8(&self.name[..len]).unwrap_or("<?>")
    }

    fn set_name(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let len = bytes.len().min(OB_NAME_LEN - 1);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len] = 0;
    }
}

// ── Object table ──

const INITIAL_TABLE_CAPACITY: usize = 64;

pub struct ObObjectTable {
    slots: Vec<Option<ObObject>>,
    count: usize,
    next_id: ObId,
}

impl ObObjectTable {
    pub fn new() -> Self {
        ObObjectTable {
            slots: Vec::with_capacity(INITIAL_TABLE_CAPACITY),
            count: 0,
            next_id: 1,
        }
    }

    /// Register a new object. Returns the assigned ObId on success.
    pub fn create(
        &mut self,
        obj_type: ObType,
        name: &str,
        native_id: u64,
        flags: u32,
        ops: Option<&'static dyn ObOperations>,
    ) -> Result<ObId, ObError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let mut object = ObObject {
            id,
            obj_type,
            name: [0u8; OB_NAME_LEN],
            refcount: 1,
            flags,
            native_id,
            ops,
        };
        object.set_name(name);

        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(object);
                self.count += 1;
                return Ok(id);
            }
        }
        self.slots.push(Some(object));
        self.count += 1;
        Ok(id)
    }

    /// Look up an object by ID. Returns a copy.
    pub fn lookup(&self, id: ObId) -> Option<ObObject> {
        self.slots.iter().flatten().find(|o| o.id == id).copied()
    }

    /// Mutable lookup.
    pub fn lookup_mut(&mut self, id: ObId) -> Option<&mut ObObject> {
        self.slots.iter_mut().flatten().find(|o| o.id == id)
    }

    /// Increment reference count. Returns new count.
    pub fn reference(&mut self, id: ObId) -> Result<u32, ObError> {
        self.lookup_mut(id)
            .map(|o| {
                o.refcount = o.refcount.saturating_add(1);
                o.refcount
            })
            .ok_or(ObError::NotFound)
    }

    /// Decrement reference count. Returns new count.
    pub fn dereference(&mut self, id: ObId) -> Result<u32, ObError> {
        self.lookup_mut(id)
            .map(|o| {
                if o.refcount > 0 {
                    o.refcount -= 1;
                }
                o.refcount
            })
            .ok_or(ObError::NotFound)
    }

    /// Destroy an object. Fails if refcount > 1 (i.e., caller still holds
    /// the initial creation reference plus any extra references).
    pub fn destroy(&mut self, id: ObId) -> Result<(), ObError> {
        let idx = match self.slots.iter().position(|s| {
            s.as_ref().is_some_and(|o| o.id == id)
        }) {
            Some(i) => i,
            None => return Err(ObError::NotFound),
        };

        let refcount = self.slots[idx].as_ref().map_or(0, |o| o.refcount);
        if refcount > 1 {
            return Err(ObError::RefCountHeld);
        }

        // Extract ops and native_id before dropping the slot
        let ops = self.slots[idx].as_ref().and_then(|o| o.ops);
        let native_id = self.slots[idx].as_ref().map_or(0, |o| o.native_id);

        if let Some(cb) = ops {
            cb.on_destroy(id, native_id);
        }

        self.slots[idx] = None;
        self.count -= 1;
        Ok(())
    }

    /// Extract destroy info (ops + native_id) without clearing the slot.
    /// Used by ob_close_object to call the callback outside the lock.
    pub fn extract_destroy_info(&mut self, id: ObId) -> Result<(Option<&'static dyn ObOperations>, u64), ObError> {
        let idx = match self.slots.iter().position(|s| {
            s.as_ref().is_some_and(|o| o.id == id)
        }) {
            Some(i) => i,
            None => return Err(ObError::NotFound),
        };
        let refcount = self.slots[idx].as_ref().map_or(0, |o| o.refcount);
        if refcount > 0 {
            return Err(ObError::RefCountHeld);
        }
        let ops = self.slots[idx].as_ref().and_then(|o| o.ops);
        let native_id = self.slots[idx].as_ref().map_or(0, |o| o.native_id);
        Ok((ops, native_id))
    }

    /// Finalize destroy — clear the slot after the callback has been called.
    pub fn finalize_destroy(&mut self, id: ObId) {
        if let Some(idx) = self.slots.iter().position(|s| {
            s.as_ref().is_some_and(|o| o.id == id)
        }) {
            self.slots[idx] = None;
            self.count -= 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn snapshot(&self) -> Vec<ObObjectSnapshot> {
        self.slots
            .iter()
            .flatten()
            .map(|o| ObObjectSnapshot {
                id: o.id,
                obj_type: o.obj_type,
                name: alloc::string::String::from(o.name_str()),
                refcount: o.refcount,
                flags: o.flags,
                native_id: o.native_id,
            })
            .collect()
    }
}

// ── Global table ──

lazy_static! {
    pub(crate) static ref OB_TABLE: Mutex<ObObjectTable> = Mutex::new(ObObjectTable::new());

    /// Separate store for SecurityDescriptors, keyed by ObId.
    /// Kept separate from ObObject to preserve Copy on ObObject.
    pub(crate) static ref OB_SECURITY: Mutex<BTreeMap<ObId, crate::security::acl::SecurityDescriptor>> =
        Mutex::new(BTreeMap::new());
}

pub fn init_object_manager() {
    let mut table = OB_TABLE.lock();
    // Create the root namespace directory
    let _ = table.create(ObType::Directory, "\\", 0, 0, None);
    // Register base type directory entries for the namespace
    for (name, typ, native_id) in &[
        ("Process", ObType::Process, 1u64),
        ("Driver", ObType::Driver, 2),
        ("Device", ObType::Device, 3),
        ("Pipe", ObType::Pipe, 4),
        ("Filesystem", ObType::Filesystem, 5),
        ("Directory", ObType::Directory, 6),
        ("Key", ObType::Key, 7),
        ("Event", ObType::Event, 8),
        ("MemoryRegion", ObType::MemoryRegion, 9),
        ("Section", ObType::Section, 10),
        ("Socket", ObType::Socket, 11),
    ] {
        let _ = table.create(*typ, name, *native_id, 0, None);
    }
}

// ── Public API ──

pub fn ob_create_object(
    obj_type: ObType,
    name: &str,
    native_id: u64,
    flags: u32,
    ops: Option<&'static dyn ObOperations>,
) -> Result<ObId, ObError> {
    OB_TABLE.lock().create(obj_type, name, native_id, flags, ops)
}

pub fn ob_destroy_object(id: ObId) -> Result<(), ObError> {
    OB_TABLE.lock().destroy(id)
}

pub fn ob_lookup(id: ObId) -> Option<ObObject> {
    OB_TABLE.lock().lookup(id)
}

pub fn ob_open_object(id: ObId, _access: u32) -> Result<(), ObError> {
    OB_TABLE.lock().reference(id)?;
    Ok(())
}

pub fn ob_close_object(id: ObId) -> Result<(), ObError> {
    let mut table = OB_TABLE.lock();
    let cnt = table.dereference(id)?;
    if cnt > 0 {
        return Ok(());
    }
    // Refcount reached 0 — extract destroy info and drop lock before callback
    let (ops, native_id) = table.extract_destroy_info(id)?;
    if ops.is_none() && native_id == 0 {
        // No callback, simple cleanup
        table.finalize_destroy(id);
        return Ok(());
    }
    drop(table);
    // Call on_destroy WITHOUT holding OB_TABLE lock (avoids deadlock with ob_destroy_object)
    if let Some(cb) = ops {
        cb.on_destroy(id, native_id);
    }
    let mut table = OB_TABLE.lock();
    table.finalize_destroy(id);
    Ok(())
}

pub fn ob_reference(id: ObId) -> Result<u32, ObError> {
    OB_TABLE.lock().reference(id)
}

pub fn ob_dereference(id: ObId) -> Result<u32, ObError> {
    OB_TABLE.lock().dereference(id)
}

pub fn ob_count() -> usize {
    OB_TABLE.lock().len()
}

pub fn ob_enum_snapshot() -> Vec<ObObjectSnapshot> {
    OB_TABLE.lock().snapshot()
}

// ── OB-010: ObOpen path-based lookup with security ──

/// Open an object by Ob namespace path.
/// 1. Resolves `path_str` through the Ob namespace
/// 2. Verifies the object exists in the Object Manager
/// 3. Performs security access check against `token` with `desired_access`
/// 4. References the object (caller must later dereference via ob_close_object)
///
/// Returns the ObId on success, or an ObError.
pub fn ob_open_path(
    path_str: &str,
    token: &crate::security::token::Token,
    desired_access: u32,
) -> Result<ObId, ObError> {
    // First try a regular lookup (finds object entries).
    if let Ok(kobj_id) = crate::object::namespace::ob_lookup_path(path_str) {
        if let Some(_obj) = ob_lookup(kobj_id) {
            let sd = OB_SECURITY.lock().get(&kobj_id).cloned();
            if !crate::security::access::se_access_check(token, sd.as_ref(), desired_access) {
                return Err(ObError::AccessDenied);
            }
            ob_reference(kobj_id)?;
            return Ok(kobj_id);
        }
        // Namespace entry exists but object is gone (destroyed on fd close).
        // Remove stale entry and continue to VFS resolution.
        let _ = crate::object::namespace::ob_remove_object(path_str);
    }

    // If not found as an object entry, check if it's a namespace directory
    // that exists but has no object entry yet. If so, create a directory
    // object for it on the fly.
    // Skip this for \Global\FileSystem\ paths — they must go through VFS
    // resolution below which sets the correct drive index (flags).
    let is_global_fs = path_str.starts_with("\\Global\\FileSystem\\");
    if !is_global_fs && crate::object::namespace::ob_is_directory(path_str) {
        let dir_id = ob_create_object(ObType::Directory, path_str, 0, 0, None)?;
        // Attempt atomic insert; if another thread created the same entry first,
        // destroy our object and use the existing one.
        let id = match crate::object::namespace::ob_insert_object(path_str, dir_id) {
            Ok(_) => dir_id,
            Err(_) => {
                let _ = ob_destroy_object(dir_id);
                match crate::object::namespace::ob_lookup_path(path_str) {
                    Ok(existing_id) => existing_id,
                    Err(_) => return Err(ObError::NotFound),
                }
            }
        };
        let sd = OB_SECURITY.lock().get(&id).cloned();
        if !crate::security::access::se_access_check(token, sd.as_ref(), desired_access) {
            let _ = ob_destroy_object(id);
            return Err(ObError::AccessDenied);
        }
        ob_reference(id)?;
        return Ok(id);
    }

    // Attempt VFS resolution for paths under \Global\FileSystem\
    // This enables ObOpen("\Global\FileSystem\C:\path\to\file") to work
    // by resolving the VFS path, creating an ObObject, and inserting it
    // into the namespace.
    if let Some(vfs_path) = path_str.strip_prefix("\\Global\\FileSystem\\") {
        if !vfs_path.is_empty() && vfs_path.contains(':') {
            let result = crate::globals::with_vfs(|vfs| vfs.resolve_path(vfs_path));
            if let Ok((drive_idx, node)) = result {
                let is_dir = (node.mode & crate::fs::vfs::MODE_DIR) != 0;
                let obj_type = if is_dir { ObType::Directory } else { ObType::Filesystem };
                let obj_id = ob_create_object(obj_type, path_str, node.inode as u64, drive_idx as u32, None)?;
                {
                    let _ = crate::object::namespace::ob_create_directory_tree(path_str);
                }
                // Atomic insert; if another thread created the same entry first,
                // destroy our object and use the existing one.
                let id = match crate::object::namespace::ob_insert_object(path_str, obj_id) {
                    Ok(_) => obj_id,
                    Err(_) => {
                        let _ = ob_destroy_object(obj_id);
                        match crate::object::namespace::ob_lookup_path(path_str) {
                            Ok(existing_id) => existing_id,
                            Err(_) => return Err(ObError::NotFound),
                        }
                    }
                };
                ob_reference(id)?;
                return Ok(id);
            }
        }
    }

    Err(ObError::NotFound)
}

/// Create an object and register it in the Ob namespace at the specified path.
/// Used by sys_ob_create (RAX=61).
/// For Pipe objects, also creates the underlying pipe buffer.
pub fn ob_create_object_path(
    path_str: &str,
    obj_type: ObType,
    attrs: u32,
    ops: Option<&'static dyn ObOperations>,
) -> Result<ObId, ObError> {
    let normalized = crate::object::namespace::normalize_path(path_str);
    let leaf = match normalized.rfind('\\') {
        Some(idx) => &normalized[idx + 1..],
        None => return Err(ObError::InvalidParam),
    };
    if leaf.is_empty() || leaf == "\\" {
        return Err(ObError::InvalidParam);
    }

    // Reject Unknown type — only concrete types are valid for path creation
    if obj_type == ObType::Unknown {
        return Err(ObError::InvalidType);
    }

    let native_id = match obj_type {
        ObType::Pipe => {
            let pipe_id = crate::object::pipe::PIPE_MANAGER.alloc()
                .ok_or(ObError::OutOfMemory)?;
            pipe_id as u64
        }
        _ => attrs as u64,
    };

    let id = ob_create_object(obj_type, leaf, native_id, attrs, ops)?;

    // Insert into namespace (create parent directories as needed)
    {
        let parent_path = match normalized.rfind('\\') {
            Some(idx) if idx > 0 => &normalized[..idx],
            _ => "\\",
        };
        if parent_path != "\\" {
            let _ = crate::object::namespace::ob_create_directory(parent_path);
        }
    }
    match crate::object::namespace::ob_insert_object(&normalized, id) {
        Ok(_) => Ok(id),
        Err(_) => {
            let _ = ob_destroy_object(id);
            Err(ObError::AlreadyExists)
        }
    }
}

/// Enumerate objects in a namespace directory by path.
/// Returns a list of ObEnumEntry values.
pub fn ob_enum_directory(path: &str) -> Result<alloc::vec::Vec<ObEnumEntry>, ObError> {
    let entries = crate::object::namespace::ob_enumerate_namespace(path)
        .map_err(|_| ObError::NotFound)?;
    Ok(entries.iter().map(|e| {
        let mut name = [0u8; 32];
        let len = e.name.iter().position(|&b| b == 0).unwrap_or(32).min(31);
        name[..len].copy_from_slice(&e.name[..len]);
        name[len] = 0;
        ObEnumEntry {
            id: e.obj_id,
            obj_type: e.obj_type,
            name,
            mode: 0,
            _pad: [0u8; 2],
            size: 0,
        }
    }).collect())
}

/// Attach a SecurityDescriptor to an existing Object Manager object.
/// Used to enable access checks for ObOpen.
/// Set the name of an ObObject (used by ObSetInfo).
pub fn ob_set_object_name(id: ObId, name: &str) -> Result<(), ObError> {
    let mut table = OB_TABLE.lock();
    let obj = table.lookup_mut(id).ok_or(ObError::NotFound)?;
    let bytes = name.as_bytes();
    let len = bytes.len().min(OB_NAME_LEN - 1);
    obj.name[..len].copy_from_slice(&bytes[..len]);
    obj.name[len] = 0;
    Ok(())
}

pub fn ob_set_security(
    id: ObId,
    sd: crate::security::acl::SecurityDescriptor,
) -> Result<(), ObError> {
    ob_lookup(id).ok_or(ObError::NotFound)?;
    OB_SECURITY.lock().insert(id, sd);
    Ok(())
}

// ── Tests ──

pub fn register_object_tests() {
    use crate::{test_case, test_eq, test_true};
    namespace::register_namespace_tests();

    test_case!("ob_create_lookup", {
        let id = ob_create_object(ObType::Process, "test_proc", 42, 0, None).unwrap();
        test_true!(id > 0);
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.id, id);
        test_eq!(obj.obj_type, ObType::Process);
        test_eq!(obj.native_id, 42);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_destroy_fails_with_ref", {
        let id = ob_create_object(ObType::Driver, "test_drv", 1, 0, None).unwrap();
        ob_reference(id).unwrap();
        let result = ob_destroy_object(id);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::RefCountHeld);
        ob_dereference(id).unwrap();
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_refcount", {
        let id = ob_create_object(ObType::Device, "ref_test", 0, 0, None).unwrap();
        let r1 = ob_reference(id).unwrap();
        test_eq!(r1, 2);
        let r2 = ob_dereference(id).unwrap();
        test_eq!(r2, 1);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_double_destroy_fails", {
        let id = ob_create_object(ObType::MemoryRegion, "double", 0, 0, None).unwrap();
        ob_destroy_object(id).unwrap();
        let result = ob_destroy_object(id);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::NotFound);
    });

    test_case!("ob_lookup_not_found", {
        let result = ob_lookup(9999);
        test_true!(result.is_none());
    });

    test_case!("ob_enum_snapshot", {
        let start_count = ob_count();
        let id1 = ob_create_object(ObType::Process, "snap1", 10, 0, None).unwrap();
        let id2 = ob_create_object(ObType::Driver, "snap2", 20, 0, None).unwrap();
        let snap = ob_enum_snapshot();
        test_eq!(snap.len(), start_count + 2);
        let snap1 = snap.iter().find(|s| s.id == id1).unwrap();
        test_eq!(snap1.name, "snap1");
        test_eq!(snap1.obj_type, ObType::Process);
        ob_destroy_object(id1).unwrap();
        ob_destroy_object(id2).unwrap();
    });

    test_case!("ob_open_close", {
        let id = ob_create_object(ObType::Filesystem, "open_close", 99, 0, None).unwrap();
        ob_open_object(id, 0).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.refcount, 2);
        ob_close_object(id).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.refcount, 1);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_type_strings", {
        test_eq!(ObType::Process.to_str(), "PROCESS");
        test_eq!(ObType::Driver.to_str(), "DRIVER");
        test_eq!(ObType::Unknown.to_str(), "UNKNOWN");
        test_eq!(ObType::Key.to_str(), "REGKEY");
        test_eq!(ObType::Semaphore.to_str(), "SEMAPHORE");
        test_eq!(ObType::Timer.to_str(), "TIMER");
        test_eq!(ObType::Section.to_str(), "SECTION");
    });

    test_case!("ob_error_codes", {
        test_eq!(ObError::NotFound.as_err_code(), -1);
        test_eq!(ObError::RefCountHeld.as_err_code(), -4);
        test_eq!(ObError::Success.as_err_code(), 0);
        test_eq!(ObError::Success.to_str(), "SUCCESS");
        test_eq!(ObError::RefCountHeld.to_str(), "REFCOUNT_HELD");
    });

    // ── OB-004: ob_close_object auto-destroy ──

    test_case!("ob_close_object_auto_destroy", {
        let id = ob_create_object(ObType::Filesystem, "close_file", 0, 0, None).unwrap();
        let before = ob_count();
        ob_close_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());
        test_eq!(ob_count(), before - 1);
    });

    test_case!("ob_close_object_keeps_alive_with_refs", {
        let id = ob_create_object(ObType::Pipe, "close_pipe", 0, 0, None).unwrap();
        ob_open_object(id, 0).unwrap(); // refcount 1→2
        ob_close_object(id).unwrap();   // refcount 2→1 (kept alive)
        test_true!(ob_lookup(id).is_some());
        test_eq!(ob_lookup(id).unwrap().refcount, 1);
        ob_close_object(id).unwrap();   // refcount 1→0 → auto-destroy
        test_true!(ob_lookup(id).is_none());
    });

    // ── OB-005: init_object_manager creates root + base types ──

    test_case!("ob_init_root_directory", {
        let snap = ob_enum_snapshot();
        test_true!(ob_count() >= 11);
        let root = snap.iter().find(|s| s.name == "\\");
        test_true!(root.is_some());
        if let Some(r) = root {
            test_eq!(r.obj_type, ObType::Directory);
        }
    });

    test_case!("ob_init_type_entries", {
        let snap = ob_enum_snapshot();
        let names: alloc::vec::Vec<&str> = snap.iter().map(|s| s.name.as_str()).collect();
        test_true!(names.contains(&"Process"));
        test_true!(names.contains(&"Pipe"));
        test_true!(names.contains(&"Device"));
        test_true!(names.contains(&"Filesystem"));
    });

    // ── OB-010: ob_open_path tests ──

    test_case!("ob_open_path_existing_object", {
        // Register an object and insert it into the namespace
        let id = ob_create_object(ObType::Driver, "test_drv", 42, 0, None).unwrap();
        let _ = crate::object::namespace::ob_create_directory("\\Driver"); // ensure dir exists
        let _ = crate::object::namespace::ob_insert_object("\\Driver\\test_drv", id);

        let admin_token = crate::security::token::Token::new_admin();
        let opened_id = ob_open_path("\\Driver\\test_drv", &admin_token,
            crate::security::acl::ACCESS_READ).unwrap();
        test_eq!(opened_id, id);
        // refcount should be 2 (1 from create + 1 from open)
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.refcount, 2);

        // Cleanup: close releases the open reference
        ob_close_object(id).unwrap();
        ob_destroy_object(id).unwrap();
        let _ = crate::object::namespace::ob_remove_object("\\Driver\\test_drv");
    });

    test_case!("ob_open_path_not_found", {
        let admin_token = crate::security::token::Token::new_admin();
        let result = ob_open_path("\\NonExistent\\Path", &admin_token,
            crate::security::acl::ACCESS_READ);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::NotFound);
    });

    test_case!("ob_open_path_access_denied", {
        // Create an object with a restrictive SD (deny user access)
        let id = ob_create_object(ObType::Driver, "secure_drv", 0, 0, None).unwrap();
        let _ = crate::object::namespace::ob_create_directory("\\Driver");
        let _ = crate::object::namespace::ob_insert_object("\\Driver\\secure_drv", id);

        // Set a SD that denies user tokens ACCESS_READ
        use crate::security::acl::{Acl, Ace, SecurityDescriptor};
        let mut acl = Acl::new();
        let user_sid = crate::security::sid::sid_builtin_user();
        acl.add_ace(Ace::deny(user_sid, crate::security::acl::ACCESS_READ));
        let sd = SecurityDescriptor::new().with_dacl(acl);
        ob_set_security(id, sd).unwrap();

        // Try to open with a user token → should be denied
        let user_token = crate::security::token::Token::new_user();
        let result = ob_open_path("\\Driver\\secure_drv", &user_token,
            crate::security::acl::ACCESS_READ);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::AccessDenied);

        // Admin should still be able to open (admin bypass)
        let admin_token = crate::security::token::Token::new_admin();
        let opened_id = ob_open_path("\\Driver\\secure_drv", &admin_token,
            crate::security::acl::ACCESS_READ).unwrap();
        test_eq!(opened_id, id);

        ob_close_object(id).unwrap();
        ob_destroy_object(id).unwrap();
        let _ = crate::object::namespace::ob_remove_object("\\Driver\\secure_drv");
    });

    test_case!("ob_open_path_non_existent_object_in_namespace", {
        // Path exists in namespace but ObId doesn't match any ObObject
        let admin_token = crate::security::token::Token::new_admin();
        let result = ob_open_path("\\Driver\\nonexistent", &admin_token,
            crate::security::acl::ACCESS_READ);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), ObError::NotFound);
    });

    // ── OBF-03: ObType::Thread ──

    test_case!("ob_type_thread_enum", {
        let t = ObType::Thread;
        test_eq!(t as u32, 16);
        test_eq!(t.to_str(), "THREAD");
    });

    // ── OBF-01: ObInfoClass variants ──

    test_case!("ob_info_class_variants", {
        test_eq!(crate::object::types::ObInfoClass::CpuInfo as u32, 7);
        test_eq!(crate::object::types::ObInfoClass::ReadContent as u32, 15);
        test_eq!(crate::object::types::ObInfoClass::VolumeLabel as u32, 16);
        test_eq!(crate::object::types::ObInfoClass::Basic as u32, 0);
        test_eq!(crate::object::types::ObInfoClass::Process as u32, 3);
    });

    // ── OBF-02: ObSetInfoClass variants ──

    test_case!("ob_set_info_class_variants", {
        test_eq!(crate::object::types::ObSetInfoClass::ProcessTerminate as u32, 4);
        test_eq!(crate::object::types::ObSetInfoClass::VfsRename as u32, 6);
        test_eq!(crate::object::types::ObSetInfoClass::WriteContent as u32, 7);
        test_eq!(crate::object::types::ObSetInfoClass::SetCwd as u32, 8);
        test_eq!(crate::object::types::ObSetInfoClass::SetVolumeLabel as u32, 9);
        test_eq!(crate::object::types::ObSetInfoClass::ProcessPriority as u32, 0);
        test_eq!(crate::object::types::ObSetInfoClass::SetProcessVt as u32, 17);
    });

    // ── OBF-04: Thread ObObject lifecycle ──

    test_case!("ob_thread_create_and_destroy", {
        let id = ob_create_object(ObType::Thread, "\\Ob\\Thread\\42", 42, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.obj_type, ObType::Thread);
        test_eq!(obj.native_id, 42);
        ob_destroy_object(id).unwrap();
    });

    test_case!("ob_thread_type_in_enum_snapshot", {
        let id = ob_create_object(ObType::Thread, "\\Ob\\Thread\\99", 99, 0, None).unwrap();
        let snap = ob_enum_snapshot();
        let found = snap.iter().find(|s| s.id == id).unwrap();
        test_eq!(found.obj_type, ObType::Thread);
        test_eq!(found.native_id, 99);
        ob_destroy_object(id).unwrap();
    });

    // ── Legacy compat tests (migrated from kobj/mod.rs) ──

    test_case!("kobj_register_unregister", {
        let id = ob_create_object(ObType::Process, "test_proc", 42, 0, None).unwrap();
        test_true!(id > 0);
        ob_destroy_object(id).unwrap();
    });

    test_case!("kobj_refcount", {
        let id = ob_create_object(ObType::Driver, "test_drv", 1, 0, None).unwrap();
        let r1 = ob_reference(id).unwrap();
        test_eq!(r1, 2);
        let r2 = ob_dereference(id).unwrap();
        test_eq!(r2, 1);
        ob_destroy_object(id).unwrap();
    });

    test_case!("kobj_type_enum", {
        test_eq!(ObType::Process.to_str(), "PROCESS");
        test_eq!(ObType::Driver.to_str(), "DRIVER");
        test_eq!(ObType::Pipe.to_str(), "PIPE");
        test_eq!(ObType::Symlink.to_str(), "SYMLINK");
        test_eq!(ObType::MountPoint.to_str(), "MOUNTPOINT");
        test_eq!(ObType::Unknown.to_str(), "UNKNOWN");
    });

    test_case!("kobj_entry_name", {
        let id = ob_create_object(ObType::Device, "my_device", 0, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.name_str(), "my_device");
        test_eq!(obj.obj_type, ObType::Device);
        test_eq!(obj.native_id, 0);
        ob_destroy_object(id).unwrap();
    });

    test_case!("kobj_registry_dynamic", {
        let mut ids = alloc::vec::Vec::new();
        for i in 0..128 {
            let name = alloc::format!("fill_{}", i);
            if let Ok(id) = ob_create_object(ObType::Unknown, &name, 0, 0, None) {
                ids.push(id);
            } else {
                break;
            }
        }
        test_eq!(ids.len(), 128);
        let one_more_id = ob_create_object(ObType::Unknown, "one_more", 0, 0, None).unwrap();
        let extra_id = ob_create_object(ObType::Unknown, "extra", 0, 0, None).unwrap();
        test_true!(extra_id > 0);
        for id in ids {
            ob_destroy_object(id).unwrap();
        }
        ob_destroy_object(one_more_id).unwrap();
        ob_destroy_object(extra_id).unwrap();
    });

    test_case!("kobj_lookup", {
        let id = ob_create_object(ObType::Filesystem, "lookup_test", 99, 0, None).unwrap();
        let obj = ob_lookup(id).unwrap();
        test_eq!(obj.native_id, 99);
        test_eq!(obj.obj_type, ObType::Filesystem);
        ob_destroy_object(id).unwrap();
        test_true!(ob_lookup(id).is_none());
    });

    test_case!("kobj_double_unregister", {
        let id = ob_create_object(ObType::MemoryRegion, "double", 0, 0, None).unwrap();
        test_true!(ob_destroy_object(id).is_ok());
        test_true!(ob_destroy_object(id).is_err());
    });

    test_case!("kobj_count", {
        let start = ob_count();
        let id1 = ob_create_object(ObType::Process, "cnt1", 0, 0, None).unwrap();
        let id2 = ob_create_object(ObType::Driver, "cnt2", 0, 0, None).unwrap();
        test_eq!(ob_count(), start + 2);
        ob_destroy_object(id1).unwrap();
        test_eq!(ob_count(), start + 1);
        ob_destroy_object(id2).unwrap();
        test_eq!(ob_count(), start);
    });
}
