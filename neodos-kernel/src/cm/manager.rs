use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

use crate::object::ObId;
use self::super::hive::Hive;

pub const MAX_HIVES: usize = 8;

pub struct HiveMount {
    pub name: String,
    pub mount_path: String,
    pub hive: Hive,
    pub ob_id: ObId,
}

impl HiveMount {
    pub fn relative_cell(&self, _encoded_cell: u32) -> Option<u32> {
        Some(_encoded_cell)
    }
}

pub struct CmManager {
    pub(crate) hives: Vec<HiveMount>,
}

impl CmManager {
    pub fn new() -> Self {
        CmManager { hives: Vec::new() }
    }

    pub fn mount(&mut self, name: &str, mount_path: &str, hive: Hive, ob_id: ObId) -> Option<usize> {
        if self.hives.len() >= MAX_HIVES { return None; }
        let idx = self.hives.len();
        self.hives.push(HiveMount {
            name: name.to_string(),
            mount_path: mount_path.to_string(),
            hive,
            ob_id,
        });
        Some(idx)
    }

    pub fn unmount(&mut self, mount_path: &str) -> bool {
        if let Some(pos) = self.hives.iter().position(|h| h.mount_path == mount_path) {
            self.hives.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn find_hive_by_key(&self, key_cell: u32) -> Option<(&HiveMount, u32)> {
        for hm in &self.hives {
            if let Some(relative) = hm.relative_cell(key_cell) {
                return Some((hm, relative));
            }
        }
        None
    }

    pub fn find_hive_by_key_mut(&mut self, key_cell: u32) -> Option<(&mut HiveMount, u32)> {
        for hm in &mut self.hives {
            if let Some(relative) = hm.relative_cell(key_cell) {
                return Some((hm, relative));
            }
        }
        None
    }

    pub fn find_hive_by_path(&self, mount_path: &str) -> Option<&HiveMount> {
        self.hives.iter().find(|h| h.mount_path == mount_path)
    }

    pub fn find_hive_by_path_mut(&mut self, mount_path: &str) -> Option<&mut HiveMount> {
        self.hives.iter_mut().find(|h| h.mount_path == mount_path)
    }

    pub fn hive_count(&self) -> usize {
        self.hives.len()
    }
}

lazy_static! {
    pub static ref CM_MANAGER: Mutex<CmManager> = Mutex::new(CmManager::new());
}

pub fn encode_cell(hive_idx: u32, cell_idx: u32) -> u64 {
    ((hive_idx as u64) << 24) | (cell_idx as u64)
}

pub fn decode_cell(native_id: u64) -> (u32, u32) {
    let hive_idx = (native_id >> 24) as u32;
    let cell_idx = (native_id & 0x00FFFFFF) as u32;
    (hive_idx, cell_idx)
}

pub fn find_by_native(native_id: u64) -> Option<(usize, u32)> {
    let (hive_idx, cell_idx) = decode_cell(native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) < cm.hives.len() {
        Some((hive_idx as usize, cell_idx))
    } else {
        None
    }
}

pub fn find_by_native_mut(native_id: u64) -> Option<(usize, u32)> {
    let (hive_idx, cell_idx) = decode_cell(native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) < cm.hives.len() {
        drop(cm);
        Some((hive_idx as usize, cell_idx))
    } else {
        None
    }
}
