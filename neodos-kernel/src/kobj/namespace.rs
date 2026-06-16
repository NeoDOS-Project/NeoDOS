use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::kobj::KObjId;
use crate::{test_case, test_eq, test_true};
use spin::Mutex;
use lazy_static::lazy_static;

const MAX_NAME_LEN: usize = 24;
const MAX_PATH_LEN: usize = 255;

fn name_to_key(name: &str) -> [u8; MAX_NAME_LEN] {
    let mut key = [0u8; MAX_NAME_LEN];
    let bytes = name.as_bytes();
    let len = bytes.len().min(MAX_NAME_LEN - 1);
    key[..len].copy_from_slice(&bytes[..len]);
    key
}

fn key_to_str(key: &[u8; MAX_NAME_LEN]) -> &str {
    let len = key.iter().position(|&b| b == 0).unwrap_or(MAX_NAME_LEN);
    core::str::from_utf8(&key[..len]).unwrap_or("<?>")
}

#[derive(Debug, Clone)]
pub struct DirectoryObject {
    pub name: [u8; MAX_NAME_LEN],
    pub children: BTreeMap<[u8; MAX_NAME_LEN], KObjId>,
    pub child_dirs: BTreeMap<[u8; MAX_NAME_LEN], DirectoryObject>,
}

impl DirectoryObject {
    pub fn new(name: &str) -> Self {
        DirectoryObject {
            name: name_to_key(name),
            children: BTreeMap::new(),
            child_dirs: BTreeMap::new(),
        }
    }

    pub fn name_str(&self) -> &str {
        key_to_str(&self.name)
    }
}

pub struct ObNamespace {
    root: DirectoryObject,
}

impl ObNamespace {
    pub fn new() -> Self {
        ObNamespace {
            root: DirectoryObject::new("\\"),
        }
    }

