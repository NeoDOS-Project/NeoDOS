//! Object enumeration — extracted from mod.rs (ob_enum_directory)
use alloc::vec::Vec;
use crate::object::types::{ObError, ObEnumEntry, ObType};

/// Enumerate objects in a namespace directory by path.
/// Returns a list of ObEnumEntry values.
/// VFS-3.1: `\Global\FileSystem\` paths delegate to VFS, not namespace.
pub fn ob_enum_directory(path: &str) -> Result<alloc::vec::Vec<ObEnumEntry>, ObError> {
    let normalized = crate::object::namespace::normalize_path(path);

    // VFS-3.1: \Global\FileSystem\ paths → VFS enumeration
    if normalized == "\\Global\\FileSystem" || normalized.starts_with("\\Global\\FileSystem\\") {
        let vfs_rest = normalized.strip_prefix("\\Global\\FileSystem").unwrap_or("");
        let vfs_path = vfs_rest.trim_start_matches('\\');

        if vfs_path.is_empty() {
            // Enumerate available drives
            let mut entries = alloc::vec::Vec::new();
            crate::globals::with_vfs(|vfs| {
                for i in 0..26usize {
                    if vfs.drives[i].is_some() {
                        let letter = (b'A' + i as u8) as char;
                        let mut name = [0u8; 32];
                        let name_str = alloc::format!("{}:", letter);
                        let bytes = name_str.as_bytes();
                        let len = bytes.len().min(31);
                        name[..len].copy_from_slice(&bytes[..len]);
                        entries.push(ObEnumEntry {
                            id: 0,
                            obj_type: ObType::Directory as u32,
                            name,
                            mode: 0,
                            _pad: [0u8; 2],
                            size: 0,
                        });
                    }
                }
            });
            return Ok(entries);
        }

        // Subpath with drive letter → use VFS readdir
        if vfs_path.contains(':') {
            let (drive_idx, node) = crate::globals::with_vfs(|vfs| {
                vfs.resolve_path(vfs_path)
            }).map_err(|_| ObError::NotFound)?;

            if (node.mode & crate::fs::vfs::MODE_DIR) == 0 {
                return Err(ObError::InvalidParam);
            }

            let dir_inode = node.inode;
            let mut entries = alloc::vec::Vec::new();
            let mut idx = 0usize;
            loop {
                let result = crate::globals::with_vfs(|vfs| {
                    vfs.readdir(drive_idx, dir_inode, idx)
                });
                match result {
                    Ok(Some(dir_entry)) => {
                        let mut name = [0u8; 32];
                        let bytes = dir_entry.name.as_bytes();
                        let len = bytes.len().min(31);
                        name[..len].copy_from_slice(&bytes[..len]);
                        let obj_type = if (dir_entry.node.mode & crate::fs::vfs::MODE_DIR) != 0 {
                            ObType::Directory
                        } else {
                            ObType::Filesystem
                        };
                        entries.push(ObEnumEntry {
                            id: dir_entry.node.inode as u64,
                            obj_type: obj_type as u32,
                            name,
                            mode: dir_entry.node.mode,
                            _pad: [0u8; 2],
                            size: dir_entry.node.size,
                        });
                        idx += 1;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            return Ok(entries);
        }
    }

    // Default: namespace enumeration
    let entries = crate::object::namespace::ob_enumerate_namespace(&normalized)
        .map_err(|_| ObError::NotFound)?;
    Ok(entries.iter().map(|e| {
        let mut name = [0u8; 32];
        let len = e.name.iter().position(|&b| b == 0).unwrap_or(32).min(31);
        name[..len].copy_from_slice(&e.name[..len]);
        name[len] = 0;
        ObEnumEntry {
            id: e.obj_id,
            obj_type: e.obj_type,
            name,
            mode: 0,
            _pad: [0u8; 2],
            size: 0,
        }
    }).collect())
}

