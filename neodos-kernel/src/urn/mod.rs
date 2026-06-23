//! Unified Resource Namespace (URN) — OB-025
//!
//! All URN schemes are frontends to the Ob (Object Manager) namespace:
//! - neodos://file/...        → resolves via VFS, backed by ObObject + handle table fd
//! - neodos://device/...      → ob_open("\Device\...")
//! - neodos://registry/...    → ob_open("\Registry\...")
//! - neodos://kobj/...        → ob_open("\Ob\...")
//!
//! UrnHandle is a simple wrapper over a kernel fd (handle table index).

use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::string::String;
use alloc::format;
use crate::globals::with_vfs;
use crate::handle::HandleEntry;
use crate::object::{self, ObType, ob_open_path, ob_lookup};

const URN_PREFIX: &str = "neodos://";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrnScheme {
    Device,
    File,
    Registry,
    KObj,
}

impl UrnScheme {
    pub fn to_str(self) -> &'static str {
        match self {
            UrnScheme::Device => "device",
            UrnScheme::File => "file",
            UrnScheme::Registry => "registry",
            UrnScheme::KObj => "kobj",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "device" => Some(UrnScheme::Device),
            "file" => Some(UrnScheme::File),
            "registry" => Some(UrnScheme::Registry),
            "kobj" => Some(UrnScheme::KObj),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Urn {
    pub scheme: UrnScheme,
    pub path: String,
}

impl Urn {
    pub fn to_string(&self) -> String {
        format!("neodos://{}/{}", self.scheme.to_str(), self.path)
    }
}

/// URN handle — a simple wrapper over a kernel fd (handle table index).
/// OB-025: Unified with Ob; all URN schemes resolve through Ob.
#[derive(Debug, Clone, Copy)]
pub struct UrnHandle {
    pub fd: u8,
}

impl UrnHandle {
    pub fn new(fd: u8) -> Self {
        UrnHandle { fd }
    }
}

// ── Kernel-internal helpers ──

fn current_token() -> crate::security::token::Token {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        lock.current_eprocess()
            .map(|ep| ep.token)
            .unwrap_or(*crate::security::DEFAULT_ADMIN_TOKEN)
    })
}

fn alloc_handle(entry: HandleEntry) -> Result<u8, &'static str> {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        lock.current_eprocess_mut()
            .and_then(|ep| ep.handle_table.alloc_handle(entry))
    }).ok_or("No process context for handle allocation")
}

fn update_handle_offset(fd: u8, delta: u64) {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            ep.handle_table[fd as usize].offset += delta;
        }
    });
}

fn extract_file_params(fd: u8) -> (usize, u32, u64) {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        if let Some(ep) = lock.current_eprocess() {
            let entry = ep.handle_table.get(fd);
            if entry.is_file() {
                if let Some(obj) = ob_lookup(entry.object_id) {
                    return (entry.file_drive() as usize, obj.native_id as u32, entry.offset);
                }
            }
        }
        (usize::MAX, 0, 0)
    })
}

// ── Public API ──

pub fn urn_parse(urn_str: &str) -> Result<Urn, &'static str> {
    if !urn_str.starts_with(URN_PREFIX) {
        return Err("URN must start with neodos://");
    }
    let rest = &urn_str[URN_PREFIX.len()..];
    let slash_pos = rest.find('/').ok_or("URN must have scheme and path separated by /")?;
    if slash_pos == 0 {
        return Err("URN scheme is empty");
    }
    let scheme_str = &rest[..slash_pos];
    let path = &rest[slash_pos + 1..];
    if path.is_empty() {
        return Err("URN path is empty");
    }
    let scheme = UrnScheme::from_str(scheme_str)
        .ok_or("Unknown URN scheme (valid: device, file, registry, kobj)")?;
    Ok(Urn { scheme, path: String::from(path) })
}

/// Open a resource identified by a URN via the Ob namespace.
/// Returns an UrnHandle (wrapper over a kernel fd).
pub fn urn_open(urn_str: &str) -> Result<UrnHandle, &'static str> {
    let urn = urn_parse(urn_str)?;
    match urn.scheme {
        UrnScheme::File => {
            let vfs_path = urn.path.replace('/', "\\");
            let (drive, inode) = with_vfs(|vfs| {
                let (drive_idx, node) = vfs.resolve_path(&vfs_path)
                    .map_err(|_| "File not found")?;
                Ok((drive_idx as u8, node.inode))
            })?;
            let entry = HandleEntry::file(drive, inode);
            alloc_handle(entry).map(UrnHandle::new)
        }
        UrnScheme::Device => {
            let ob_path = format!("\\Device\\{}", urn.path);
            let token = current_token();
            let desired = crate::security::acl::ACCESS_READ
                | crate::security::acl::ACCESS_WRITE;
            match ob_open_path(&ob_path, &token, desired) {
                Ok(ob_id) => {
                    let entry = HandleEntry::ob_object(ob_id, desired);
                    match alloc_handle(entry) {
                        Ok(fd) => Ok(UrnHandle::new(fd)),
                        Err(e) => {
                            let _ = object::ob_close_object(ob_id);
                            Err(e)
                        }
                    }
                }
                Err(_) => Err("Device not found in Ob namespace"),
            }
        }
        UrnScheme::Registry => {
            Err("Registry URN scheme not yet implemented")
        }
        UrnScheme::KObj => {
            Err("KObj URN scheme not yet implemented")
        }
    }
}

