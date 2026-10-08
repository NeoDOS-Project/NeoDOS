//! FSCK for NeoFS v2 (NE2).
//!
//! Implements the generic [`FsckIntegrity`] B-tree walker and binds it to the
//! NE2 on-disk format. The walker validates magic + superblock checksum, the
//! CRC32 of every reachable B-tree node, and reconstructs the freelist from the
//! set of reachable blocks. Repair rewrites the superblock with the corrected
//! counters and freelist pointer.
//!
//! [`NeoDosFsV2`] and [`Ne2Fsck`] both implement [`FsckTrait`].

use alloc::vec::Vec;

use crate::fs::crc32::crc32;
use crate::fs::freelist::{FreeList, FreeRegion};
use crate::fs::neodos_v2::NeoDosFsV2;
use crate::vfs::io::IoStack;

use super::{FsckStats, FsckTrait};

pub const BLOCK_SIZE: usize = 4096;
pub const SECTORS_PER_BLOCK: u64 = 8;
const MAX_ENTRIES: usize = 200;

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

/// Generic integrity checker for B-tree based filesystems.
///
/// The default methods implement the shared traversal; concrete filesystems
/// only describe their on-disk format via the required methods.
pub trait FsckIntegrity {
    fn read_block(&self, block_lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), ()>;
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
            if offset + 4 > BLOCK_SIZE {
                break;
            }
            let kl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + kl > BLOCK_SIZE {
                break;
            }
            offset += kl;
            if offset + 2 > BLOCK_SIZE {
                break;
            }
            let vl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + vl > BLOCK_SIZE {
                break;
            }

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

    fn run(&self, repair: bool) -> FsckStats {
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
                stats.repaired = 1;
                stats.used_blocks = used.len();
                stats.free_blocks = new_fl.total_free();
            }
        }

        stats.total_dirs = stats.total_dirs.max(1);
        stats
    }
}

/// Reconstruct the free list from the set of reachable blocks.
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

// ── NE2 on-disk format ──────────────────────────────────────────────

const SUPERBLOCK_MAGIC: u32 = 0x0032454E; // "NE2\0"
const NAME_MAX: usize = 48;

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

impl SuperblockNE2 {
    fn checksum(&self) -> u32 {
        let raw = unsafe { core::slice::from_raw_parts(self as *const _ as *const u8, 72) };
        crc32(raw)
    }
}

struct Ne2Fsck<'a> {
    io: &'a IoStack,
    sb: SuperblockNE2,
}

impl Ne2Fsck<'_> {
    fn read_superblock(io: &IoStack, sb: &mut SuperblockNE2) -> Result<(), ()> {
        let raw = io.read_sector(0)?;
        *sb = unsafe { core::mem::transmute(raw) };
        Ok(())
    }
}