    fn parse_path(path: &str) -> Result<Vec<&str>, &'static str> {
        if !path.starts_with('\\') {
            return Err("OB_INVALID_PATH: must start with \\");
        }
        if path.len() > MAX_PATH_LEN {
            return Err("OB_PATH_TOO_LONG");
        }
        let trimmed = path.trim_end_matches('\\');
        if trimmed.len() <= 1 {
            return Ok(Vec::new());
        }
        let components: Vec<&str> = trimmed[1..].split('\\').collect();
        for c in &components {
            if c.is_empty() {
                return Err("OB_INVALID_PATH: empty component");
            }
            if c.len() > MAX_NAME_LEN {
                return Err("OB_NAME_TOO_LONG");
            }
        }
        Ok(components)
    }

    pub fn create_directory(&mut self, path: &str) -> Result<(), &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err("OB_CANNOT_CREATE_ROOT");
        }
        Self::create_dir_internal(&mut self.root, &components)
    }

    fn create_dir_internal(dir: &mut DirectoryObject, components: &[&str]) -> Result<(), &'static str> {
        let name = components[0];
        let key = name_to_key(name);

        if components.len() == 1 {
            if dir.child_dirs.contains_key(&key) {
                return Err("OB_ALREADY_EXISTS");
            }
            dir.child_dirs.insert(key, DirectoryObject::new(name));
            Ok(())
        } else {
            if let Some(subdir) = dir.child_dirs.get_mut(&key) {
                Self::create_dir_internal(subdir, &components[1..])
            } else {
                Err("OB_NOT_FOUND: parent directory")
            }
        }
    }

    pub fn insert_object(&mut self, path: &str, kobj_id: KObjId) -> Result<(), &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err("OB_INVALID_PATH: root is not an object");
        }
        let obj_name = components[components.len() - 1];
        let key = name_to_key(obj_name);

        if components.len() == 1 {
            if self.root.children.contains_key(&key) {
                return Err("OB_ALREADY_EXISTS");
            }
            self.root.children.insert(key, kobj_id);
            return Ok(());
        }

        let parent_components = &components[..components.len() - 1];
        let mut current = &mut self.root;
        for &comp in parent_components {
            let ckey = name_to_key(comp);
            if let Some(subdir) = current.child_dirs.get_mut(&ckey) {
                current = subdir;
            } else {
                return Err("OB_NOT_FOUND: parent directory");
            }
        }
        if current.children.contains_key(&key) {
            return Err("OB_ALREADY_EXISTS");
        }
        current.children.insert(key, kobj_id);
        Ok(())
    }

    pub fn lookup_path(&self, path: &str) -> Result<KObjId, &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err("OB_NOT_FOUND: root has no KObjId");
        }
        let mut current = &self.root;
        for i in 0..components.len() {
            let is_last = i == components.len() - 1;
            let key = name_to_key(components[i]);

            if is_last {
                return current.children.get(&key).copied().ok_or("OB_NOT_FOUND");
            }

            current = current.child_dirs.get(&key).ok_or("OB_NOT_FOUND")?;
        }
        Err("OB_NOT_FOUND")
    }

    pub fn remove_object(&mut self, path: &str) -> Result<KObjId, &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err("OB_INVALID_PATH");
        }
        let obj_name = components[components.len() - 1];
        let key = name_to_key(obj_name);

        if components.len() == 1 {
            return self.root.children.remove(&key).ok_or("OB_NOT_FOUND");
        }

        let parent_components = &components[..components.len() - 1];
        let mut current = &mut self.root;
        for &comp in parent_components {
            let ckey = name_to_key(comp);
            if let Some(subdir) = current.child_dirs.get_mut(&ckey) {
                current = subdir;
            } else {
                return Err("OB_NOT_FOUND");
            }
        }
        current.children.remove(&key).ok_or("OB_NOT_FOUND")
    }

    pub fn rename_directory(&mut self, old_path: &str, new_name: &str) -> Result<(), &'static str> {
        let components = Self::parse_path(old_path)?;
        if components.is_empty() {
            return Err("OB_INVALID: cannot rename root");
        }
        let old_name = components[components.len() - 1];
        let old_key = name_to_key(old_name);
        let new_key = name_to_key(new_name);

        if new_key == old_key {
            return Err("OB_SAME_NAME");
        }

        if components.len() == 1 {
            // Rename in root
            match self.root.child_dirs.remove(&old_key) {
                Some(dir) => {
                    if self.root.child_dirs.contains_key(&new_key) {
                        self.root.child_dirs.insert(old_key, dir);
                        return Err("OB_ALREADY_EXISTS");
                    }
                    let mut renamed = dir;
                    renamed.name = new_key;
                    self.root.child_dirs.insert(new_key, renamed);
                    Ok(())
                }
                None => Err("OB_NOT_FOUND"),
            }
        } else {
            let parent_components = &components[..components.len() - 1];
            let mut current = &mut self.root;
            for &comp in parent_components {
                let ckey = name_to_key(comp);
                if let Some(subdir) = current.child_dirs.get_mut(&ckey) {
                    current = subdir;
                } else {
                    return Err("OB_NOT_FOUND");
                }
            }
            match current.child_dirs.remove(&old_key) {
                Some(dir) => {
                    if current.child_dirs.contains_key(&new_key) {
                        current.child_dirs.insert(old_key, dir);
                        return Err("OB_ALREADY_EXISTS");
                    }
                    let mut renamed = dir;
                    renamed.name = new_key;
                    current.child_dirs.insert(new_key, renamed);
                    Ok(())
                }
                None => Err("OB_NOT_FOUND"),
            }
        }
    }

    pub fn dir_count(&self) -> usize {
        self.count_dirs(&self.root)
    }

    fn count_dirs(&self, dir: &DirectoryObject) -> usize {
        let mut count = 1;
        for subdir in dir.child_dirs.values() {
            count += self.count_dirs(subdir);
        }
        count
    }

    pub fn object_count(&self) -> usize {
        self.count_objects(&self.root)
    }

    fn count_objects(&self, dir: &DirectoryObject) -> usize {
        let mut count = dir.children.len();
        for subdir in dir.child_dirs.values() {
            count += self.count_objects(subdir);
        }
        count
    }
}

