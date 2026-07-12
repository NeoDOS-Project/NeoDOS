use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::object::{self, ObType};
use crate::object::namespace;
use self::super::hive::{Hive, ValueCell};
use self::super::manager::{CM_MANAGER, encode_cell, decode_cell};
use self::super::init::flush_hive_to_vfs;

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

pub fn cm_create_key(key_native_id: u64, name: &str) -> Result<u64, ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
    let new_idx = hm.hive.create_key(cell_idx, name).ok_or(())?;
    Ok(encode_cell(hive_idx, new_idx))
}

pub fn cm_delete_key(key_native_id: u64) -> Result<(), ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
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

pub fn cm_set_value(key_native_id: u64, name: &str, value_type: u32, data: &[u8]) -> Result<(), ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
    hm.hive.set_value(cell_idx, name, value_type, data).ok_or(())
}

pub fn cm_delete_value(key_native_id: u64, name: &str) -> Result<(), ()> {
    let (hive_idx, cell_idx) = decode_cell(key_native_id);
    let mut cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &mut cm.hives[hive_idx as usize];
    if hm.hive.delete_value(cell_idx, name) { Ok(()) } else { Err(()) }
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

    crate::serial_println!("[CM] Flushing {} dirty hive(s) to disk...", snapshots.len());

    for (hive_idx, hive) in &snapshots {
        if flush_hive_to_vfs(hive).is_ok() {
            let mut cm = CM_MANAGER.lock();
            if (*hive_idx as usize) < cm.hives.len() {
                cm.hives[*hive_idx as usize].hive.mark_clean();
            }
        } else {
            crate::serial_println!("[CM] WARNING: Failed to flush hive '{}'", hive.name);
        }
    }
}
