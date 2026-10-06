//! Object Manager namespace.
//!
//! The `ObNamespace` structure is defined here; its inherent impl is split
//! across `query` (read) and `mutate` (write) submodules, and the public
//! types live in `types`. `path` holds normalization/key helpers. Children
//! access the parent-private `root` and helpers via `use super::*`.

use alloc::string::String;
use alloc::vec::Vec;
use crate::object::{ObId, ObType};
use crate::log::LogSubsys;
use spin::Mutex;
use lazy_static::lazy_static;

mod path;
mod types;
mod query;
mod mutate;
mod tests;

use path::{name_to_key, key_to_str, MAX_NAME_LEN, MAX_PATH_LEN, MAX_SYMLINK_HOPS,
           OB_INVALID_PATH, OB_PATH_TOO_LONG, OB_NAME_TOO_LONG, OB_NOT_FOUND,
           OB_ALREADY_EXISTS, OB_SAME_NAME, OB_SYMLINK_LOOP, OB_PROTECTED};
pub use path::normalize_path;
pub use types::{SymlinkEntry, NamespaceEntry, DirectoryObject};

pub use tests::register_namespace_tests;

pub struct ObNamespace {
    root: DirectoryObject,
}

impl ObNamespace {
    pub fn new() -> Self {
        ObNamespace {
            root: DirectoryObject::new("\\"),
        }
    }
}

lazy_static! {
    pub static ref OB_NAMESPACE: Mutex<ObNamespace> = Mutex::new(ObNamespace::new());
}

pub fn init_object_namespace() {
    {
        let mut ns = OB_NAMESPACE.lock();
        let root_dirs = ["Device", "DosDevices", "Global", "Driver", "FileSystem", "Ob", "Registry", "System", "Process"];
        for dir in root_dirs {
            let path = alloc::format!("\\{}", dir);
            let _ = ns.create_directory(&path);
        }
        // Create \Global\FileSystem namespace subtree for VFS path resolution
        // (\Global\FileSystem\C:\path\to\file) — used by ob_open_path and ob_create
        let _ = ns.create_directory("\\Global\\FileSystem");
        // Mark root directories as protected
        for dir in root_dirs {
            let path = alloc::format!("\\{}", dir);
            let _ = ns.set_protected(&path, true);
        }
    }
    // Register KObjs outside the lock to avoid deadlock with kobj_register → ob_insert_object_auto
    let root_dirs = ["Device", "DosDevices", "Global", "Driver", "FileSystem", "Ob", "Registry", "System", "Process"];
    for dir in root_dirs {
        let _ = crate::object::ob_create_object(ObType::Directory, dir, 0, 0, None);
    }
    // Register \Global\FileSystem as a Directory ObObject too
    let _ = crate::object::ob_create_object(ObType::Directory, "Global\\FileSystem", 0, 0, None);
    // Register root "\" in the namespace so ObOpen("\") works
    let root_id = crate::object::ob_create_object(
        ObType::Directory, "\\", 0, 0, None,
    ).unwrap_or(0);
    if root_id != 0 {
        let _ = OB_NAMESPACE.lock().insert_object("\\", root_id);
    }
    ob_namespace_debug();
}

pub fn ob_insert_object(path: &str, obj_id: ObId) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().insert_object(path, obj_id)
}

pub fn ob_lookup_path(path: &str) -> Result<ObId, &'static str> {
    OB_NAMESPACE.lock().lookup_path(path)
}

pub fn ob_lookup_path_no_follow(path: &str) -> Result<ObId, &'static str> {
    OB_NAMESPACE.lock().lookup_path_no_follow(path)
}

pub fn ob_lookup_by_path(path: &str) -> Result<ObId, &'static str> {
    OB_NAMESPACE.lock().lookup_by_path(path)
}

pub fn ob_remove_object(path: &str) -> Result<ObId, &'static str> {
    OB_NAMESPACE.lock().remove_object(path)
}

pub fn ob_create_directory(path: &str) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().create_directory(path)
}

/// Create parent directories for a namespace path, excluding the leaf (mkdir -p on parents).
/// Iterates from root down to the parent of the last component, creating each
/// non-existent directory. The last component (leaf) is NOT created as a directory
/// since it will be inserted as an object entry instead.
/// Path is expected to already be a valid Ob namespace path (e.g. \Global\FileSystem\C:\Temp\file.txt).
pub fn ob_create_directory_tree(path: &str) -> Result<(), &'static str> {
    if path.len() <= 1 || path == "\\" {
        return Ok(());
    }
    let body = path.trim_start_matches('\\');
    if body.is_empty() {
        return Ok(());
    }
    let parts: Vec<&str> = body.split('\\').collect();
    if parts.len() <= 1 {
        return Ok(());  // Single component, no parents to create
    }
    let mut p = alloc::string::String::new();
    for comp in &parts[..parts.len() - 1] {
        p.push('\\');
        p.push_str(comp);
        let _ = OB_NAMESPACE.lock().create_directory(&p);
    }
    Ok(())
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
            kdebug!(LogSubsys::Object, "{}children['{}'] = {}", prefix, name, id);
        }
        for (key, subdir) in &dir.child_dirs {
            let mut name_buf = [0u8; 25];
            let mut i = 0;
            while i < 24 && key[i] != 0 { name_buf[i] = key[i]; i += 1; }
            let name = core::str::from_utf8(&name_buf[..i]).unwrap_or("<?>");
            let sub_prefix = alloc::format!("{}{}/", prefix, name);
            kdebug!(LogSubsys::Object, "{}dir (enter)", sub_prefix);
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

/// Remove any namespace entry that points to the given ObId.
/// Returns the path that was removed, if any.
/// Uses ob_find_path_by_id + ob_remove_object.
pub fn ob_remove_by_id(id: ObId) -> Option<String> {
    let path = ob_find_path_by_id(id)?;
    let _ = ob_remove_object(&path);
    Some(path)
}

pub fn ob_set_protected(path: &str, protected: bool) -> Result<(), &'static str> {
    OB_NAMESPACE.lock().set_protected(path, protected)
}

