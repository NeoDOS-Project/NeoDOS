use crate::{test_case, test_eq, test_true};
use crate::object::{self, ObType, ObId, ObObject};

pub const MAX_KOBJ_ENTRIES_HINT: usize = 64;
const KOBJ_NAME_LEN: usize = 24;

pub type KObjId = ObId;

/// Legacy KObjType — mirrors ObType for shared values.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KObjType {
    Unknown = 0,
    Process = 1,
    Driver = 2,
    Device = 3,
    Pipe = 4,
    EventBus = 5,
    BlockDevice = 6,
    Filesystem = 7,
    MemoryRegion = 8,
    Symlink = 9,
    MountPoint = 10,
    Directory = 11,
}

impl KObjType {
    fn to_ob_type(self) -> ObType {
        match self {
            KObjType::Unknown => ObType::Unknown,
            KObjType::Process => ObType::Process,
            KObjType::Driver => ObType::Driver,
            KObjType::Device => ObType::Device,
            KObjType::Pipe => ObType::Pipe,
            KObjType::EventBus => ObType::EventBus,
            KObjType::BlockDevice => ObType::BlockDevice,
            KObjType::Filesystem => ObType::Filesystem,
            KObjType::MemoryRegion => ObType::MemoryRegion,
            KObjType::Symlink => ObType::Symlink,
            KObjType::MountPoint => ObType::MountPoint,
            KObjType::Directory => ObType::Directory,
        }
    }

    fn from_ob_type(t: ObType) -> Self {
        match t {
            ObType::Unknown => KObjType::Unknown,
            ObType::Process => KObjType::Process,
            ObType::Driver => KObjType::Driver,
            ObType::Device => KObjType::Device,
            ObType::Pipe => KObjType::Pipe,
            ObType::EventBus => KObjType::EventBus,
            ObType::BlockDevice => KObjType::BlockDevice,
            ObType::Filesystem => KObjType::Filesystem,
            ObType::MemoryRegion => KObjType::MemoryRegion,
            ObType::Symlink => KObjType::Symlink,
            ObType::MountPoint => KObjType::MountPoint,
            ObType::Directory => KObjType::Directory,
            _ => KObjType::Unknown,
        }
    }

    pub fn to_str(self) -> &'static str {
        self.to_ob_type().to_str()
    }
}

/// Legacy KObjEntry — now a snapshot wrapper around ObObject.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KObjEntry {
    pub id: KObjId,
    pub refcount: u32,
    pub obj_type: KObjType,
    pub name: [u8; KOBJ_NAME_LEN],
    pub flags: u32,
    pub creation_tick: u64,
    pub native_id: u64,
}

impl KObjEntry {
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(KOBJ_NAME_LEN);
        core::str::from_utf8(&self.name[..len]).unwrap_or("<?>")
    }

    fn from_ob_object(obj: &ObObject) -> Self {
        let mut name = [0u8; KOBJ_NAME_LEN];
        let src = obj.name;
        let len = src.iter().position(|&b| b == 0).unwrap_or(KOBJ_NAME_LEN).min(KOBJ_NAME_LEN - 1);
        name[..len].copy_from_slice(&src[..len]);
        name[len] = 0;
        KObjEntry {
            id: obj.id,
            refcount: obj.refcount,
            obj_type: KObjType::from_ob_type(obj.obj_type),
            name,
            flags: obj.flags,
            creation_tick: 0,
            native_id: obj.native_id,
        }
    }
}

// ── Public API (unchanged signatures) ──

pub fn kobj_register(obj_type: KObjType, name: &str, native_id: u64) -> Result<KObjId, &'static str> {
    let ob_type = obj_type.to_ob_type();
    let id = object::ob_create_object(ob_type, name, native_id, 0, None)
        .map_err(|_| "kobj_register: ob_create_object failed")?;
    let _ = namespace::ob_insert_object_auto(obj_type, name, id);
    Ok(id)
}

pub fn kobj_unregister(id: KObjId) -> bool {
    let obj = object::ob_lookup(id);
    let (obj_type, name_bytes) = obj.map(|o| (o.obj_type, o.name)).unwrap_or((ObType::Unknown, [0u8; crate::object::OB_NAME_LEN]));

    if object::ob_destroy_object(id).is_ok() {
        let name_str = {
            let len = name_bytes.iter().position(|&b| b == 0).unwrap_or(32);
            core::str::from_utf8(&name_bytes[..len]).unwrap_or("?")
        };
        namespace::ob_remove_object_auto(KObjType::from_ob_type(obj_type), name_str);
        true
    } else {
        false
    }
}

pub fn kobj_ref(id: KObjId) -> Option<u32> {
    object::ob_reference(id).ok()
}

