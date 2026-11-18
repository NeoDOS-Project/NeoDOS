use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::object::{self, ObType};
use crate::object::namespace;
use crate::log::LogSubsys;
use self::super::hive::{Hive, ValueCell};
use self::super::manager::{CM_MANAGER, encode_cell, decode_cell};
use self::super::init::flush_hive_to_vfs;
use self::super::security;
use crate::security::acl::{ACCESS_DELETE, ACCESS_WRITE};
use crate::security::token::Token;

/// Errors that a Cm mutation can report to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmError {
    /// The caller is not allowed to touch this key.
    Denied,
    /// The key (or its hive) does not exist.
    NotFound,
    /// The hive could not allocate the cell/value.
    NoMemory,
    /// The subkey already exists.
    AlreadyExists,
}

impl From<CmError> for () {
    fn from(_: CmError) {}
}

pub fn cm_load_hive(name: &str, mount_path: &str) -> Result<(), ()> {
    {
        let cm = CM_MANAGER.lock();
        if cm.find_hive_by_path(mount_path).is_some() {
            return Err(());
        }
    }

    let hive = {
        let file_path = alloc::format!("C:\\System\\Registry\\{}.hiv", name);
        crate::globals::with_vfs(|vfs| {
            let (drive_idx, node) = vfs.resolve_path(&file_path).ok()?;
            let size = node.size as usize;
            if size < 16 { return None; }
            let mut buf = alloc::vec![0u8; size];
            let read = vfs.read(drive_idx, node.inode, 0, &mut buf).ok()?;
            buf.truncate(read);
            let mut hive = Hive::deserialize(&buf).ok()?;
            hive.name = name.to_string();
            Some(hive)
        }).unwrap_or_else(|| Hive::new(name))
    };

    let _ = namespace::ob_create_directory_tree(mount_path);

    let cm2 = CM_MANAGER.lock();
    let hive_idx = cm2.hive_count() as u32;
    drop(cm2);

    let encoded = encode_cell(hive_idx, 0);
    let leaf = match mount_path.rfind('\\') {
        Some(idx) => &mount_path[idx + 1..],
        None => mount_path,
    };
    if let Ok(ob_id) = object::ob_create_object(
        ObType::Key,
        leaf,
        encoded,
        0,
        None,
    ) {
        let _ = namespace::ob_insert_object(mount_path, ob_id);
        let mut cm = CM_MANAGER.lock();
        cm.mount(name, mount_path, hive, ob_id);
        Ok(())
    } else {
        Err(())
    }
}

pub fn cm_unload_hive(mount_path: &str) -> Result<(), ()> {
    {
        let cm = CM_MANAGER.lock();
        if let Some(hm) = cm.find_hive_by_path(mount_path) {
            if hm.hive.is_dirty() {
                let snapshot = hm.hive.clone();
                drop(cm);
                flush_hive_to_vfs(&snapshot)?;
            }
        }
    }
    let mut cm = CM_MANAGER.lock();
    if let Some(hm) = cm.find_hive_by_path_mut(mount_path) {
        hm.hive.mark_clean();
    }
    let _ = namespace::ob_remove_object(mount_path);
    if cm.unmount(mount_path) {
        Ok(())
    } else {
        Err(())
    }
}

pub fn cm_open_key(key_native_id: u64, subkey_path: &str) -> Result<u64, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    let found = hm.hive.open_key_by_path(cell_idx, subkey_path).ok_or(())?;
    Ok(encode_cell(hive_idx, found))
}

pub fn cm_create_key(key_native_id: u64, name: &str) -> Result<u64, CmError> {
    let token = crate::security::current_token();
    cm_create_key_as(&token, key_native_id, name)
}

pub(crate) fn cm_create_key_as(
    token: &Token,
    key_native_id: u64,
    name: &str,
) -> Result<u64, CmError> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    let hm = cm.hives.get_mut(hive_idx as usize).ok_or(CmError::NotFound)?;
    let path = security::new_key_path(&hm.mount_path, &hm.hive, cell_idx, name)
        .ok_or(CmError::NotFound)?;
    if !security::authorize_token(token, &path, ACCESS_WRITE) {
        drop(cm);
        security::deny(&path, ACCESS_WRITE);
        return Err(CmError::Denied);
    }
    let new_idx = hm
        .hive
        .create_key(cell_idx, name)
        .ok_or(CmError::AlreadyExists)?;
    Ok(encode_cell(hive_idx, new_idx))
}

pub fn cm_delete_key(key_native_id: u64) -> Result<(), CmError> {
    let token = crate::security::current_token();
    cm_delete_key_as(&token, key_native_id)
}

