//! Object table — extracted from mod.rs (mechanical split)
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::object::types::{ObError, ObId, ObType, OB_NAME_LEN, ObObjectSnapshot, ObEnumEntry};

pub trait ObOperations: Send + Sync {
    fn on_destroy(&self, _id: ObId, _native_id: u64) {}
}

/// Ops for file/directory handles opened via ObOpen → VFS resolution.
/// Currently a no-op; future use: release file locks, flush dirty pages.
pub struct FileHandleOps;

impl ObOperations for FileHandleOps {
    fn on_destroy(&self, _id: ObId, _native_id: u64) {}
}

pub static FILE_HANDLE_OPS: FileHandleOps = FileHandleOps;

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
    let result = OB_TABLE.lock().destroy(id);
    if result.is_ok() {
        // VFS-1.3: Remove stale namespace entry for this ObId
        let _ = crate::object::namespace::ob_remove_by_id(id);
    }
    result
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
        drop(table);
        // VFS-1.3: Remove stale namespace entry
        let _ = crate::object::namespace::ob_remove_by_id(id);
        return Ok(());
    }
    drop(table);
    // Call on_destroy WITHOUT holding OB_TABLE lock (avoids deadlock with ob_destroy_object)
    if let Some(cb) = ops {
        cb.on_destroy(id, native_id);
    }
    let mut table = OB_TABLE.lock();
    table.finalize_destroy(id);
    drop(table);
    // VFS-1.3: Remove stale namespace entry
    let _ = crate::object::namespace::ob_remove_by_id(id);
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

    // VFS-3.3: Prevent ob_create inside \Global\FileSystem\ — FS objects go through VFS
    if normalized.starts_with("\\Global\\FileSystem") {
        return Err(ObError::InvalidParam);
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
    let _ = crate::object::namespace::ob_create_directory_tree(&normalized);
    match crate::object::namespace::ob_insert_object(&normalized, id) {
        Ok(_) => Ok(id),
        Err(_) => {
            let _ = ob_destroy_object(id);
            Err(ObError::AlreadyExists)
        }
    }
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

