//! NeoFS v2 (NE2) — implementación completa del FileSystem trait.

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use crate::vfs::io::IoStack;
use crate::fs::vfs::{FileSystem, VfsNode, DirEntry, VfsError, MODE_DIR, MODE_FILE};
use crate::fs::btree::{BTree, BTreeNode, BTreeIO, NodeType, NODE_SIZE};
use crate::fs::freelist::FreeList;
use crate::fs::neodos_dir::{DirEntryV2, dir_lookup, dir_readdir, dir_count, DIRENTRY_SIZE, PERM_R, PERM_W, PERM_X, PERM_D};
use crate::fs::neodos_io::{file_read, file_write, file_free_extents, crc32};
use crate::fs::snapshot::{SnapshotTable, SnapshotEntryRaw};

const SUPERBLOCK_MAGIC: u32 = 0x0032454E; // "NE2\0"

#[repr(C, packed)]
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct SuperblockNE2 {
    magic: u32,
    version: u32,
    root_btree_lba: u64,
    root_version: u64,
    root_timestamp: u64,
    num_blocks: u64,
    num_used: u64,
    num_free: u64,
    label_len: u8,
    label: [u8; 32],
    flags: u32,
    freelist_lba: u64,
    snapshot_table_lba: u64,
    reserved: [u8; 403],
}

pub struct NeoDosFsV2 {
    sb: SuperblockNE2,
    freelist: FreeList,
    pub io_stack: IoStack,
    inode_cache: Vec<Option<(u64, DirEntryV2)>>,
    next_inode: u32,
    pub snapshot_table: SnapshotTable,
}

impl BTreeIO for NeoDosFsV2 {
    fn read_node(&self, block_lba: u64) -> Option<BTreeNode> {
        let sector_lba = block_lba * 8;
        let abs_sector = self.io_stack.translate_lba(sector_lba);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id)?;
        let mut buf = [0u8; NODE_SIZE];
        for i in 0..8usize {
            let s = dev.read_sector(abs_sector + i as u64).ok()?;
            buf[i * 512..(i + 1) * 512].copy_from_slice(&s);
        }
        drop(bdevs);
        BTreeNode::deserialize(&buf)
    }

    fn write_node(&mut self, node: &BTreeNode) -> u64 {
        let block_lba = self.freelist.alloc_blocks(1).unwrap_or(0);
        if block_lba == 0 { return 0; }
        let sector_lba = block_lba * 8;
        let abs_sector = self.io_stack.translate_lba(sector_lba);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs.get(self.io_stack.device_id) { Some(d) => d, None => return 0 };
        let mut buf = [0u8; NODE_SIZE];
        node.serialize(&mut buf);
        for i in 0..8usize {
            let mut sec = [0u8; 512];
            sec.copy_from_slice(&buf[i * 512..(i + 1) * 512]);
            if dev.write_sector(abs_sector + i as u64, &sec).is_err() { return 0; }
        }
        // Invalidate page cache for these sectors — a freed data block
        // may have dirty pages left over from file_write, which would
        // overwrite B-tree metadata on flush.
        {
            let mut pc = crate::globals::PAGE_CACHE.lock();
            pc.invalidate_range(abs_sector, abs_sector + 8);
        }
        block_lba
    }
}

