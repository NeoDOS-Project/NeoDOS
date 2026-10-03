//! ObNamespace — read-only lookup, enumeration and counting.

use super::*;

impl ObNamespace {
    pub(super) fn parse_path(path: &str) -> Result<Vec<&str>, &'static str> {
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

    pub fn is_directory(&self, path: &str) -> bool {
        let components = match Self::parse_path(path) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if components.is_empty() {
            return true;
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
        for subdir in dir.child_dirs.values() {
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

    fn resolve_symlink_internal(&self, path: &str, depth: u32) -> Result<ObId, &'static str> {
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
                if let Some(&obj_id) = current.children.get(&key) {
                    return Ok(obj_id);
                }
                if let Some(symlink) = current.symlinks.get(&key) {
                    let target = symlink.target_str();
                    if target.starts_with('\\') {
                        return self.resolve_symlink_internal(target, depth + 1);
                    }
                    let parent_path = {
                        let mut p = alloc::string::String::new();
                        for &c in &components[..i] {
                            p.push('\\');
                            p.push_str(c);
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
                    for &c in &components[(i + 1)..] {
                        p.push('\\');
                        p.push_str(c);
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

    pub fn lookup_path(&self, path: &str) -> Result<ObId, &'static str> {
        self.resolve_symlink_internal(path, 0)
    }

    pub fn lookup_path_no_follow(&self, path: &str) -> Result<ObId, &'static str> {
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

    pub fn lookup_by_path(&self, path: &str) -> Result<ObId, &'static str> {
        let normalized = normalize_path(path);
        let components = Self::parse_path(&normalized)?;
        if components.is_empty() {
            return Err(OB_NOT_FOUND);
        }
        self.resolve_symlink_internal(&normalized, 0)
    }

    /// Find the namespace path for a given ObId by searching the tree.
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