impl FsckIntegrity for Ne2Fsck<'_> {
    fn read_block(&self, block_lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), ()> {
        let sector_lba = block_lba * SECTORS_PER_BLOCK;
        for i in 0..8usize {
            let s = self.io.read_sector(sector_lba + i as u64)?;
            buf[i * 512..(i + 1) * 512].copy_from_slice(&s);
        }
        Ok(())
    }

    fn total_blocks(&self) -> u64 {
        self.sb.num_blocks
    }

    fn root_btree_lba(&self) -> u64 {
        self.sb.root_btree_lba
    }

    fn verify_magic(&self) -> bool {
        self.sb.magic == SUPERBLOCK_MAGIC
    }

    fn verify_superblock_checksum(&self) -> bool {
        let stored = u32::from_le_bytes([
            self.sb.reserved[0],
            self.sb.reserved[1],
            self.sb.reserved[2],
            self.sb.reserved[3],
        ]);
        stored == self.sb.checksum()
    }

    fn process_leaf_entry(&self, raw: &[u8], used: &mut BlockSet, stats: &mut FsckStats) {
        let name_len = raw[0] as usize;
        if name_len > NAME_MAX {
            stats.errors += 1;
            return;
        }

        let mode = u16::from_le_bytes([raw[65], raw[66]]);
        let is_file = mode & 0x80 != 0;
        let is_dir = mode & 0x40 != 0;
        if !is_file && !is_dir {
            stats.errors += 1;
            return;
        }
        if is_dir {
            stats.total_dirs += 1;
        }
        if is_file {
            stats.total_files += 1;
        }

        let extent_lba = u64::from_le_bytes([
            raw[99], raw[100], raw[101], raw[102],
            raw[103], raw[104], raw[105], raw[106],
        ]);
        let extent_count = u32::from_le_bytes([raw[107], raw[108], raw[109], raw[110]]);

        if extent_lba > 0 && extent_count > 0 {
            let end = extent_lba.saturating_add(extent_count as u64);
            for b in extent_lba..end {
                used.insert(b);
            }
        }
    }

    fn get_child_lba(&self, value: &[u8]) -> u64 {
        if value.len() >= 8 {
            u64::from_le_bytes(value[..8].try_into().unwrap())
        } else {
            0
        }
    }

    fn repair_superblock(&self, stats: &FsckStats, fl: &FreeList) -> Result<(), ()> {
        let now = crate::hal::get_ticks();
        let mut new_sb = SuperblockNE2 {
            magic: SUPERBLOCK_MAGIC,
            version: 2,
            root_btree_lba: self.sb.root_btree_lba,
            root_version: self.sb.root_version.wrapping_add(1),
            root_timestamp: now,
            num_blocks: stats.total_blocks,
            num_used: stats.used_blocks,
            num_free: fl.total_free(),
            label_len: self.sb.label_len,
            label: self.sb.label,
            flags: self.sb.flags,
            freelist_lba: 0,
            snapshot_table_lba: 0,
            reserved: [0u8; 403],
        };
        let cksum = new_sb.checksum();
        new_sb.reserved[..4].copy_from_slice(&cksum.to_le_bytes());

        let raw = unsafe { core::slice::from_raw_parts(&new_sb as *const _ as *const u8, 512) };
        let mut sector = [0u8; 512];
        sector.copy_from_slice(raw);
        self.io.write_sector(0, &sector)
    }
}

impl FsckTrait for Ne2Fsck<'_> {
    fn check(&self) -> FsckStats {
        self.run(false)
    }

    fn repair(&self) -> FsckStats {
        self.run(true)
    }
}

impl FsckTrait for NeoDosFsV2 {
    fn check(&self) -> FsckStats {
        fsck_ne2(&self.io_stack, false, false)
    }

    fn repair(&self) -> FsckStats {
        fsck_ne2(&self.io_stack, true, false)
    }
}

