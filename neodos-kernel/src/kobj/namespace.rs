use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::kobj::{KObjId, KObjType};
use crate::object::{ObId, ObType};
use crate::{test_case, test_eq, test_true};
use spin::Mutex;
use lazy_static::lazy_static;

const MAX_NAME_LEN: usize = 24;
const MAX_PATH_LEN: usize = 255;
const MAX_SYMLINK_HOPS: u32 = 10;

pub const OB_INVALID_PATH: &str = "OB_INVALID_PATH";
pub const OB_PATH_TOO_LONG: &str = "OB_PATH_TOO_LONG";
pub const OB_NAME_TOO_LONG: &str = "OB_NAME_TOO_LONG";
pub const OB_NOT_FOUND: &str = "OB_NOT_FOUND";
pub const OB_ALREADY_EXISTS: &str = "OB_ALREADY_EXISTS";
pub const OB_CANNOT_CREATE_ROOT: &str = "OB_CANNOT_CREATE_ROOT";
pub const OB_SAME_NAME: &str = "OB_SAME_NAME";
pub const OB_SYMLINK_LOOP: &str = "OB_SYMLINK_LOOP";

fn name_to_key(name: &str) -> [u8; MAX_NAME_LEN] {
    let mut key = [0u8; MAX_NAME_LEN];
    let bytes = name.as_bytes();
    let len = bytes.len().min(MAX_NAME_LEN - 1);
    for i in 0..len {
        key[i] = bytes[i].to_ascii_lowercase();
    }
    key
}

fn key_to_str(key: &[u8; MAX_NAME_LEN]) -> &str {
    let len = key.iter().position(|&b| b == 0).unwrap_or(MAX_NAME_LEN);
    core::str::from_utf8(&key[..len]).unwrap_or("<?>")
}

pub fn normalize_path(path: &str) -> alloc::string::String {
    if path.is_empty() {
        return "\\".to_string();
    }
    let mut result = alloc::string::String::new();
    result.push('\\');
    let has_drive = path.len() >= 2 && path.as_bytes()[1] == b':';
    let path_body = if has_drive { &path[2..] } else { path };
    let trimmed = path_body.trim_start_matches('\\');
    if trimmed.is_empty() {
        return result;
    }
    let parts: Vec<&str> = trimmed.split('\\').collect();
    let mut out_parts: Vec<&str> = Vec::new();
    for part in parts {
        match part {
            "" | "." => continue,
            ".." => {
                out_parts.pop();
            }
            _ => {
                out_parts.push(part);
            }
        }
    }
    if has_drive {
        let drive_letter = path.as_bytes()[0].to_ascii_uppercase();
        result.push(drive_letter as char);
        result.push(':');
    }
    for (i, p) in out_parts.iter().enumerate() {
        if i > 0 || has_drive {
            result.push('\\');
        }
        result.push_str(p);
    }
    result
}

#[derive(Debug, Clone)]
pub struct SymlinkEntry {
    pub name: [u8; MAX_NAME_LEN],
    pub target: [u8; 255],
}

impl SymlinkEntry {
    pub fn new(name: &str, target: &str) -> Self {
        let mut entry = SymlinkEntry {
            name: [0u8; MAX_NAME_LEN],
            target: [0u8; 255],
        };
        let nkey = name_to_key(name);
        entry.name.copy_from_slice(&nkey);
        let tbytes = target.as_bytes();
        let tlen = tbytes.len().min(254);
        entry.target[..tlen].copy_from_slice(&tbytes[..tlen]);
        entry
    }

    pub fn target_str(&self) -> &str {
        let len = self.target.iter().position(|&b| b == 0).unwrap_or(254);
        core::str::from_utf8(&self.target[..len]).unwrap_or("<?>")
    }

    pub fn name_str(&self) -> &str {
        key_to_str(&self.name)
    }
}

/// Entry returned by namespace enumeration.
#[derive(Debug, Clone)]
pub struct NamespaceEntry {
    pub name: [u8; 32],
    pub obj_type: u32,
    pub obj_id: ObId,
}

#[derive(Debug, Clone)]
pub struct DirectoryObject {
    pub name: [u8; MAX_NAME_LEN],
    pub children: BTreeMap<[u8; MAX_NAME_LEN], KObjId>,
    pub child_dirs: BTreeMap<[u8; MAX_NAME_LEN], DirectoryObject>,
    pub symlinks: BTreeMap<[u8; MAX_NAME_LEN], SymlinkEntry>,
}