impl NeoDosFsV2 {
    pub fn new(io_stack: IoStack) -> Result<Self, ()> {
        let raw = io_stack.read_sector(0)?;
        // Manually parse to avoid transmute layout issues
        let magic = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        if magic != SUPERBLOCK_MAGIC { return Err(()); }
        let root_btree_lba = u64::from_le_bytes([raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15]]);
        let version = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        let root_version = u64::from_le_bytes([raw[16], raw[17], raw[18], raw[19], raw[20], raw[21], raw[22], raw[23]]);
        let root_timestamp = u64::from_le_bytes([raw[24], raw[25], raw[26], raw[27], raw[28], raw[29], raw[30], raw[31]]);
        let num_blocks = u64::from_le_bytes([raw[32], raw[33], raw[34], raw[35], raw[36], raw[37], raw[38], raw[39]]);
        let num_used = u64::from_le_bytes([raw[40], raw[41], raw[42], raw[43], raw[44], raw[45], raw[46], raw[47]]);
        let num_free = u64::from_le_bytes([raw[48], raw[49], raw[50], raw[51], raw[52], raw[53], raw[54], raw[55]]);
        let label_len = raw[56];
        let mut label = [0u8; 32];
        label.copy_from_slice(&raw[57..89]);
        let flags = u32::from_le_bytes([raw[89], raw[90], raw[91], raw[92]]);
        let freelist_lba = u64::from_le_bytes([raw[93], raw[94], raw[95], raw[96], raw[97], raw[98], raw[99], raw[100]]);
        let snapshot_table_lba = u64::from_le_bytes([raw[101], raw[102], raw[103], raw[104], raw[105], raw[106], raw[107], raw[108]]);
        let reserved = {
            let mut r = [0u8; 403];
            let len = r.len().min(512 - 109);
            r[..len].copy_from_slice(&raw[109..109 + len]);
            r
        };
        let sb = SuperblockNE2 { magic, version, root_btree_lba, root_version, root_timestamp,
            num_blocks, num_used, num_free, label_len, label, flags, freelist_lba, snapshot_table_lba, reserved };

        let mut inode_cache = Vec::new();
        let root_entry = DirEntryV2::new_dir("\\");
        inode_cache.push(Some((sb.root_btree_lba, root_entry)));
        // Freelist: num_used tells how many blocks are allocated (0 to num_used-1)
        let first_free = sb.num_used;
        let free_count = sb.num_blocks.saturating_sub(first_free);
        let snapshot_table = if sb.snapshot_table_lba > 0 {
            let mut node_buf = [0u8; NODE_SIZE];
            let sector_lba = sb.snapshot_table_lba * 8;
            let abs_sector = io_stack.translate_lba(sector_lba);
            let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
            if let Some(dev) = bdevs.get(io_stack.device_id) {
                for i in 0..8usize {
                    if let Ok(s) = dev.read_sector(abs_sector + i as u64) {
                        node_buf[i * 512..(i + 1) * 512].copy_from_slice(&s);
                    }
                }
            }
            drop(bdevs);
            SnapshotTable::deserialize(&node_buf).unwrap_or_else(|| SnapshotTable::new())
        } else {
            SnapshotTable::new()
        };

        Ok(NeoDosFsV2 {
            sb, freelist: FreeList::with_range(first_free, free_count),
            io_stack,
            inode_cache, next_inode: 1,
            snapshot_table,
        })
    }

    fn alloc_inum(&mut self) -> u32 {
        let i = self.next_inode; self.next_inode += 1; i
    }

    fn cache(&mut self, btree_root: u64, entry: DirEntryV2) -> u32 {
        if entry.is_dir() && entry.extent_lba > 0 {
            for i in 0..self.inode_cache.len() {
                if let Some((_, cached)) = &self.inode_cache[i] {
                    if cached.extent_lba == entry.extent_lba && cached.name == entry.name {
                        return i as u32;
                    }
                }
            }
        }
        let i = self.alloc_inum();
        if i as usize >= self.inode_cache.len() { self.inode_cache.resize(i as usize + 1, None); }
        self.inode_cache[i as usize] = Some((btree_root, entry)); i
    }

    fn update_inode_root(&mut self, inode: u32, new_root: u64) {
        if let Some(c) = self.inode_cache.get_mut(inode as usize).and_then(|x| x.as_mut()) {
            c.0 = new_root;
        }
    }

    fn save_sb(&mut self) -> Result<(), ()> {
        self.sb.root_version = self.sb.root_version.wrapping_add(1);
        self.sb.root_timestamp = crate::hal::get_ticks();
        self.sb.num_used = self.sb.num_blocks - self.freelist.total_free() as u64;
        self.sb.num_free = self.freelist.total_free() as u64;
        let raw = unsafe { core::slice::from_raw_parts(&self.sb as *const _ as *const u8, 512) };
        let mut sector = [0u8; 512];
        sector.copy_from_slice(raw);
        self.io_stack.write_sector(0, &sector).ok();
        Ok(())
    }
}

