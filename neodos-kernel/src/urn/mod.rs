use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::string::String;
use alloc::format;
use crate::globals::with_vfs;
use crate::kobj::namespace as ns;

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

/// A handle to an opened URN resource.
#[derive(Debug, Clone)]
pub struct UrnHandle {
    pub scheme: UrnScheme,
    pub drive: u8,
    pub inode: u32,
    pub offset: u64,
    pub device_ob_path: String,
}

impl UrnHandle {
    fn new_file(drive: u8, inode: u32) -> Self {
        UrnHandle {
            scheme: UrnScheme::File,
            drive,
            inode,
            offset: 0,
            device_ob_path: String::new(),
        }
    }

    fn new_device(path: &str) -> Self {
        UrnHandle {
            scheme: UrnScheme::Device,
            drive: 0,
            inode: 0,
            offset: 0,
            device_ob_path: String::from(path),
        }
    }
}

/// Parse a URN string into its scheme and path components.
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

/// Open a resource identified by a URN.
/// Returns an UrnHandle that can be used for read/write operations.
pub fn urn_open(urn_str: &str) -> Result<UrnHandle, &'static str> {
    let urn = urn_parse(urn_str)?;
    match urn.scheme {
        UrnScheme::File => {
            let path = &urn.path;
            // Normalize forward slashes to backslashes for VFS
            let vfs_path = path.replace('/', "\\");
            // Ensure drive letter format (e.g., "C:/..." -> "C:\...")
            let vfs_path = if vfs_path.len() >= 2 && vfs_path.as_bytes()[1] == b':' {
                vfs_path
            } else {
                return Err("File URN path must include drive letter (e.g., C:/path)");
            };
            with_vfs(|vfs| {
                let (drive_idx, node) = vfs.resolve_path(&vfs_path)
                    .map_err(|_| "File not found")?;
                Ok(UrnHandle::new_file(drive_idx as u8, node.inode))
            })
        }
        UrnScheme::Device => {
            let ob_path = format!("\\Device\\{}", urn.path);
            match ns::ob_lookup_path(&ob_path) {
                Ok(_) => Ok(UrnHandle::new_device(&ob_path)),
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
/// Returns the number of bytes read.
pub fn urn_read(handle: &mut UrnHandle, buf: &mut [u8]) -> Result<usize, &'static str> {
    match handle.scheme {
        UrnScheme::File => {
            let mut bytes_read = 0usize;
            with_vfs(|vfs| {
                bytes_read = vfs.read(handle.drive as usize, handle.inode, handle.offset, buf)
                    .map_err(|_| "VFS read failed")?;
                Ok(())
            })?;
            handle.offset += bytes_read as u64;
            Ok(bytes_read)
        }
        _ => Err("Read not supported for this URN scheme"),
    }
}

/// Write to an open URN handle from a buffer.
/// Returns the number of bytes written.
pub fn urn_write(handle: &mut UrnHandle, buf: &[u8]) -> Result<usize, &'static str> {
    match handle.scheme {
        UrnScheme::File => {
            with_vfs(|vfs| {
                let written = vfs.write(handle.drive as usize, handle.inode, handle.offset, buf)
                    .map_err(|_| "VFS write failed")?;
                handle.offset += written as u64;
                Ok(written)
            })
        }
        _ => Err("Write not supported for this URN scheme"),
    }
}

/// Seek to a position in an open URN handle.
pub fn urn_seek(handle: &mut UrnHandle, pos: u64) {
    handle.offset = pos;
}

pub fn register_urn_tests() {
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

    test_case!("urn_resolve_file_nonexistent", {
        // VFS not mounted in tests, should fail with file not found
        let r = urn_open("neodos://file/C:/nonexistent/file.txt");
        test_true!(r.is_err());
    });

    test_case!("urn_resolve_device_nonexistent", {
        // No device registered in Ob namespace in tests
        let r = urn_open("neodos://device/NonexistentDevice");
        test_true!(r.is_err());
    });

    test_case!("urn_to_string_roundtrip", {
        let urn = Urn { scheme: UrnScheme::File, path: String::from("C:/test.txt") };
        let s = urn.to_string();
        test_eq!(s, "neodos://file/C:/test.txt");
        let parsed = urn_parse(&s).unwrap();
        test_eq!(parsed.scheme, UrnScheme::File);
        test_eq!(parsed.path, "C:/test.txt");
    });
}