impl DirectoryObject {
    pub fn new(name: &str) -> Self {
        DirectoryObject {
            name: name_to_key(name),
            children: BTreeMap::new(),
            child_dirs: BTreeMap::new(),
            symlinks: BTreeMap::new(),
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
            return Err(OB_INVALID_PATH);
        }
        if path.len() > MAX_PATH_LEN {
            return Err(OB_PATH_TOO_LONG);
        }
        let trimmed = path.trim_end_matches('\\');
        if trimmed.len() <= 1 {
            return Ok(Vec::new());
        }
        let components: Vec<&str> = trimmed[1..].split('\\').collect();
        for c in &components {
            if c.is_empty() {
                return Err(OB_INVALID_PATH);
            }
            if c.len() > MAX_NAME_LEN {
                return Err(OB_NAME_TOO_LONG);
            }
        }
        Ok(components)
    }

    pub fn create_directory(&mut self, path: &str) -> Result<(), &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            if let Some(&_id) = self.root.children.get(&name_to_key("\\")) {
                return Ok(());
            }
            return Err(OB_NOT_FOUND);
        }
        Self::create_dir_internal(&mut self.root, &components)
    }

    fn create_dir_internal(dir: &mut DirectoryObject, components: &[&str]) -> Result<(), &'static str> {
        let name = components[0];
        let key = name_to_key(name);

        if components.len() == 1 {
            if dir.child_dirs.contains_key(&key) {
                return Err(OB_ALREADY_EXISTS);
            }
            if dir.symlinks.contains_key(&key) {
                return Err(OB_ALREADY_EXISTS);
            }
            dir.child_dirs.insert(key, DirectoryObject::new(name));
            Ok(())
        } else {
            if let Some(subdir) = dir.child_dirs.get_mut(&key) {
                Self::create_dir_internal(subdir, &components[1..])
            } else {
                Err(OB_NOT_FOUND)
            }
        }
    }

    pub fn insert_object(&mut self, path: &str, kobj_id: KObjId) -> Result<(), &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err(OB_INVALID_PATH);
        }
        let obj_name = components[components.len() - 1];
        let key = name_to_key(obj_name);

        if components.len() == 1 {
            if self.root.children.contains_key(&key) {
                return Err(OB_ALREADY_EXISTS);
            }
            if self.root.symlinks.contains_key(&key) {
                return Err(OB_ALREADY_EXISTS);
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
                return Err(OB_NOT_FOUND);
            }
        }
        if current.children.contains_key(&key) {
            return Err(OB_ALREADY_EXISTS);
        }
        if current.symlinks.contains_key(&key) {
            return Err(OB_ALREADY_EXISTS);
        }
        current.children.insert(key, kobj_id);
        Ok(())
    }

    pub fn insert_symlink(&mut self, path: &str, target: &str) -> Result<(), &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err(OB_INVALID_PATH);
        }
        let sl_name = components[components.len() - 1];
        let key = name_to_key(sl_name);

        if target.is_empty() || target.len() > 254 {
            return Err("OB_SYMLINK_INVALID_TARGET");
        }

        if components.len() == 1 {
            if self.root.children.contains_key(&key) {
                return Err(OB_ALREADY_EXISTS);
            }
            if self.root.child_dirs.contains_key(&key) {
                return Err(OB_ALREADY_EXISTS);
            }
            if self.root.symlinks.contains_key(&key) {
                return Err(OB_ALREADY_EXISTS);
            }
            self.root.symlinks.insert(key, SymlinkEntry::new(sl_name, target));
            return Ok(());
        }

        let parent_components = &components[..components.len() - 1];
        let mut current = &mut self.root;
        for &comp in parent_components {
            let ckey = name_to_key(comp);
            if let Some(subdir) = current.child_dirs.get_mut(&ckey) {
                current = subdir;
            } else {
                return Err(OB_NOT_FOUND);
            }
        }
        if current.children.contains_key(&key) {
            return Err(OB_ALREADY_EXISTS);
        }
        if current.child_dirs.contains_key(&key) {
            return Err(OB_ALREADY_EXISTS);
        }
        if current.symlinks.contains_key(&key) {
            return Err(OB_ALREADY_EXISTS);
        }
        current.symlinks.insert(key, SymlinkEntry::new(sl_name, target));
        Ok(())
    }

    /// Enumerate children of a directory by path.
    /// Check whether a path exists as a directory node in the namespace.
    pub fn is_directory(&self, path: &str) -> bool {
        let components = match Self::parse_path(path) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if components.is_empty() {
            return true; // root is always a directory
        }
        let mut current = &self.root;
        for &comp in &components {
            let key = name_to_key(comp);
            if let Some(subdir) = current.child_dirs.get(&key) {
                current = subdir;
            } else {
                return false;
            }
        }
        true
    }

    pub fn enumerate(&self, path: &str) -> Result<Vec<NamespaceEntry>, &'static str> {
        let components = Self::parse_path(path)?;
        let mut dir = &self.root;
        for comp in &components {
            let key = name_to_key(comp);
            dir = dir.child_dirs.get(&key).ok_or(OB_NOT_FOUND)?;
        }
        let mut result = Vec::new();
        // Add child directories
        for (_key, subdir) in &dir.child_dirs {
            let name_str = subdir.name_str();
            let mut name = [0u8; 32];
            let bytes = name_str.as_bytes();
            let len = bytes.len().min(31);
            name[..len].copy_from_slice(&bytes[..len]);
            result.push(NamespaceEntry {
                name,
                obj_type: ObType::Directory as u32,
                obj_id: 0,
            });
        }
        // Add objects
        for (_key, &obj_id) in &dir.children {
            let name_str = key_to_str(_key);
            let mut name = [0u8; 32];
            let bytes = name_str.as_bytes();
            let len = bytes.len().min(31);
            name[..len].copy_from_slice(&bytes[..len]);
            let obj_type = crate::object::ob_lookup(obj_id)
                .map(|o| o.obj_type as u32)
                .unwrap_or(0);
            result.push(NamespaceEntry { name, obj_type, obj_id });
        }
        Ok(result)
    }

    fn resolve_symlink_internal<'a>(&self, path: &str, depth: u32) -> Result<KObjId, &'static str> {
        if depth > MAX_SYMLINK_HOPS {
            return Err(OB_SYMLINK_LOOP);
        }
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err(OB_NOT_FOUND);
        }
        let mut current = &self.root;
        for i in 0..components.len() {
            let is_last = i == components.len() - 1;
            let key = name_to_key(components[i]);
            if is_last {
                if let Some(&kobj_id) = current.children.get(&key) {
                    return Ok(kobj_id);
                }
                if let Some(symlink) = current.symlinks.get(&key) {
                    let target = symlink.target_str();
                    if target.starts_with('\\') {
                        return self.resolve_symlink_internal(target, depth + 1);
                    }
                    let parent_path = {
                        let mut p = alloc::string::String::new();
                        for j in 0..i {
                            p.push('\\');
                            p.push_str(components[j]);
                        }
                        p
                    };
                    let resolved_path = if parent_path.len() <= 1 {
                        alloc::format!("\\{}", target)
                    } else {
                        alloc::format!("{}\\{}", parent_path, target)
                    };
                    return self.resolve_symlink_internal(&resolved_path, depth + 1);
                }
                return Err(OB_NOT_FOUND);
            }
            if let Some(subdir) = current.child_dirs.get(&key) {
                current = subdir;
            } else if let Some(symlink) = current.symlinks.get(&key) {
                let target = symlink.target_str();
                let rest_path = {
                    let mut p = alloc::string::String::new();
                    p.push_str(target);
                    for j in (i + 1)..components.len() {
                        p.push('\\');
                        p.push_str(components[j]);
                    }
                    p
                };
                let resolved = if rest_path.starts_with('\\') { rest_path.clone() } else { alloc::format!("\\{}", rest_path) };
                return self.resolve_symlink_internal(&resolved, depth + 1);
            } else {
                return Err(OB_NOT_FOUND);
            }
        }
        Err(OB_NOT_FOUND)
    }

    pub fn lookup_path(&self, path: &str) -> Result<KObjId, &'static str> {
        self.resolve_symlink_internal(path, 0)
    }

    pub fn lookup_path_no_follow(&self, path: &str) -> Result<KObjId, &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err(OB_NOT_FOUND);
        }
        let mut current = &self.root;
        for i in 0..components.len() {
            let is_last = i == components.len() - 1;
            let key = name_to_key(components[i]);
            if is_last {
                return current.children.get(&key).copied().ok_or(OB_NOT_FOUND);
            }
            current = current.child_dirs.get(&key).ok_or(OB_NOT_FOUND)?;
        }
        Err(OB_NOT_FOUND)
    }

    pub fn lookup_symlink(&self, path: &str) -> Result<SymlinkEntry, &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err(OB_NOT_FOUND);
        }
        let sl_name = components[components.len() - 1];
        let key = name_to_key(sl_name);
        if components.len() == 1 {
            return self.root.symlinks.get(&key).cloned().ok_or(OB_NOT_FOUND);
        }
        let parent_components = &components[..components.len() - 1];
        let mut current = &self.root;
        for &comp in parent_components {
            let ckey = name_to_key(comp);
            if let Some(subdir) = current.child_dirs.get(&ckey) {
                current = subdir;
            } else {
                return Err(OB_NOT_FOUND);
            }
        }
        current.symlinks.get(&key).cloned().ok_or(OB_NOT_FOUND)
    }

    pub fn remove_object(&mut self, path: &str) -> Result<KObjId, &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err(OB_INVALID_PATH);
        }
        let obj_name = components[components.len() - 1];
        let key = name_to_key(obj_name);

        if components.len() == 1 {
            return self.root.children.remove(&key).ok_or(OB_NOT_FOUND);
        }

        let parent_components = &components[..components.len() - 1];
        let mut current = &mut self.root;
        for &comp in parent_components {
            let ckey = name_to_key(comp);
            if let Some(subdir) = current.child_dirs.get_mut(&ckey) {
                current = subdir;
            } else {
                return Err(OB_NOT_FOUND);
            }
        }
        current.children.remove(&key).ok_or(OB_NOT_FOUND)
    }

    pub fn remove_symlink(&mut self, path: &str) -> Result<SymlinkEntry, &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            return Err(OB_INVALID_PATH);
        }
        let sl_name = components[components.len() - 1];
        let key = name_to_key(sl_name);

        if components.len() == 1 {
            return self.root.symlinks.remove(&key).ok_or(OB_NOT_FOUND);
        }

        let parent_components = &components[..components.len() - 1];
        let mut current = &mut self.root;
        for &comp in parent_components {
            let ckey = name_to_key(comp);
            if let Some(subdir) = current.child_dirs.get_mut(&ckey) {
                current = subdir;
            } else {
                return Err(OB_NOT_FOUND);
            }
        }
        current.symlinks.remove(&key).ok_or(OB_NOT_FOUND)
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
            return Err(OB_SAME_NAME);
        }

        if components.len() == 1 {
            match self.root.child_dirs.remove(&old_key) {
                Some(dir) => {
                    if self.root.child_dirs.contains_key(&new_key) || self.root.symlinks.contains_key(&new_key) {
                        self.root.child_dirs.insert(old_key, dir);
                        return Err(OB_ALREADY_EXISTS);
                    }
                    let mut renamed = dir;
                    renamed.name = new_key;
                    self.root.child_dirs.insert(new_key, renamed);
                    Ok(())
                }
                None => Err(OB_NOT_FOUND),
            }
        } else {
            let parent_components = &components[..components.len() - 1];
            let mut current = &mut self.root;
            for &comp in parent_components {
                let ckey = name_to_key(comp);
                if let Some(subdir) = current.child_dirs.get_mut(&ckey) {
                    current = subdir;
                } else {
                    return Err(OB_NOT_FOUND);
                }
            }
            match current.child_dirs.remove(&old_key) {
                Some(dir) => {
                    if current.child_dirs.contains_key(&new_key) || current.symlinks.contains_key(&new_key) {
                        current.child_dirs.insert(old_key, dir);
                        return Err(OB_ALREADY_EXISTS);
                    }
                    let mut renamed = dir;
                    renamed.name = new_key;
                    current.child_dirs.insert(new_key, renamed);
                    Ok(())
                }
                None => Err(OB_NOT_FOUND),
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

    pub fn symlink_count(&self) -> usize {
        self.count_symlinks(&self.root)
    }

    fn count_symlinks(&self, dir: &DirectoryObject) -> usize {
        let mut count = dir.symlinks.len();
        for subdir in dir.child_dirs.values() {
            count += self.count_symlinks(subdir);
        }
        count
    }

    pub fn lookup_by_path(&self, path: &str) -> Result<KObjId, &'static str> {
        let normalized = normalize_path(path);
        let components = Self::parse_path(&normalized)?;
        if components.is_empty() {
            return Err(OB_NOT_FOUND);
        }
        self.resolve_symlink_internal(&normalized, 0)
    }

    /// Find the namespace path for a given ObId by searching the tree.
    /// Returns None if the ObId is not found in the namespace.
    pub fn find_path_by_id(&self, target_id: ObId) -> Option<String> {
        fn search(dir: &DirectoryObject, prefix: &str, target_id: ObId) -> Option<String> {
            for (key, &id) in &dir.children {
                if id == target_id {
                    let name = key_to_str(key);
                    if prefix.is_empty() {
                        return Some(alloc::format!("\\{}", name));
                    }
                    return Some(alloc::format!("{}\\{}", prefix, name));
                }
            }
            for (key, subdir) in &dir.child_dirs {
                let name = key_to_str(key);
                let sub_prefix = if prefix.is_empty() {
                    alloc::format!("\\{}", name)
                } else {
                    alloc::format!("{}\\{}", prefix, name)
                };
                if let Some(path) = search(subdir, &sub_prefix, target_id) {
                    return Some(path);
                }
            }
            None
        }
        search(&self.root, "", target_id)
    }
}

