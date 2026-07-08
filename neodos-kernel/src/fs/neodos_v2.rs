//! NeoFS v2 (NE2) — implementación completa del FileSystem trait.

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use crate::drivers::block::BlockDevice;
use crate::buffer::page_cache::PageCache;
use crate::vfs::io::IoStack;
use crate::fs::vfs::{FileSystem, VfsNode, DirEntry, VfsError, MODE_DIR, MODE_FILE};
use crate::fs::btree::{BTree, BTreeNode, BTreeIO, NodeType, NODE_SIZE};
use crate::fs::freelist::FreeList;
use crate::fs::snapshot::SnapshotTable;
use crate::fs::neodos_dir::{DirEntryV2, dir_lookup, dir_readdir, dir_count, DIRENTRY_SIZE, PERM_R, PERM_W, PERM_X, PERM_D};
use crate::fs::neodos_io::{file_read, file_write, file_free_extents, crc32};

const SUPERBLOCK_MAGIC: u32 = 0x32454E32;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct SuperblockNE2 {
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
}

impl BTreeIO for NeoDosFsV2 {
    fn read_node(&self, lba: u64) -> Option<BTreeNode> {
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id)?;
        let mut buf = [0u8; NODE_SIZE];
        for i in 0..8usize {
            let s = dev.read_sector(lba + i as u64).ok()?;
            buf[i * 512..(i + 1) * 512].copy_from_slice(&s);
        }
        drop(bdevs);
        BTreeNode::deserialize(&buf)
    }

    fn write_node(&mut self, node: &BTreeNode) -> u64 {
        let lba = self.freelist.alloc_blocks(1).unwrap_or(0);
        if lba == 0 { return 0; }
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs.get(self.io_stack.device_id) { Some(d) => d, None => return 0 };
        let mut buf = [0u8; NODE_SIZE];
        node.serialize(&mut buf);
        for i in 0..8usize {
            let mut sec = [0u8; 512];
            sec.copy_from_slice(&buf[i * 512..(i + 1) * 512]);
            if dev.write_sector(lba + i as u64, &sec).is_err() { return 0; }
        }
        lba
    }
}

impl NeoDosFsV2 {
    pub fn new(io_stack: IoStack) -> Result<Self, ()> {
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(io_stack.device_id).ok_or(())?;
        let raw = dev.read_sector(0)?;
        let sb: SuperblockNE2 = unsafe { core::mem::transmute(raw) };
        if sb.magic != SUPERBLOCK_MAGIC { return Err(()); }
        drop(bdevs);

        let slab = 2; // estimación: LBA 1 en adelante para datos
        Ok(NeoDosFsV2 {
            sb, freelist: FreeList::with_range(1, sb.num_blocks - 1),
            io_stack,
            inode_cache: Vec::new(), next_inode: 1,
        })
    }

    fn alloc_inum(&mut self) -> u32 {
        let i = self.next_inode; self.next_inode += 1; i
    }

    fn cache(&mut self, btree_root: u64, entry: DirEntryV2) -> u32 {
        let i = self.alloc_inum();
        if i as usize >= self.inode_cache.len() { self.inode_cache.resize(i as usize + 1, None); }
        self.inode_cache[i as usize] = Some((btree_root, entry)); i
    }

    fn save_sb(&mut self) -> Result<(), ()> {
        self.sb.root_version = self.sb.root_version.wrapping_add(1);
        self.sb.root_timestamp = crate::hal::get_ticks();
        self.sb.num_used = self.sb.num_blocks - self.freelist.total_free() as u64;
        self.sb.num_free = self.freelist.total_free() as u64;
        let raw = unsafe { core::slice::from_raw_parts(&self.sb as *const _ as *const u8, 512) };
        let mut sector = [0u8; 512];
        sector.copy_from_slice(raw);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id).ok_or(())?;
        dev.write_sector(0, &sector).ok();
        Ok(())
    }
}

impl FileSystem for NeoDosFsV2 {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let (_, entry) = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()).ok_or(VfsError::NotFound)?;
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;
        let mut pc = crate::globals::PAGE_CACHE.lock();
        file_read(entry, offset, buf, &mut *pc, dev).map_err(|_| VfsError::IOError)
    }

    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError> {
        let (btree_root, entry) = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()).cloned().ok_or(VfsError::NotFound)?;
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;
        let mut pc = crate::globals::PAGE_CACHE.lock();
        let new_entry = file_write(&entry, offset, buf, &mut self.freelist, &mut *pc, dev).map_err(|_| VfsError::IOError)?;
        drop(pc);
        drop(bdevs);

        let new_root = BTree::insert(self, btree_root, &new_entry.name, &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; new_entry.serialize(&mut tmp); tmp.to_vec()
        }).ok_or(VfsError::IOError)?;

        if btree_root == self.sb.root_btree_lba { self.sb.root_btree_lba = new_root; }
        self.save_sb().map_err(|_| VfsError::IOError)?;
        if let Some(c) = self.inode_cache.get_mut(inode as usize).and_then(|x| x.as_mut()) { *c = (btree_root, new_entry); }
        Ok(buf.len())
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        let entry = dir_lookup(self, btree_root, name).ok_or(VfsError::NotFound)?;
        let size = if entry.inline_len > 0 { entry.inline_len as u32 } else { entry.size as u32 };
        let mode = entry.mode;
        let inum = self.cache(btree_root, entry);
        Ok(VfsNode { inode: inum, mode, size })
    }

    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        match dir_readdir(self, btree_root, index) {
            Some(e) => {
                let size = if e.inline_len > 0 { e.inline_len as u32 } else { e.size as u32 };
                Ok(Some(DirEntry { name: core::str::from_utf8(&e.name).unwrap_or("?").into(), node: VfsNode { inode: 0, mode: e.mode, size } }))
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

        if btree_root == self.sb.root_btree_lba { self.sb.root_btree_lba = new_root; }
        self.save_sb().map_err(|_| VfsError::IOError)?;
        let inum = self.cache(subdir_root, entry);
        Ok(VfsNode { inode: inum, mode: MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D, size: 0 })
    }

    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|x| x.0).ok_or(VfsError::NotFound)?;
        let entry = DirEntryV2::new_file(name);
        let new_root = BTree::insert(self, btree_root, name.as_bytes(), &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; entry.serialize(&mut tmp); tmp.to_vec()
        }).ok_or(VfsError::IOError)?;
        if btree_root == self.sb.root_btree_lba { self.sb.root_btree_lba = new_root; }
        self.save_sb().map_err(|_| VfsError::IOError)?;
        let inum = self.cache(btree_root, entry);
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
        if let Some(r) = nr { if btree_root == self.sb.root_btree_lba { self.sb.root_btree_lba = r; } }
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
        if let Some(r) = nr { if btree_root == self.sb.root_btree_lba { self.sb.root_btree_lba = r; } }
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
        if btree_root == self.sb.root_btree_lba { self.sb.root_btree_lba = nr; }
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
}
