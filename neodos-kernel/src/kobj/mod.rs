use crate::{test_case, test_eq, test_true};
use spin::Mutex;
use lazy_static::lazy_static;

pub const MAX_KOBJ_ENTRIES: usize = 64;
const KOBJ_NAME_LEN: usize = 24;

pub type KObjId = u64;

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
}

impl KObjType {
    pub fn to_str(self) -> &'static str {
        match self {
            KObjType::Unknown => "UNKNOWN",
            KObjType::Process => "PROCESS",
            KObjType::Driver => "DRIVER",
            KObjType::Device => "DEVICE",
            KObjType::Pipe => "PIPE",
            KObjType::EventBus => "EVENTBUS",
            KObjType::BlockDevice => "BLOCKDEV",
            KObjType::Filesystem => "FILESYSTEM",
            KObjType::MemoryRegion => "MEMREGION",
        }
    }
}

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

    fn set_name(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let len = bytes.len().min(KOBJ_NAME_LEN - 1);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len] = 0;
    }
}

pub struct KObjRegistry {
    entries: [Option<KObjEntry>; MAX_KOBJ_ENTRIES],
    count: usize,
    next_id: KObjId,
}

impl KObjRegistry {
    pub const fn new() -> Self {
        const NONE: Option<KObjEntry> = None;
        KObjRegistry {
            entries: [NONE; MAX_KOBJ_ENTRIES],
            count: 0,
            next_id: 1,
        }
    }

    pub fn register(
        &mut self,
        obj_type: KObjType,
        name: &str,
        native_id: u64,
    ) -> Result<KObjId, &'static str> {
        if self.count >= MAX_KOBJ_ENTRIES {
            return Err("KOBJ registry full");
        }
        let id = self.next_id;
        self.next_id += 1;

        let mut entry = KObjEntry {
            id,
            refcount: 1,
            obj_type,
            name: [0u8; KOBJ_NAME_LEN],
            flags: 0,
            creation_tick: crate::hal::get_ticks(),
            native_id,
        };
        entry.set_name(name);

        for slot in self.entries.iter_mut() {
            if slot.is_none() {
                *slot = Some(entry);
                self.count += 1;
                return Ok(id);
            }
        }
        Err("No free KOBJ slot")
    }

    pub fn unregister(&mut self, id: KObjId) -> bool {
        for slot in self.entries.iter_mut() {
            if let Some(entry) = slot {
                if entry.id == id {
                    *slot = None;
                    self.count -= 1;
                    return true;
                }
            }
        }
        false
    }

    pub fn lookup(&self, id: KObjId) -> Option<&KObjEntry> {
        self.entries.iter().flatten().find(|e| e.id == id)
    }

    pub fn lookup_mut(&mut self, id: KObjId) -> Option<&mut KObjEntry> {
        self.entries.iter_mut().flatten().find(|e| e.id == id)
    }

    pub fn ref_inc(&mut self, id: KObjId) -> Option<u32> {
        self.lookup_mut(id).map(|e| {
            e.refcount = e.refcount.saturating_add(1);
            e.refcount
        })
    }

    pub fn ref_dec(&mut self, id: KObjId) -> Option<u32> {
        self.lookup_mut(id).map(|e| {
            if e.refcount > 0 {
                e.refcount -= 1;
            }
            e.refcount
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &KObjEntry> {
        self.entries.iter().flatten()
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

lazy_static! {
    pub static ref KOBJ_REGISTRY: Mutex<KObjRegistry> = Mutex::new(KObjRegistry::new());
}

pub fn kobj_register(obj_type: KObjType, name: &str, native_id: u64) -> Result<KObjId, &'static str> {
    KOBJ_REGISTRY.lock().register(obj_type, name, native_id)
}

pub fn kobj_unregister(id: KObjId) -> bool {
    KOBJ_REGISTRY.lock().unregister(id)
}

pub fn kobj_ref(id: KObjId) -> Option<u32> {
    KOBJ_REGISTRY.lock().ref_inc(id)
}

pub fn kobj_unref(id: KObjId) -> Option<u32> {
    KOBJ_REGISTRY.lock().ref_dec(id)
}

pub fn kobj_lookup(id: KObjId) -> Option<KObjEntry> {
    KOBJ_REGISTRY.lock().lookup(id).copied()
}

pub fn kobj_count() -> usize {
    KOBJ_REGISTRY.lock().len()
}

pub fn kobj_iter_snapshot() -> alloc::vec::Vec<(KObjId, KObjType, [u8; KOBJ_NAME_LEN], u32, u64)> {
    let reg = KOBJ_REGISTRY.lock();
    let mut res = alloc::vec::Vec::new();
    for e in reg.iter() {
        res.push((e.id, e.obj_type, e.name, e.refcount, e.native_id));
    }
    res
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

    test_case!("kobj_registry_full", {
        let mut ids = alloc::vec::Vec::new();
        while let Ok(id) = kobj_register(KObjType::Unknown, "fill", 0) {
            ids.push(id);
        }
        test_true!(ids.len() > 0);
        test_true!(kobj_register(KObjType::Unknown, "should_fail", 0).is_err());
        for id in ids {
            kobj_unregister(id);
        }
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
