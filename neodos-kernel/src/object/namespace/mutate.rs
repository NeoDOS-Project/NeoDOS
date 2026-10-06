//! ObNamespace — directory/object/symlink mutation.

use super::*;

impl ObNamespace {
    pub fn create_directory(&mut self, path: &str) -> Result<(), &'static str> {
        let components = Self::parse_path(path)?;
        if components.is_empty() {
            if let Some(&_id) = self.root.children.get(&name_to_key("\\")) {
                return Ok(());
            }
            return Err(OB_NOT_FOUND);
        }
        if components.len() > 1 {
            let mut current = &self.root;
            for &comp in &components[..components.len() - 1] {
                let key = name_to_key(comp);
                match current.child_dirs.get(&key) {
                    Some(dir) => current = dir,
                    None => return Err(OB_NOT_FOUND),
                }
            }
        }
        Self::create_dir_internal(&mut self.root, &components)
    }

    pub(super) fn create_dir_internal(dir: &mut DirectoryObject, components: &[&str]) -> Result<(), &'static str> {
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
            dir.child_dirs.entry(key).or_insert_with(|| DirectoryObject::new(name));
            Self::create_dir_internal(
                dir.child_dirs.get_mut(&key).unwrap(),
                &components[1..]
            )
        }
    }

    pub fn set_protected(&mut self, path: &str, protected: bool) -> Result<(), &'static str> {
        let components = Self::parse_path(path)?;
        let mut current = &mut self.root;
        for comp in &components {
            let key = name_to_key(comp);
            current = current.child_dirs.get_mut(&key).ok_or(OB_NOT_FOUND)?;
        }
        current.protected = protected;
        Ok(())
    }

    pub fn insert_object(&mut self, path: &str, obj_id: ObId) -> Result<(), &'static str> {
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
            self.root.children.insert(key, obj_id);
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
        current.children.insert(key, obj_id);
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

    pub fn remove_object(&mut self, path: &str) -> Result<ObId, &'static str> {
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

}
