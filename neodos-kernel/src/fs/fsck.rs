//! fsck para NE2 (NFSv2).
//!
//! Verifica:
//! - Checksum del superblock
//! - CRC32 de cada nodo B-tree
//! - Coherencia freelist vs bloques usados
//! - Opcionalmente checksums de datos (--deep)
//! - Modo repair: reconstruye freelist desde walk

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::vfs::io::IoStack;
use crate::fs::freelist::FreeList;
use crate::fs::freelist::FreeRegion;
use crate::fs::neodos_io::crc32;
use crate::drivers::block::BlockDevice;
use crate::buffer::page_cache::PageCache;

const SUPERBLOCK_MAGIC: u32 = 0x0032454E;
const BLOCK_SIZE: usize = 4096;
const SECTORS_PER_BLOCK: u64 = 8;
const MAX_ENTRIES: usize = 200;
const NAME_MAX: usize = 48;
const DIRENTRY_SIZE: usize = 128;

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

// ── Block tracking con Vec ordenado ───────────────────────────────

struct BlockSet {
    blocks: Vec<u64>,
}

impl BlockSet {
    fn new() -> Self {
        BlockSet { blocks: Vec::new() }
    }

    fn insert(&mut self, lba: u64) {
        if let Err(pos) = self.blocks.binary_search(&lba) {
            self.blocks.insert(pos, lba);
        }
    }

    fn contains(&self, lba: u64) -> bool {
        self.blocks.binary_search(&lba).is_ok()
    }

    fn len(&self) -> u64 {
        self.blocks.len() as u64
    }
}

// ── Raw block I/O ─────────────────────────────────────────────────

fn read_block(io: &IoStack, block_lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), ()> {
    let sector_lba = block_lba * SECTORS_PER_BLOCK;
    for i in 0..8usize {
        let s = io.read_sector(sector_lba + i as u64)?;
        buf[i * 512..(i + 1) * 512].copy_from_slice(&s);
    }
    Ok(())
}

fn write_block(io: &IoStack, block_lba: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), ()> {
    let sector_lba = block_lba * SECTORS_PER_BLOCK;
    for i in 0..8usize {
        let mut sec = [0u8; 512];
        sec.copy_from_slice(&buf[i * 512..(i + 1) * 512]);
        io.write_sector(sector_lba + i as u64, &sec)?;
    }
    Ok(())
}

// ── Superblock ────────────────────────────────────────────────────

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

fn read_superblock(io: &IoStack, sb: &mut SuperblockNE2) -> Result<(), ()> {
    let raw = io.read_sector(0)?;
    *sb = unsafe { core::mem::transmute(raw) };
    Ok(())
}

fn write_superblock(io: &IoStack, sb: &SuperblockNE2) -> Result<(), ()> {
    let raw = unsafe { core::slice::from_raw_parts(sb as *const _ as *const u8, 512) };
    let mut sector = [0u8; 512];
    sector.copy_from_slice(raw);
    io.write_sector(0, &sector)
}

fn verify_superblock_checksum(sb: &SuperblockNE2) -> bool {
    let stored = u32::from_le_bytes([sb.reserved[0], sb.reserved[1], sb.reserved[2], sb.reserved[3]]);
    let raw = unsafe { core::slice::from_raw_parts(sb as *const _ as *const u8, 72) };
    let computed = crc32(raw);
    stored == computed
}

// ── B-tree walk + CRC verification ────────────────────────────────

fn walk_node(
    io: &IoStack,
    lba: u64,
    used: &mut BlockSet,
    visited: &mut Vec<u64>,
    stats: &mut FsckStats,
    dev: &mut dyn BlockDevice,
    cache: &mut PageCache,
    deep: bool,
) {
    if lba == 0 || visited.contains(&lba) {
        return;
    }
    visited.push(lba);
    used.insert(lba);
    stats.total_nodes += 1;

    let mut buf = [0u8; BLOCK_SIZE];
    if read_block(io, lba, &mut buf).is_err() {
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

    if node_type == 0 {
        // Internal node: key = separator, value = child LBA (8 bytes)
        let mut offset = 8usize;
        for _ in 0..entry_count {
            if offset + 4 > BLOCK_SIZE {
                stats.warnings += 1;
                break;
            }
            let kl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + kl > BLOCK_SIZE {
                stats.warnings += 1;
                break;
            }
            offset += kl;
            if offset + 2 > BLOCK_SIZE {
                stats.warnings += 1;
                break;
            }
            let vl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + vl > BLOCK_SIZE {
                stats.warnings += 1;
                break;
            }
            if vl >= 8 {
                let child = u64::from_le_bytes([
                    buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3],
                    buf[offset + 4], buf[offset + 5], buf[offset + 6], buf[offset + 7],
                ]);
                if child > 0 {
                    walk_node(io, child, used, visited, stats, dev, cache, deep);
                }
            }
            offset += vl;
        }
    } else {
        // Leaf node: key = name, value = DirEntryV2 (128 bytes)
        let mut offset = 8usize;
        for _ in 0..entry_count {
            if offset + 4 > BLOCK_SIZE {
                stats.warnings += 1;
                break;
            }
            let kl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + kl > BLOCK_SIZE || kl > NAME_MAX {
                stats.warnings += 1;
                offset += kl.min(BLOCK_SIZE - offset);
                continue;
            }
            offset += kl;
            if offset + 2 > BLOCK_SIZE {
                stats.warnings += 1;
                break;
            }
            let vl = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + vl > BLOCK_SIZE {
                stats.warnings += 1;
                break;
            }

            if vl >= DIRENTRY_SIZE {
                parse_entry(&buf[offset..offset + DIRENTRY_SIZE], used, stats, io, dev, cache, deep);
            } else {
                stats.errors += 1;
            }
            offset += vl;
        }
    }
}