impl FileSystem for NeoDosFsV2 {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let (_, entry) = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()).ok_or(VfsError::NotFound)?;
        let abs_lba = self.io_stack.translate_lba(entry.extent_lba * 8);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;
        let mut pc = crate::globals::PAGE_CACHE.lock();
        // Translate partition-relative block LBA to absolute sector LBA for page cache
        let mut adj_entry = entry.clone();
        adj_entry.extent_lba = abs_lba;
        file_read(&adj_entry, offset, buf, &mut *pc, dev).map_err(|_| VfsError::IOError)
    }

    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError> {
        let (btree_root, entry) = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()).cloned().ok_or(VfsError::NotFound)?;
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;
        let mut pc = crate::globals::PAGE_CACHE.lock();
        let part_base = self.io_stack.translate_lba(0);
        let new_entry = file_write(&entry, offset, buf, &mut self.freelist, &mut *pc, dev, part_base).map_err(|_| VfsError::IOError)?;
        drop(pc);
        drop(bdevs);

        let new_root = BTree::insert(self, btree_root, &new_entry.name, &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; new_entry.serialize(&mut tmp); tmp.to_vec()
        }).ok_or(VfsError::IOError)?;

        if btree_root == self.sb.root_btree_lba { self.sb.root_btree_lba = new_root; }
        // Update parent directory's cache entry if its root changed
        for i in 0..self.inode_cache.len() {
            if let Some(c) = &mut self.inode_cache[i] {
                if c.0 == btree_root && c.1.is_dir() {
                    c.0 = new_root;
                }
            }
        }
        if let Some(c) = self.inode_cache.get_mut(inode as usize).and_then(|x| x.as_mut()) { *c = (new_root, new_entry); }
        Ok(buf.len())
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let cached = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).ok_or(VfsError::NotFound)?;
        let btree_root = cached.0;
        let entry = dir_lookup(self, btree_root, name).ok_or(VfsError::NotFound)?;
        let size = if entry.inline_len > 0 { entry.inline_len as u32 } else { entry.size as u32 };
        let mode = entry.mode;
        let child_root = if entry.is_dir() && entry.extent_lba > 0 { entry.extent_lba } else { btree_root };
        let inum = self.cache(child_root, entry);
        Ok(VfsNode { inode: inum, mode, size })
    }

    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        match dir_readdir(self, btree_root, index) {
            Some(e) => {
                let child_root = if e.is_dir() && e.extent_lba > 0 { e.extent_lba } else { btree_root };
                let inum = self.cache(child_root, e);
                let cached = self.inode_cache[inum as usize].as_ref().ok_or(VfsError::NotFound)?;
                let dname = core::str::from_utf8(&cached.1.name).unwrap_or("?");
                let size = if cached.1.inline_len > 0 { cached.1.inline_len as u32 } else { cached.1.size as u32 };
                Ok(Some(DirEntry { name: dname.into(), node: VfsNode { inode: inum, mode: cached.1.mode, size } }))
            }
            None => Ok(None),
        }
    }

    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        let empty = BTreeNode::new(NodeType::Leaf);
        let subdir_root = self.write_node(&empty);
        if subdir_root == 0 { return Err(VfsError::IOError); }

        let mut entry = DirEntryV2::new_dir(name);
        entry.extent_lba = subdir_root;
        entry.created = crate::hal::get_ticks(); entry.modified = entry.created;

        let new_root = BTree::insert(self, btree_root, name.as_bytes(), &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; entry.serialize(&mut tmp); tmp.to_vec()
        }).ok_or(VfsError::IOError)?;

        if dir_inode == 0 { self.sb.root_btree_lba = new_root; }
        self.update_inode_root(dir_inode, new_root);
        self.save_sb().map_err(|_| VfsError::IOError)?;
        let inum = self.cache(new_root, entry);
        Ok(VfsNode { inode: inum, mode: MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D, size: 0 })
    }

    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        let entry = DirEntryV2::new_file(name);
        let new_root = BTree::insert(self, btree_root, name.as_bytes(), &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; entry.serialize(&mut tmp); tmp.to_vec()
        }).ok_or(VfsError::IOError)?;
        if dir_inode == 0 { self.sb.root_btree_lba = new_root; }
        self.update_inode_root(dir_inode, new_root);
        self.save_sb().map_err(|_| VfsError::IOError)?;
        let inum = self.cache(new_root, entry);
        Ok(VfsNode { inode: inum, mode: MODE_FILE | PERM_R | PERM_W | PERM_X | PERM_D, size: 0 })
    }

    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError> {
        let (_, entry) = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()).ok_or(VfsError::NotFound)?;
        let size = if entry.inline_len > 0 { entry.inline_len as u32 } else { entry.size as u32 };
        Ok(VfsNode { inode, mode: entry.mode, size })
    }

    fn remove_file(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        if let Some(e) = dir_lookup(self, btree_root, name) { file_free_extents(&e, &mut self.freelist); }
        let nr = BTree::delete(self, btree_root, name.as_bytes()).ok_or(VfsError::IOError)?;
        if let Some(r) = nr {
            if dir_inode == 0 { self.sb.root_btree_lba = r; }
            self.update_inode_root(dir_inode, r);
        }
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn remove_dir(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        if let Some(e) = dir_lookup(self, btree_root, name) {
            if e.is_dir() {
                let count = dir_count(self, e.extent_lba);
                if count > 0 { return Err(VfsError::NotAFile); }
            }
            file_free_extents(&e, &mut self.freelist);
        }
        let nr = BTree::delete(self, btree_root, name.as_bytes()).ok_or(VfsError::IOError)?;
        if let Some(r) = nr {
            if dir_inode == 0 { self.sb.root_btree_lba = r; }
            self.update_inode_root(dir_inode, r);
        }
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn rename(&mut self, dir_inode: u32, old: &str, new: &str) -> Result<(), VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        let entry = dir_lookup(self, btree_root, old).ok_or(VfsError::NotFound)?;
        let entry_clone = entry.clone();
        let ad = BTree::delete(self, btree_root, old.as_bytes()).ok_or(VfsError::IOError)?;
        let ad = ad.unwrap_or(btree_root);
        let mut renamed = entry_clone; renamed.name = new.as_bytes().to_vec();
        let nr = BTree::insert(self, ad, new.as_bytes(), &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; renamed.serialize(&mut tmp); tmp.to_vec()
        }).ok_or(VfsError::IOError)?;
        if dir_inode == 0 { self.sb.root_btree_lba = nr; }
        self.update_inode_root(dir_inode, nr);
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn volume_label(&self) -> Result<String, VfsError> {
        let len = self.sb.label_len as usize;
        Ok(core::str::from_utf8(&self.sb.label[..len]).unwrap_or("").into())
    }

    fn set_volume_label(&mut self, label: &str) -> Result<(), VfsError> {
        let len = label.len().min(32);
        self.sb.label_len = len as u8;
        self.sb.label[..len].copy_from_slice(&label.as_bytes()[..len]);
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn fs_type(&self) -> &'static str { "NE2" }
    fn total_sectors(&self) -> u64 { self.sb.num_blocks * 8 }

    fn snapshot_create(&mut self) -> Result<u64, VfsError> {
        let root_lba = self.sb.root_btree_lba;
        let timestamp = crate::hal::get_ticks();
        let id = self.snapshot_table.create(root_lba, timestamp);
        self.save_sb().map_err(|_| VfsError::IOError)?;
        Ok(id)
    }

    fn snapshot_restore(&mut self, id: u64) -> Result<(), VfsError> {
        let root_lba = self.snapshot_table.restore(id).ok_or(VfsError::NotFound)?;
        self.sb.root_btree_lba = root_lba;
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn snapshot_list(&mut self, buf: &mut [u8]) -> Result<usize, VfsError> {
        let entries = self.snapshot_table.list();
        let entry_size = core::mem::size_of::<SnapshotEntryRaw>();
        let max_entries = buf.len() / entry_size;
        let count = entries.len().min(max_entries);
        for i in 0..count {
            let (id, snap) = &entries[i];
            let raw = SnapshotEntryRaw {
                id: *id,
                root_lba: snap.root_lba,
                timestamp: snap.timestamp,
            };
            let offset = i * entry_size;
            if offset + entry_size <= buf.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const SnapshotEntryRaw as *const u8,
                        buf.as_mut_ptr().add(offset),
                        entry_size,
                    );
                }
            }
        }
        Ok(entries.len())
    }

    fn snapshot_purge(&mut self) -> Result<(), VfsError> {
        self.snapshot_table.purge();
        self.save_sb().map_err(|_| VfsError::IOError)
    }
    fn fsck(&mut self, repair: bool, deep: bool, stats: &mut crate::fs::fsck::FsckStatsRaw) -> Result<(), VfsError> {
        let s = crate::fs::fsck::fsck_ne2(&self.io_stack, repair, deep);
        *stats = s.to_raw();
        Ok(())
    }
}