lazy_static! {
    pub static ref OB_NAMESPACE: Mutex<ObNamespace> = Mutex::new(ObNamespace::new());
}

pub fn init_object_namespace() {
    {
        let mut ns = OB_NAMESPACE.lock();
        let root_dirs = ["Device", "DosDevices", "Global", "Driver", "FileSystem", "Ob", "Registry", "Process"];
        for dir in root_dirs {
            let path = alloc::format!("\\{}", dir);
            let _ = ns.create_directory(&path);
        }
    }
    // Register KObjs outside the lock to avoid deadlock with kobj_register → ob_insert_object_auto
    let root_dirs = ["Device", "DosDevices", "Global", "Driver", "FileSystem", "Ob", "Registry", "Process"];
    for dir in root_dirs {
        let _ = crate::kobj::kobj_register(KObjType::Directory, dir, 0);
    }
    // Register root "\" in the namespace so ObOpen("\") works
    let root_id = crate::object::ob_create_object(
        ObType::Directory, "\\", 0, 0, None,
    ).unwrap_or(0);
    if root_id != 0 {
        let _ = OB_NAMESPACE.lock().insert_object("\\", root_id);
    }
    ob_namespace_debug();
}

pub fn ob_insert_object(path: &str, kobj_id: KObjId) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().insert_object(path, kobj_id)
}

