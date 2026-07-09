# NeoDOS VFS Usage Patterns - Complete Reference

## Overview
The NeoDOS VFS (Virtual File System) is a unified filesystem interface accessed through a global `Mutex<Vfs>` protected by helper functions in `globals.rs`.

## Global VFS Access

### Method 1: Using `with_vfs()` Helper (Recommended)
The preferred pattern for accessing VFS in kernel code:

```rust
use crate::globals;

let result = crate::globals::with_vfs(|vfs| {
    // vfs is &mut Vfs - perform operations here
    vfs.resolve_path("C:\\path\\to\\file.txt")
});
```

**Location**: [`neodos-kernel/src/globals.rs`](neodos-kernel/src/globals.rs)

**Definition**:
```rust
pub fn with_vfs<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::fs::vfs::Vfs) -> R
{
    let mut lock = VFS.lock();
    f(&mut lock)
}
```

### Method 2: Direct Lock (Legacy)
```rust
match crate::globals::VFS.lock().read_file(inode_num, 0, &mut buf) {
    Ok(bytes_read) => { /* ... */ }
    Err(_) => { /* ... */ }
}
```

## Core Vfs Struct Definition

**File**: [`neodos-kernel/src/fs/vfs.rs`](neodos-kernel/src/fs/vfs.rs)

```rust
pub struct Vfs {
    pub drives: [Option<Box<dyn FileSystem>>; 26],  // A: through Z:
    mounts: [Option<Mount>; MAX_MOUNTS],             // Mount points (max 8)
    mount_count: usize,
}
```

### VfsNode - File/Directory Metadata
```rust
pub struct VfsNode {
    pub inode: u32,     // Inode number
    pub mode: u16,      // Type and permissions (use MODE_FILE, MODE_DIR)
    pub size: u32,      // File size in bytes
}
```

### Mode Constants
```rust
pub const MODE_DIR: u16 = 0x40;   // Directory
pub const MODE_FILE: u16 = 0x80;  // Regular file
```

### VfsError Enum
```rust
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
```

---

## Vfs Public Methods

### 1. Path Resolution

#### `resolve_path(path: &str) -> Result<(usize, VfsNode), VfsError>`
Resolves a full path and returns drive index + file metadata.

**Parameters**:
- `path`: Full path like `"C:\\System\\Config\\system.cfg"`

**Returns**: 
- `Ok((drive_idx, VfsNode))` - drive index (0-25) and file metadata
- `Err(VfsError)` - if path is invalid or file not found

**Example**:
```rust
crate::globals::with_vfs(|vfs| {
    match vfs.resolve_path("C:\\readme.txt") {
        Ok((drive_idx, node)) => {
            println!("Inode: {}, Size: {}, IsFile: {}", 
                     node.inode, node.size, node.mode & MODE_FILE != 0);
        }
        Err(e) => eprintln!("Path error: {:?}", e),
    }
})
```

**Used in**: Syscalls, shell commands, driver loader

---

### 2. File Reading

#### `read(drive_idx: usize, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>`
Reads file content at specified offset.

**Parameters**:
- `drive_idx`: Drive index (from `resolve_path`)
- `inode`: Inode number (from `VfsNode`)
- `offset`: Byte offset in file
- `buf`: Mutable buffer to read into

**Returns**: 
- `Ok(bytes_read)` - number of bytes read
- `Err(VfsError)` - on I/O or permission error

**Example**:
```rust
crate::globals::with_vfs(|vfs| {
    let (drive_idx, node) = vfs.resolve_path("C:\\test.nxe")?;
    
    if node.mode & MODE_FILE == 0 {
        return Err(VfsError::NotAFile);
    }
    
    let mut buf = vec![0u8; node.size as usize];
    let bytes_read = vfs.read(drive_idx, node.inode, 0, &mut buf)?;
    println!("Read {} bytes", bytes_read);
    Ok(())
})
```

**Real example from codebase** ([`neodos-kernel/src/drivers/boot_loader/mod.rs`](neodos-kernel/src/drivers/boot_loader/mod.rs)):
```rust
fn read_nem_file(path: &str) -> Result<Vec<u8>, &'static str> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path)
            .map_err(|_| "VFS resolve failed")?;
        if node.mode & MODE_FILE == 0 {
            return Err("Not a file");
        }
        let size = node.size as usize;
        if size == 0 || size > 65536 {
            return Err("Bad size");
        }
        let mut buf = alloc::vec::Vec::with_capacity(size);
        buf.resize(size, 0);
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf)
            .map_err(|_| "Read error")?;
        buf.truncate(read);
        Ok(buf)
    })
}
```