/// Formatear una partición con NE2 (mkfs).
/// Escribe superblock, raíz B-tree vacía, y freelist inicial.
pub fn mkfs_ne2(io_stack: &IoStack, num_blocks: u64, label: &str) -> Result<(), ()> {
    // 1. Escribir superblock
    let mut label_arr = [0u8; 32];
    let len = label.len().min(32);
    label_arr[..len].copy_from_slice(&label.as_bytes()[..len]);

    let sb = SuperblockNE2 {
        magic: SUPERBLOCK_MAGIC,
        version: 2,
        root_btree_lba: 1,
        root_version: 1,
        root_timestamp: crate::hal::get_ticks(),
        num_blocks,
        num_used: 1,
        num_free: num_blocks - 2,
        label_len: len as u8,
        label: label_arr,
        flags: 0,
        freelist_lba: 0,
        snapshot_table_lba: 0,
        reserved: [0u8; 403],
    };

    let raw = unsafe { core::slice::from_raw_parts(&sb as *const _ as *const u8, 512) };
    let mut sector = [0u8; 512];
    sector.copy_from_slice(raw);
    io_stack.write_sector(0, &sector)?;

    // 2. Escribir raíz B-tree vacía (block_lba = 1 → sector 8)
    let root_node = BTreeNode::new(NodeType::Leaf);
    let mut buf = [0u8; NODE_SIZE];
    root_node.serialize(&mut buf);
    for i in 0..8usize {
        let mut sec = [0u8; 512];
        sec.copy_from_slice(&buf[i * 512..(i + 1) * 512]);
        io_stack.write_sector(8 + i as u64, &sec)?;
    }

    // 3. Superblock definitivo con checksum
    let mut sb2 = sb;
    sb2.root_version = 1;
    let crc = crc32(unsafe { core::slice::from_raw_parts(&sb2 as *const _ as *const u8, 72) });
    sb2.reserved[..4].copy_from_slice(&crc.to_le_bytes());
    let raw2 = unsafe { core::slice::from_raw_parts(&sb2 as *const _ as *const u8, 512) };
    sector.copy_from_slice(raw2);
    io_stack.write_sector(0, &sector)?;

    Ok(())
}
