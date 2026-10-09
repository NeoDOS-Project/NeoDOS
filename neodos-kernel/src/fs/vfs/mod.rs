#![allow(dead_code)]

pub mod io;
pub mod mount;
pub mod partition;

use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    NotADirectory,
    NotAFile,
    AlreadyExists,
    IOError,
    InvalidPath,
    PermissionDenied,
    NotImplemented,
    MountTableFull,
    AlreadyMounted,
    NotMounted,
    DirectoryNotEmpty,
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<()> for VfsError {
    fn from(_: ()) -> Self {
        VfsError::IOError
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VfsNode {
    pub inode: u32,
    pub mode: u16,
    pub size: u32,
}

pub const MODE_DIR: u16 = 0x40;
pub const MODE_FILE: u16 = 0x80;

pub struct DirEntry {
    pub name: String,
    pub node: VfsNode,
}

pub trait FileSystem: Send {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError>;
    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError>;
    fn remove_file(&mut self, _dir_inode: u32, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }
    fn remove_dir(&mut self, _dir_inode: u32, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }
    fn rename(&mut self, _dir_inode: u32, _old_name: &str, _new_name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }
    fn volume_label(&self) -> Result<String, VfsError> {
        Err(VfsError::NotImplemented)
    }
    fn set_volume_label(&mut self, _label: &str) -> Result<(), VfsError> {
        Err(VfsError::PermissionDenied)
    }
    fn fs_type(&self) -> &'static str {
        "Unknown"
    }
    fn total_sectors(&self) -> u64 {
        0
    }
    fn fsck(&mut self, _repair: bool, _deep: bool, _stats: &mut crate::fs::fsck::FsckStatsRaw) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn snapshot_create(&mut self) -> Result<u64, VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn snapshot_restore(&mut self, _id: u64) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn snapshot_list(&mut self, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn snapshot_delete(&mut self, _id: u64) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }

    /// Copiar un fichero tal como estaba en el snapshot `id` a `dst` (árbol
    /// actual). Devuelve los bytes copiados.
    fn snapshot_extract(&mut self, _id: u64, _src: &str, _dst: &str) -> Result<u64, VfsError> {
        Err(VfsError::NotImplemented)
    }

    fn snapshot_purge(&mut self) -> Result<(), VfsError> {
        Err(VfsError::NotImplemented)
    }
}

const MAX_SUBDIR_MOUNTS: usize = 8;

/// Maximum number of path components handled without heap allocation.
/// Path resolution runs while the VFS spinlock is held, so it must not
/// allocate (allocation under the lock is a documented deadlock risk and
/// lengthens the global critical section). Paths deeper than this are rejected
/// with `InvalidPath`; NeoDOS paths are short (Windows-style MAX_PATH).
const MAX_PATH_COMPONENTS: usize = 32;

/// Split a volume-relative path (`\A\B` or `A\B`) into components on the stack.
fn split_components(path: &str) -> Result<([&str; MAX_PATH_COMPONENTS], usize), VfsError> {
    let mut arr: [&str; MAX_PATH_COMPONENTS] = [""; MAX_PATH_COMPONENTS];
    let mut n = 0usize;
    for c in path.split(['\\', '/']) {
        if n >= MAX_PATH_COMPONENTS {
            return Err(VfsError::InvalidPath);
        }
        arr[n] = c;
        n += 1;
    }
    Ok((arr, n))
}

#[derive(Debug, Clone, Copy)]
struct Mount {
    parent_drive: usize,
    parent_inode: u32,
    mounted_drive: usize,
}

pub struct Vfs {
    pub drives: [Option<Box<dyn FileSystem>>; 26],
    mounts: [Option<Mount>; MAX_SUBDIR_MOUNTS],
    mount_count: usize,
}

impl Vfs {
    fn find_mount(mounts: &[Option<Mount>], mount_count: usize, drive_idx: usize, inode: u32) -> Option<usize> {
        for m in mounts.iter().take(mount_count).flatten() {
            if m.parent_drive == drive_idx && m.parent_inode == inode {
                return Some(m.mounted_drive);
            }
        }
        None
    }

    pub const fn new() -> Self {
        const NONE_DRIVE: Option<Box<dyn FileSystem>> = None;
        const NONE_MOUNT: Option<Mount> = None;
        Vfs {
            drives: [NONE_DRIVE; 26],
            mounts: [NONE_MOUNT; MAX_SUBDIR_MOUNTS],
            mount_count: 0,
        }
    }

    pub fn drive_index(letter: char) -> Option<usize> {
        let l = letter.to_ascii_uppercase();
        if l.is_ascii_uppercase() {
            Some((l as u8 - b'A') as usize)
        } else {
            None
        }
    }

    pub fn mount(&mut self, letter: char, fs: Box<dyn FileSystem>) -> Result<(), VfsError> {
        let idx = Self::drive_index(letter).ok_or(VfsError::InvalidPath)?;
        self.drives[idx] = Some(fs);
        Ok(())
    }

    pub fn unmount(&mut self, letter: char) -> Result<(), VfsError> {
        let idx = Self::drive_index(letter).ok_or(VfsError::InvalidPath)?;
        self.drives[idx] = None;
        let mut kept = 0;
        for i in 0..self.mount_count {
            if let Some(ref m) = self.mounts[i] {
                if m.mounted_drive == idx || m.parent_drive == idx {
                    continue;
                }
                self.mounts[kept] = self.mounts[i];
                kept += 1;
            }
        }
        self.mount_count = kept;
        Ok(())
    }

    fn walk_components(&mut self, mut drive_idx: usize, mut inode: u32, components: &[&str]) -> Result<(usize, u32), VfsError> {
        // Fixed-size stack: path resolution must not allocate under the VFS
        // lock. Depth is bounded by MAX_PATH_COMPONENTS (+1 root, +1 per mount
        // crossing); we check before every push.
        let mut stack: [(usize, u32); MAX_PATH_COMPONENTS + 2] = [(0, 0); MAX_PATH_COMPONENTS + 2];
        let mut depth = 0usize;

        if let Some(mounted) = Self::find_mount(&self.mounts, self.mount_count, drive_idx, inode) {
            drive_idx = mounted;
        }
        stack[depth] = (drive_idx, inode);
        depth += 1;

        for &comp in components {
            match comp {
                "" | "." => continue,
                ".." => {
                    if depth > 1 {
                        depth -= 1;
                        let (d, i) = stack[depth - 1];
                        drive_idx = d;
                        inode = i;
                    }
                    continue;
                }
                _ => {}
            }

            let node = {
                let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
                fs.lookup(inode, comp)?
            };
            inode = node.inode;
            if depth >= stack.len() { return Err(VfsError::InvalidPath); }
            stack[depth] = (drive_idx, inode);
            depth += 1;

            if let Some(mounted) = Self::find_mount(&self.mounts, self.mount_count, drive_idx, inode) {
                drive_idx = mounted;
                inode = 0;
                if depth >= stack.len() { return Err(VfsError::InvalidPath); }
                stack[depth] = (drive_idx, inode);
                depth += 1;
            }
        }

        Ok((drive_idx, inode))
    }

    pub fn resolve_path(&mut self, path: &str) -> Result<(usize, VfsNode), VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;

        let (components, n) = split_components(rest)?;

        let (drive_idx, inode) = self.walk_components(drive_idx, 0, &components[..n])?;

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        let node = fs.stat(inode)?;
        Ok((drive_idx, node))
    }

    pub fn split_drive(path: &str) -> Result<(char, &str), VfsError> {
        if path.len() >= 2 && path.as_bytes()[1] == b':' {
            let drive = path.as_bytes()[0] as char;
            Ok((drive, &path[2..]))
        } else {
            Err(VfsError::InvalidPath)
        }
    }

    pub fn read(&mut self, drive_idx: usize, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.read(inode, offset, buf)
    }

    pub fn stat(&mut self, drive_idx: usize, inode: u32) -> Result<VfsNode, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.stat(inode)
    }

    pub fn write(&mut self, drive_idx: usize, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.write(inode, offset, buf)
    }

    pub fn readdir(&mut self, drive_idx: usize, inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.readdir(inode, index)
    }

    pub fn volume_label(&mut self, drive: char) -> Result<String, VfsError> {
        let idx = Self::drive_index(drive).ok_or(VfsError::InvalidPath)?;
        let fs = self.drives[idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.volume_label()
    }

    pub fn set_volume_label(&mut self, drive: char, label: &str) -> Result<(), VfsError> {
        let idx = Self::drive_index(drive).ok_or(VfsError::InvalidPath)?;
        let fs = self.drives[idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.set_volume_label(label)
    }

    pub fn mount_at_path(&mut self, path: &str, mounted_drive: char) -> Result<(), VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;
        let mounted_idx = Self::drive_index(mounted_drive).ok_or(VfsError::InvalidPath)?;

        let (components, n) = split_components(rest)?;

        let (resolved_drive, resolved_inode) = self.walk_components(drive_idx, 0, &components[..n])?;

        {
            let fs = self.drives[resolved_drive].as_mut().ok_or(VfsError::NotFound)?;
            let node = fs.stat(resolved_inode)?;
            if node.mode & MODE_DIR == 0 {
                return Err(VfsError::NotADirectory);
            }
        }

        if self.drives[mounted_idx].is_none() {
            return Err(VfsError::NotFound);
        }

        for i in 0..self.mount_count {
            if let Some(ref m) = self.mounts[i] {
                if m.parent_drive == resolved_drive && m.parent_inode == resolved_inode {
                    return Err(VfsError::AlreadyMounted);
                }
            }
        }

        if self.mount_count >= MAX_SUBDIR_MOUNTS {
            return Err(VfsError::MountTableFull);
        }

        self.mounts[self.mount_count] = Some(Mount {
            parent_drive: resolved_drive,
            parent_inode: resolved_inode,
            mounted_drive: mounted_idx,
        });
        self.mount_count += 1;
        Ok(())
    }

    pub fn unmount_path(&mut self, path: &str) -> Result<(), VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;

        let (components, n) = split_components(rest)?;

        let (resolved_drive, resolved_inode) = self.walk_components(drive_idx, 0, &components[..n])?;

        for i in 0..self.mount_count {
            if let Some(ref m) = self.mounts[i] {
                if m.parent_drive == resolved_drive && m.parent_inode == resolved_inode {
                    self.mounts[i] = None;
                    for j in i + 1..self.mount_count {
                        self.mounts[j - 1] = self.mounts[j];
                    }
                    self.mount_count -= 1;
                    return Ok(());
                }
            }
        }
        Err(VfsError::NotMounted)
    }

    fn split_parent_leaf(rest: &str) -> (&str, &str) {
        match rest.rfind(['\\', '/']) {
            Some(idx) => (&rest[..idx], &rest[idx + 1..]),
            None => ("", rest),
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<VfsNode, VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;

        let (parent_path, leaf) = Self::split_parent_leaf(rest);

        let (parent_components, n) = split_components(parent_path)?;

        let (drive_idx, parent_inode) = self.walk_components(drive_idx, 0, &parent_components[..n])?;

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.mkdir(parent_inode, leaf)
    }

    pub fn create(&mut self, path: &str) -> Result<VfsNode, VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;

        let (parent_path, leaf) = Self::split_parent_leaf(rest);

        let (parent_components, n) = split_components(parent_path)?;

        let (drive_idx, parent_inode) = self.walk_components(drive_idx, 0, &parent_components[..n])?;

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.create(parent_inode, leaf)
    }

    pub fn remove_file(&mut self, path: &str) -> Result<(), VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;

        let (parent_path, leaf) = Self::split_parent_leaf(rest);

        let (parent_components, n) = split_components(parent_path)?;

        let (drive_idx, parent_inode) = self.walk_components(drive_idx, 0, &parent_components[..n])?;

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.remove_file(parent_inode, leaf)
    }

    pub fn remove_dir(&mut self, path: &str) -> Result<(), VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;

        let (parent_path, leaf) = Self::split_parent_leaf(rest);

        let (parent_components, n) = split_components(parent_path)?;

        let (drive_idx, parent_inode) = self.walk_components(drive_idx, 0, &parent_components[..n])?;

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.remove_dir(parent_inode, leaf)
    }

    pub fn rename(&mut self, path: &str, new_name: &str) -> Result<(), VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;

        let (parent_path, leaf) = Self::split_parent_leaf(rest);

        let (parent_components, n) = split_components(parent_path)?;

        let (drive_idx, parent_inode) = self.walk_components(drive_idx, 0, &parent_components[..n])?;

        // Extract just the leaf name from new_name (user may pass full path)
        let new_leaf = new_name.rsplit(['\\', '/']).next().unwrap_or(new_name);

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.rename(parent_inode, leaf, new_leaf)
    }

    pub fn snapshot_create(&mut self, drive_idx: usize) -> Result<u64, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.snapshot_create()
    }

    pub fn snapshot_restore(&mut self, drive_idx: usize, id: u64) -> Result<(), VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.snapshot_restore(id)
    }

    pub fn snapshot_list(&mut self, drive_idx: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.snapshot_list(buf)
    }

    pub fn snapshot_delete(&mut self, drive_idx: usize, id: u64) -> Result<(), VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.snapshot_delete(id)
    }

    pub fn snapshot_extract(&mut self, drive_idx: usize, id: u64, src: &str, dst: &str) -> Result<u64, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.snapshot_extract(id, src, dst)
    }

    pub fn snapshot_purge(&mut self, drive_idx: usize) -> Result<(), VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.snapshot_purge()
    }
}