pub fn ob_lookup_path(path: &str) -> Result<KObjId, &'static str> {
    OB_NAMESPACE.lock().lookup_path(path)
}

pub fn ob_lookup_path_no_follow(path: &str) -> Result<KObjId, &'static str> {
    OB_NAMESPACE.lock().lookup_path_no_follow(path)
}

pub fn ob_lookup_by_path(path: &str) -> Result<KObjId, &'static str> {
    OB_NAMESPACE.lock().lookup_by_path(path)
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

pub fn ob_enumerate_namespace(path: &str) -> Result<Vec<NamespaceEntry>, &'static str> {
    OB_NAMESPACE.lock().enumerate(path)
}

pub fn ob_namespace_debug() {
    let ns = OB_NAMESPACE.lock();
    fn dump(dir: &DirectoryObject, prefix: &str) {
        for (key, &id) in &dir.children {
            let mut name_buf = [0u8; 25];
            let mut i = 0;
            while i < 24 && key[i] != 0 { name_buf[i] = key[i]; i += 1; }
            let name = core::str::from_utf8(&name_buf[..i]).unwrap_or("<?>");
            crate::serial_println!("[NS] {}children['{}'] = {}", prefix, name, id);
        }
        for (key, subdir) in &dir.child_dirs {
            let mut name_buf = [0u8; 25];
            let mut i = 0;
            while i < 24 && key[i] != 0 { name_buf[i] = key[i]; i += 1; }
            let name = core::str::from_utf8(&name_buf[..i]).unwrap_or("<?>");
            let sub_prefix = alloc::format!("{}{}/", prefix, name);
            crate::serial_println!("[NS] {}dir (enter)", sub_prefix);
            dump(subdir, &sub_prefix);
        }
    }
    dump(&ns.root, "\\");
}

