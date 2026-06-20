use alloc::vec::Vec;
use alloc::string::String;
use crate::kobj::{self, KObjId, KObjType};
use crate::kobj::namespace as ns;
use spin::Mutex;
use lazy_static::lazy_static;

const MAX_MOUNTS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub enum FilesystemType {
    NeoDosFs,
    Fat32,
    Iso9660,
}

impl FilesystemType {
    pub fn to_str(self) -> &'static str {
        match self {
            FilesystemType::NeoDosFs => "NEODOSFS",
            FilesystemType::Fat32 => "FAT32",
            FilesystemType::Iso9660 => "ISO9660",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MountPoint {
    pub name: String,
    pub device_path: String,
    pub volume_path: String,
    pub fs_type: FilesystemType,
    pub obj_id: Option<KObjId>,
}

pub struct MountManager {
    mounts: Vec<MountPoint>,
}

impl MountManager {
    pub const fn new() -> Self {
        MountManager {
            mounts: Vec::new(),
        }
    }

    pub fn mount(&mut self, device_path: &str, drive_letter: char, fs_type: FilesystemType) -> Result<usize, &'static str> {
        if self.mounts.len() >= MAX_MOUNTS {
            return Err("MountManager: max mounts reached");
        }
        let mount_id = self.mounts.len();
        let letter_upper = drive_letter.to_ascii_uppercase();
        let name = alloc::format!("{}:", letter_upper);
        let volume_path = alloc::format!("\\Device\\{}:", letter_upper);
        let kobj_id = kobj::kobj_register(KObjType::MountPoint, &name, mount_id as u64)
            .map_err(|_| "MountManager: kobj_register failed")?;
        let mount = MountPoint {
            name: name.clone(),
            device_path: alloc::string::String::from(device_path),
            volume_path: volume_path.clone(),
            fs_type,
            obj_id: Some(kobj_id),
        };
        let _ = ns::ob_create_directory("\\FileSystem\\Mounts");
        let _ = ns::ob_insert_object(&volume_path, kobj_id);
        let symlink_path = alloc::format!("\\DosDevices\\{}:", letter_upper);
        let _ = ns::ob_create_directory("\\DosDevices");
        let _ = ns::ob_insert_symlink(&symlink_path, &volume_path);
        self.mounts.push(mount);
        Ok(mount_id)
    }

    pub fn unmount(&mut self, index: usize) -> bool {
        if index >= self.mounts.len() {
            return false;
        }
        if let Some(kobj_id) = self.mounts[index].obj_id {
            let _ = kobj::kobj_unregister(kobj_id);
        }
        self.mounts.remove(index);
        true
    }

    pub fn get(&self, index: usize) -> Option<&MountPoint> {
        self.mounts.get(index)
    }

    pub fn find_by_volume(&self, volume_name: &str) -> Option<&MountPoint> {
        self.mounts.iter().find(|m| m.name == volume_name)
    }

    pub fn count(&self) -> usize {
        self.mounts.len()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, MountPoint> {
        self.mounts.iter()
    }
}

lazy_static! {
    pub static ref MOUNT_MANAGER: Mutex<MountManager> = Mutex::new(MountManager::new());
}

pub fn vfs_mount(device_path: &str, drive_letter: char, fs_type: FilesystemType) -> Result<usize, &'static str> {
    MOUNT_MANAGER.lock().mount(device_path, drive_letter, fs_type)
}

pub fn vfs_unmount(index: usize) -> bool {
    MOUNT_MANAGER.lock().unmount(index)
}

pub fn vfs_get_mount(index: usize) -> Option<MountPoint> {
    MOUNT_MANAGER.lock().get(index).cloned()
}

pub fn vfs_path_to_mount(path: &str) -> Option<(usize, MountPoint)> {
    let mgr = MOUNT_MANAGER.lock();
    for (i, m) in mgr.mounts.iter().enumerate() {
        if path.starts_with(&m.volume_path) || path.starts_with(&m.name) {
            return Some((i, m.clone()));
        }
    }
    None
}

pub fn register_mount_tests() {
    crate::test_case!("vfs_mount_create_device_object", {
        let mut mgr = MountManager::new();
        let r = mgr.mount("\\Device\\Harddisk0\\Partition1", 'C', FilesystemType::NeoDosFs);
        crate::test_true!(r.is_ok());
        crate::test_eq!(mgr.count(), 1);
        let _ = mgr.unmount(0);
    });

    crate::test_case!("vfs_mount_dosdevices_symlink", {
        let mut mgr = MountManager::new();
        mgr.mount("\\Device\\Harddisk0\\Partition1", 'C', FilesystemType::NeoDosFs).unwrap();
        let mount = mgr.get(0).unwrap();
        crate::test_eq!(mount.fs_type.to_str(), "NEODOSFS");
        crate::test_true!(!mount.volume_path.is_empty());
        let _ = mgr.unmount(0);
    });

    crate::test_case!("vfs_mount_multiple_filesystems", {
        let mut mgr = MountManager::new();
        mgr.mount("\\Device\\Harddisk0\\Partition1", 'C', FilesystemType::NeoDosFs).unwrap();
        mgr.mount("\\Device\\Harddisk0\\Partition0", 'A', FilesystemType::Fat32).unwrap();
        crate::test_eq!(mgr.count(), 2);
        let fs0 = mgr.find_by_volume("C:").unwrap();
        crate::test_eq!(fs0.fs_type.to_str(), "NEODOSFS");
        let fs1 = mgr.find_by_volume("A:").unwrap();
        crate::test_eq!(fs1.fs_type.to_str(), "FAT32");
        let _ = mgr.unmount(0);
        let _ = mgr.unmount(0);
    });

    crate::test_case!("vfs_mount_unmount", {
        let mut mgr = MountManager::new();
        mgr.mount("\\Device\\Test", 'X', FilesystemType::Iso9660).unwrap();
        crate::test_eq!(mgr.count(), 1);
        crate::test_true!(mgr.unmount(0));
        crate::test_eq!(mgr.count(), 0);
    });

    crate::test_case!("vfs_path_to_mount", {
        let mut mgr = MountManager::new();
        mgr.mount("\\Device\\Harddisk0\\Partition1", 'C', FilesystemType::NeoDosFs).unwrap();
        let r = mgr.find_by_volume("C:");
        crate::test_true!(r.is_some());
        let _ = mgr.unmount(0);
    });
}