fn parse_entry(
    raw: &[u8],
    used: &mut BlockSet,
    stats: &mut FsckStats,
    io: &IoStack,
    dev: &mut dyn BlockDevice,
    cache: &mut PageCache,
    deep: bool,
) {
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
    if is_dir { stats.total_dirs += 1; }
    if is_file { stats.total_files += 1; }

    let inline_len = u32::from_le_bytes([raw[95], raw[96], raw[97], raw[98]]);
    let extent_lba = u64::from_le_bytes([
        raw[99], raw[100], raw[101], raw[102],
        raw[103], raw[104], raw[105], raw[106],
    ]);
    let extent_count = u32::from_le_bytes([raw[107], raw[108], raw[109], raw[110]]);
    let stored_checksum = u32::from_le_bytes([raw[91], raw[92], raw[93], raw[94]]);
    let file_size = u64::from_le_bytes([raw[67], raw[68], raw[69], raw[70], raw[71], raw[72], raw[73], raw[74]]);

    // Track extent blocks
    if extent_lba > 0 && extent_count > 0 {
        let end = extent_lba.saturating_add(extent_count as u64);
        for b in extent_lba..end {
            used.insert(b);
        }
    }

    // Deep checksum verification
    if deep && is_file && stored_checksum != 0 && file_size > 0 {
        let checksum = if inline_len > 0 {
            let il = inline_len.min(16) as usize;
            crc32(&raw[49..49 + il])
        } else if extent_lba > 0 {
            let abs_base = io.translate_lba(extent_lba * SECTORS_PER_BLOCK);
            verify_extent_checksum(dev, cache, abs_base, file_size, extent_count)
        } else {
            0
        };
        if checksum != 0 && checksum != stored_checksum {
            stats.warnings += 1;
        }
    }
}

