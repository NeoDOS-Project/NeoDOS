---
name: filesystem
description: Modify NeoFS v2, VFS, FAT32, GPT, page cache, or the I/O stack
---

# Filesystem

## When to use

Modifying NeoFS v2, VFS, the FAT32 ESP driver, GPT parsing, the block-device
manager, the I/O stack, the page cache, or partition handling.

## Goal

Make correct filesystem changes without corrupting data, breaking mount/unmount,
or violating VFS abstractions.

## References

- `docs/filesystem/overview.md` — subsystem documentation
- `docs/filesystem/neofs-v2.md`, `docs/filesystem/vfs-patterns.md`
- NeoFS v2: `src/fs/neofs/` — `neodos_v2.rs` (superblock + `FileSystem`),
  `neodos_dir.rs` (B-tree dirs), `neodos_io.rs` (extents + inline data),
  `btree/` (COW B-tree), `freelist.rs`, `snapshot.rs`
- VFS: `src/fs/vfs/` — `mod.rs` (`Vfs`), `io.rs` (IoStack), `mount.rs` (mount
  manager), `partition.rs`
- FAT32: `src/fs/fat32.rs` (ESP, mounted `A:`)
- FSCK: `src/fs/fsck/` (`FsckTrait`, `ne2.rs`, `fat32.rs`)
- GPT: `src/drivers/storage/gpt.rs`; block layer `src/drivers/storage/{block,manager}.rs`
- Page cache: `src/buffer/page_cache.rs` (128 × 4 KB, LRU, dirty/write-back)

## Steps

1. **Read `docs/filesystem/overview.md`** — NeoFS v2 layout, VFS architecture,
   IoStack, page cache, storage priority.

2. **Identify the subsystem** (see References). NeoFS v1 (NEOD) is removed;
   NeoFS v2 (NE2) is the only native format.

3. **VFS layer** (`src/fs/vfs/mod.rs`)
   The `FileSystem` trait is the driver contract:

   ```rust
   pub trait FileSystem: Send {
       fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
       fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
       fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
       fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError>;
       fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
       fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
       fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError>;
       // remove_file/remove_dir/rename/volume_label/... default to NotImplemented
   }
   ```

   `Vfs { drives: [Option<Box<dyn FileSystem>>; 26], mounts: [Option<Mount>;
   MAX_SUBDIR_MOUNTS], MAX_SUBDIR_MOUNTS = 8 }`. Path resolution walks
   components, resolving `.` / `..` and traversing mount points.

4. **IoStack** (`src/fs/vfs/io.rs`, `partition.rs`)
   Unified block I/O. `iostack_read_sectors()` / `iostack_write_sectors()`
   translate partition-relative LBAs by adding `partition.base_lba`. The block
   device manager probes controllers in priority order: **NVMe > VirtIO > AHCI
   > ATA**.

5. **Page cache** (`src/buffer/page_cache.rs`)
   Global `PAGE_CACHE: Mutex<PageCache>`, 128 × 4 KB entries with LRU eviction
   and dirty tracking. Keep it coherent on write (invalidate/update cached
   blocks); file-backed mmap checks the cache before a VFS read. Flush dirty
   entries before unmount.

6. **Mount / unmount** (`src/fs/vfs/mount.rs`)
   Mount: parse GPT, find the filesystem partition, mount via the `FileSystem`
   impl, and register `\Global\FileSystem\<drive>:` + `\DosDevices\<letter>:`
   entries in the Ob namespace. Unmount flushes then unregisters. Multiple
   subdirectory mounts are supported.

7. **FSCK** (`src/fs/fsck/`)
   Implement/verify `FsckTrait::check` (read-only) and `repair`. Exposed to user
   mode via `ObInfoClass::FsckStatus(33)` and `ObSetInfoClass::FsckRepair(39)`;
   driven by `fsck.nxe`.

8. **Write tests** with `test_case!`: create → write → read-back; mkdir/readdir;
   mount/unmount; overwrite/truncate; error paths (not found, full).

9. **Build and test**

   ```bash
   cd neodos-kernel && cargo build
   neodev build --quick --image && neodev test
   neodev check-deps
   ```

## Best practices

- Validate path lengths/components — no traversal outside the mount point.
- All block I/O goes through the IoStack; bypassing it breaks caching/ordering.
- Flush the page cache before unmount; keep page-cache coherence on writes.
- Handle partial reads/writes — loop until complete.
- Storage priority: prefer NVMe over VirtIO/AHCI for boot.

## Common mistakes

- Reading directly from the block device, bypassing the page cache (stale data).
- Forgetting to update directory entries (size, timestamps) after a write.
- `..` traversal escaping the mount point.
- Assuming a device is a whole disk without checking the GPT.
- Broken COW/snapshot assumptions when editing NeoFS v2 B-tree code.

## Final checklist

- [ ] `FileSystem` trait implemented (new FS) / updated safely (existing)
- [ ] Mount/unmount tested with no leaks
- [ ] Page-cache coherence maintained on writes
- [ ] Path traversal prevented
- [ ] Storage priority respected; GPT parsed correctly
- [ ] Tests added; `cargo build`, `neodev test`, `neodev check-deps` pass
- [ ] `docs/filesystem/overview.md` updated if VFS/format changed