lazy_static! {
    pub static ref OB_NAMESPACE: Mutex<ObNamespace> = Mutex::new(ObNamespace::new());
}

pub fn init_object_namespace() {
    let mut ns = OB_NAMESPACE.lock();
    let _ = ns.create_directory("\\Device");
    let _ = ns.create_directory("\\DosDevices");
    let _ = ns.create_directory("\\Global");
    let _ = ns.create_directory("\\Driver");
    let _ = ns.create_directory("\\FileSystem");
    let _ = ns.create_directory("\\Ob");
}

pub fn ob_insert_object(path: &str, kobj_id: KObjId) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().insert_object(path, kobj_id)
}

pub fn ob_lookup_path(path: &str) -> Result<KObjId, &'static str> {
    OB_NAMESPACE.lock().lookup_path(path)
}

pub fn ob_remove_object(path: &str) -> Result<KObjId, &'static str> {
    OB_NAMESPACE.lock().remove_object(path)
}

pub fn ob_create_directory(path: &str) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().create_directory(path)
}

pub fn ob_rename_directory(old_path: &str, new_name: &str) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().rename_directory(old_path, new_name)
}

pub fn register_namespace_tests() {
    test_case!("ob_directory_create", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        test_eq!(ns.dir_count(), 2);
        test_eq!(ns.object_count(), 0);
    });

    test_case!("ob_directory_hierarchy", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.create_directory("\\Device\\Harddisk0").unwrap();
        test_eq!(ns.dir_count(), 3);

        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "part", 1).unwrap();
        ns.create_directory("\\Device\\Harddisk0\\Partition1").unwrap();
        ns.insert_object("\\Device\\Harddisk0\\Partition1", id).unwrap();

        let found = ns.lookup_path("\\Device\\Harddisk0\\Partition1").unwrap();
        test_eq!(found, id);
        test_eq!(ns.object_count(), 1);
    });

    test_case!("ob_lookup_path_simple", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\DosDevices").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::Device, "C:", 0).unwrap();
        ns.insert_object("\\DosDevices\\C:", id).unwrap();

        let found = ns.lookup_path("\\DosDevices\\C:").unwrap();
        test_eq!(found, id);
    });

    test_case!("ob_lookup_path_nested", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.create_directory("\\Device\\Harddisk0").unwrap();

        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "part2", 2).unwrap();
        ns.insert_object("\\Device\\Harddisk0\\Partition2", id).unwrap();

        let found = ns.lookup_path("\\Device\\Harddisk0\\Partition2").unwrap();
        test_eq!(found, id);

        test_true!(ns.lookup_path("\\Device\\Harddisk0\\Partition99").is_err());
        test_true!(ns.lookup_path("\\NonExistent").is_err());
    });

    test_case!("ob_rename_directory", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        test_true!(ns.rename_directory("\\Device", "Devices").is_ok());

        test_true!(ns.create_directory("\\Device\\Sub").is_err());

        ns.create_directory("\\Devices\\Sub").unwrap();
        test_eq!(ns.dir_count(), 3);

        test_true!(ns.rename_directory("\\Devices", "Devices").is_err());
    });

    test_case!("ob_tree_stress_1000_objects", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Test").unwrap();
        let mut ids = alloc::vec::Vec::new();
        for i in 0..1000 {
            let name = alloc::format!("obj_{}", i);
            let id = crate::kobj::kobj_register(crate::kobj::KObjType::Unknown, &name, i as u64).unwrap();
            let path = alloc::format!("\\Test\\{}", name);
            ns.insert_object(&path, id).unwrap();
            ids.push((name, id));
        }
        for (name, expected_id) in &ids {
            let path = alloc::format!("\\Test\\{}", name);
            let found = ns.lookup_path(&path).unwrap();
            test_eq!(found, *expected_id);
        }
        test_eq!(ns.object_count(), 1000);
        test_eq!(ns.dir_count(), 2);
    });
}
