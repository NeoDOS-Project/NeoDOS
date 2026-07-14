use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::string::String;
use crate::object::{self, ObId, ObType};
use crate::object::namespace as ns;
use crate::fs::vfs::FileSystem;
use spin::Mutex;
use lazy_static::lazy_static;

const MAX_MOUNTS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub enum FilesystemType {
    NeoDosFs,
    Fat32,
}

impl FilesystemType {
    pub fn to_str(self) -> &'static str {
        match self {
            FilesystemType::NeoDosFs => "NEODOSFS",
            FilesystemType::Fat32 => "FAT32",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MountPoint {
    pub name: String,
    pub device_path: String,
    pub volume_path: String,
    pub fs_type: FilesystemType,
    pub obj_id: Option<ObId>,
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
        let obj_id = object::ob_create_object(ObType::MountPoint, &name, mount_id as u64, 0, None)
            .map_err(|_| "MountManager: ob_create_object failed")?;
        let mount = MountPoint {
            name: name.clone(),
            device_path: alloc::string::String::from(device_path),
            volume_path: volume_path.clone(),
            fs_type,
            obj_id: Some(obj_id),
        };
        let _ = ns::ob_create_directory("\\FileSystem\\Mounts");
        let _ = ns::ob_insert_object(&volume_path, obj_id);
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
        // Remove DosDevices symlink
        if let Some(ref m) = self.mounts.get(index) {
            let symlink_path = alloc::format!("\\DosDevices\\{}", m.name);
            let _ = ns::ob_remove_object(&symlink_path);
            // Remove MountPoint from \FileSystem\Mounts
            let _ = ns::ob_remove_object(&m.volume_path);
            // Destroy ObObject
            if let Some(obj_id) = m.obj_id {
                let _ = object::ob_destroy_object(obj_id);
            }
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

    pub fn find_by_letter(&self, letter: char) -> Option<&MountPoint> {
        let name = alloc::format!("{}:", letter.to_ascii_uppercase());
        self.mounts.iter().find(|m| m.name == name)
    }

    pub fn unmount_by_letter(&mut self, letter: char) -> bool {
        let name = alloc::format!("{}:", letter.to_ascii_uppercase());
        if let Some(pos) = self.mounts.iter().position(|m| m.name == name) {
            self.unmount(pos)
        } else {
            false
        }
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

/// Unified mount: registers filesystem on the VFS drive letter AND
/// creates Ob MountPoint + \DosDevices symlink.
/// Replaces the dual-call pattern `vfs.mount('C', fs)` + `vfs_mount(...)`.
pub fn vfs_mount_filesystem(
    device_path: &str,
    drive_letter: char,
    fs: Box<dyn FileSystem>,
    fs_type: FilesystemType,
) -> Result<usize, &'static str> {
    let letter_upper = drive_letter.to_ascii_uppercase();
    // Step 1: Register in Vfs.drives[]
    let mut vfs = crate::globals::VFS.lock();
    if vfs.drives[crate::fs::vfs::Vfs::drive_index(letter_upper).ok_or("invalid drive")?].is_some() {
        return Err("drive already mounted");
    }
    vfs.mount(letter_upper, fs).map_err(|_| "Vfs::mount failed")?;
    drop(vfs);

    // Step 2: Create Ob MountPoint + namespace entries
    let mut mgr = MOUNT_MANAGER.lock();
    if mgr.find_by_letter(letter_upper).is_some() {
        // Rollback VFS mount
        let _ = crate::globals::VFS.lock().unmount(letter_upper);
        return Err("MountManager: drive already mounted");
    }
    let result = mgr.mount(device_path, letter_upper, fs_type);
    if result.is_err() {
        let _ = crate::globals::VFS.lock().unmount(letter_upper);
    }
    result
}

/// Unified unmount: removes from Vfs.drives[] AND cleans up MountManager.
pub fn vfs_unmount_filesystem(drive_letter: char) -> Result<(), &'static str> {
    let letter_upper = drive_letter.to_ascii_uppercase();
    // Step 1: Remove from Vfs.drives[]
    let mut vfs = crate::globals::VFS.lock();
    let idx = crate::fs::vfs::Vfs::drive_index(letter_upper).ok_or("invalid drive")?;
    if vfs.drives[idx].is_none() {
        return Err("drive not mounted in VFS");
    }
    vfs.unmount(letter_upper).map_err(|_| "Vfs::unmount failed")?;
    drop(vfs);

    // Step 2: Remove from MountManager (Ob MountPoint + DosDevices symlink)
    let mut mgr = MOUNT_MANAGER.lock();
    if !mgr.unmount_by_letter(letter_upper) {
        return Err("drive not found in MountManager");
    }
    Ok(())
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
        mgr.mount("\\Device\\Test", 'X', FilesystemType::Fat32).unwrap();
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

    // ── VFS-1.1: Unified mount tests ──

    crate::test_case!("vfs_mount_dual_sync", {
        // VFS-1.1: Verify unified mount registers in both Vfs.drives and MountManager
        let mut mgr = MountManager::new();
        mgr.mount("\\Device\\TestVol", 'X', FilesystemType::NeoDosFs).unwrap();
        crate::test_eq!(mgr.count(), 1);

        let by_letter = mgr.find_by_letter('X');
        crate::test_true!(by_letter.is_some());
        crate::test_eq!(by_letter.unwrap().name, "X:");

        let by_volume = mgr.find_by_volume("X:");
        crate::test_true!(by_volume.is_some());

        let _ = mgr.unmount(0);
        crate::test_eq!(mgr.count(), 0);
        crate::test_true!(mgr.find_by_letter('X').is_none());
    });

    crate::test_case!("vfs_mount_unmount_removes_both", {
        // VFS-1.1: Verify unmount removes from both MountManager and namespace
        let mut mgr = MountManager::new();
        mgr.mount("\\Device\\TestVol", 'Y', FilesystemType::Fat32).unwrap();
        crate::test_eq!(mgr.count(), 1);

        let r = mgr.unmount_by_letter('Y');
        crate::test_true!(r);
        crate::test_eq!(mgr.count(), 0);
        crate::test_true!(mgr.find_by_letter('Y').is_none());
        crate::test_true!(mgr.find_by_volume("Y:").is_none());

        // Second unmount should fail
        let r2 = mgr.unmount_by_letter('Y');
        crate::test_false!(r2);
    });

}