pub(crate) fn cm_delete_key_as(token: &Token, key_native_id: u64) -> Result<(), CmError> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    let path = {
        let hm = cm.hives.get(hive_idx as usize).ok_or(CmError::NotFound)?;
        security::full_path(&hm.mount_path, &hm.hive, cell_idx).ok_or(CmError::NotFound)?
    };
    if !security::authorize_delete_token(token, &path) {
        drop(cm);
        security::deny(&path, ACCESS_DELETE);
        return Err(CmError::Denied);
    }
    let hm = cm.hives.get_mut(hive_idx as usize).ok_or(CmError::NotFound)?;
    hm.hive.delete_key(cell_idx);
    Ok(())
}

pub fn cm_enum_key(key_native_id: u64, index: u32) -> Result<String, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    hm.hive.enum_key(cell_idx, index).ok_or(())
}

pub fn cm_set_value(
    key_native_id: u64,
    name: &str,
    value_type: u32,
    data: &[u8],
) -> Result<(), CmError> {
    let token = crate::security::current_token();
    cm_set_value_as(&token, key_native_id, name, value_type, data)
}

pub(crate) fn cm_set_value_as(
    token: &Token,
    key_native_id: u64,
    name: &str,
    value_type: u32,
    data: &[u8],
) -> Result<(), CmError> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    let path = {
        let hm = cm.hives.get(hive_idx as usize).ok_or(CmError::NotFound)?;
        security::full_path(&hm.mount_path, &hm.hive, cell_idx).ok_or(CmError::NotFound)?
    };
    if !security::authorize_token(token, &path, ACCESS_WRITE) {
        drop(cm);
        security::deny(&path, ACCESS_WRITE);
        return Err(CmError::Denied);
    }
    let hm = cm.hives.get_mut(hive_idx as usize).ok_or(CmError::NotFound)?;
    hm.hive
        .set_value(cell_idx, name, value_type, data)
        .ok_or(CmError::NoMemory)
}

pub fn cm_delete_value(key_native_id: u64, name: &str) -> Result<(), CmError> {
    let token = crate::security::current_token();
    cm_delete_value_as(&token, key_native_id, name)
}

pub(crate) fn cm_delete_value_as(
    token: &Token,
    key_native_id: u64,
    name: &str,
) -> Result<(), CmError> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    let path = {
        let hm = cm.hives.get(hive_idx as usize).ok_or(CmError::NotFound)?;
        security::full_path(&hm.mount_path, &hm.hive, cell_idx).ok_or(CmError::NotFound)?
    };
    if !security::authorize_token(token, &path, ACCESS_WRITE) {
        drop(cm);
        security::deny(&path, ACCESS_WRITE);
        return Err(CmError::Denied);
    }
    let hm = cm.hives.get_mut(hive_idx as usize).ok_or(CmError::NotFound)?;
    if hm.hive.delete_value(cell_idx, name) {
        Ok(())
    } else {
        Err(CmError::NotFound)
    }
}

pub fn cm_query_value(key_native_id: u64, name: &str) -> Result<ValueCell, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    hm.hive.query_value(cell_idx, name).ok_or(())
}

pub fn cm_enum_value(key_native_id: u64, index: u32) -> Result<String, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];
    hm.hive.enum_value(cell_idx, index).ok_or(())
}

pub fn cm_flush_key(key_native_id: u64) -> Result<(), ()> {
    let (hive_idx, _cell_idx) = decode_cell(key_native_id);

    let hive_snapshot = {
        let cm = CM_MANAGER.lock();
        if (hive_idx as usize) >= cm.hives.len() {
            return Err(());
        }
        if !cm.hives[hive_idx as usize].hive.is_dirty() {
            return Ok(());
        }
        let hm = &cm.hives[hive_idx as usize];
        hm.hive.clone()
    };

    flush_hive_to_vfs(&hive_snapshot)?;

    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) < cm.hives.len() {
        cm.hives[hive_idx as usize].hive.mark_clean();
    }
    Ok(())
}

pub fn cm_flush_all_hives() {
    let snapshots: Vec<(usize, Hive)> = {
        let cm = CM_MANAGER.lock();
        cm.hives.iter()
            .filter(|hm| hm.hive.is_dirty())
            .enumerate()
            .map(|(i, hm)| (i, hm.hive.clone()))
            .collect()
    };

    if snapshots.is_empty() {
        return;
    }

    kinfo!(LogSubsys::Cm, "Flushing {} dirty hive(s) to disk...", snapshots.len());

    for (hive_idx, hive) in &snapshots {
        if flush_hive_to_vfs(hive).is_ok() {
            let mut cm = CM_MANAGER.lock();
            if (*hive_idx as usize) < cm.hives.len() {
                cm.hives[*hive_idx as usize].hive.mark_clean();
            }
        } else {
            kwarn!(LogSubsys::Cm, "Failed to flush hive '{}'", hive.name);
        }
    }
}