pub fn kobj_unref(id: KObjId) -> Option<u32> {
    object::ob_dereference(id).ok()
}

pub fn kobj_lookup(id: KObjId) -> Option<KObjEntry> {
    object::ob_lookup(id).as_ref().map(KObjEntry::from_ob_object)
}

pub fn kobj_count() -> usize {
    object::ob_count()
}

pub fn kobj_iter_snapshot() -> alloc::vec::Vec<(KObjId, KObjType, [u8; KOBJ_NAME_LEN], u32, u64)> {
    let snap = object::ob_enum_snapshot();
    snap.iter().map(|s| {
        let mut name = [0u8; KOBJ_NAME_LEN];
        let src = s.name.as_bytes();
        let len = src.len().min(KOBJ_NAME_LEN - 1);
        name[..len].copy_from_slice(&src[..len]);
        name[len] = 0;
        (s.id, KObjType::from_ob_type(s.obj_type), name, s.refcount, s.native_id)
    }).collect()
}

pub fn kobj_update_name(_id: KObjId, _name: &str) -> bool {
    // For now, destroy + recreate is not supported via the Ob API.
    // This is a no-op until ObObjectTable supports rename.
    false
}

pub fn kobj_iter_mut_snapshot() -> alloc::vec::Vec<(u64, &'static mut [u8])> {
    alloc::vec::Vec::new()
}

pub mod namespace;

pub fn register_kobj_tests() {
    namespace::register_namespace_tests();
    test_case!("kobj_register_unregister", {
        let id = kobj_register(KObjType::Process, "test_proc", 42).unwrap();
        test_true!(id > 0);
        test_true!(kobj_unregister(id));
    });

    test_case!("kobj_refcount", {
        let id = kobj_register(KObjType::Driver, "test_drv", 1).unwrap();
        let r1 = kobj_ref(id).unwrap();
        test_eq!(r1, 2);
        let r2 = kobj_unref(id).unwrap();
        test_eq!(r2, 1);
        kobj_unregister(id);
    });

    test_case!("kobj_type_enum", {
        test_eq!(KObjType::Process.to_str(), "PROCESS");
        test_eq!(KObjType::Driver.to_str(), "DRIVER");
        test_eq!(KObjType::Pipe.to_str(), "PIPE");
        test_eq!(KObjType::Symlink.to_str(), "SYMLINK");
        test_eq!(KObjType::MountPoint.to_str(), "MOUNTPOINT");
        test_eq!(KObjType::Unknown.to_str(), "UNKNOWN");
    });

    test_case!("kobj_entry_name", {
        let id = kobj_register(KObjType::Device, "my_device", 0).unwrap();
        let entry = kobj_lookup(id).unwrap();
        test_eq!(entry.name_str(), "my_device");
        test_eq!(entry.obj_type, KObjType::Device);
        test_eq!(entry.native_id, 0);
        kobj_unregister(id);
    });

    test_case!("kobj_registry_dynamic", {
        let mut ids = alloc::vec::Vec::new();
        for i in 0..128 {
            let name = alloc::format!("fill_{}", i);
            if let Ok(id) = kobj_register(KObjType::Unknown, &name, 0) {
                ids.push(id);
            } else {
                break;
            }
        }
        test_eq!(ids.len(), 128);
        let one_more_id = kobj_register(KObjType::Unknown, "one_more", 0).unwrap();
        let extra_id = kobj_register(KObjType::Unknown, "extra", 0).unwrap();
        test_true!(extra_id > 0);
        for id in ids {
            kobj_unregister(id);
        }
        kobj_unregister(one_more_id);
        kobj_unregister(extra_id);
    });

    test_case!("kobj_lookup", {
        let id = kobj_register(KObjType::Filesystem, "lookup_test", 99).unwrap();
        let entry = kobj_lookup(id).unwrap();
        test_eq!(entry.native_id, 99);
        test_eq!(entry.obj_type, KObjType::Filesystem);
        kobj_unregister(id);
        test_eq!(kobj_lookup(id), None);
    });

    test_case!("kobj_double_unregister", {
        let id = kobj_register(KObjType::MemoryRegion, "double", 0).unwrap();
        test_true!(kobj_unregister(id));
        test_eq!(kobj_unregister(id), false);
    });

    test_case!("kobj_count", {
        let start = kobj_count();
        let id1 = kobj_register(KObjType::Process, "cnt1", 0).unwrap();
        let id2 = kobj_register(KObjType::Driver, "cnt2", 0).unwrap();
        test_eq!(kobj_count(), start + 2);
        kobj_unregister(id1);
        test_eq!(kobj_count(), start + 1);
        kobj_unregister(id2);
        test_eq!(kobj_count(), start);
    });
}