pub fn ob_is_path_protected(path: &str) -> bool {
    let ns = OB_NAMESPACE.lock();
    let components = match ObNamespace::parse_path(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if components.is_empty() {
        return ns.root.protected;
    }
    let mut current = &ns.root;
    for comp in &components {
        let key = name_to_key(comp);
        match current.child_dirs.get(&key) {
            Some(dir) => current = dir,
            None => return false,
        }
    }
    current.protected
}

pub fn ob_insert_object_checked(path: &str, obj_id: ObId) -> Result<(), &'static str> {
    let mut ns = OB_NAMESPACE.lock();
    let components = ObNamespace::parse_path(path).map_err(|_| OB_INVALID_PATH)?;
    // Check if the parent directory is protected
    if components.len() > 1 {
        let parent_components = &components[..components.len() - 1];
        let mut current = &ns.root;
        for &comp in parent_components {
            let key = name_to_key(comp);
            match current.child_dirs.get(&key) {
                Some(dir) => current = dir,
                None => return Err(OB_NOT_FOUND),
            }
        }
        if current.protected {
            return Err(OB_PROTECTED);
        }
    }
    ns.insert_object(path, obj_id)
}

pub fn ob_create_directory_checked(path: &str) -> Result<(), &'static str> {
    let mut ns = OB_NAMESPACE.lock();
    let components = ObNamespace::parse_path(path).map_err(|_| OB_INVALID_PATH)?;
    if components.len() > 1 {
        let parent_components = &components[..components.len() - 1];
        let mut current = &ns.root;
        for &comp in parent_components {
            let key = name_to_key(comp);
            match current.child_dirs.get(&key) {
                Some(dir) => current = dir,
                None => return Err(OB_NOT_FOUND),
            }
        }
        if current.protected {
            return Err(OB_PROTECTED);
        }
    }
    ns.create_directory(path)
}

pub fn ob_insert_symlink_checked(path: &str, target: &str) -> Result<(), &'static str> {
    let mut ns = OB_NAMESPACE.lock();
    let components = ObNamespace::parse_path(path).map_err(|_| OB_INVALID_PATH)?;
    if components.len() > 1 {
        let parent_components = &components[..components.len() - 1];
        let mut current = &ns.root;
        for &comp in parent_components {
            let key = name_to_key(comp);
            match current.child_dirs.get(&key) {
                Some(dir) => current = dir,
                None => return Err(OB_NOT_FOUND),
            }
        }
        if current.protected {
            return Err(OB_PROTECTED);
        }
    }
    ns.insert_symlink(path, target)
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

fn obj_type_to_auto_path(obj_type: ObType, name: &str) -> alloc::string::String {
    match obj_type {
        ObType::Process => alloc::format!("\\Ob\\Process\\{}", name),
        ObType::Driver => alloc::format!("\\Driver\\{}", name),
        ObType::Pipe => alloc::format!("\\Ob\\Pipe\\{}", name),
        ObType::Device => alloc::format!("\\Device\\{}", name),
        ObType::BlockDevice => alloc::format!("\\Device\\{}", name),
        ObType::EventBus => alloc::format!("\\Global\\EventBus\\{}", name),
        ObType::Filesystem => alloc::format!("\\FileSystem\\{}", name),
        ObType::MemoryRegion => alloc::format!("\\Ob\\Memory\\{}", name),
        ObType::Symlink => alloc::format!("\\Ob\\Symlink\\{}", name),
        ObType::MountPoint => alloc::format!("\\Global\\Mount\\{}", name),
        ObType::Directory => alloc::format!("\\Ob\\Dir\\{}", name),
        ObType::Socket => alloc::format!("\\Ob\\Socket\\{}", name),
        ObType::Unknown => alloc::format!("\\Ob\\Unknown\\{}", name),
        _ => alloc::format!("\\Ob\\Unknown\\{}", name),
    }
}

pub fn ob_insert_object_auto(obj_type: ObType, name: &str, obj_id: ObId) -> Result<(), &'static str> {
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
    OB_NAMESPACE.lock().insert_object(&path, obj_id)
}

pub fn ob_remove_object_auto(obj_type: ObType, name: &str) {
    let path = obj_type_to_auto_path(obj_type, name);
    let _ = OB_NAMESPACE.lock().remove_object(&path);
}
