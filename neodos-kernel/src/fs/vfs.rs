// src/fs/vfs.rs - Virtual File System Layer
//
// This module provides a unified interface for all filesystem operations,
// abstracting away the differences between NeoDOS FS, FAT32, etc.

#![allow(dead_code)]

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
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
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

/// The FileSystem trait defines the interface that all filesystem drivers must implement.
pub trait FileSystem: Send {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError>;
    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError>;
    fn volume_label(&self) -> Result<String, VfsError> {
        Err(VfsError::NotImplemented)
    }
    fn set_volume_label(&mut self, _label: &str) -> Result<(), VfsError> {
        Err(VfsError::PermissionDenied)
    }
}

/// The Virtual File System (VFS) manager.
pub struct Vfs {
    pub drives: [Option<alloc::boxed::Box<dyn FileSystem>>; 26],
}

impl Vfs {
    pub const fn new() -> Self {
        // We can't initialize an array of Options with Box in a const context easily
        // but we can use a placeholder if we use a different structure.
        // For now, let's use a simpler way.
        const NONE: Option<alloc::boxed::Box<dyn FileSystem>> = None;
        Vfs {
            drives: [NONE; 26],
        }
    }

    pub fn drive_index(letter: char) -> Option<usize> {
        let l = letter.to_ascii_uppercase();
        if l >= 'A' && l <= 'Z' {
            Some((l as u8 - b'A') as usize)
        } else {
            None
        }
    }

    pub fn mount(&mut self, letter: char, fs: alloc::boxed::Box<dyn FileSystem>) -> Result<(), VfsError> {
        let idx = Self::drive_index(letter).ok_or(VfsError::InvalidPath)?;
        self.drives[idx] = Some(fs);
        Ok(())
    }

    pub fn unmount(&mut self, letter: char) -> Result<(), VfsError> {
        let idx = Self::drive_index(letter).ok_or(VfsError::InvalidPath)?;
        self.drives[idx] = None;
        Ok(())
    }

    /// Resolves a full path like "C:\WINDOWS\SYSTEM.INI"
    pub fn resolve_path(&mut self, path: &str) -> Result<(usize, VfsNode), VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;
        
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        
        let mut current_node = fs.stat(0)?; // Root is usually 0
        
        let components = rest
            .split(|c| c == '\\' || c == '/')
            .filter(|s| !s.is_empty() && *s != ".");
        
        for comp in components {
            current_node = fs.lookup(current_node.inode, comp)?;
        }
        
        Ok((drive_idx, current_node))
    }

    pub fn split_drive(path: &str) -> Result<(char, &str), VfsError> {
        if path.len() >= 2 && path.as_bytes()[1] == b':' {
            let drive = path.chars().next().unwrap();
            Ok((drive, &path[2..]))
        } else {
            Err(VfsError::InvalidPath)
        }
    }

    pub fn read(&mut self, drive_idx: usize, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        fs.read(inode, offset, buf)
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

    pub fn mkdir(&mut self, path: &str) -> Result<VfsNode, VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;
        
        let (parent_path, leaf) = if let Some(idx) = rest.rfind('\\') {
            (&rest[..idx], &rest[idx+1..])
        } else if let Some(idx) = rest.rfind('/') {
            (&rest[..idx], &rest[idx+1..])
        } else {
            ("", rest)
        };

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;
        
        let mut current_inode = 0;
        if !parent_path.is_empty() {
            let components = parent_path
                .split(|c| c == '\\' || c == '/')
                .filter(|s| !s.is_empty() && *s != ".");
            for comp in components {
                current_inode = fs.lookup(current_inode, comp)?.inode;
            }
        }

        fs.mkdir(current_inode, leaf)
    }

    pub fn create(&mut self, path: &str) -> Result<VfsNode, VfsError> {
        let (drive_letter, rest) = Self::split_drive(path)?;
        let drive_idx = Self::drive_index(drive_letter).ok_or(VfsError::InvalidPath)?;
        
        let (parent_path, leaf) = if let Some(idx) = rest.rfind('\\') {
            (&rest[..idx], &rest[idx+1..])
        } else if let Some(idx) = rest.rfind('/') {
            (&rest[..idx], &rest[idx+1..])
        } else {
            ("", rest)
        };

        let fs = self.drives[drive_idx].as_mut().ok_or(VfsError::NotFound)?;

        let mut current_inode = 0;
        if !parent_path.is_empty() {
            let components = parent_path
                .split(|c| c == '\\' || c == '/')
                .filter(|s| !s.is_empty() && *s != ".");
            for comp in components {
                current_inode = fs.lookup(current_inode, comp)?.inode;
            }
        }

        fs.create(current_inode, leaf)
    }
}