pub fn ob_is_directory(path: &str) -> bool {
    OB_NAMESPACE.lock().is_directory(path)
}

pub fn ob_find_path_by_id(target_id: ObId) -> Option<String> {
    OB_NAMESPACE.lock().find_path_by_id(target_id)
}

pub fn ob_insert_symlink(path: &str, target: &str) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().insert_symlink(path, target)
}

pub fn ob_lookup_symlink(path: &str) -> Result<SymlinkEntry, &'static str> {
    OB_NAMESPACE.lock().lookup_symlink(path)
}

pub fn ob_remove_symlink(path: &str) -> Result<SymlinkEntry, &'static str> {
    OB_NAMESPACE.lock().remove_symlink(path)
}

fn obj_type_to_auto_path(obj_type: KObjType, name: &str) -> alloc::string::String {
    match obj_type {
        KObjType::Process => alloc::format!("\\Ob\\Process\\{}", name),
        KObjType::Driver => alloc::format!("\\Driver\\{}", name),
        KObjType::Pipe => alloc::format!("\\Ob\\Pipe\\{}", name),
        KObjType::Device => alloc::format!("\\Device\\{}", name),
        KObjType::BlockDevice => alloc::format!("\\Device\\{}", name),
        KObjType::EventBus => alloc::format!("\\Global\\EventBus\\{}", name),
        KObjType::Filesystem => alloc::format!("\\FileSystem\\{}", name),
        KObjType::MemoryRegion => alloc::format!("\\Ob\\Memory\\{}", name),
        KObjType::Symlink => alloc::format!("\\Ob\\Symlink\\{}", name),
        KObjType::MountPoint => alloc::format!("\\Global\\Mount\\{}", name),
        KObjType::Directory => alloc::format!("\\Ob\\Dir\\{}", name),
        KObjType::Unknown => alloc::format!("\\Ob\\Unknown\\{}", name),
    }
}

