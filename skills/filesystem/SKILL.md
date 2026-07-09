# Filesystem

## When to use
Modifying NeoFS, VFS, FAT32 driver, GPT parser, block device manager, I/O stack, or partition handling.

## Goal
Make correct filesystem changes without corrupting data, breaking mount/unmount, or violating VFS abstractions.

## Steps

1. **Read `docs/filesystem.md`**
   Understand NeoFS layout, VFS architecture, IoStack, the page cache, and storage priority.

2. **Identify the relevant subsystem**
   - **NeoFS v2**: `src/fs/neodos_v2.rs` — native filesystem format (NE2). NeoFS v1 (NEOD) is obsolete and removed.
   - **FAT32**: `src/drivers/fat32.rs` — FAT32 read/write support
   - **VFS**: `src/vfs/mod.rs` — virtual filesystem layer (path resolution, file ops)
   - **IoStack**: `src/vfs/io.rs` — I/O request stack (IRP-like)
   - **Partitions**: `src/vfs/partition.rs` — partition table handling
   - **GPT**: `src/drivers/gpt.rs` — GPT parser
   - **Block device manager**: block device enumeration and priority (NVMe > VirtIO > AHCI > ATA)

3. **VFS layer** (`src/vfs/mod.rs`)
   VFS operations: `open`, `close`, `read`, `write`, `ioctl`, `mount`, `unmount`.
   Each operation is dispatched to the underlying filesystem driver through the `VfsDriver` trait.
   Adding a new operation: add it to `VfsDriver` trait and implement in all registered FS drivers.

4. **Filesystem driver implementation**
   For a new FS: implement `VfsDriver` trait. Key methods:
   ```rust
   fn mount(&self, device: &mut BlockDevice) -> Result<VfsMount, Status>;
   fn unmount(&self, mount: &VfsMount) -> Result<(), Status>;
   fn open(&self, mount: &VfsMount, path: &str) -> Result<VfsFileHandle, Status>;
   fn read(&self, file: &VfsFileHandle, buf: &mut [u8], offset: u64) -> Result<u64, Status>;
   fn write(&self, file: &VfsFileHandle, buf: &[u8], offset: u64) -> Result<u64, Status>;
   ```
   Place in `src/fs/` or `src/drivers/` depending on the FS type.

5. **IoStack** (`src/vfs/io.rs`, `src/vfs/partition.rs`)
   I/O requests are IRP-like packets queued through the IoStack.
   The stack manages: IRP allocation, completion routines, I/O priorities.
   For `BlockDeviceManager`: storage devices are probed in priority order (NVMe first, then VirtIO, AHCI, ATA).

6. **Page cache**
   FS reads go through the page cache (`src/vfs/cache.rs` if it exists). The cache holds recently accessed blocks.
   Ensure cache coherence when writing: invalidate or update cached blocks on write.

7. **Mount/unmount**
   Mount: parse GPT, find filesystem partition, call `VfsDriver::mount()`, register in VFS namespace.
   Unmount: flush dirty pages, call `VfsDriver::unmount()`, unregister.
   Multiple mounts at different paths are supported.

8. **Write tests**
   Add tests in `src/testing.rs` for:
   - Create file, write data, read back, verify
   - Directory creation and listing
   - Mount/unmount cycle
   - Overwrite and truncate
   - Error handling (file not found, disk full)

## Best practices
- Always validate path lengths and components — no path traversal outside mount point.
- Use `IoStack` for all block I/O — bypassing it breaks caching and ordering.
- Flush page cache before unmount.
- Partition-aware: don't assume a device is a whole disk; check GPT.
- Handle storage priority correctly — prefer NVMe over VirtIO for boot.

## Common mistakes
- Bypassing the page cache and reading directly from the block device — stale data.
- Not handling partial reads/writes — FS operations must loop until complete.
- Forgetting to update directory entries after file write (size, timestamps).
- Path traversal vulnerability via `..` components.
- Storage priority mismatch — booting from ATA when NVMe is available.

## Final checklist
- [ ] VfsDriver trait implemented (if new FS)
- [ ] Mount/unmount tested (no leaks)
- [ ] Page cache coherence maintained (writes invalidate cached reads)
- [ ] Path traversal prevented
- [ ] Storage priority respected
- [ ] GPT and partition table parsed correctly
- [ ] Kernel tests added and pass
- [ ] `docs/filesystem.md` updated if VFS or FS format changed