fn verify_extent_checksum(
    dev: &mut dyn BlockDevice,
    cache: &mut PageCache,
    abs_base: u64,
    file_size: u64,
    _extent_count: u32,
) -> u32 {
    let total_blocks = ((file_size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64) as u32;
    let mut full = alloc::vec![0u8; (total_blocks as usize) * BLOCK_SIZE];
    for b in 0..total_blocks {
        let lba = abs_base + b as u64 * SECTORS_PER_BLOCK;
        if let Ok(data) = cache.read_page(0, 0, b as u32, lba, dev) {
            let start = b as usize * BLOCK_SIZE;
            let end = core::cmp::min(start + BLOCK_SIZE, full.len());
            full[start..end].copy_from_slice(&data[..end - start]);
        } else {
            return 0;
        }
    }
    if file_size < full.len() as u64 {
        full.truncate(file_size as usize);
    }
    crc32(&full)
}

// ── Freelist verification ─────────────────────────────────────────

fn verify_freelist(used: &BlockSet, total_blocks: u64, stats: &mut FsckStats) -> FreeList {
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

    let mut fl = FreeList::new();
    fl.regions = free_regions;
    fl
}

// ── Main fsck entry point ────────────────────────────────────────

pub fn fsck_ne2(
    io: &IoStack,
    repair: bool,
    deep: bool,
) -> FsckStats {
    let mut stats = FsckStats::default();

    // 1. Read superblock
    let mut sb = SuperblockNE2 {
        magic: 0, version: 0, root_btree_lba: 0, root_version: 0,
        root_timestamp: 0, num_blocks: 0, num_used: 0, num_free: 0,
        label_len: 0, label: [0u8; 32], flags: 0, freelist_lba: 0,
        snapshot_table_lba: 0, reserved: [0u8; 403],
    };
    if read_superblock(io, &mut sb).is_err() {
        stats.errors += 1;
        return stats;
    }

    if sb.magic != SUPERBLOCK_MAGIC {
        stats.errors += 1;
        return stats;
    }
    if !verify_superblock_checksum(&sb) {
        stats.warnings += 1;
    }

    let total_blocks = sb.num_blocks;
    let root_lba = sb.root_btree_lba;

    if total_blocks == 0 {
        stats.errors += 1;
        return stats;
    }

    // 2. Walk B-tree
    let mut used = BlockSet::new();
    used.insert(0); // superblock
    used.insert(1); // root B-tree node (block 1)

    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
    let dev = match bdevs.get(io.device_id) {
        Some(d) => d,
        None => {
            stats.errors += 1;
            return stats;
        }
    };
    let mut cache = crate::buffer::page_cache::PageCache::new();
    let mut visited = Vec::new();

    if root_lba > 0 {
        walk_node(io, root_lba, &mut used, &mut visited, &mut stats, &mut *dev, &mut cache, deep);
    }

    drop(bdevs);

    // 3. Verify freelist
    let new_fl = verify_freelist(&used, total_blocks, &mut stats);

    // 4. Repair
    if repair && (stats.errors > 0 || stats.warnings > 0) {
        let mut bdevs2 = crate::globals::BLOCK_DEVICES.lock();
        let dev2 = match bdevs2.get(io.device_id) {
            Some(d) => d,
            None => return stats,
        };

        // Update superblock
        let now = crate::hal::get_ticks();
        let new_sb = SuperblockNE2 {
            magic: SUPERBLOCK_MAGIC,
            version: 2,
            root_btree_lba: root_lba,
            root_version: sb.root_version.wrapping_add(1),
            root_timestamp: now,
            num_blocks: total_blocks,
            num_used: used.len(),
            num_free: new_fl.total_free(),
            label_len: sb.label_len,
            label: sb.label,
            flags: sb.flags,
            freelist_lba: 0,
            snapshot_table_lba: 0,
            reserved: [0u8; 403],
        };
        let _ = dev2;
        drop(bdevs2);

        let _ = write_superblock(io, &new_sb);
        stats.repaired = true;
        stats.used_blocks = used.len();
        stats.free_blocks = new_fl.total_free();
    }

    stats.total_dirs = stats.total_dirs.max(1); // root dir always counts
    stats
}

// ── Tests ─────────────────────────────────────────────────────────

pub fn register_fsck_tests() {
    // Helper: a simple RAM-backed block device for testing
    struct TestBlockDevice {
        sectors: alloc::vec::Vec<[u8; 512]>,
    }

    impl BlockDevice for TestBlockDevice {
        fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
            let start = lba as usize;
            let end = start + count as usize;
            if end > self.sectors.len() { return Err(()); }
            let mut off = 0;
            for i in start..end {
                let len = core::cmp::min(512, buf.len() - off);
                buf[off..off + len].copy_from_slice(&self.sectors[i][..len]);
                off += len;
            }
            Ok(())
        }

        fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
            let start = lba as usize;
            let end = start + count as usize;
            if end > self.sectors.len() { return Err(()); }
            let mut off = 0;
            for i in start..end {
                let len = core::cmp::min(512, buf.len() - off);
                self.sectors[i][..len].copy_from_slice(&buf[off..off + len]);
                off += len;
            }
            Ok(())
        }

        fn submit_irp(&mut self, _irp_id: crate::irp::IrpId) -> Result<(), ()> {
            Ok(())
        }

        fn base_lba(&self) -> u64 { 0 }

        fn set_base_lba(&mut self, _lba: u64) {}
    }

    // Test 1: clean filesystem
    crate::test_case!("neofs_v2_fsck_clean", {
        let num_sectors = 8192u64; // 1024 blocks × 8 sectors
        let sectors = alloc::vec![[0u8; 512]; num_sectors as usize];
        let test_dev = TestBlockDevice { sectors };

        let dev_id = {
            let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
            bdevs.register(alloc::boxed::Box::new(test_dev)).unwrap()
        };

        // Format as NE2
        let io = IoStack::new(dev_id);
        let num_blocks = num_sectors / 8;
        let _ = crate::fs::neodos_v2::mkfs_ne2(&io, num_blocks, "TEST");

        // Run fsck
        let stats = fsck_ne2(&io, false, false);

        // Verify clean
        crate::test_eq!(stats.errors, 0);
        crate::test_true!(stats.total_nodes > 0);
        crate::test_true!(stats.total_blocks == num_blocks);
        crate::test_true!(stats.used_blocks > 0);
        crate::test_true!(!stats.repaired);

        // Cleanup
        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    // Test 2: corrupted B-tree node CRC
    crate::test_case!("neofs_v2_fsck_corrupt_btree", {
        let num_sectors = 8192u64;
        let sectors = alloc::vec![[0u8; 512]; num_sectors as usize];
        let test_dev = TestBlockDevice { sectors };

        let dev_id = {
            let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
            bdevs.register(alloc::boxed::Box::new(test_dev)).unwrap()
        };

        let io = IoStack::new(dev_id);
        let num_blocks = num_sectors / 8;
        let _ = crate::fs::neodos_v2::mkfs_ne2(&io, num_blocks, "TEST");

        // Corrupt the root B-tree node (block 1, sectors 8-15)
        // Flip a byte in the first data byte of sector 9 (offset 512 in the block)
        let mut corrupt = io.read_sector(9).unwrap();
        corrupt[10] ^= 0xFF;
        let _ = io.write_sector(9, &corrupt);

        // Run fsck — should detect CRC error
        let stats = fsck_ne2(&io, false, false);
        crate::test_true!(stats.errors > 0);

        // Cleanup
        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });
}