pub fn ob_insert_object_auto(obj_type: KObjType, name: &str, kobj_id: KObjId) -> Result<(), &'static str> {
    let path = obj_type_to_auto_path(obj_type, name);
    {
        let mut ns = OB_NAMESPACE.lock();
        let components = ObNamespace::parse_path(&path).ok();
        if let Some(comp) = components {
            if comp.len() > 1 {
                let _ = ObNamespace::create_dir_internal(&mut ns.root, &comp[..comp.len() - 1]);
            }
        }
    }
    OB_NAMESPACE.lock().insert_object(&path, kobj_id)
}

pub fn ob_remove_object_auto(obj_type: KObjType, name: &str) {
    let path = obj_type_to_auto_path(obj_type, name);
    let _ = OB_NAMESPACE.lock().remove_object(&path);
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
        crate::kobj::kobj_unregister(id);
    });

    test_case!("ob_lookup_path_simple", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\DosDevices").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::Device, "C:", 0).unwrap();
        ns.insert_object("\\DosDevices\\C:", id).unwrap();

        let found = ns.lookup_path("\\DosDevices\\C:").unwrap();
        test_eq!(found, id);
        crate::kobj::kobj_unregister(id);
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
        crate::kobj::kobj_unregister(id);
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
        test_true!(ns.create_directory("\\Stress").is_ok());
        let mut ids = alloc::vec::Vec::new();
        for i in 0..1000 {
            let name = alloc::format!("obj_{}", i);
            match crate::kobj::kobj_register(crate::kobj::KObjType::Unknown, &name, i as u64) {
                Ok(id) => {
                    let path = alloc::format!("\\Stress\\{}", name);
                    if ns.insert_object(&path, id).is_ok() {
                        ids.push((name, id));
                    }
                }
                Err(_) => break,
            }
        }
        test_eq!(ids.len(), 1000);
        for (name, expected_id) in &ids {
            let path = alloc::format!("\\Stress\\{}", name);
            if let Ok(found) = ns.lookup_path(&path) {
                test_eq!(found, *expected_id);
            }
        }
        test_eq!(ns.object_count(), 1000);
        test_true!(ns.dir_count() >= 2);
        for (_, id) in &ids {
            crate::kobj::kobj_unregister(*id);
        }
    });

    test_case!("ob_symlink_create_simple", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "hdvol0", 0).unwrap();
        ns.insert_object("\\Device\\HarddiskVolume0", id).unwrap();
        ns.create_directory("\\DosDevices").unwrap();
        ns.insert_symlink("\\DosDevices\\C:", "\\Device\\HarddiskVolume0").unwrap();
        test_eq!(ns.symlink_count(), 1);
        let symlink = ns.lookup_symlink("\\DosDevices\\C:").unwrap();
        test_eq!(symlink.target_str(), "\\Device\\HarddiskVolume0");
        crate::kobj::kobj_unregister(id);
    });

    test_case!("ob_symlink_resolve_one_level", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "hdvol0", 0).unwrap();
        ns.insert_object("\\Device\\HarddiskVolume0", id).unwrap();
        ns.create_directory("\\DosDevices").unwrap();
        ns.insert_symlink("\\DosDevices\\C:", "\\Device\\HarddiskVolume0").unwrap();
        let found = ns.lookup_path("\\DosDevices\\C:").unwrap();
        test_eq!(found, id);
        crate::kobj::kobj_unregister(id);
    });

    test_case!("ob_symlink_resolve_chain", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\A").unwrap();
        ns.create_directory("\\A\\B").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::Unknown, "target", 42).unwrap();
        ns.insert_object("\\A\\B\\Target", id).unwrap();
        ns.insert_symlink("\\A\\Link1", "B\\Target").unwrap();
        ns.insert_symlink("\\Link2", "A\\Link1").unwrap();
        let found = ns.lookup_path("\\Link2").unwrap();
        test_eq!(found, id);
        crate::kobj::kobj_unregister(id);
    });

    test_case!("ob_symlink_loop_detection", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Loop").unwrap();
        ns.insert_symlink("\\Loop\\A", "\\Loop\\B").unwrap();
        ns.insert_symlink("\\Loop\\B", "\\Loop\\A").unwrap();
        test_true!(ns.lookup_path("\\Loop\\A").is_err());
    });

    test_case!("ob_symlink_invalid_target", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\DosDevices").unwrap();
        ns.create_directory("\\Device").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "vol", 0).unwrap();
        ns.insert_object("\\Device\\RealVol", id).unwrap();
        ns.insert_symlink("\\DosDevices\\X:", "\\Device\\NonExistent").unwrap();
        test_true!(ns.lookup_path("\\DosDevices\\X:").is_err());
        crate::kobj::kobj_unregister(id);
    });

    test_case!("ob_case_insensitive_lookup", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "MYDRV", 7).unwrap();
        ns.insert_object("\\Device\\MYDRV", id).unwrap();
        let found = ns.lookup_path("\\device\\mydrv").unwrap();
        test_eq!(found, id);
        let found2 = ns.lookup_path("\\Device\\MyDrv").unwrap();
        test_eq!(found2, id);
        crate::kobj::kobj_unregister(id);
    });

    test_case!("ob_normalize_path", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        ns.create_directory("\\Device\\Harddisk0").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "part", 5).unwrap();
        ns.insert_object("\\Device\\Harddisk0\\Partition1", id).unwrap();
        let found = ns.lookup_by_path("\\Device\\Harddisk0\\Partition1").unwrap();
        test_eq!(found, id);
        let found2 = ns.lookup_by_path("\\Device\\.\\Harddisk0\\..\\Harddisk0\\Partition1").unwrap();
        test_eq!(found2, id);
        crate::kobj::kobj_unregister(id);
    });

    test_case!("ob_lookup_by_path_normalized", {
        let mut ns = ObNamespace::new();
        ns.create_directory("\\Device").unwrap();
        let id = crate::kobj::kobj_register(crate::kobj::KObjType::BlockDevice, "rootdev", 9).unwrap();
        ns.insert_object("\\Device\\RootDev", id).unwrap();
        test_true!(ns.lookup_by_path("\\Device\\RootDev").is_ok());
        test_true!(ns.lookup_by_path("\\Device\\rootdev").is_ok());
        test_true!(ns.lookup_by_path("\\Device\\RootDev\\..\\RootDev").is_ok());
        crate::kobj::kobj_unregister(id);
    });
}