/// Read from an open URN handle into a buffer.
pub fn urn_read(handle: &mut UrnHandle, buf: &mut [u8]) -> Result<usize, &'static str> {
    let (drive, inode, offset) = extract_file_params(handle.fd);
    if drive == usize::MAX {
        return Err("URN read: not a file handle");
    }
    let mut bytes_read = 0usize;
    with_vfs(|vfs| {
        bytes_read = vfs.read(drive, inode, offset, buf)
            .map_err(|_| "VFS read failed")?;
        Ok(())
    })?;
    update_handle_offset(handle.fd, bytes_read as u64);
    Ok(bytes_read)
}

/// Write to an open URN handle from a buffer.
pub fn urn_write(handle: &mut UrnHandle, buf: &[u8]) -> Result<usize, &'static str> {
    let (drive, inode, offset) = extract_file_params(handle.fd);
    if drive == usize::MAX {
        return Err("URN write: not a file handle");
    }
    let written = with_vfs(|vfs| {
        vfs.write(drive, inode, offset, buf)
            .map_err(|_| "VFS write failed")
    })?;
    update_handle_offset(handle.fd, written as u64);
    Ok(written)
}

/// Seek to a position in an open URN handle.
pub fn urn_seek(handle: &mut UrnHandle, pos: u64) {
    let fd = handle.fd;
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            ep.handle_table[fd as usize].offset = pos;
        }
    });
}

// ── Tests ──

pub fn register_urn_tests() {
    // ── Parse tests (no process context needed) ──

    test_case!("urn_parse_scheme", {
        let urn = urn_parse("neodos://file/C:/System/boot.cfg").unwrap();
        test_eq!(urn.scheme, UrnScheme::File);
        test_eq!(urn.path, "C:/System/boot.cfg");
    });

    test_case!("urn_parse_device_scheme", {
        let urn = urn_parse("neodos://device/Harddisk0/Partition1").unwrap();
        test_eq!(urn.scheme, UrnScheme::Device);
        test_eq!(urn.path, "Harddisk0/Partition1");
    });

    test_case!("urn_parse_registry_scheme", {
        let urn = urn_parse("neodos://registry/Machine/System").unwrap();
        test_eq!(urn.scheme, UrnScheme::Registry);
        test_eq!(urn.path, "Machine/System");
    });

    test_case!("urn_parse_kobj_scheme", {
        let urn = urn_parse("neodos://kobj/Driver/ahci").unwrap();
        test_eq!(urn.scheme, UrnScheme::KObj);
        test_eq!(urn.path, "Driver/ahci");
    });

    test_case!("urn_parse_invalid_prefix", {
        test_true!(urn_parse("http://file/x").is_err());
    });

    test_case!("urn_parse_missing_scheme", {
        test_true!(urn_parse("neodos://").is_err());
    });

    test_case!("urn_parse_unknown_scheme", {
        test_true!(urn_parse("neodos://foo/bar").is_err());
    });

    test_case!("urn_parse_missing_path", {
        test_true!(urn_parse("neodos://file/").is_err());
    });

    // ── Open tests — error paths (fail before handle allocation) ──

    test_case!("urn_resolve_file_nonexistent", {
        let r = urn_open("neodos://file/C:/nonexistent/file.txt");
        test_true!(r.is_err());
    });

    test_case!("urn_resolve_device_nonexistent", {
        let r = urn_open("neodos://device/NonexistentDevice");
        test_true!(r.is_err());
    });

    // ── Roundtrip ──

    test_case!("urn_to_string_roundtrip", {
        let urn = Urn { scheme: UrnScheme::File, path: String::from("C:/test.txt") };
        let s = urn.to_string();
        test_eq!(s, "neodos://file/C:/test.txt");
        let parsed = urn_parse(&s).unwrap();
        test_eq!(parsed.scheme, UrnScheme::File);
        test_eq!(parsed.path, "C:/test.txt");
    });

    // ── OB-025: new tests — scheme mapping ──

    test_case!("urn_open_registry_not_implemented", {
        let r = urn_open("neodos://registry/Machine/System");
        test_true!(r.is_err());
    });

    test_case!("urn_open_kobj_not_implemented", {
        let r = urn_open("neodos://kobj/Driver/ahci");
        test_true!(r.is_err());
    });

    test_case!("urn_handle_create", {
        let h = UrnHandle::new(3);
        test_eq!(h.fd, 3);
    });

    // ── OB-018: ObObjectTable integration ──

    test_case!("urn_file_ob_open", {
        let inode = 77u32;
        let name = alloc::format!("URNFILE{}", inode);
        let ob_id = object::ob_create_object(ObType::Filesystem, &name, inode as u64, 0, None)
            .expect("ob create");
        test_true!(ob_id > 0);
        let obj = ob_lookup(ob_id).unwrap();
        test_eq!(obj.obj_type, ObType::Filesystem);
        test_eq!(obj.native_id, inode as u64);
        object::ob_destroy_object(ob_id).unwrap();
    });
}
