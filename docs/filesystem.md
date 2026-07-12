# Filesystem Subsystem

The filesystem stack comprises NeoFS (native format), VFS (virtual filesystem
layer), FAT32 (ESP compat), IoStack (unified block I/O), and cache layers.

## NeoFS

**NeoFS v1 (NEOD) is obsolete and has been removed.**

NeoFS v2 (NE2) is the only native filesystem format supported by NeoDOS.

Source: `src/fs/neodos_v2.rs`. Native on-disk format for NeoDOS partitions.
All multi-byte integers are little-endian.

### Superblock v2

Located at LBA 0, exactly 512 bytes. Magic value: `0x0032454E` ("NE2\0").

| Offset | Size | Field              | Description                      |
|--------|------|--------------------|----------------------------------|
| 0      | 4    | magic              | "NE2\0" magic                    |
| 4      | 4    | version            | Format version                   |
| 8      | 8    | root_btree_lba     | Root B-tree block address        |
| 16     | 8    | root_version       | Root version counter             |
| 24     | 8    | root_timestamp     | Last root update timestamp       |
| 32     | 8    | num_blocks         | Total blocks in partition        |
| 40     | 8    | num_used           | Used blocks                      |
| 48     | 8    | num_free           | Free blocks                      |
| 56     | 1    | label_len          | Volume label length (0-32)       |
| 57     | 32   | label              | Volume label                     |
| 89     | 4    | flags              | Feature flags (reserved)         |
| 93     | 8    | freelist_lba       | Freelist root block              |
| 101    | 8    | snapshot_table_lba | Snapshot table block             |
| 109    | 403  | reserved           | Padding to 512 bytes             |

### Architecture

- **B-tree directories**: Each directory is a persistent B-tree indexed by entry name.
- **Extent-based files**: Files use extent lists (stored in B-trees) for data block tracking.
- **Inline data**: Small files (≤16 bytes) store data directly in the directory entry.
- **Copy-on-Write**: B-tree updates use COW semantics for crash safety.
- **Freelist**: A dedicated free block allocator replaces the old bitmap approach.
- **Snapshots**: Up to 64 snapshot entries in a circular table.
- **Feature flags**: The superblock `flags` field enables forward-compatible format evolution.

### Files

| File | Purpose |
| ------ | --------- |
| `src/fs/neodos_v2.rs` | NeoFS v2 implementation (`FileSystem` trait) |
| `src/fs/neodos_dir.rs` | B-tree directory operations (`DirEntryV2`) |
| `src/fs/neodos_io.rs` | Extent-based read/write + inline data |
| `src/fs/btree.rs` | Generic persistent B-tree with COW |
| `src/fs/freelist.rs` | Free block allocator |
| `src/fs/snapshot.rs` | Snapshot table |

### Permission Flags

Permission flags stored in directory entry mode bits:

| Flag | Bit | Meaning   |
|------|-----|-----------|
| PERM_R | 0 | Read      |
| PERM_W | 1 | Write     |
| PERM_X | 2 | Execute   |
| PERM_S | 3 | System    |
| PERM_D | 4 | Delete    |

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

### Page Cache

Source: `src/buffer/page_cache.rs`. 128 entries, 4 KB pages, LRU eviction.
Dirty tracking with pending-write accounting. Checked by file-backed mmap before
issuing a VFS read.

The page cache is global:

```rust
pub static PAGE_CACHE: Mutex<PageCache>;
```

Write-back: dirty entries are written to disk when evicted or on explicit sync.