---

### 3. File Writing

#### `write(drive_idx: usize, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError>`
Writes data to a file.

**Parameters**:
- `drive_idx`: Drive index
- `inode`: Inode number
- `offset`: Byte offset
- `buf`: Data to write

**Returns**: 
- `Ok(bytes_written)` - number of bytes written
- `Err(VfsError)` - on I/O or permission error

---

### 4. Directory Navigation

#### `readdir(drive_idx: usize, inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError>`
Lists directory entries.

**Returns**:
- `Ok(Some(DirEntry))` - next directory entry
- `Ok(None)` - end of directory
- `Err(VfsError)` - if not a directory or I/O error

```rust
pub struct DirEntry {
    pub name: String,
    pub node: VfsNode,
}
```

---

### 5. File/Directory Metadata

#### `stat(drive_idx: usize, inode: u32) -> Result<VfsNode, VfsError>`
Gets file/directory information (size, type, etc).

---

### 6. File/Directory Creation

#### `create(path: &str) -> Result<VfsNode, VfsError>`
Creates a new file.

#### `mkdir(path: &str) -> Result<VfsNode, VfsError>`
Creates a new directory.

---

### 7. File/Directory Deletion

#### `remove_file(path: &str) -> Result<(), VfsError>`
Deletes a file.

#### `remove_dir(path: &str) -> Result<(), VfsError>`
Deletes an empty directory.

---

### 8. File Operations

#### `rename(path: &str, new_name: &str) -> Result<(), VfsError>`
Renames a file or directory.

---

### 9. Drive/Mount Operations

#### `mount(letter: char, fs: Box<dyn FileSystem>) -> Result<(), VfsError>`
Mounts a filesystem on a drive letter (A-Z).

#### `unmount(letter: char) -> Result<(), VfsError>`
Unmounts a filesystem.

#### `mount_at_path(path: &str, mounted_drive: char) -> Result<(), VfsError>`
Mounts a drive at a subdirectory.

#### `unmount_path(path: &str) -> Result<(), VfsError>`
Unmounts from a subdirectory.

---

### 10. Volume Operations

#### `volume_label(drive: char) -> Result<String, VfsError>`
Gets the volume label of a drive.

#### `set_volume_label(drive: char, label: &str) -> Result<(), VfsError>`
Sets the volume label.

---

## FileSystem Trait

Any filesystem implementation must implement this trait:

```rust
pub trait FileSystem: Send {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) 
        -> Result<usize, VfsError>;
    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) 
        -> Result<usize, VfsError>;
    fn lookup(&mut self, dir_inode: u32, name: &str) 
        -> Result<VfsNode, VfsError>;
    fn readdir(&mut self, dir_inode: u32, index: usize) 
        -> Result<Option<DirEntry>, VfsError>;
    fn mkdir(&mut self, dir_inode: u32, name: &str) 
        -> Result<VfsNode, VfsError>;
    fn create(&mut self, dir_inode: u32, name: &str) 
        -> Result<VfsNode, VfsError>;
    fn stat(&mut self, inode: u32) 
        -> Result<VfsNode, VfsError>;
    fn remove_file(&mut self, dir_inode: u32, name: &str) 
        -> Result<(), VfsError>;
    fn remove_dir(&mut self, dir_inode: u32, name: &str) 
        -> Result<(), VfsError>;
    fn rename(&mut self, dir_inode: u32, old_name: &str, new_name: &str) 
        -> Result<(), VfsError>;
    fn volume_label(&self) 
        -> Result<String, VfsError>;
    fn set_volume_label(&mut self, label: &str) 
        -> Result<(), VfsError>;
}
```

**Current implementations**:
- [`NeoDosFsV2`](neodos-kernel/src/fs/neodos_v2.rs) - NeoDOS native filesystem (NE2, NeoFS v2)
- [`Fat32Driver`](neodos-kernel/src/drivers/fat32.rs) - FAT32 (ESP boot partition)
- [`Iso9660Driver`](neodos-kernel/src/drivers/iso9660.rs) - ISO 9660 (CD-ROM)

---

## Common Usage Patterns

