# Filesystem Subsystem

The filesystem stack comprises NeoFS (native format), VFS (virtual filesystem
layer), FAT32 (ESP compat), IoStack (unified block I/O), and cache layers.

## NeoFS

Source: `src/fs/neodos_fs.rs`. Native on-disk format for NeoDOS partitions.
All multi-byte integers are little-endian.

### Superblock

Located at LBA 0, exactly 512 bytes. Magic value: `0x4F444F4E` ("NEOD").

| Offset | Size | Field        | Description                      |
|--------|------|--------------|----------------------------------|
| 0      | 4    | magic        | "NEOD" magic                     |
| 4      | 4    | block_size   | Typically 4096                   |
| 8      | 4    | num_blocks   | Total blocks in partition        |
| 12     | 4    | num_inodes   | Maximum number of inodes         |
| 16     | 8    | created      | Creation timestamp               |
| 24     | 1    | label_len    | Volume label length (0-11)       |
| 25     | 11   | label        | DOS-standard volume label        |
| 36     | 476  | reserved     | Padding to 512 bytes             |

### Inode Table

Fixed offset after superblock. Each inode is 256 bytes:

```rust
pub struct Inode {
    pub inode_num: u32,
    pub mode: u16,              // 0x40=dir, 0x80=file + permission bits
    pub size: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub link_count: u16,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub direct_blocks: [u32; 12],  // 12 direct block pointers
    pub indirect_block: u32,       // Single indirect block
    pub padding: [u8; 160],
}
```

Permission flags stored in mode bits 0-4:

| Flag | Bit | Meaning   |
|------|-----|-----------|
| PERM_R | 0 | Read      |
| PERM_W | 1 | Write     |
| PERM_X | 2 | Execute   |
| PERM_S | 3 | System    |
| PERM_D | 4 | Delete    |

DOS attributes coexist in a separate field on directory entries:

| Attribute | Value |
|-----------|-------|
| ATTR_READONLY | 0x01 |
| ATTR_HIDDEN   | 0x02 |
| ATTR_SYSTEM   | 0x04 |
| ATTR_VOLUME   | 0x08 |
| ATTR_DIR      | 0x10 |
| ATTR_ARCHIVE  | 0x20 |

### Directory Entries

Each entry is 256 bytes, stored in directory inodes:

```rust
pub struct DirectoryEntry {
    pub inode_num: u32,
    pub name_len: u8,
    pub entry_type: u8,   // 1=file, 2=dir
    pub attributes: u8,
    pub name: [u8; 249],
}
```

Sentinel entry_type values: 0x00 = unused slot, 0xE5 = deleted, 0x2E = self (`.`),
0x2E2E = parent (`..`). Long filenames use consecutive entries with a marker in
the name field.

### Block Bitmap

Tracks free/used blocks via a byte vector. Each bit represents one block:

```rust
pub fn alloc(&mut self) -> Option<u32>;
pub fn free(&mut self, block: u32);
pub fn mark_used(&mut self, block: u32);
```

75 tests covering inode metadata, permissions, timestamps, serialization,
corruption detection, and directory walking.

## Default File Permissions

Applied at VFS level based on file extension:

| Extension  | Permissions | Notes                            |
|------------|-------------|----------------------------------|
| .NXE       | R-X         | Ring 3 executables               |
| .COM       | R-X         | Legacy COM executables           |
| .EXE       | R-X         | Legacy EXE executables           |
| .NEM       | R           | Kernel driver (read-only, crit.) |
| .NXL       | R-X         | User-mode loadable library       |
| .BAT/.CMD  | R-X         | Script execution                 |
| .SYS       | R           | Critical config (read-only)      |
| .CFG/.INI  | RW          | Configuration files              |
| .TXT/.MD   | RW          | Text documents                   |
| .LOG       | RW          | Log files                        |
| (other)    | RW          | Default                          |
| Directory  | RWXD        | Full control                     |

Kernel-created files (boot config) use SYSTEM owner, read-only+system.
User-created files inherit the current token's default permissions.

## VFS Layer

Source: `src/fs/vfs.rs`. Abstract filesystem interface.

### FileSystem Trait

```rust
pub trait FileSystem: Send {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError>;
    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError>;
    fn remove_file(...) -> ...;  // default = NotImplemented
    fn remove_dir(...) -> ...;
    fn rename(...) -> ...;
    fn volume_label(&self) -> Result<String, VfsError>;
    fn set_volume_label(...) -> ...;
    fn fs_type(&self) -> &'static str;
    fn total_sectors(&self) -> u64;
}
```

Unimplemented methods return `VfsError::NotImplemented`, allowing partial
implementations for read-only or simple filesystems.

### Vfs

```rust
pub struct Vfs {
    pub drives: [Option<Box<dyn FileSystem>>; 26],  // O(1) A:-Z:
    pub mounts: [Option<MountEntry>; 8],
}
```

Path resolution via `walk_components()`: parses drive letter (`C:\path` -> C, \path),
then walks each component with `.` and `..` resolution, traversing mount points.

### Mount Manager

Source: `src/vfs/mount.rs`. Creates ObObject entries for mounted filesystems
and `\DosDevices\` symlinks for drive letters in the Object Manager namespace.

## IoStack

Source: `src/vfs/io.rs`, `src/vfs/partition.rs`. Unified block I/O abstraction.

```rust
pub struct IoStack {
    pub device_id: u32,
    pub partition: Option<PartitionInfo>,
    pub cache_level: PageCacheLevel,
}
```

- `iostack_read_sectors()` -- reads from device, translating partition-relative
  LBAs by adding `partition.base_lba`
- `iostack_write_sectors()` -- same for writes

### Partition Info

```rust
pub struct PartitionInfo {
    pub base_lba: u64,
    pub sector_count: u64,
    pub partition_type: Guid,
}
```

GPT parsing identifies `PART_TYPE_ESP` and `PART_TYPE_NEODOS` GUIDs.
`find_all_esp_partitions()` discovers the ESP partition(s) on a device.

## FAT32

Source: `src/drivers/fat32.rs`. ESP partition mounted on `A:` for UEFI boot
compatibility. Uses the same IoStack layer for block I/O. Supports long filenames.

## Cache Layers

### Block Cache

Source: `src/buffer/block_cache.rs`. 64 entries, 512-byte sectors, LRU eviction.
Dirty tracking with deferred write-back.

### Page Cache

Source: `src/buffer/page_cache.rs`. 128 entries, 4 KB pages, LRU eviction.
Dirty tracking with pending-write accounting. Checked by file-backed mmap before
issuing a VFS read.

Both caches are global:
```rust
pub static BLOCK_CACHE: Mutex<BlockCache>;
pub static PAGE_CACHE: Mutex<PageCache>;
```

Write-back: dirty entries are written to disk when evicted or on explicit sync.
