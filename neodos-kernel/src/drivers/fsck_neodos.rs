//! Common FSCK integrity checking for block-based filesystems.
//!
//! Provides `FsckIntegrity` trait with generic B-tree walking,
//! CRC validation, block tracking, and freelist reconstruction.
//!
//! Filesystem drivers implement the trait for their on-disk format.

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::fs::freelist::{FreeList, FreeRegion};

pub const BLOCK_SIZE: usize = 4096;
pub const SECTORS_PER_BLOCK: u64 = 8;
const MAX_ENTRIES: usize = 200;

#[repr(C)]
pub struct FsckStatsRaw {
    pub total_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub total_nodes: u64,
    pub total_dirs: u64,
    pub total_files: u64,
    pub errors: u32,
    pub warnings: u32,
    pub repaired: u32,
}

#[derive(Default)]
pub struct FsckStats {
    pub total_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub total_nodes: u64,
    pub total_dirs: u64,
    pub total_files: u64,
    pub errors: u32,
    pub warnings: u32,
    pub repaired: bool,
}

impl FsckStats {
    pub fn to_raw(&self) -> FsckStatsRaw {
        FsckStatsRaw {
            total_blocks: self.total_blocks,
            used_blocks: self.used_blocks,
            free_blocks: self.free_blocks,
            total_nodes: self.total_nodes,
            total_dirs: self.total_dirs,
            total_files: self.total_files,
            errors: self.errors,
            warnings: self.warnings,
            repaired: self.repaired as u32,
        }
    }
}

pub struct BlockSet {
    blocks: Vec<u64>,
}

impl BlockSet {
    pub fn new() -> Self {
        BlockSet { blocks: Vec::new() }
    }

    pub fn insert(&mut self, lba: u64) {
        if let Err(pos) = self.blocks.binary_search(&lba) {
            self.blocks.insert(pos, lba);
        }
    }

    pub fn contains(&self, lba: u64) -> bool {
        self.blocks.binary_search(&lba).is_ok()
    }

    pub fn len(&self) -> u64 {
        self.blocks.len() as u64
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub trait FsckIntegrity {
    fn read_block(&self, block_lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), ()>;
    fn write_block(&self, block_lba: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), ()>;
    fn total_blocks(&self) -> u64;
    fn root_btree_lba(&self) -> u64;
    fn verify_magic(&self) -> bool;
    fn verify_superblock_checksum(&self) -> bool;

    fn process_leaf_entry(&self, raw: &[u8], used: &mut BlockSet, stats: &mut FsckStats);
    fn get_child_lba(&self, value: &[u8]) -> u64;
    fn repair_superblock(&self, stats: &FsckStats, fl: &FreeList) -> Result<(), ()>;

    fn walk_node(&self, lba: u64, used: &mut BlockSet, visited: &mut Vec<u64>, stats: &mut FsckStats) {
        if lba == 0 || visited.contains(&lba) {
            return;
        }
        visited.push(lba);
        used.insert(lba);
        stats.total_nodes += 1;

        let mut buf = [0u8; BLOCK_SIZE];
        if self.read_block(lba, &mut buf).is_err() {
            stats.errors += 1;
            return;
        }

        let stored_crc = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let computed_crc = crc32(&buf[8..]);
        if stored_crc != 0 && stored_crc != computed_crc {
            stats.errors += 1;
        }

        let node_type = u16::from_le_bytes([buf[0], buf[1]]);
        let entry_count = u16::from_le_bytes([buf[2], buf[3]]) as usize;

        if node_type > 1 {
            stats.errors += 1;
            return;
        }
        if entry_count > MAX_ENTRIES {
            stats.errors += 1;
            return;
        }
        if entry_count == 0 {
            return;
        }

        let mut offset = 8usize;
        for _ in 0..entry_count {
            if offset + 4 > BLOCK_SIZE { break; }
            let kl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + kl > BLOCK_SIZE { break; }
            offset += kl;
            if offset + 2 > BLOCK_SIZE { break; }
            let vl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + vl > BLOCK_SIZE { break; }

            if node_type == 0 {
                if vl >= 8 {
                    let child = self.get_child_lba(&buf[offset..offset + vl]);
                    if child > 0 {
                        self.walk_node(child, used, visited, stats);
                    }
                }
            } else {
                self.process_leaf_entry(&buf[offset..offset + vl], used, stats);
            }
            offset += vl;
        }
    }

    fn run_fsck(&self, repair: bool) -> FsckStats {
        let mut stats = FsckStats::default();

        if !self.verify_magic() {
            stats.errors += 1;
            return stats;
        }
        if !self.verify_superblock_checksum() {
            stats.warnings += 1;
        }

        let total_blocks = self.total_blocks();
        let root_lba = self.root_btree_lba();

        if total_blocks == 0 {
            stats.errors += 1;
            return stats;
        }

        let mut used = BlockSet::new();
        used.insert(0);
        used.insert(1);

        let mut visited = Vec::new();
        if root_lba > 0 {
            self.walk_node(root_lba, &mut used, &mut visited, &mut stats);
        }

        let new_fl = verify_freelist(&used, total_blocks, &mut stats);

        if repair && (stats.errors > 0 || stats.warnings > 0) {
            if self.repair_superblock(&stats, &new_fl).is_ok() {
                stats.repaired = true;
                stats.used_blocks = used.len();
                stats.free_blocks = new_fl.total_free();
            }
        }

        stats.total_dirs = stats.total_dirs.max(1);
        stats
    }
}

pub fn verify_freelist(used: &BlockSet, total_blocks: u64, stats: &mut FsckStats) -> FreeList {
    let mut free_regions = Vec::new();
    let mut i = 0u64;
    while i < total_blocks {
        if used.contains(i) {
            i += 1;
        } else {
            let start = i;
            while i < total_blocks && !used.contains(i) {
                i += 1;
            }
            free_regions.push(FreeRegion {
                start_lba: start,
                length: (i - start) as u32,
            });
        }
    }

    let mut free_count = 0u64;
    for r in &free_regions {
        free_count += r.length as u64;
    }

    stats.used_blocks = used.len();
    stats.free_blocks = free_count;
    stats.total_blocks = total_blocks;

    if used.len() + free_count != total_blocks {
        stats.warnings += 1;
    }

    FreeList { regions: free_regions }
}