### Pattern 1: Read Entire File
```rust
crate::globals::with_vfs(|vfs| {
    let (drive_idx, node) = vfs.resolve_path("C:\\file.txt")?;
    
    if node.mode & MODE_FILE == 0 {
        return Err(VfsError::NotAFile);
    }
    
    let size = node.size as usize;
    let mut buf = vec![0u8; size];
    let bytes_read = vfs.read(drive_idx, node.inode, 0, &mut buf)?;
    buf.truncate(bytes_read);
    Ok(buf)
})
```

### Pattern 2: Read File by Handle (From Syscall)
```rust
// From sys_readfile syscall
let result = crate::globals::with_vfs(|vfs| {
    vfs.read(drive_idx, inode_num, offset, &mut temp_buf)
});

match result {
    Ok(bytes_read) => {
        unsafe {
            core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, bytes_read);
        }
    }
    Err(_) => return err_to_u64(SyscallError::IOError),
}
```

### Pattern 3: List Directory
```rust
crate::globals::with_vfs(|vfs| {
    let (drive_idx, dir_node) = vfs.resolve_path("C:\\System")?;
    
    let mut index = 0;
    while let Ok(Some(entry)) = vfs.readdir(drive_idx, dir_node.inode, index) {
        println!("{}: {} bytes", entry.name, entry.node.size);
        index += 1;
    }
    Ok(())
})
```

### Pattern 4: Create and Write File
```rust
crate::globals::with_vfs(|vfs| {
    let file_node = vfs.create("C:\\newfile.txt")?;
    vfs.write(drive_idx, file_node.inode, 0, b"Hello, World!")?;
    Ok(())
})
```

---

## Synchronization Model

The VFS is protected by a `spin::Mutex` in `globals.rs`:

```rust
pub static VFS: Mutex<crate::fs::vfs::Vfs> = Mutex::new(crate::fs::vfs::Vfs::new());
```

**Safe access patterns**:
1. ✅ Use `with_vfs()` helper (auto-locks/unlocks)
2. ✅ Lock directly with `VFS.lock()` if performing multiple operations

**Unsafe patterns**:
1. ❌ Hold the lock across syscall boundaries
2. ❌ Allocate memory while holding the lock (deadlock risk)
3. ❌ Call blocking functions while locked

---

## Examples from Codebase

### 1. Syscall Handler - `sys_readfile`
[`neodos-kernel/src/syscall.rs` line ~737](neodos-kernel/src/syscall.rs#L737)
```rust
let result = crate::globals::with_vfs(|vfs| {
    vfs.read(drive_idx, inode_num, offset, &mut temp_buf)
});
```

### 2. Driver Loader - Reading .nem Files
[`neodos-kernel/src/drivers/boot_loader/mod.rs` line ~274](neodos-kernel/src/drivers/boot_loader/mod.rs#L274)
```rust
fn read_nem_file(path: &str) -> Result<Vec<u8>, &'static str> {
    crate::globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(path).map_err(|_| "VFS resolve failed")?;
        // ... validation ...
        let mut buf = alloc::vec::Vec::with_capacity(size);
        buf.resize(size, 0);
        let read = vfs.read(drive_idx, node.inode, 0, &mut buf).map_err(|_| "Read error")?;
        buf.truncate(read);
        Ok(buf)
    })
}
```

### 3. Shell Command - `TYPE` (Display File)
[`userbin/coretype/src/main.rs`](userbin/coretype/src/main.rs)
```rust
crate::globals::with_vfs(|vfs| {
    let (drive_idx, node) = vfs.resolve_path(path)?;
    // ... read and display ...
})
```

### 4. Boot Benchmark - Reading BOOT.CFG
[`neodos-kernel/src/boot_benchmark.rs` line ~364](neodos-kernel/src/boot_benchmark.rs#L364)
```rust
match crate::globals::VFS.lock().open_file(path) {
    Ok(inode_num) => {
        let mut buf = [0u8; 512];
        match crate::globals::VFS.lock().read_file(inode_num, 0, &mut buf) {
            Ok(bytes_read) => { /* ... */ }
        }
    }
}
```

---

## Key Design Principles

1. **Path-based API**: Vfs provides high-level methods that take full paths
2. **Inode-based API**: FileSystem trait works with inode numbers
3. **Drive abstraction**: Drive letters (A-Z) map to mounted filesystems
4. **Error handling**: All operations return `Result<T, VfsError>`
5. **Zero-copy reads**: Data copied directly to user buffers where possible
6. **Mount points**: Supports mounting drives at directory locations

