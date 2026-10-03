//! Ob namespace — public entry and directory types.

use alloc::collections::BTreeMap;
use super::*;

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
    pub children: BTreeMap<[u8; MAX_NAME_LEN], ObId>,
    pub child_dirs: BTreeMap<[u8; MAX_NAME_LEN], DirectoryObject>,
    pub symlinks: BTreeMap<[u8; MAX_NAME_LEN], SymlinkEntry>,
    pub protected: bool,
    pub creator: Option<ObId>,
}

impl DirectoryObject {
    pub fn new(name: &str) -> Self {
        DirectoryObject {
            name: name_to_key(name),
            children: BTreeMap::new(),
            child_dirs: BTreeMap::new(),
            symlinks: BTreeMap::new(),
            protected: false,
            creator: None,
        }
    }

    pub fn name_str(&self) -> &str {
        key_to_str(&self.name)
    }
}
