pub mod types;

pub use types::{ObError, ObId, ObType, OB_NAME_LEN};
pub use types::ObObjectSnapshot;

use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;


/// Operations trait — each object type can provide callbacks.
pub trait ObOperations: Send + Sync {
    fn on_destroy(&self, _id: ObId) {}
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
            s.as_ref().map_or(false, |o| o.id == id)
        }) {
            Some(i) => i,
            None => return Err(ObError::NotFound),
        };

        let refcount = self.slots[idx].as_ref().map_or(0, |o| o.refcount);
        if refcount > 1 {
            return Err(ObError::RefCountHeld);
        }

        // Extract ops before dropping the slot
        let ops = self.slots[idx].as_ref().and_then(|o| o.ops);

        if let Some(cb) = ops {
            cb.on_destroy(id);
        }

        self.slots[idx] = None;
        self.count -= 1;
        Ok(())
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
    static ref OB_TABLE: Mutex<ObObjectTable> = Mutex::new(ObObjectTable::new());
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
    let new_count = table.dereference(id)?;
    if new_count == 0 {
        table.destroy(id).ok();
    }
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

// ── Tests ──

pub fn register_object_tests() {
    use crate::{test_case, test_eq, test_true};

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
        test_true!(ob_count() >= 10);
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
}