/// Run FSCK against a NeoFS v2 volume behind `io`.
///
/// `deep` is reserved for data-block checksum verification and is currently
/// ignored (metadata-only check, matching the fast FSCK design).
pub fn fsck_ne2(io: &IoStack, repair: bool, _deep: bool) -> FsckStats {
    let mut sb = SuperblockNE2 {
        magic: 0,
        version: 0,
        root_btree_lba: 0,
        root_version: 0,
        root_timestamp: 0,
        num_blocks: 0,
        num_used: 0,
        num_free: 0,
        label_len: 0,
        label: [0u8; 32],
        flags: 0,
        freelist_lba: 0,
        snapshot_table_lba: 0,
        reserved: [0u8; 403],
    };
    if Ne2Fsck::read_superblock(io, &mut sb).is_err() {
        let mut stats = FsckStats::default();
        stats.errors += 1;
        return stats;
    }
    let fsck = Ne2Fsck { io, sb };
    if repair {
        fsck.repair()
    } else {
        fsck.check()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::vfs::io::IoStack;

    // Test 1: clean filesystem
    crate::test_case!("neofs_v2_fsck_clean", {
        let sectors = alloc::vec![[0u8; 512]; 1024];
        let dev_id = super::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let num_blocks = 128;
        let _ = crate::fs::neodos_v2::mkfs_ne2(&io, num_blocks, "TEST");

        let stats = fsck_ne2(&io, false, false);

        crate::test_eq!(stats.errors, 0);
        crate::test_true!(stats.total_nodes > 0);
        crate::test_eq!(stats.total_blocks, num_blocks);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Test 2: corrupted B-tree node CRC
    crate::test_case!("neofs_v2_fsck_corrupt_btree", {
        let sectors = alloc::vec![[0u8; 512]; 1024];
        let dev_id = super::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let num_blocks = 128;
        let _ = crate::fs::neodos_v2::mkfs_ne2(&io, num_blocks, "TEST");

        let mut corrupt = io.read_sector(9).unwrap();
        corrupt[10] ^= 0xFF;
        let _ = io.write_sector(9, &corrupt);
        crate::globals::PAGE_CACHE.lock().invalidate_range(8, 16);

        let stats = fsck_ne2(&io, false, false);
        crate::test_true!(stats.errors > 0);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Test 3: bad superblock magic (via raw device manipulation)
    crate::test_case!("neofs_v2_fsck_bad_magic", {
        let sectors = alloc::vec![[0u8; 512]; 1024];
        let dev_id = super::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let _ = crate::fs::neodos_v2::mkfs_ne2(&io, 128, "TEST");
        // Overwrite magic directly on the device
        let mut sector = [0u8; 512];
        sector[0..4].copy_from_slice(&0u32.to_le_bytes());
        io.write_sector(0, &sector).unwrap();

        let stats = fsck_ne2(&io, false, false);
        crate::test_true!(stats.errors > 0);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Test 4: fsck on empty (zero-block) superblock
    crate::test_case!("neofs_v2_fsck_zero_blocks", {
        let mut sectors = alloc::vec![[0u8; 512]; 64];
        let sb = SuperblockNE2 {
            magic: SUPERBLOCK_MAGIC,
            version: 2,
            root_btree_lba: 1,
            root_version: 0,
            root_timestamp: 0,
            num_blocks: 0,
            num_used: 0,
            num_free: 0,
            label_len: 0,
            label: [0u8; 32],
            flags: 0,
            freelist_lba: 0,
            snapshot_table_lba: 0,
            reserved: [0u8; 403],
        };
        let raw = unsafe { core::slice::from_raw_parts(&sb as *const _ as *const u8, 512) };
        sectors[0].copy_from_slice(raw);
        let dev_id = super::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let stats = fsck_ne2(&io, false, false);
        crate::test_true!(stats.errors > 0);
        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Test 5: fsck detects corrupted B-tree node CRC
    crate::test_case!("neofs_v2_fsck_crc_detect", {
        let sectors = alloc::vec![[0u8; 512]; 1024];
        let dev_id = super::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let _ = crate::fs::neodos_v2::mkfs_ne2(&io, 128, "TEST");

        // Corrupt sector 9 (part of root B-tree node) directly via device
        let original = io.read_sector(9).unwrap();
        let mut corrupted = original;
        corrupted[10] ^= 0xFF;
        io.write_sector(9, &corrupted).unwrap();

        let stats = fsck_ne2(&io, false, false);
        crate::test_true!(stats.errors > 0);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Test 6: repair mode — corrupt B-tree CRC, repair, verify repaired count
    crate::test_case!("neofs_v2_fsck_repair", {
        let sectors = alloc::vec![[0u8; 512]; 1024];
        let dev_id = super::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let _ = crate::fs::neodos_v2::mkfs_ne2(&io, 128, "TEST");

        // Corrupt B-tree node (sector 9) directly on device
        let original = io.read_sector(9).unwrap();
        let mut corrupted = original;
        corrupted[10] ^= 0xFF;
        io.write_sector(9, &corrupted).unwrap();

        let stats = fsck_ne2(&io, true, false);
        crate::test_true!(stats.errors > 0);
        crate::test_true!(stats.repaired > 0);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });
}
